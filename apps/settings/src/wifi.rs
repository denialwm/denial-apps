//! Wi-Fi through whichever manager the phone runs: NetworkManager when its
//! name is on the system bus, since it may drive iwd itself, else iwd
//! directly. Either one is read whole into a `State` at start and again
//! after every change it signals; what the user does goes to it through a
//! `Link`. Denial's shell chooses between the two the same way.

mod iwd;
mod kernel;
mod nm;

use std::time::Duration;

use async_channel::Sender;
use futures_lite::{FutureExt, StreamExt};
use zbus::fdo::DBusProxy;
use zbus::{Connection, MatchRule, MessageStream};

/// Which program manages Wi-Fi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Manager {
    NetworkManager,
    Iwd,
}

impl Manager {
    pub fn label(self) -> &'static str {
        match self {
            Manager::NetworkManager => "NetworkManager",
            Manager::Iwd => "iwd",
        }
    }
}

/// How a network keeps strangers out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    Open,
    /// Open but encrypted: Wi-Fi Enhanced Open.
    Owe,
    Wep,
    /// A shared password: WPA2, or WPA2 and WPA3 together.
    Personal,
    /// A shared password, WPA3 only.
    Sae,
    /// A user name or a certificate per person: 802.1X.
    Enterprise,
}

impl Security {
    pub fn label(self) -> &'static str {
        match self {
            Security::Open => "Open",
            Security::Owe => "Enhanced open",
            Security::Wep => "WEP",
            Security::Personal => "WPA2",
            Security::Sae => "WPA3",
            Security::Enterprise => "Enterprise",
        }
    }

    /// Joining it takes a password.
    pub fn password(self) -> bool {
        matches!(self, Security::Wep | Security::Personal | Security::Sae)
    }

    /// The same kind of network to a saved connection: a WPA2 network saved
    /// earlier still joins once it also offers WPA3.
    fn joins(self, other: Security) -> bool {
        let family = |security| match security {
            Security::Personal | Security::Sae => Security::Personal,
            security => security,
        };
        family(self) == family(other)
    }
}

/// A network in range, the strongest of its access points.
#[derive(Clone, Debug)]
pub struct Network {
    /// The name as the network sends it: bytes, not always UTF-8.
    pub ssid: Vec<u8>,
    pub name: String,
    pub security: Security,
    /// Percent.
    pub strength: u8,
    /// The saved network it joins as, by `Saved::id`.
    pub saved: Option<String>,
    /// The manager's object for it: an access point or an iwd network.
    path: String,
}

/// Whether a network is billed by the amount of data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metered {
    Auto,
    Yes,
    No,
}

/// The hardware address the phone shows a network.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Address {
    /// What NetworkManager is set to do for every network.
    Default,
    /// A made-up address, the same for this network every time.
    Random,
    /// The phone's own.
    Device,
}

/// A network the phone remembers, and joins by itself unless told not to.
#[derive(Clone, Debug)]
pub struct Saved {
    /// The manager's object for it.
    pub id: String,
    pub name: String,
    pub ssid: Vec<u8>,
    pub security: Security,
    pub autoconnect: bool,
    pub hidden: bool,
    /// None where the manager cannot say: iwd.
    pub metered: Option<Metered>,
    /// None where the manager cannot say: iwd.
    pub address: Option<Address>,
}

/// How far joining has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Connecting,
    Authenticating,
    Addressing,
    Connected,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Connecting => "Connecting…",
            Phase::Authenticating => "Checking the password…",
            Phase::Addressing => "Getting an address…",
            Phase::Connected => "Connected",
        }
    }
}

/// The network the phone is on or joining.
#[derive(Clone, Debug, Default)]
pub struct Current {
    pub ssid: Vec<u8>,
    pub name: String,
    /// The saved network, by `Saved::id`.
    pub saved: Option<String>,
    pub phase: Option<Phase>,
    pub security: Option<Security>,
    pub strength: Option<u8>,
    pub frequency: Option<u32>,
    /// Mb/s.
    pub bitrate: Option<u32>,
    /// Address and prefix length, as 192.168.1.22/24.
    pub ipv4: Option<String>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    pub ipv6: Vec<String>,
}

