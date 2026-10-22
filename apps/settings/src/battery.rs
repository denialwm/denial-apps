//! The battery through UPower (`org.freedesktop.UPower` on the system bus).
//! Its devices are read whole at start and again after every change it
//! signals. Only batteries that report how much energy they hold when full
//! count: a firmware battery that reports a level and nothing else (the Ace
//! 3's battery manager says 50% and charging, always) is left out.

use std::collections::HashMap;
use std::time::Duration;

use async_channel::Sender;
use futures_lite::{FutureExt, StreamExt};
use zbus::fdo::DBusProxy;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{Connection, MatchRule, MessageStream};

const NAME: &str = "org.freedesktop.UPower";
const PATH: &str = "/org/freedesktop/UPower";
const DEVICE: &str = "org.freedesktop.UPower.Device";
/// A burst of changes is read once, this long after the first.
const SETTLE: Duration = Duration::from_millis(150);
const RETRY: Duration = Duration::from_secs(2);

/// UPower's device types and states.
const LINE_POWER: u32 = 1;
const BATTERY: u32 = 2;
const CHARGING: u32 = 1;
const FULL: u32 = 4;
const PENDING_CHARGE: u32 = 5;

/// What the battery is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Charge {
    Charging,
    Draining,
    Full,
    /// Plugged in, but the charger holds it where it is.
    Held,
}

/// The phone's batteries, as one.
#[derive(Clone, Debug)]
pub struct Battery {
    /// Percent.
    pub level: f64,
    pub charge: Charge,
    /// Until full while charging, until empty otherwise.
    pub until: Option<Duration>,
    /// Watts in or out.
    pub power: f64,
    /// What it holds when full, in percent of what it held new.
    pub health: Option<f64>,
    pub cycles: Option<u32>,
    /// °C.
    pub temperature: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    /// None on mains power only.
    pub battery: Option<Battery>,
    /// Whether a charger is connected.
    pub plugged: bool,
}

/// One UPower device's properties.
struct Device(HashMap<String, OwnedValue>);

impl Device {
    fn get<T: TryFrom<OwnedValue>>(&self, name: &str) -> Option<T> {
        self.0
            .get(name)
            .and_then(|value| value.try_clone().ok()?.try_into().ok())
    }

    fn number(&self, name: &str) -> f64 {
        self.get(name).unwrap_or(0.0)
    }
}

/// The batteries in `devices` as one, if there are any.
fn battery(devices: &[Device]) -> Option<Battery> {
    let batteries: Vec<&Device> = devices
        .iter()
        .filter(|device| {
            device.get::<u32>("Type") == Some(BATTERY)
                && device.get("PowerSupply").unwrap_or(false)
                && device.get("IsPresent").unwrap_or(false)
                && device.number("EnergyFull") > 0.0
        })
        .collect();
    if batteries.is_empty() {
        return None;
    }
    let sum = |name: &str| {
        batteries
            .iter()
            .map(|device| device.number(name))
            .sum::<f64>()
    };
    let (energy, full, design, power) = (
        sum("Energy"),
        sum("EnergyFull"),
        sum("EnergyFullDesign"),
        sum("EnergyRate"),
    );
    let states: Vec<u32> = batteries
        .iter()
        .map(|device| device.get("State").unwrap_or(0))
        .collect();
    let charge = if states.contains(&CHARGING) {
        Charge::Charging
    } else if states.iter().all(|&state| state == FULL) {
        Charge::Full
    } else if states.contains(&PENDING_CHARGE) {
        Charge::Held
    } else {
        Charge::Draining
    };
    let hours = match charge {
        Charge::Charging => (full - energy) / power,
        // Full and off the charger, it drains like any other.
        Charge::Draining | Charge::Full => energy / power,
        Charge::Held => f64::NAN,
    };
    Some(Battery {
        // Each battery's own level, weighed by its size: the gauge's figure
        // when there is one battery.
        level: batteries
            .iter()
            .map(|device| device.number("Percentage") * device.number("EnergyFull"))
            .sum::<f64>()
            / full,
        charge,
        until: (hours.is_finite() && hours > 0.0).then(|| Duration::from_secs_f64(hours * 3600.0)),
        power,
        health: (design > 0.0).then(|| (full / design * 100.0).min(100.0)),
        cycles: batteries
            .iter()
            .filter_map(|device| device.get::<i32>("ChargeCycles"))
            .filter(|&cycles| cycles >= 0)
            .max()
            .map(|cycles| cycles as u32),
        temperature: batteries
            .iter()
            .map(|device| device.number("Temperature"))
            .filter(|&celsius| celsius != 0.0)
            .reduce(f64::max),
    })
}

async fn read(bus: &Connection) -> zbus::Result<State> {
    let reply = bus
        .call_method(Some(NAME), PATH, Some(NAME), "EnumerateDevices", &())
        .await?;
    let paths: Vec<OwnedObjectPath> = reply.body().deserialize()?;
    let mut devices = Vec::new();
    for path in paths {
        let reply = bus
            .call_method(
                Some(NAME),
                &path,
                Some("org.freedesktop.DBus.Properties"),
                "GetAll",
                &(DEVICE,),
            )
            .await?;
        devices.push(Device(reply.body().deserialize()?));
    }
    let plugged = devices.iter().any(|device| {
        device.get::<u32>("Type") == Some(LINE_POWER)
            && device.get("PowerSupply").unwrap_or(false)
            && device.get("Online").unwrap_or(false)
    });
    Ok(State {
        battery: battery(&devices),
        plugged,
    })
}

/// Follows UPower for as long as `updates` is heard: what it reports, or
/// None while it does not run.
pub async fn follow(updates: Sender<Option<State>>) {
    loop {
        if let Err(error) = follow_upower(&updates).await {
            if updates.is_closed() {
                return;
            }
            eprintln!("settings: following UPower: {error}");
            let _ = updates.send(None).await;
            async_io::Timer::after(RETRY).await;
        }
    }
}

/// Follows UPower until it stops. It starts when first asked, so it is
/// missing only when it is not installed: then this waits for it to appear.
async fn follow_upower(updates: &Sender<Option<State>>) -> zbus::Result<()> {
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
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(NAME)?
        .build();
    let mut signals = MessageStream::for_match_rule(rule, &bus, Some(64)).await?;
    let first = match read(&bus).await {
        Err(zbus::Error::MethodError(name, _, _))
            if name.as_str() == "org.freedesktop.DBus.Error.ServiceUnknown" =>
        {
            send(None).await?;
            owners.next().await;
            return Ok(());
        }
        first => first?,
    };
    send(Some(first)).await?;
    loop {
        // The next change, or UPower stopping.
        let changed = async { signals.next().await.is_some() }
            .or(async { owners.next().await.map(|_| false).unwrap_or(false) })
            .await;
        if !changed {
            return Ok(());
        }
        async_io::Timer::after(SETTLE).await;
        while futures_lite::future::poll_once(signals.next())
            .await
            .is_some()
        {}
        send(Some(read(&bus).await?)).await?;
    }
}

/// Reads UPower on a phone and prints it: `cargo test -- --ignored
/// --nocapture reads_upower`.
#[cfg(test)]
#[test]
#[ignore]
fn reads_upower() {
    let state = futures_lite::future::block_on(async {
        read(&Connection::system().await.unwrap()).await.unwrap()
    });
    println!("{state:#?}");
}
