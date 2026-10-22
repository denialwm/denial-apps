//! Bluetooth through BlueZ (`org.bluez` on the system bus). The adapter and
//! its devices are read whole at start and again after every change BlueZ
//! signals; what the user does goes to it through a `Link`.
//!
//! Settings serves its own pairing agent and registers it without asking to
//! be the default one: BlueZ asks the agent of whoever started a pairing, so
//! pairings started here are answered here, in a dialog, and pairings a
//! remote device starts go to Denial's shell, the default agent.

use std::time::Duration;

use async_channel::Sender;
use futures_lite::{FutureExt, StreamExt};
use zbus::fdo::DBusProxy;
use zbus::zvariant::{OwnedObjectPath, Value};
use zbus::{Connection, MatchRule, MessageStream};

use crate::props::{self, Interfaces, get};

const NAME: &str = "org.bluez";
const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";
const BATTERY: &str = "org.bluez.Battery1";
const AGENT_PATH: &str = "/org/denialwm/Settings/BluetoothAgent";
/// A burst of changes is read once, this long after the first. Discovery
/// sends a signal strength change for every advertisement.
const SETTLE: Duration = Duration::from_millis(250);
const RETRY: Duration = Duration::from_secs(2);
/// How often to make sure discovery runs while devices are shown: it stops
/// when the adapter does.
pub const REDISCOVER: Duration = Duration::from_secs(10);

/// What a device is, from the icon name BlueZ gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Headphones,
    Speaker,
    Keyboard,
    Mouse,
    Gamepad,
    Phone,
    Computer,
    Other,
}

