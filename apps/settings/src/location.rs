//! The phone's GPS through dmail's `org.dmail.Location1` (system bus,
//! phonewave/services/modem/dmail/DESIGN.md). dmail runs the engine only
//! while a client asks: Settings asks through a `Link` while the user wants
//! the page to locate the phone, and dmail lets go once Settings stops or
//! leaves the bus.

use std::collections::HashMap;
use std::time::Duration;

use async_channel::Sender;
use futures_lite::{FutureExt, StreamExt};
use zbus::fdo::DBusProxy;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, MatchRule, MessageStream};

const NAME: &str = "org.dmail";
const PATH: &str = "/org/dmail";
const LOCATION: &str = "org.dmail.Location1";
/// A burst of changes is read once, this long after the first.
const SETTLE: Duration = Duration::from_millis(150);
const RETRY: Duration = Duration::from_secs(2);

/// Where the GPS is, as dmail names it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Gps {
    /// No engine, or the modem failed.
    #[default]
    Unavailable,
    /// Nobody asks for location.
    Off,
    Searching,
    Fixed,
}

impl Gps {
    fn parse(state: &str) -> Self {
        match state {
            "off" => Gps::Off,
            "searching" => Gps::Searching,
            "fixed" => Gps::Fixed,
            _ => Gps::Unavailable,
        }
    }
}

/// A position from the engine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fix {
    pub latitude: f64,
    pub longitude: f64,
    /// Metres.
    pub accuracy: Option<f64>,
    /// Metres a second.
    pub speed: Option<f64>,
    /// Degrees from true north.
    pub heading: Option<f64>,
    /// Unix seconds.
    pub time: i64,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub gps: Gps,
    /// The last fix of this session.
    pub fix: Option<Fix>,
    /// Satellites in the last fix.
    pub used: u32,
    /// Satellites the engine hears now.
    pub tracked: u32,
}

/// The way to dmail's location.
#[derive(Clone)]
pub struct Link {
    bus: Connection,
}

impl Link {
    /// Asks dmail to run the GPS until `stop`, or until Settings leaves the
    /// bus.
    pub async fn start(&self) -> zbus::Result<()> {
        self.call("Start").await
    }

    pub async fn stop(&self) -> zbus::Result<()> {
        self.call("Stop").await
    }

    async fn call(&self, method: &str) -> zbus::Result<()> {
        self.bus
            .call_method(Some(NAME), PATH, Some(LOCATION), method, &())
            .await
            .map(drop)
    }

    /// None when this dmail has no location.
    async fn read(&self) -> zbus::Result<Option<State>> {
        let reply = self
            .bus
            .call_method(
                Some(NAME),
                PATH,
                Some("org.freedesktop.DBus.Properties"),
                "GetAll",
                &(LOCATION,),
            )
            .await;
        let props = match reply {
            Ok(reply) => reply.body().deserialize::<HashMap<String, OwnedValue>>()?,
            // A dmail from before location.
            Err(zbus::Error::MethodError(..)) => return Ok(None),
            Err(error) => return Err(error),
        };
        fn get<T: TryFrom<OwnedValue>>(
            props: &HashMap<String, OwnedValue>,
            name: &str,
        ) -> Option<T> {
            props
                .get(name)
                .and_then(|value| value.try_clone().ok()?.try_into().ok())
        }
        let position: Option<(f64, f64, f64, f64, f64, f64, i64)> = get(&props, "Position");
        Ok(Some(State {
            gps: Gps::parse(&get::<String>(&props, "State").unwrap_or_default()),
            fix: position.and_then(fix),
            used: get(&props, "SatellitesUsed").unwrap_or(0),
            tracked: get(&props, "SatellitesTracked").unwrap_or(0),
        }))
    }
}

/// dmail's position, NaN where the fix lacks something.
fn fix(
    (latitude, longitude, accuracy, _altitude, speed, heading, time): (
        f64,
        f64,
        f64,
        f64,
        f64,
        f64,
        i64,
    ),
) -> Option<Fix> {
    let known = |value: f64| value.is_finite().then_some(value);
    Some(Fix {
        latitude: known(latitude)?,
        longitude: known(longitude)?,
        accuracy: known(accuracy),
        speed: known(speed),
        heading: known(heading),
        time: time / 1000,
    })
}

/// Follows dmail's location for as long as `updates` is heard: what it
/// reports, or None while dmail does not run or has no location.
pub async fn follow(updates: Sender<Option<(Link, State)>>) {
    loop {
        if let Err(error) = follow_dmail(&updates).await {
            if updates.is_closed() {
                return;
            }
            eprintln!("settings: following dmail's location: {error}");
            let _ = updates.send(None).await;
            async_io::Timer::after(RETRY).await;
        }
    }
}

/// Follows dmail until it starts or stops.
async fn follow_dmail(updates: &Sender<Option<(Link, State)>>) -> zbus::Result<()> {
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
    if !dbus.name_has_owner(NAME.try_into()?).await? {
        send(None).await?;
        owners.next().await;
        return Ok(());
    }
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(NAME)?
        .path(PATH)?
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .arg(0, LOCATION)?
        .build();
    let mut signals = MessageStream::for_match_rule(rule, &bus, Some(64)).await?;
    let link = Link { bus };
    loop {
        let update = link.read().await?.map(|state| (link.clone(), state));
        let present = update.is_some();
        send(update).await?;
        if !present {
            // Nothing to follow until another dmail starts.
            owners.next().await;
            return Ok(());
        }
        let changed = async { signals.next().await.is_some() }
            .or(async { owners.next().await.map(|_| false).unwrap_or(false) })
            .await;
        if !changed {
            return Ok(());
        }
        async_io::Timer::after(SETTLE).await;
        while let Some(Some(_)) = futures_lite::future::poll_once(signals.next()).await {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_position() {
        let nan = f64::NAN;
        assert_eq!(fix((nan, nan, nan, nan, nan, nan, 0)), None);
        let found = fix((45.5, 9.2, 12.0, nan, 0.4, nan, 1_795_000_000_500)).unwrap();
        assert_eq!(found.accuracy, Some(12.0));
        assert_eq!(found.heading, None);
        assert_eq!(found.time, 1_795_000_000);
    }
}

/// Reads dmail's location on a phone for a while and prints it, with the
/// GPS asked for: `cargo test -- --ignored --nocapture reads_location`.
#[cfg(test)]
#[test]
#[ignore]
fn reads_location() {
    let (updates, receiver) = async_channel::unbounded();
    futures_lite::future::block_on(
        async {
            follow_dmail(&updates).await.unwrap();
        }
        .or(async {
            let started = async {
                while let Ok(update) = receiver.recv().await {
                    println!("{:?}", update.as_ref().map(|(_, state)| state));
                    if let Some((link, _)) = update {
                        return link;
                    }
                }
                unreachable!("no location");
            };
            let link = started.await;
            link.start().await.unwrap();
            let until = std::time::Instant::now() + Duration::from_secs(40);
            while std::time::Instant::now() < until {
                if let Ok(Some((_, state))) = receiver
                    .recv()
                    .or(async {
                        async_io::Timer::after(Duration::from_secs(1)).await;
                        Err(async_channel::RecvError)
                    })
                    .await
                {
                    println!("{state:?}");
                }
            }
            link.stop().await.unwrap();
        }),
    );
}
