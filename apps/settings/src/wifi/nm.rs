//! Wi-Fi through NetworkManager's D-Bus API. One `GetManagedObjects` holds
//! the device, the access points, the active connection and its addresses;
//! each saved Wi-Fi connection adds a `GetSettings`. The phone's session is
//! allowed every Wi-Fi action by NetworkManager's polkit rules, so nothing
//! here asks for a password.

use std::collections::HashMap;
use std::time::Duration;

use futures_lite::{FutureExt, StreamExt};
use serde::Serialize;
use serde::de::DeserializeOwned;
use zbus::zvariant::{DynamicType, ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream};

use super::{
    Address, Current, Device, Failure, Metered, Network, Phase, Saved, Security, State, strongest,
};
use crate::props::{self, Objects, Props, get, owned, path};

const SERVICE: &str = "org.freedesktop.NetworkManager";
const ROOT: &str = "/org/freedesktop/NetworkManager";
const MANAGER: &str = "org.freedesktop.NetworkManager";
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const ACCESS_POINT: &str = "org.freedesktop.NetworkManager.AccessPoint";
const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const IP4: &str = "org.freedesktop.NetworkManager.IP4Config";
const IP6: &str = "org.freedesktop.NetworkManager.IP6Config";
const CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";

/// A connection's settings, by setting, then by property.
type Settings = HashMap<String, HashMap<String, OwnedValue>>;

/// NMDeviceState values.
mod device_state {
    pub const DISCONNECTED: u32 = 30;
    pub const PREPARE: u32 = 40;
    pub const NEED_AUTH: u32 = 60;
    pub const IP_CONFIG: u32 = 70;
    pub const ACTIVATED: u32 = 100;
    pub const FAILED: u32 = 120;
}

/// NMDeviceStateReason values that say why joining failed.
mod reason {
    pub const IP_CONFIG_UNAVAILABLE: u32 = 5;
    pub const NO_SECRETS: u32 = 7;
    pub const SUPPLICANT_DISCONNECT: u32 = 8;
    pub const SUPPLICANT_TIMEOUT: u32 = 11;
    pub const SSID_NOT_FOUND: u32 = 53;
}

/// NM80211ApFlags and NM80211ApSecurityFlags bits.
const AP_PRIVACY: u32 = 0x1;
const KEY_PSK: u32 = 0x100;
const KEY_8021X: u32 = 0x200;
const KEY_SAE: u32 = 0x400;
const KEY_OWE: u32 = 0x800;
const KEY_EAP_SUITE_B: u32 = 0x2000;

/// Joining gives up after this long without NetworkManager saying how it
/// went; it says within about half a minute.
const JOIN_LIMIT: Duration = Duration::from_secs(45);

#[derive(Clone)]
pub struct Nm {
    bus: Connection,
}

impl Nm {
    pub fn new(bus: Connection) -> Self {
        Self { bus }
    }

    async fn call<B, R>(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        body: &B,
    ) -> zbus::Result<R>
    where
        B: Serialize + DynamicType,
        R: DeserializeOwned + zbus::zvariant::Type,
    {
        let reply = self
            .bus
            .call_method(Some(SERVICE), path, Some(interface), method, body)
            .await?;
        reply.body().deserialize()
    }