/// The phone's Wi-Fi interface.
#[derive(Clone, Debug)]
pub struct Device {
    path: String,
    pub address: String,
}

/// Everything about Wi-Fi at one moment.
#[derive(Clone, Debug, Default)]
pub struct State {
    /// None while neither manager runs.
    pub manager: Option<Manager>,
    /// None without Wi-Fi hardware.
    pub device: Option<Device>,
    pub enabled: bool,
    /// Switched off where software cannot switch it on: rfkill.
    pub blocked: bool,
    pub scanning: bool,
    /// In range, saved ones first, then the strongest first; the current
    /// network is not among them.
    pub networks: Vec<Network>,
    pub saved: Vec<Saved>,
    pub current: Option<Current>,
}

impl State {
    pub fn saved(&self, id: &str) -> Option<&Saved> {
        self.saved.iter().find(|saved| saved.id == id)
    }

    /// The saved network `id` in range, if it is.
    pub fn in_range(&self, id: &str) -> Option<&Network> {
        self.networks
            .iter()
            .find(|network| network.saved.as_deref() == Some(id))
    }

    /// Puts networks in their order, marks the saved ones, and takes the
    /// current one out.
    fn settle(&mut self) {
        let saved = &self.saved;
        for network in &mut self.networks {
            network.saved = saved
                .iter()
                .find(|saved| saved.ssid == network.ssid && saved.security.joins(network.security))
                .map(|saved| saved.id.clone());
        }
        if let Some(current) = &self.current {
            self.networks.retain(|network| network.ssid != current.ssid);
        }
        self.networks
            .sort_by_key(|network| (network.saved.is_none(), std::cmp::Reverse(network.strength)));
    }
}

/// Keeps the strongest access point of each network, by name and kind,
/// leaving out hidden ones, which have no name.
fn strongest(points: impl IntoIterator<Item = Network>) -> Vec<Network> {
    let mut networks: Vec<Network> = Vec::new();
    for point in points.into_iter().filter(|point| !point.ssid.is_empty()) {
        match networks
            .iter_mut()
            .find(|network| network.ssid == point.ssid && network.security.joins(point.security))
        {
            Some(network) if network.strength < point.strength => *network = point,
            Some(_) => {}
            None => networks.push(point),
        }
    }
    networks
}

/// Why joining a network did not work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    WrongPassword,
    /// The phone cannot join this kind of network yet.
    Unsupported,
    NotInRange,
    Other(&'static str),
}

impl Failure {
    pub fn label(&self) -> &'static str {
        match self {
            Failure::WrongPassword => "Wrong password",
            Failure::Unsupported => "This kind of network is not supported yet",
            Failure::NotInRange => "The network is not in range",
            Failure::Other(why) => why,
        }
    }
}

/// What the app hears from `follow`.
pub enum Update {
    /// A manager to talk to, or none.
    Link(Option<Link>),
    State(State),
}

/// The way to the manager in charge.
#[derive(Clone)]
pub enum Link {
    NetworkManager(nm::Nm),
    Iwd(iwd::Iwd),
}

impl Link {
    pub async fn set_enabled(&self, on: bool) -> zbus::Result<()> {
        match self {
            Link::NetworkManager(nm) => nm.set_enabled(on).await,
            Link::Iwd(iwd) => iwd.set_enabled(on).await,
        }
    }

    /// Asks for a fresh look around. A manager scanning already says no,
    /// which is fine.
    pub async fn scan(&self, device: &Device) {
        let asked = match self {
            Link::NetworkManager(nm) => nm.scan(device).await,
            Link::Iwd(iwd) => iwd.scan(device).await,
        };
        if let Err(error) = asked {
            eprintln!("settings: Wi-Fi scan: {error}");
        }
    }

