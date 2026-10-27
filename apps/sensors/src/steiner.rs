//! The sensors through steinerd, Phonewave's sensor service (`org.steinerd`
//! on the system bus, phonewave/services/sensors/steinerd). Its properties
//! are read whole when it appears and follow its change signals after. A
//! sensor gives readings only while someone claims it, so Sensors claims
//! them all while its window shows and releases them while it is hidden;
//! steinerd also drops the claims once this connection closes.

use std::time::Duration;

use async_channel::{Receiver, Sender};
use futures_lite::{FutureExt, StreamExt};
use zbus::Connection;
use zbus::fdo::{DBusProxy, PropertiesProxy};
use zbus::names::InterfaceName;
use zbus::zvariant::Value;

const NAME: &str = "org.steinerd";
const PATH: &str = "/org/steinerd";
const INTERFACE: &str = "org.steinerd.Sensors1";
/// What `Claim` and `Release` take.
const SENSORS: [&str; 6] = [
    "orientation",
    "light",
    "proximity",
    "accel",
    "gyro",
    "compass",
];
const RETRY: Duration = Duration::from_secs(2);

/// The physical edge pointing up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    /// No reading yet.
    #[default]
    Undefined,
    /// The top edge.
    Normal,
    BottomUp,
    LeftUp,
    RightUp,
}

impl Orientation {
    fn parse(name: &str) -> Self {
        match name {
            "normal" => Orientation::Normal,
            "bottom-up" => Orientation::BottomUp,
            "left-up" => Orientation::LeftUp,
            "right-up" => Orientation::RightUp,
            _ => Orientation::Undefined,
        }
    }

    /// How far the phone is turned from upright, clockwise as one looks at
    /// its screen, in degrees.
    pub fn turn(self) -> f32 {
        match self {
            Orientation::Undefined | Orientation::Normal => 0.0,
            Orientation::LeftUp => 90.0,
            Orientation::BottomUp => 180.0,
            Orientation::RightUp => -90.0,
        }
    }
}

/// What steinerd reports.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub has_orientation: bool,
    pub orientation: Orientation,
    pub has_light: bool,
    /// Lux.
    pub light: f64,
    pub has_proximity: bool,
    pub near: bool,
    pub has_accel: bool,
    /// m/s²: x toward the right edge, y toward the top edge, z out of the
    /// screen.
    pub acceleration: [f64; 3],
    pub has_gyro: bool,
    /// rad/s about the same axes.
    pub rotation_rate: [f64; 3],
    pub has_compass: bool,
    /// Degrees clockwise from magnetic north; None before a reading.
    pub heading: Option<f64>,
    /// 0 unreliable to 3 high.
    pub heading_accuracy: u8,
}

impl State {
    fn set(&mut self, name: &str, value: &Value<'_>) {
        let flag = || value.downcast_ref::<bool>().ok();
        match name {
            "HasOrientation" => self.has_orientation = flag().unwrap_or(false),
            "Orientation" => {
                self.orientation = Orientation::parse(value.downcast_ref::<&str>().unwrap_or(""))
            }
            "HasLight" => self.has_light = flag().unwrap_or(false),
            "Light" => self.light = value.downcast_ref::<f64>().unwrap_or(0.0),
            "HasProximity" => self.has_proximity = flag().unwrap_or(false),
            "Near" => self.near = flag().unwrap_or(false),
            "HasAccel" => self.has_accel = flag().unwrap_or(false),
            "Acceleration" => self.acceleration = vector(value),
            "HasGyro" => self.has_gyro = flag().unwrap_or(false),
            "RotationRate" => self.rotation_rate = vector(value),
            "HasCompass" => self.has_compass = flag().unwrap_or(false),
            "Heading" => self.heading = value.downcast_ref::<f64>().ok().filter(|h| *h >= 0.0),
            "HeadingAccuracy" => self.heading_accuracy = value.downcast_ref::<u8>().unwrap_or(0),
            _ => {}
        }
    }
}

/// A `(ddd)` property.
fn vector(value: &Value<'_>) -> [f64; 3] {
    value
        .try_clone()
        .ok()
        .and_then(|v| <(f64, f64, f64)>::try_from(v).ok())
        .map_or([0.0; 3], |(x, y, z)| [x, y, z])
}

