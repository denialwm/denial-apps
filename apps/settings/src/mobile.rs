//! The mobile network through dmail, Phonewave's modem service (`org.dmail`
//! on the system bus, phonewave/services/modem/dmail/DESIGN.md). Its modem
//! and data are read whole at start and again after every change it
//! signals; what the user does goes to it through a `Link`.

use std::time::Duration;

use async_channel::Sender;
use futures_lite::{FutureExt, StreamExt};
use zbus::fdo::DBusProxy;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, MatchRule, MessageStream};

const NAME: &str = "org.dmail";
const PATH: &str = "/org/dmail";
const MODEM: &str = "org.dmail.Modem1";
const DATA: &str = "org.dmail.Data1";
/// A burst of changes is read once, this long after the first.
const SETTLE: Duration = Duration::from_millis(150);
const RETRY: Duration = Duration::from_secs(2);

/// Where the modem is, as dmail names it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Modem {
    #[default]
    Off,
    Starting,
    NoSim,
    /// The SIM waits for its PIN.
    Locked,
    Searching,
    Registered,
    Failed,
}

impl Modem {
    fn parse(state: &str) -> Self {
        match state {
            "starting" => Modem::Starting,
            "no-sim" => Modem::NoSim,
            "locked" => Modem::Locked,
            "searching" => Modem::Searching,
            "registered" => Modem::Registered,
            "failed" => Modem::Failed,
            _ => Modem::Off,
        }
    }
}

/// Where mobile data is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Data {
    #[default]
    Off,
    Connecting,
    Connected,
    /// The network turned the session down; dmail tries again once packet
    /// service comes back.
    Failed,
}

impl Data {
    fn parse(state: &str) -> Self {
        match state {
            "connecting" => Data::Connecting,
            "connected" => Data::Connected,
            "failed" => Data::Failed,
            _ => Data::Off,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub modem: Modem,
    /// none, 2g, 3g, 4g or 5g.
    pub technology: String,
    /// Percent.
    pub signal: u8,
    pub operator: String,
    pub roaming: bool,
    pub pin_retries: u32,
    /// 1 or 2: the SIM for SMS and data.
    pub sim_slot: u8,
    /// The SIM's own number as it holds it; empty when it holds none.
    pub number: String,
    /// Registered with packet service: data can start.
    pub attached: bool,
    /// The last registration the network refused, in dmail's words, and
    /// when (Unix time, 0 for none).
    pub last_reject: (String, i64),
    /// Whether mobile data is on, and where its session is.
    pub data_enabled: bool,
    pub data: Data,
    pub interface: String,
    pub apn: String,
    /// "10.0.0.2/30", while connected.
    pub address: String,
    pub dns: Vec<String>,
    pub mtu: u32,
    /// When the session came up, Unix time; 0 unless connected.
    pub since: i64,
    /// When dmail tries again, Unix time; 0 for no waiting try.
    pub retry_at: i64,
    /// Sessions the network ended since dmail started.
    pub drops: u32,
    /// Why the last session ended or did not start, and when.
    pub last_failure: (String, i64),
}

impl State {
    /// The network type as people say it: "4G", or none.
    pub fn technology(&self) -> Option<String> {
        (self.technology != "none" && !self.technology.is_empty())
            .then(|| self.technology.to_uppercase())
    }
}

/// Why dmail said no.
pub enum Refusal {
    WrongPin,
    /// Anything else, as dmail put it.
    Other(String),
}

impl From<zbus::Error> for Refusal {
    fn from(error: zbus::Error) -> Self {
        match error {
            zbus::Error::MethodError(name, said, _) => {
                if name.as_str() == "org.dmail.Error.WrongPin" {
                    Refusal::WrongPin
                } else {
                    Refusal::Other(said.unwrap_or_else(|| name.to_string()))
                }
            }
            error => Refusal::Other(error.to_string()),
        }
    }
}

/// The way to dmail.
#[derive(Clone)]
pub struct Link {
    bus: Connection,
}

impl Link {
    async fn call<B>(&self, interface: &str, method: &str, body: &B) -> Result<(), Refusal>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        self.bus
            .call_method(Some(NAME), PATH, Some(interface), method, body)
            .await?;
        Ok(())
    }

    pub async fn set_data(&self, on: bool) -> Result<(), Refusal> {
        self.call(DATA, "SetEnabled", &(on,)).await
    }

    /// One attempt; dmail counts the tries left.
    pub async fn unlock(&self, pin: String) -> Result<(), Refusal> {
        self.call(MODEM, "UnlockPin", &(pin,)).await
    }

    /// Moves SMS and data to the SIM in `slot`, at once.
    pub async fn set_sim_slot(&self, slot: u8) -> Result<(), Refusal> {
        self.call(MODEM, "SetSimSlot", &(slot,)).await
    }

    /// A new data session at once, also after the network refused one.
    pub async fn reconnect(&self) -> Result<(), Refusal> {
        self.call(DATA, "Reconnect", &()).await
    }

    /// Radio off and on: the phone registers anew. Answers once the radio
    /// is back on.
    pub async fn restart_radio(&self) -> Result<(), Refusal> {
        self.call(MODEM, "RestartRadio", &()).await
    }

    /// dmail sets the modem up again from the start.
    pub async fn restart(&self) -> Result<(), Refusal> {
        self.call(MODEM, "Restart", &()).await
    }

    async fn read(&self) -> zbus::Result<State> {
        let all = |interface: &'static str| async move {
            let reply = self
                .bus
                .call_method(
                    Some(NAME),
                    PATH,
                    Some("org.freedesktop.DBus.Properties"),
                    "GetAll",
                    &(interface,),
                )
                .await?;
            reply
                .body()
                .deserialize::<std::collections::HashMap<String, OwnedValue>>()
        };
        let (modem, data) = (all(MODEM).await?, all(DATA).await?);
        fn get<T: TryFrom<OwnedValue>>(
            props: &std::collections::HashMap<String, OwnedValue>,
            name: &str,
        ) -> Option<T> {
            props
                .get(name)
                .and_then(|value| value.try_clone().ok()?.try_into().ok())
        }
        Ok(State {
            modem: Modem::parse(&get::<String>(&modem, "State").unwrap_or_default()),
            technology: get(&modem, "Technology").unwrap_or_default(),
            signal: get(&modem, "Signal").unwrap_or(0),
            operator: get(&modem, "Operator").unwrap_or_default(),
            roaming: get(&modem, "Roaming").unwrap_or(false),
            pin_retries: get(&modem, "PinRetries").unwrap_or(0),
            sim_slot: get(&modem, "SimSlot").unwrap_or(1),
            number: get(&modem, "Number").unwrap_or_default(),
            attached: get(&modem, "Attached").unwrap_or(false),
            last_reject: get(&modem, "LastReject").unwrap_or_default(),
            data_enabled: get(&data, "Enabled").unwrap_or(false),
            data: Data::parse(&get::<String>(&data, "State").unwrap_or_default()),
            interface: get(&data, "Interface").unwrap_or_default(),
            apn: get(&data, "Apn").unwrap_or_default(),
            address: get(&data, "Address").unwrap_or_default(),
            dns: get(&data, "Dns").unwrap_or_default(),
            mtu: get(&data, "Mtu").unwrap_or(0),
            since: get(&data, "Since").unwrap_or(0),
            retry_at: get(&data, "RetryAt").unwrap_or(0),
            drops: get(&data, "Drops").unwrap_or(0),
            last_failure: get(&data, "LastFailure").unwrap_or_default(),
        })
    }
}