    async fn set_property(
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

    pub async fn read(&self) -> zbus::Result<State> {
        let objects = props::objects(&self.bus, SERVICE, "/org/freedesktop").await?;
        let manager = objects.get(ROOT).and_then(|object| object.get(MANAGER));
        let mut saved = Vec::new();
        for (path, interfaces) in &objects {
            if !interfaces.contains_key(CONNECTION) {
                continue;
            }
            // A connection removed since the list was read is simply left out.
            if let Ok(settings) = self
                .call::<_, Settings>(path, CONNECTION, "GetSettings", &())
                .await
            {
                saved.extend(saved_network(path, &settings));
            }
        }
        // The first Wi-Fi interface; a phone has one.
        let device = objects
            .iter()
            .find(|(_, interfaces)| interfaces.contains_key(WIRELESS));
        let mut state = State {
            enabled: get(manager, "WirelessEnabled").unwrap_or(false),
            blocked: !get(manager, "WirelessHardwareEnabled").unwrap_or(true),
            saved,
            ..Default::default()
        };
        let Some((device_path, interfaces)) = device else {
            return Ok(state);
        };
        let (device, wireless) = (interfaces.get(DEVICE), interfaces.get(WIRELESS));
        state.device = Some(Device {
            path: device_path.clone(),
            address: get(wireless, "HwAddress").unwrap_or_default(),
        });
        if state.enabled {
            state.networks = strongest(objects.iter().filter_map(|(path, interfaces)| {
                access_point(path, interfaces.get(ACCESS_POINT)?)
            }));
        }
        state.current = current(&objects, device, wireless, &state.saved);
        Ok(state)
    }

    pub async fn set_enabled(&self, on: bool) -> zbus::Result<()> {
        self.set_property(ROOT, MANAGER, "WirelessEnabled", Value::from(on))
            .await
    }

    pub async fn scan(&self, device: &Device) -> zbus::Result<()> {
        let options: HashMap<&str, Value> = HashMap::new();
        self.call(&device.path, WIRELESS, "RequestScan", &(options,))
            .await
    }

    pub async fn connect(
        &self,
        device: &Device,
        network: &Network,
        password: Option<String>,
    ) -> Result<(), Failure> {
        let states = self.device_states(device).await.map_err(failed)?;
        match &network.saved {
            Some(id) => {
                if let Some(password) = password {
                    let security = network.security;
                    self.change(id, |settings| set_password(settings, security, password))
                        .await
                        .map_err(failed)?;
                }
                self.activate(id, device, &network.path)
                    .await
                    .map_err(failed)?;
                outcome(states, network.security).await
            }
            None => {
                let settings = new_connection(
                    &network.name,
                    &network.ssid,
                    network.security,
                    password,
                    false,
                );
                self.add_and_join(settings, device, &network.path, network.security, states)
                    .await
            }
        }
    }

    pub async fn connect_hidden(
        &self,
        device: &Device,
        name: &str,
        password: Option<String>,
    ) -> Result<(), Failure> {
        let states = self.device_states(device).await.map_err(failed)?;
        let security = if password.is_some() {
            Security::Personal
        } else {
            Security::Open
        };
        let settings = new_connection(name, name.as_bytes(), security, password, true);
        self.add_and_join(settings, device, "/", security, states)
            .await
    }

    /// Saves a new connection and joins with it; one that does not join is
    /// deleted again, so a wrong password is never kept.
    async fn add_and_join(
        &self,
        settings: HashMap<&'static str, HashMap<&'static str, Value<'static>>>,
        device: &Device,
        point: &str,
        security: Security,
        states: MessageStream,
    ) -> Result<(), Failure> {
        let options: HashMap<&str, Value> = HashMap::from([("persist", Value::from("disk"))]);
        let (connection, _, _): (
            OwnedObjectPath,
            OwnedObjectPath,
            HashMap<String, OwnedValue>,
        ) = self
            .call(
                ROOT,
                MANAGER,
                "AddAndActivateConnection2",
                &(settings, object(&device.path), object(point), options),
            )
            .await
            .map_err(failed)?;
        let joined = outcome(states, security).await;
        if joined.is_err() {
            let _ = self.forget(connection.as_str()).await;
        }
        joined
    }

    async fn activate(&self, id: &str, device: &Device, point: &str) -> zbus::Result<()> {
        let _: OwnedObjectPath = self
            .call(
                ROOT,
                MANAGER,
                "ActivateConnection",
                &(object(id), object(&device.path), object(point)),
            )
            .await?;
        Ok(())
    }

    /// The device's state changes from now on.
    async fn device_states(&self, device: &Device) -> zbus::Result<MessageStream> {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(SERVICE)?
            .path(device.path.as_str())?
            .interface(DEVICE)?
            .member("StateChanged")?
            .build();
        MessageStream::for_match_rule(rule, &self.bus, Some(64)).await
    }

    pub async fn disconnect(&self, device: &Device) -> zbus::Result<()> {
        self.call(&device.path, DEVICE, "Disconnect", &()).await
    }

    pub async fn forget(&self, id: &str) -> zbus::Result<()> {
        self.call(id, CONNECTION, "Delete", &()).await
    }

    pub async fn set_autoconnect(&self, id: &str, on: bool) -> zbus::Result<()> {
        self.change(id, |settings| {
            section(settings, "connection").insert("autoconnect".into(), owned(on));
        })
        .await
    }

    pub async fn set_metered(&self, id: &str, metered: Metered) -> zbus::Result<()> {
        // NMMetered: unknown, yes, no.
        let value: i32 = match metered {
            Metered::Auto => 0,
            Metered::Yes => 1,
            Metered::No => 2,
        };
        self.change(id, |settings| {
            section(settings, "connection").insert("metered".into(), owned(value));
        })
        .await
    }

    pub async fn set_address(&self, id: &str, address: Address) -> zbus::Result<()> {
        self.change(id, |settings| {
            let wireless = section(settings, "802-11-wireless");
            // The old byte form would win over the string.
            wireless.remove("cloned-mac-address");
            match address {
                Address::Default => wireless.remove("assigned-mac-address"),
                Address::Random => wireless.insert("assigned-mac-address".into(), owned("stable")),
                Address::Device => {
                    wireless.insert("assigned-mac-address".into(), owned("permanent"))
                }
            };
        })
        .await
    }

    /// Rewrites saved connection `id` with `edit` made to its settings, its
    /// secrets in them, so none is lost in the rewrite.
    async fn change(&self, id: &str, edit: impl FnOnce(&mut Settings)) -> zbus::Result<()> {
        let mut settings: Settings = self.call(id, CONNECTION, "GetSettings", &()).await?;
        if settings.contains_key("802-11-wireless-security") {
            let secrets: zbus::Result<Settings> = self
                .call(id, CONNECTION, "GetSecrets", &("802-11-wireless-security",))
                .await;
            for (name, values) in secrets.into_iter().flatten() {
                section(&mut settings, &name).extend(values);
            }
        }
        edit(&mut settings);
        self.call(id, CONNECTION, "Update", &(settings,)).await
    }
}

/// Settings section `name`, made when missing.
fn section<'a>(settings: &'a mut Settings, name: &str) -> &'a mut HashMap<String, OwnedValue> {
    settings.entry(name.to_owned()).or_default()
}