/// Follows steinerd for as long as `updates` is heard: what it reports, or
/// None while it does not run. `shown` says whether the window shows, and
/// so whether the sensors are claimed.
pub async fn follow(updates: Sender<Option<State>>, shown: Receiver<bool>) {
    let mut showing = false;
    loop {
        match follow_steinerd(&updates, &shown, &mut showing).await {
            Ok(()) => {}
            Err(_) if updates.is_closed() || shown.is_closed() => return,
            Err(error) => {
                eprintln!("sensors: following steinerd: {error}");
                let _ = updates.send(None).await;
                async_io::Timer::after(RETRY).await;
            }
        }
    }
}

/// What woke the follower.
enum Wake {
    Changed(bool),
    Owner,
    Shown(Option<bool>),
}

/// Follows steinerd until it starts or stops.
async fn follow_steinerd(
    updates: &Sender<Option<State>>,
    shown: &Receiver<bool>,
    showing: &mut bool,
) -> zbus::Result<()> {
    let send = |update| async move {
        updates
            .send(update)
            .await
            .map_err(|_| zbus::Error::Failure("the window is gone".into()))
    };
    let bus = Connection::system().await?;
    let dbus = DBusProxy::new(&bus).await?;
    let mut owners = dbus
        .receive_name_owner_changed_with_args(&[(0, NAME)])
        .await?;
    let running = dbus.name_has_owner(NAME.try_into()?).await?;
    let mut changes = None;
    let mut state = State::default();
    if running {
        let props = PropertiesProxy::builder(&bus)
            .destination(NAME)?
            .path(PATH)?
            .build()
            .await?;
        changes = Some(props.receive_properties_changed().await?);
        for (name, value) in props.get_all(InterfaceName::try_from(INTERFACE)?).await? {
            state.set(&name, &value);
        }
        if *showing {
            claim(&bus, true).await;
        }
        send(Some(state.clone())).await?;
    } else {
        send(None).await?;
    }
    loop {
        let changed = async {
            match changes.as_mut() {
                Some(changes) => match changes.next().await {
                    Some(signal) => {
                        if let Ok(args) = signal.args() {
                            for (name, value) in args.changed_properties() {
                                state.set(name, value);
                            }
                        }
                        Wake::Changed(true)
                    }
                    None => Wake::Changed(false),
                },
                None => std::future::pending().await,
            }
        };
        let wake = changed
            .or(async {
                owners.next().await;
                Wake::Owner
            })
            .or(async { Wake::Shown(shown.recv().await.ok()) })
            .await;
        match wake {
            Wake::Changed(true) => send(Some(state.clone())).await?,
            Wake::Changed(false) | Wake::Owner => return Ok(()),
            Wake::Shown(None) => return Err(zbus::Error::Failure("the window is gone".into())),
            Wake::Shown(Some(now)) => {
                if now != *showing && running {
                    claim(&bus, now).await;
                }
                *showing = now;
            }
        }
    }
}

/// Claims every sensor, or releases them.
async fn claim(bus: &Connection, on: bool) {
    let method = if on { "Claim" } else { "Release" };
    for sensor in SENSORS {
        if let Err(error) = bus
            .call_method(Some(NAME), PATH, Some(INTERFACE), method, &(sensor,))
            .await
        {
            eprintln!("sensors: {method} {sensor}: {error}");
        }
    }
}

/// Follows steinerd on a phone for 5 s, the sensors claimed, and prints what
/// it reports: `cargo test -- --ignored --nocapture reads_steinerd`.
#[cfg(test)]
#[test]
#[ignore]
fn reads_steinerd() {
    let (updates, received) = async_channel::unbounded();
    let (shown, showing) = async_channel::unbounded();
    shown.send_blocking(true).unwrap();
    futures_lite::future::block_on(follow(updates, showing).or(async {
        async_io::Timer::after(Duration::from_secs(5)).await;
    }));
    while let Ok(update) = received.try_recv() {
        println!("{update:?}");
    }
}