    /// Joins `network`: a saved one as it is saved, or with `password` in
    /// place of its old one; a new one is saved once it is joined. Ends when
    /// the phone is on the network or could not get on it.
    pub async fn connect(
        &self,
        device: &Device,
        network: &Network,
        password: Option<String>,
    ) -> Result<(), Failure> {
        if network.security == Security::Enterprise {
            return Err(Failure::Unsupported);
        }
        match self {
            Link::NetworkManager(nm) => nm.connect(device, network, password).await,
            Link::Iwd(iwd) => iwd.connect(network, password).await,
        }
    }

    /// Joins a network that does not say its name, and saves it once joined.
    pub async fn connect_hidden(
        &self,
        device: &Device,
        name: &str,
        password: Option<String>,
    ) -> Result<(), Failure> {
        match self {
            Link::NetworkManager(nm) => nm.connect_hidden(device, name, password).await,
            Link::Iwd(iwd) => iwd.connect_hidden(device, name, password).await,
        }
    }

    /// Leaves the current network. The phone does not join another by
    /// itself until the user picks one.
    pub async fn disconnect(&self, device: &Device) -> zbus::Result<()> {
        match self {
            Link::NetworkManager(nm) => nm.disconnect(device).await,
            Link::Iwd(iwd) => iwd.disconnect(device).await,
        }
    }

    pub async fn forget(&self, saved: &Saved) -> zbus::Result<()> {
        match self {
            Link::NetworkManager(nm) => nm.forget(&saved.id).await,
            Link::Iwd(iwd) => iwd.forget(&saved.id).await,
        }
    }

    pub async fn set_autoconnect(&self, saved: &Saved, on: bool) -> zbus::Result<()> {
        match self {
            Link::NetworkManager(nm) => nm.set_autoconnect(&saved.id, on).await,
            Link::Iwd(iwd) => iwd.set_autoconnect(&saved.id, on).await,
        }
    }

    /// Only NetworkManager keeps it; `Saved::metered` is None elsewhere.
    pub async fn set_metered(&self, saved: &Saved, metered: Metered) -> zbus::Result<()> {
        match self {
            Link::NetworkManager(nm) => nm.set_metered(&saved.id, metered).await,
            Link::Iwd(_) => Ok(()),
        }
    }

    /// Only NetworkManager keeps it; `Saved::address` is None elsewhere.
    pub async fn set_address(&self, saved: &Saved, address: Address) -> zbus::Result<()> {
        match self {
            Link::NetworkManager(nm) => nm.set_address(&saved.id, address).await,
            Link::Iwd(_) => Ok(()),
        }
    }
}

const NETWORK_MANAGER: &str = "org.freedesktop.NetworkManager";
const IWD: &str = "net.connman.iwd";
/// A burst of signals is read once, this long after its first.
const SETTLE: Duration = Duration::from_millis(150);
/// Before trying the system bus again after it failed.
const RETRY: Duration = Duration::from_secs(2);
/// How often to look for networks while they are shown: managers keep only
/// what their last scan found, and scan seldom on their own.
pub const RESCAN: Duration = Duration::from_secs(10);

/// Follows Wi-Fi until `updates` closes, across either manager stopping,
/// starting or being replaced by the other.
pub async fn follow(updates: Sender<Update>) {
    while !updates.is_closed() {
        if let Err(error) = follow_manager(&updates).await {
            eprintln!("settings: Wi-Fi: {error}");
            let _ = updates.send(Update::Link(None)).await;
            async_io::Timer::after(RETRY).await;
        }
    }
}