fn set_password(settings: &mut Settings, security: Security, password: String) {
    let section = section(settings, "802-11-wireless-security");
    match security {
        Security::Wep => {
            section.insert("wep-key0".into(), owned(password));
        }
        _ => {
            section.insert("psk".into(), owned(password));
        }
    }
    section.insert("psk-flags".into(), owned(0u32));
}

fn object(path: &str) -> ObjectPath<'_> {
    ObjectPath::try_from(path).unwrap_or_else(|_| ObjectPath::from_static_str_unchecked("/"))
}

fn failed(error: zbus::Error) -> Failure {
    eprintln!("settings: NetworkManager: {error}");
    match error {
        zbus::Error::MethodError(name, ..) if name.as_str().ends_with("PermissionDenied") => {
            Failure::Other("Not allowed to change Wi-Fi")
        }
        _ => Failure::Other("NetworkManager could not join it"),
    }
}

/// Waits for the device to finish joining: on the network, or failed and
/// why.
async fn outcome(mut states: MessageStream, security: Security) -> Result<(), Failure> {
    let wait = async {
        let mut started = false;
        while let Some(Ok(message)) = states.next().await {
            let Ok((new, _, why)) = message.body().deserialize::<(u32, u32, u32)>() else {
                continue;
            };
            started |= new >= device_state::PREPARE;
            match new {
                device_state::ACTIVATED => return Ok(()),
                device_state::FAILED => return Err(failure(why, security)),
                device_state::DISCONNECTED if started => return Err(failure(why, security)),
                _ => {}
            }
        }
        Err(Failure::Other("NetworkManager stopped"))
    };
    wait.or(async {
        async_io::Timer::after(JOIN_LIMIT).await;
        Err(Failure::Other("The network did not answer"))
    })
    .await
}