/// Bytes received and sent on `interface` since it came up.
pub fn traffic(interface: &str) -> Option<(u64, u64)> {
    if interface.is_empty() || interface.contains('/') {
        return None;
    }
    let read = |what: &str| {
        let path = format!("/sys/class/net/{interface}/statistics/{what}");
        std::fs::read_to_string(path).ok()?.trim().parse().ok()
    };
    Some((read("rx_bytes")?, read("tx_bytes")?))
}

/// Follows dmail for as long as `updates` is heard: what it reports, or
/// None while it does not run.
pub async fn follow(updates: Sender<Option<(Link, State)>>) {
    loop {
        if let Err(error) = follow_dmail(&updates).await {
            if updates.is_closed() {
                return;
            }
            eprintln!("settings: following dmail: {error}");
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
        .build();
    let mut signals = MessageStream::for_match_rule(rule, &bus, Some(64)).await?;
    let link = Link { bus };
    loop {
        send(Some((link.clone(), link.read().await?))).await?;
        // The next change, or dmail starting or stopping.
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
    }
}

/// Reads dmail on a phone and prints it: `cargo test -- --ignored
/// --nocapture reads_dmail`.
#[cfg(test)]
#[test]
#[ignore]
fn reads_dmail() {
    let (updates, receiver) = async_channel::unbounded();
    futures_lite::future::block_on(
        async {
            follow_dmail(&updates).await.unwrap();
        }
        .or(async {
            async_io::Timer::after(Duration::from_secs(2)).await;
        }),
    );
    while let Ok(update) = receiver.try_recv() {
        println!("{:#?}", update.map(|(_, state)| state));
    }
}