/// Follows the manager in charge now, until another takes over or it goes.
async fn follow_manager(updates: &Sender<Update>) -> zbus::Result<()> {
    let bus = Connection::system().await?;
    let dbus = DBusProxy::new(&bus).await?;
    let owners = dbus
        .receive_name_owner_changed_with_args(&[(0, NETWORK_MANAGER)])
        .await?
        .or(dbus
            .receive_name_owner_changed_with_args(&[(0, IWD)])
            .await?);
    let mut owners = std::pin::pin!(owners);
    let manager = if dbus.name_has_owner(NETWORK_MANAGER.try_into()?).await? {
        Some(Manager::NetworkManager)
    } else if dbus.name_has_owner(IWD.try_into()?).await? {
        Some(Manager::Iwd)
    } else {
        None
    };
    let (link, sender) = match manager {
        Some(Manager::NetworkManager) => (
            Link::NetworkManager(nm::Nm::new(bus.clone())),
            NETWORK_MANAGER,
        ),
        Some(Manager::Iwd) => (Link::Iwd(iwd::Iwd::new(bus.clone()).await?), IWD),
        None => {
            send(updates, Update::Link(None)).await?;
            send(updates, Update::State(State::default())).await?;
            owners.next().await;
            return Ok(());
        }
    };
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(sender)?
        .build();
    let mut signals = MessageStream::for_match_rule(rule, &bus, Some(256)).await?;
    send(updates, Update::Link(Some(link.clone()))).await?;
    loop {
        let mut state = match &link {
            Link::NetworkManager(nm) => nm.read().await?,
            Link::Iwd(iwd) => iwd.read().await?,
        };
        state.manager = manager;
        state.settle();
        send(updates, Update::State(state)).await?;
        // The next change, or the manager changing hands.
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

async fn send(updates: &Sender<Update>, update: Update) -> zbus::Result<()> {
    updates
        .send(update)
        .await
        .map_err(|_| zbus::Error::Failure("the window is gone".into()))
}

/// Signal bars, 1 to 4, for a strength in percent.
pub fn bars(strength: u8) -> u8 {
    match strength {
        70.. => 4,
        50..=69 => 3,
        30..=49 => 2,
        _ => 1,
    }
}

/// How the strength reads, for a strength in percent.
pub fn quality(strength: u8) -> &'static str {
    match bars(strength) {
        4 => "Excellent",
        3 => "Good",
        2 => "Fair",
        _ => "Weak",
    }
}

/// The band of a frequency in MHz.
pub fn band(frequency: u32) -> &'static str {
    match frequency {
        5925.. => "6 GHz",
        4900..=5924 => "5 GHz",
        _ => "2.4 GHz",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(name: &str, security: Security, strength: u8) -> Network {
        Network {
            ssid: name.as_bytes().to_vec(),
            name: name.into(),
            security,
            strength,
            saved: None,
            path: String::new(),
        }
    }

    #[test]
    fn networks_keep_their_strongest_point_and_saved_ones_lead() {
        let mut state = State {
            networks: strongest([
                point("Cafe", Security::Open, 40),
                point("Home", Security::Personal, 30),
                point("Home", Security::Sae, 60),
                point("", Security::Personal, 99),
                point("Office", Security::Personal, 90),
            ]),
            saved: vec![Saved {
                id: "/1".into(),
                name: "Home".into(),
                ssid: b"Home".to_vec(),
                security: Security::Personal,
                autoconnect: true,
                hidden: false,
                metered: None,
                address: None,
            }],
            ..Default::default()
        };
        state.settle();
        let order: Vec<(&str, u8)> = state
            .networks
            .iter()
            .map(|network| (network.name.as_str(), network.strength))
            .collect();
        assert_eq!(order, [("Home", 60), ("Office", 90), ("Cafe", 40)]);
        assert_eq!(state.networks[0].saved.as_deref(), Some("/1"));
    }
}

/// Prints what the system's Wi-Fi manager reports, on a phone:
/// `cargo test -- --ignored --nocapture reads_the_system`.
#[cfg(test)]
#[test]
#[ignore]
fn reads_the_system() {
    let (_, state) = read_system(Duration::from_secs(3));
    println!("{state:#?}");
}

/// What the manager reports over `time`: its link and its last state.
#[cfg(test)]
fn read_system(time: Duration) -> (Option<Link>, Option<State>) {
    let (updates, receiver) = async_channel::unbounded();
    futures_lite::future::block_on(async {
        let follow = async {
            follow_manager(&updates).await.unwrap();
        };
        follow
            .or(async {
                async_io::Timer::after(time).await;
            })
            .await;
    });
    let (mut link, mut state) = (None, None);
    while let Ok(update) = receiver.try_recv() {
        match update {
            Update::Link(new) => link = new,
            Update::State(new) => state = Some(new),
        }
    }
    (link, state)
}

/// Changes how saved network `$WIFI_TEST_NETWORK` is joined, checks the
/// manager kept it, and puts it back, on a phone:
/// `WIFI_TEST_NETWORK=name cargo test -- --ignored changes_a_saved_network`.
/// Compare its password before and after.
#[cfg(test)]
#[test]
#[ignore]
fn changes_a_saved_network_and_back() {
    let name = std::env::var("WIFI_TEST_NETWORK").expect("WIFI_TEST_NETWORK");
    let (link, state) = read_system(Duration::from_secs(2));
    let (link, state) = (link.unwrap(), state.unwrap());
    let saved = state
        .saved
        .iter()
        .find(|saved| saved.name == name)
        .unwrap()
        .clone();
    use futures_lite::future::block_on as block;
    block(link.set_metered(&saved, Metered::Yes)).unwrap();
    block(link.set_address(&saved, Address::Random)).unwrap();
    block(link.set_autoconnect(&saved, !saved.autoconnect)).unwrap();
    let (_, changed) = read_system(Duration::from_secs(2));
    let changed = changed.unwrap().saved(&saved.id).unwrap().clone();
    assert_eq!(changed.metered, Some(Metered::Yes));
    assert_eq!(changed.address, Some(Address::Random));
    assert_eq!(changed.autoconnect, !saved.autoconnect);
    block(link.set_metered(&saved, saved.metered.unwrap())).unwrap();
    block(link.set_address(&saved, saved.address.unwrap())).unwrap();
    block(link.set_autoconnect(&saved, saved.autoconnect)).unwrap();
    let (_, back) = read_system(Duration::from_secs(2));
    let back = back.unwrap().saved(&saved.id).unwrap().clone();
    assert_eq!(back.metered, saved.metered);
    assert_eq!(back.address, saved.address);
    assert_eq!(back.autoconnect, saved.autoconnect);
}

/// Leaves the network the phone is on and joins it again, on a phone:
/// `cargo test -- --ignored leaves_and_rejoins`.
#[cfg(test)]
#[test]
#[ignore]
fn leaves_and_rejoins() {
    use futures_lite::future::block_on as block;
    let (link, state) = read_system(Duration::from_secs(2));
    let (link, state) = (link.unwrap(), state.unwrap());
    let device = state.device.clone().unwrap();
    let id = state
        .current
        .as_ref()
        .and_then(|current| current.saved.clone())
        .unwrap();
    block(link.disconnect(&device)).unwrap();
    // The network is in range again once a scan sees it.
    let network = (0..10)
        .find_map(|_| {
            block(link.scan(&device));
            let (_, state) = read_system(Duration::from_secs(3));
            state.unwrap().in_range(&id).cloned()
        })
        .unwrap();
    let started = std::time::Instant::now();
    assert_eq!(block(link.connect(&device, &network, None)), Ok(()));
    println!("joined {} in {:?}", network.name, started.elapsed());
    let (_, state) = read_system(Duration::from_secs(2));
    let current = state.unwrap().current.unwrap();
    assert_eq!(current.phase, Some(Phase::Connected));
    println!("{current:#?}");
}