fn failure(why: u32, security: Security) -> Failure {
    match why {
        reason::NO_SECRETS | reason::SUPPLICANT_DISCONNECT if security.password() => {
            Failure::WrongPassword
        }
        reason::SUPPLICANT_TIMEOUT => Failure::Other("The network did not answer"),
        reason::IP_CONFIG_UNAVAILABLE => Failure::Other("The network gave no address"),
        reason::SSID_NOT_FOUND => Failure::NotInRange,
        _ => Failure::Other("Could not connect"),
    }
}

/// A new connection's settings; NetworkManager fills in the rest from the
/// access point.
fn new_connection(
    name: &str,
    ssid: &[u8],
    security: Security,
    password: Option<String>,
    hidden: bool,
) -> HashMap<&'static str, HashMap<&'static str, Value<'static>>> {
    let mut settings = HashMap::from([
        (
            "connection",
            HashMap::from([
                ("id", Value::from(name.to_owned())),
                ("type", Value::from("802-11-wireless")),
            ]),
        ),
        (
            "802-11-wireless",
            HashMap::from([
                ("ssid", Value::from(ssid.to_vec())),
                ("mode", Value::from("infrastructure")),
                ("hidden", Value::from(hidden)),
            ]),
        ),
    ]);
    let key = match security {
        Security::Open | Security::Enterprise => None,
        Security::Owe => Some("owe"),
        Security::Wep => Some("none"),
        Security::Personal => Some("wpa-psk"),
        Security::Sae => Some("sae"),
    };
    if let Some(key) = key {
        let mut section = HashMap::from([("key-mgmt", Value::from(key))]);
        if let Some(password) = password {
            if security == Security::Wep {
                // A key of 5 or 13 characters, or 10 or 26 hex digits, is the
                // key itself; anything else a passphrase to make one from.
                let key_type: u32 = if matches!(password.len(), 5 | 10 | 13 | 26) {
                    1
                } else {
                    2
                };
                section.insert("wep-key0", Value::from(password));
                section.insert("wep-key-type", Value::from(key_type));
            } else {
                section.insert("psk", Value::from(password));
            }
        }
        settings.insert("802-11-wireless-security", section);
    }
    settings
}

/// What a saved connection is, when it is a Wi-Fi one.
fn saved_network(path: &str, settings: &Settings) -> Option<Saved> {
    let connection = settings.get("connection");
    if get::<String>(connection, "type").as_deref() != Some("802-11-wireless") {
        return None;
    }
    let wireless = settings.get("802-11-wireless");
    let ssid: Vec<u8> = get(wireless, "ssid").unwrap_or_default();
    let security = match settings.get("802-11-wireless-security") {
        None => Security::Open,
        Some(section) => match get::<String>(Some(section), "key-mgmt").as_deref() {
            Some("wpa-psk") => Security::Personal,
            Some("sae") => Security::Sae,
            Some("owe") => Security::Owe,
            Some("none") => Security::Wep,
            _ => Security::Enterprise,
        },
    };
    let metered = match get::<i32>(connection, "metered") {
        Some(1) => Metered::Yes,
        Some(2) => Metered::No,
        _ => Metered::Auto,
    };
    let address = match get::<String>(wireless, "assigned-mac-address").as_deref() {
        Some("stable" | "random") => Address::Random,
        Some("permanent") => Address::Device,
        _ => Address::Default,
    };
    Some(Saved {
        id: path.to_owned(),
        name: get(connection, "id").unwrap_or_else(|| String::from_utf8_lossy(&ssid).into_owned()),
        ssid,
        security,
        autoconnect: get(connection, "autoconnect").unwrap_or(true),
        hidden: get(wireless, "hidden").unwrap_or(false),
        metered: Some(metered),
        address: Some(address),
    })
}