impl Kind {
    fn from_icon(icon: &str) -> Self {
        match icon {
            "audio-headset" | "audio-headphones" => Kind::Headphones,
            "audio-card" => Kind::Speaker,
            "input-keyboard" => Kind::Keyboard,
            "input-mouse" | "input-tablet" => Kind::Mouse,
            "input-gaming" => Kind::Gamepad,
            "phone" => Kind::Phone,
            "computer" => Kind::Computer,
            _ => Kind::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Headphones => "Headphones",
            Kind::Speaker => "Speaker",
            Kind::Keyboard => "Keyboard",
            Kind::Mouse => "Mouse",
            Kind::Gamepad => "Game controller",
            Kind::Phone => "Phone",
            Kind::Computer => "Computer",
            Kind::Other => "Device",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Device {
    /// BlueZ's object for it.
    pub path: String,
    pub name: String,
    pub address: String,
    pub kind: Kind,
    pub paired: bool,
    pub connected: bool,
    /// dBm, while discovery hears it.
    pub rssi: Option<i16>,
    /// Percent, when it reports one.
    pub battery: Option<u8>,
}

/// The phone's Bluetooth controller.
#[derive(Clone, Debug)]
pub struct Adapter {
    path: String,
    /// What other devices see the phone as.
    pub name: String,
    pub address: String,
    pub powered: bool,
    /// Off by rfkill: airplane mode or a switch.
    pub blocked: bool,
    pub discovering: bool,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    /// None when BlueZ finds no controller.
    pub adapter: Option<Adapter>,
    /// Paired devices first, by name; then the others discovery hears,
    /// nearest first.
    pub devices: Vec<Device>,
}

impl State {
    pub fn device(&self, path: &str) -> Option<&Device> {
        self.devices.iter().find(|device| device.path == path)
    }

    fn read(objects: &props::Objects) -> Self {
        let adapter = objects.iter().find_map(|(path, interfaces)| {
            let props = interfaces.get(ADAPTER)?;
            let power: Option<String> = get(Some(props), "PowerState");
            Some(Adapter {
                path: path.clone(),
                name: get(Some(props), "Alias").unwrap_or_default(),
                address: get(Some(props), "Address").unwrap_or_default(),
                powered: get(Some(props), "Powered").unwrap_or(false),
                blocked: power.as_deref() == Some("off-blocked"),
                discovering: get(Some(props), "Discovering").unwrap_or(false),
            })
        });
        let Some(adapter) = adapter else {
            return State::default();
        };
        let mut devices: Vec<Device> = objects
            .iter()
            .filter_map(|(path, interfaces)| device(path, interfaces))
            .filter(|device| path_under(&device.path, &adapter.path))
            .collect();
        devices.sort_by(|a, b| {
            let by_name = || a.name.to_lowercase().cmp(&b.name.to_lowercase());
            b.paired.cmp(&a.paired).then_with(|| {
                if a.paired {
                    by_name()
                } else {
                    b.rssi.cmp(&a.rssi).then_with(by_name)
                }
            })
        });
        State {
            adapter: Some(adapter),
            devices,
        }
    }
}

fn path_under(path: &str, parent: &str) -> bool {
    path.strip_prefix(parent)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// A device worth showing: paired, or with a name. Discovery hears plenty
/// of nameless beacons.
fn device(path: &str, interfaces: &Interfaces) -> Option<Device> {
    let props = interfaces.get(DEVICE);
    let paired = get(props, "Paired").unwrap_or(false);
    let name: Option<String> = get(props, "Name");
    let alias: String = get(props, "Alias").unwrap_or_default();
    if !paired && name.is_none() {
        return None;
    }
    Some(Device {
        path: path.to_owned(),
        name: alias,
        address: get(props, "Address").unwrap_or_default(),
        kind: Kind::from_icon(&get::<String>(props, "Icon").unwrap_or_default()),
        paired,
        connected: get(props, "Connected").unwrap_or(false),
        rssi: get(props, "RSSI"),
        battery: get(interfaces.get(BATTERY), "Percentage"),
    })
}

/// What pairing asks the user, through Settings' agent.
#[derive(Clone, Debug)]
pub enum Ask {
    /// Whether both devices show `passkey`.
    Confirm(u32),
    /// Whether to pair at all, with nothing to compare.
    Authorize,
    /// A PIN to type, for older devices: often 0000.
    PinCode,
    /// A six-digit passkey to type, as the other device shows it.
    Passkey,
    /// A code to type on the other device; nothing to answer.
    Show(String),
    /// BlueZ took back what it asked.
    Cancel,
}

/// One question from the agent, about the device at `device`, and where
/// its answer goes: a code or yes, or None for no.
#[derive(Clone, Debug)]
pub struct Request {
    pub device: String,
    pub ask: Ask,
    pub reply: Option<Sender<Option<String>>>,
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Rejected(String),
    Canceled(String),
}

struct Agent {
    requests: Sender<Request>,
}

impl Agent {
    /// Hands `ask` to the window and waits for its answer.
    async fn ask(&self, device: OwnedObjectPath, ask: Ask) -> Result<String, AgentError> {
        let (reply, answer) = async_channel::bounded(1);
        let request = Request {
            device: device.to_string(),
            ask,
            reply: Some(reply),
        };
        self.requests
            .send(request)
            .await
            .map_err(|_| AgentError::Canceled("Settings is closing".into()))?;
        match answer.recv().await {
            Ok(Some(answer)) => Ok(answer),
            Ok(None) => Err(AgentError::Rejected("Turned down".into())),
            Err(_) => Err(AgentError::Canceled("Not answered".into())),
        }
    }

    async fn show(&self, device: OwnedObjectPath, ask: Ask) {
        let _ = self
            .requests
            .send(Request {
                device: device.to_string(),
                ask,
                reply: None,
            })
            .await;
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {}

    async fn request_pin_code(&self, device: OwnedObjectPath) -> Result<String, AgentError> {
        self.ask(device, Ask::PinCode).await
    }

    async fn display_pin_code(&self, device: OwnedObjectPath, code: String) {
        self.show(device, Ask::Show(code)).await
    }

    async fn request_passkey(&self, device: OwnedObjectPath) -> Result<u32, AgentError> {
        let typed = self.ask(device, Ask::Passkey).await?;
        typed
            .trim()
            .parse()
            .map_err(|_| AgentError::Rejected("Not a passkey".into()))
    }

    async fn display_passkey(&self, device: OwnedObjectPath, passkey: u32, _entered: u16) {
        self.show(device, Ask::Show(format!("{passkey:06}"))).await
    }

    async fn request_confirmation(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
    ) -> Result<(), AgentError> {
        self.ask(device, Ask::Confirm(passkey)).await.map(drop)
    }

    async fn request_authorization(&self, device: OwnedObjectPath) -> Result<(), AgentError> {
        self.ask(device, Ask::Authorize).await.map(drop)
    }

    /// Only for devices this agent paired: they may use what they paired
    /// for.
    fn authorize_service(&self, _device: OwnedObjectPath, _uuid: String) {}

    async fn cancel(&self) {
        let _ = self
            .requests
            .send(Request {
                device: String::new(),
                ask: Ask::Cancel,
                reply: None,
            })
            .await;
    }
}

/// Why pairing or connecting did not work.
pub fn failure(error: &zbus::Error) -> &'static str {
    let zbus::Error::MethodError(name, said, _) = error else {
        return "BlueZ did not answer";
    };
    let said = said.as_deref().unwrap_or_default();
    match name.as_str() {
        "org.bluez.Error.AuthenticationFailed" => "The codes did not match",
        "org.bluez.Error.AuthenticationCanceled" | "org.bluez.Error.AuthenticationRejected" => {
            "Pairing was turned down"
        }
        "org.bluez.Error.AuthenticationTimeout" => "Pairing took too long",
        "org.bluez.Error.ConnectionAttemptFailed" => "The device did not answer",
        "org.bluez.Error.NotReady" => "Bluetooth is off",
        "org.bluez.Error.InProgress" => "Already on its way",
        "org.bluez.Error.NotAvailable" | "org.bluez.Error.NotSupported" => {
            "The device offers nothing the phone can use"
        }
        _ if said.contains("page-timeout") || said.contains("Host is down") => {
            "The device did not answer"
        }
        _ if name.as_str().ends_with("NotAuthorized")
            || name.as_str().ends_with("AccessDenied") =>
        {
            "not allowed"
        }
        _ => "It did not work",
    }
}

/// The way to BlueZ, with Settings' agent registered on the same
/// connection, so pairings started through it are answered here.
#[derive(Clone)]
pub struct Link {
    bus: Connection,
}

impl Link {
    async fn call<B>(&self, path: &str, interface: &str, method: &str, body: &B) -> zbus::Result<()>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        self.bus
            .call_method(Some(NAME), path, Some(interface), method, body)
            .await?;
        Ok(())
    }

    async fn set(
        &self,
        path: &str,
        interface: &str,
        name: &str,
        value: Value<'_>,
    ) -> zbus::Result<()> {
        self.call(
            path,
            "org.freedesktop.DBus.Properties",
            "Set",
            &(interface, name, value),
        )
        .await
    }

    pub async fn set_powered(&self, adapter: &Adapter, on: bool) -> zbus::Result<()> {
        self.set(&adapter.path, ADAPTER, "Powered", on.into()).await
    }

    /// Renames the phone as other devices see it.
    pub async fn set_name(&self, adapter: &Adapter, name: &str) -> zbus::Result<()> {
        self.set(&adapter.path, ADAPTER, "Alias", name.into()).await
    }

    /// Looks for devices until `stop_looking` or Settings closes. Asking
    /// again while it runs is fine.
    pub async fn look(&self, adapter: &Adapter) -> zbus::Result<()> {
        match self
            .call(&adapter.path, ADAPTER, "StartDiscovery", &())
            .await
        {
            Err(zbus::Error::MethodError(name, _, _))
                if name.as_str() == "org.bluez.Error.InProgress" =>
            {
                Ok(())
            }
            result => result,
        }
    }

    pub async fn stop_looking(&self, adapter: &Adapter) {
        let _ = self
            .call(&adapter.path, ADAPTER, "StopDiscovery", &())
            .await;
    }

    /// Pairs with `device`, trusts it so it may come back by itself, and
    /// connects it. The agent asks what pairing needs meanwhile.
    pub async fn pair(&self, device: &Device) -> zbus::Result<()> {
        match self.call(&device.path, DEVICE, "Pair", &()).await {
            Err(zbus::Error::MethodError(name, _, _))
                if name.as_str() == "org.bluez.Error.AlreadyExists" => {}
            result => result?,
        }
        self.set(&device.path, DEVICE, "Trusted", true.into())
            .await?;
        self.connect(device).await
    }

    pub async fn cancel_pairing(&self, device: &Device) {
        let _ = self.call(&device.path, DEVICE, "CancelPairing", &()).await;
    }

    pub async fn connect(&self, device: &Device) -> zbus::Result<()> {
        self.call(&device.path, DEVICE, "Connect", &()).await
    }

    pub async fn disconnect(&self, device: &Device) -> zbus::Result<()> {
        self.call(&device.path, DEVICE, "Disconnect", &()).await
    }

    /// Unpairs `device` and forgets it.
    pub async fn forget(&self, adapter: &Adapter, device: &Device) -> zbus::Result<()> {
        let path = OwnedObjectPath::try_from(device.path.as_str())?;
        self.call(&adapter.path, ADAPTER, "RemoveDevice", &(path,))
            .await
    }
}

/// Follows BlueZ for as long as `updates` is heard: what it reports, or
/// None while it does not run. The agent's questions go to `requests`.
pub async fn follow(updates: Sender<Option<(Link, State)>>, requests: Sender<Request>) {
    loop {
        if let Err(error) = follow_bluez(&updates, &requests).await {
            if updates.is_closed() {
                return;
            }
            eprintln!("settings: following BlueZ: {error}");
            let _ = updates.send(None).await;
            async_io::Timer::after(RETRY).await;
        }
    }
}

/// Follows BlueZ until it starts or stops.
async fn follow_bluez(
    updates: &Sender<Option<(Link, State)>>,
    requests: &Sender<Request>,
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
    if !dbus.name_has_owner(NAME.try_into()?).await? {
        send(None).await?;
        owners.next().await;
        return Ok(());
    }
    let agent = Agent {
        requests: requests.clone(),
    };
    bus.object_server().at(AGENT_PATH, agent).await?;
    let path = OwnedObjectPath::try_from(AGENT_PATH)?;
    let registered = bus
        .call_method(
            Some(NAME),
            "/org/bluez",
            Some("org.bluez.AgentManager1"),
            "RegisterAgent",
            &(path, "KeyboardDisplay"),
        )
        .await;
    if let Err(error) = registered {
        // Pairing then works for devices that ask nothing.
        eprintln!("settings: registering the Bluetooth agent: {error}");
    }
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(NAME)?
        .build();
    let mut signals = MessageStream::for_match_rule(rule, &bus, Some(1024)).await?;
    let link = Link { bus };
    loop {
        let objects = props::objects(&link.bus, NAME, "/").await?;
        send(Some((link.clone(), State::read(&objects)))).await?;
        // The next change, or BlueZ starting or stopping.
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

/// Reads BlueZ and prints what Settings shows: `cargo test -- --ignored
/// --nocapture reads_bluez`. Changes nothing.
#[cfg(test)]
#[test]
#[ignore]
fn reads_bluez() {
    let (updates, receiver) = async_channel::unbounded();
    let (requests, _) = async_channel::unbounded();
    futures_lite::future::block_on(
        async {
            follow_bluez(&updates, &requests).await.unwrap();
        }
        .or(async {
            async_io::Timer::after(Duration::from_secs(2)).await;
        }),
    );
    while let Ok(update) = receiver.try_recv() {
        println!("{:#?}", update.map(|(_, state)| state));
    }
}