fn access_point(path: &str, point: &Props) -> Option<Network> {
    let point = Some(point);
    let ssid: Vec<u8> = get(point, "Ssid")?;
    Some(Network {
        name: String::from_utf8_lossy(&ssid).into_owned(),
        ssid,
        security: point_security(point),
        strength: get(point, "Strength").unwrap_or(0),
        saved: None,
        path: path.to_owned(),
    })
}

fn point_security(point: Option<&Props>) -> Security {
    let flags: u32 = get(point, "Flags").unwrap_or(0);
    let keys =
        get::<u32>(point, "WpaFlags").unwrap_or(0) | get::<u32>(point, "RsnFlags").unwrap_or(0);
    if keys & (KEY_8021X | KEY_EAP_SUITE_B) != 0 {
        Security::Enterprise
    } else if keys & KEY_PSK != 0 {
        Security::Personal
    } else if keys & KEY_SAE != 0 {
        Security::Sae
    } else if keys & KEY_OWE != 0 {
        Security::Owe
    } else if flags & AP_PRIVACY != 0 {
        Security::Wep
    } else {
        Security::Open
    }
}

/// The network the device is on or joining, with its addresses.
fn current(
    objects: &Objects,
    device: Option<&Props>,
    wireless: Option<&Props>,
    saved: &[Saved],
) -> Option<Current> {
    let state: u32 = get(device, "State").unwrap_or(0);
    let phase = match state {
        device_state::PREPARE..device_state::NEED_AUTH => Phase::Connecting,
        device_state::NEED_AUTH..device_state::IP_CONFIG => Phase::Authenticating,
        device_state::IP_CONFIG..device_state::ACTIVATED => Phase::Addressing,
        device_state::ACTIVATED => Phase::Connected,
        _ => return None,
    };
    let lookup = |path: Option<String>, interface: &str| {
        path.and_then(|path| objects.get(&path))
            .and_then(|object| object.get(interface))
    };
    let active = lookup(path(device, "ActiveConnection"), ACTIVE);
    let id = path(active, "Connection");
    let known = id
        .as_deref()
        .and_then(|id| saved.iter().find(|saved| saved.id == id));
    let point = lookup(path(wireless, "ActiveAccessPoint"), ACCESS_POINT);
    let ssid = known
        .map(|saved| saved.ssid.clone())
        .or_else(|| get(point, "Ssid"))
        .unwrap_or_default();
    let mut current = Current {
        name: known
            .map(|saved| saved.name.clone())
            .unwrap_or_else(|| String::from_utf8_lossy(&ssid).into_owned()),
        ssid,
        saved: known.map(|saved| saved.id.clone()),
        phase: Some(phase),
        security: point
            .map(|point| point_security(Some(point)))
            .or(known.map(|saved| saved.security)),
        strength: get(point, "Strength"),
        frequency: get(point, "Frequency"),
        bitrate: get::<u32>(wireless, "Bitrate").map(|kbits| kbits / 1000),
        ..Default::default()
    };
    if phase == Phase::Connected {
        let ip4 = lookup(path(device, "Ip4Config"), IP4);
        current.ipv4 = addresses(ip4).into_iter().next();
        current.gateway = get::<String>(ip4, "Gateway").filter(|gateway| !gateway.is_empty());
        current.dns = get::<Vec<HashMap<String, OwnedValue>>>(ip4, "NameserverData")
            .unwrap_or_default()
            .iter()
            .filter_map(|server| get(Some(server), "address"))
            .collect();
        current.ipv6 = addresses(lookup(path(device, "Ip6Config"), IP6));
    }
    Some(current)
}

/// An IP config's addresses, as address/prefix.
fn addresses(config: Option<&Props>) -> Vec<String> {
    get::<Vec<HashMap<String, OwnedValue>>>(config, "AddressData")
        .unwrap_or_default()
        .iter()
        .filter_map(|data| {
            let address: String = get(Some(data), "address")?;
            let prefix: u32 = get(Some(data), "prefix")?;
            Some(format!("{address}/{prefix}"))
        })
        .collect()
}
