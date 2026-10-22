//! Wi-Fi through iwd's D-Bus API, where NetworkManager does not run. iwd
//! asks for a network's password through an agent, so Settings serves one
//! while it follows iwd: it answers with the password the user just typed,
//! once, and refuses otherwise. iwd keeps no metered flag nor address choice
//! on D-Bus, and leaves addresses to the system, which the kernel reports.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_lite::FutureExt;
use serde::Serialize;
use serde::de::DeserializeOwned;
use zbus::Connection;
use zbus::zvariant::{DynamicType, OwnedObjectPath, OwnedValue, Value};

use super::{Current, Device, Failure, Network, Phase, Saved, Security, State, kernel, strongest};
use crate::props::{self, Objects, Props, get, path};

const SERVICE: &str = "net.connman.iwd";
const ADAPTER: &str = "net.connman.iwd.Adapter";
const DEVICE: &str = "net.connman.iwd.Device";
const STATION: &str = "net.connman.iwd.Station";
const DIAGNOSTIC: &str = "net.connman.iwd.StationDiagnostic";
const NETWORK: &str = "net.connman.iwd.Network";
const KNOWN: &str = "net.connman.iwd.KnownNetwork";
const AGENT_PATH: &str = "/org/denialwm/Settings/WifiAgent";
/// iwd answers Connect once joined or failed; this is in case it never does.
const JOIN_LIMIT: Duration = Duration::from_secs(45);

/// The password the agent gives iwd next, once.
type Pending = Arc<Mutex<Option<String>>>;

#[derive(Clone)]
pub struct Iwd {
    bus: Connection,
    password: Pending,
}

struct Agent {
    password: Pending,
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "net.connman.iwd.Agent.Error")]
enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Canceled(String),
}

#[zbus::interface(name = "net.connman.iwd.Agent")]
impl Agent {
    fn release(&self) {}

    fn request_passphrase(&self, _network: OwnedObjectPath) -> Result<String, AgentError> {
        self.password
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| AgentError::Canceled("No password was typed".into()))
    }

    fn request_private_key_passphrase(
        &self,
        _network: OwnedObjectPath,
    ) -> Result<String, AgentError> {
        Err(AgentError::Canceled(
            "Enterprise networks are not supported".into(),
        ))
    }

    fn request_user_name_and_password(
        &self,
        _network: OwnedObjectPath,
    ) -> Result<(String, String), AgentError> {
        Err(AgentError::Canceled(
            "Enterprise networks are not supported".into(),
        ))
    }

    fn request_user_password(
        &self,
        _network: OwnedObjectPath,
        _user: String,
    ) -> Result<String, AgentError> {
        Err(AgentError::Canceled(
            "Enterprise networks are not supported".into(),
        ))
    }

    fn cancel(&self, _reason: String) {
        self.password.lock().unwrap().take();
    }
}

impl Iwd {
    /// Serves the agent on `bus` and registers it with iwd.
    pub async fn new(bus: Connection) -> zbus::Result<Self> {
        let password = Pending::default();
        bus.object_server()
            .at(
                AGENT_PATH,
                Agent {
                    password: password.clone(),
                },
            )
            .await?;
        let iwd = Self { bus, password };
        let agent = OwnedObjectPath::try_from(AGENT_PATH)?;
        iwd.call::<_, ()>(
            "/net/connman/iwd",
            "net.connman.iwd.AgentManager",
            "RegisterAgent",
            &(agent,),
        )
        .await?;
        Ok(iwd)
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
        let objects = props::objects(&self.bus, SERVICE, "/").await?;
        let saved: Vec<Saved> = objects
            .iter()
            .filter_map(|(path, interfaces)| known_network(path, interfaces.get(KNOWN)?))
            .collect();
        let mut state = State {
            saved,
            ..Default::default()
        };
        let Some((device_path, interfaces)) = station_device(&objects) else {
            return Ok(state);
        };
        let device = interfaces.get(DEVICE);
        let station = interfaces.get(STATION);
        let adapter = path(device, "Adapter")
            .and_then(|adapter| objects.get(&adapter))
            .and_then(|adapter| adapter.get(ADAPTER));
        let interface: String = get(device, "Name").unwrap_or_default();
        state.enabled =
            get(device, "Powered").unwrap_or(false) && get(adapter, "Powered").unwrap_or(false);
        state.scanning = get(station, "Scanning").unwrap_or(false);
        state.device = Some(Device {
            path: device_path.clone(),
            address: get(device, "Address").unwrap_or_default(),
        });
        if station.is_none() {
            return Ok(state);
        }
        // Strongest first, signal in hundredths of a dBm.
        let ordered: Vec<(OwnedObjectPath, i16)> = self
            .call(device_path, STATION, "GetOrderedNetworks", &())
            .await
            .unwrap_or_default();
        let networks: Vec<Network> = ordered
            .iter()
            .filter_map(|(path, signal)| {
                let props = objects.get(path.as_str())?.get(NETWORK);
                let name: String = get(props, "Name")?;
                Some(Network {
                    ssid: name.as_bytes().to_vec(),
                    name,
                    security: security(get::<String>(props, "Type").as_deref()),
                    strength: strength(*signal),
                    saved: None,
                    path: path.to_string(),
                })
            })
            .collect();
        state.current = self
            .current(
                &objects,
                device_path,
                station,
                &networks,
                &state.saved,
                &interface,
            )
            .await;
        state.networks = strongest(networks);
        Ok(state)
    }

    async fn current(
        &self,
        objects: &Objects,
        device_path: &str,
        station: Option<&Props>,
        networks: &[Network],
        saved: &[Saved],
        interface: &str,
    ) -> Option<Current> {
        let phase = match get::<String>(station, "State")?.as_str() {
            "connecting" => Phase::Connecting,
            "connected" | "roaming" => Phase::Connected,
            _ => return None,
        };
        let network_path = path(station, "ConnectedNetwork")?;
        let props = objects
            .get(&network_path)
            .and_then(|object| object.get(NETWORK));
        let name: String = get(props, "Name").unwrap_or_default();
        let known = path(props, "KnownNetwork");
        let mut current = Current {
            ssid: name.as_bytes().to_vec(),
            saved: known.filter(|known| saved.iter().any(|saved| saved.id == *known)),
            phase: Some(phase),
            security: Some(security(get::<String>(props, "Type").as_deref())),
            strength: networks
                .iter()
                .find(|network| network.path == network_path)
                .map(|network| network.strength),
            name,
            ..Default::default()
        };
        let diagnostics: zbus::Result<std::collections::HashMap<String, OwnedValue>> = self
            .call(device_path, DIAGNOSTIC, "GetDiagnostics", &())
            .await;
        if let Ok(diagnostics) = diagnostics {
            current.frequency = get(Some(&diagnostics), "Frequency");
            // In 100 kbit/s.
            current.bitrate = get::<u32>(Some(&diagnostics), "RxBitrate").map(|rate| rate / 10);
        }
        if phase == Phase::Connected {
            let addresses = kernel::addresses(interface);
            current.ipv4 = addresses.ipv4.into_iter().next();
            current.ipv6 = addresses.ipv6;
            current.gateway = kernel::gateway(interface);
            current.dns = kernel::nameservers();
        }
        Some(current)
    }

    pub async fn set_enabled(&self, on: bool) -> zbus::Result<()> {
        let objects = props::objects(&self.bus, SERVICE, "/").await?;
        let Some((device, interfaces)) = station_device(&objects) else {
            return Ok(());
        };
        if on && let Some(adapter) = path(interfaces.get(DEVICE), "Adapter") {
            self.set_property(&adapter, ADAPTER, "Powered", Value::from(true))
                .await?;
        }
        self.set_property(device, DEVICE, "Powered", Value::from(on))
            .await
    }

    pub async fn scan(&self, device: &Device) -> zbus::Result<()> {
        self.call(&device.path, STATION, "Scan", &()).await
    }

    pub async fn connect(
        &self,
        network: &Network,
        password: Option<String>,
    ) -> Result<(), Failure> {
        if let Some(password) = password {
            // iwd keeps one password per network and takes a new one only for
            // a network it does not know.
            if let Some(known) = &network.saved {
                self.forget(known)
                    .await
                    .map_err(|error| failed(error, network.security))?;
            }
            *self.password.lock().unwrap() = Some(password);
        }
        let joined = self.call::<_, ()>(&network.path, NETWORK, "Connect", &());
        self.join(joined, network.security).await
    }

    pub async fn connect_hidden(
        &self,
        device: &Device,
        name: &str,
        password: Option<String>,
    ) -> Result<(), Failure> {
        let security = if password.is_some() {
            Security::Personal
        } else {
            Security::Open
        };
        *self.password.lock().unwrap() = password;
        let name = (name,);
        let joined = self.call::<_, ()>(&device.path, STATION, "ConnectHiddenNetwork", &name);
        self.join(joined, security).await
    }

    /// Waits for iwd to answer a Connect, within `JOIN_LIMIT`, and takes back
    /// a password it never asked for.
    async fn join(
        &self,
        joined: impl Future<Output = zbus::Result<()>>,
        security: Security,
    ) -> Result<(), Failure> {
        let answer = joined
            .or(async {
                async_io::Timer::after(JOIN_LIMIT).await;
                Err(zbus::Error::Failure("timed out".into()))
            })
            .await;
        self.password.lock().unwrap().take();
        answer.map_err(|error| failed(error, security))
    }

    pub async fn disconnect(&self, device: &Device) -> zbus::Result<()> {
        self.call(&device.path, STATION, "Disconnect", &()).await
    }

    pub async fn forget(&self, id: &str) -> zbus::Result<()> {
        self.call(id, KNOWN, "Forget", &()).await
    }

    pub async fn set_autoconnect(&self, id: &str, on: bool) -> zbus::Result<()> {
        self.set_property(id, KNOWN, "AutoConnect", Value::from(on))
            .await
    }
}

/// The Wi-Fi device in station mode, the phone's own.
fn station_device(objects: &Objects) -> Option<(&String, &crate::props::Interfaces)> {
    objects.iter().find(|(_, interfaces)| {
        get::<String>(interfaces.get(DEVICE), "Mode").as_deref() == Some("station")
    })
}

fn known_network(path: &str, props: &Props) -> Option<Saved> {
    let props = Some(props);
    let name: String = get(props, "Name")?;
    Some(Saved {
        id: path.to_owned(),
        ssid: name.as_bytes().to_vec(),
        name,
        security: security(get::<String>(props, "Type").as_deref()),
        autoconnect: get(props, "AutoConnect").unwrap_or(true),
        hidden: get(props, "Hidden").unwrap_or(false),
        metered: None,
        address: None,
    })
}

fn security(kind: Option<&str>) -> Security {
    match kind {
        Some("psk") => Security::Personal,
        Some("wep") => Security::Wep,
        Some("8021x") => Security::Enterprise,
        Some("owe") => Security::Owe,
        _ => Security::Open,
    }
}

/// Percent for a signal in hundredths of a dBm: -100 dBm and below is 0,
/// -50 and above 100.
fn strength(signal: i16) -> u8 {
    ((i32::from(signal) / 100 + 100) * 2).clamp(0, 100) as u8
}

fn failed(error: zbus::Error, security: Security) -> Failure {
    eprintln!("settings: iwd: {error}");
    let zbus::Error::MethodError(name, ..) = &error else {
        return Failure::Other("iwd could not join it");
    };
    match name.as_str().rsplit('.').next() {
        Some("Failed") if security.password() => Failure::WrongPassword,
        Some("NotSupported") => Failure::Unsupported,
        Some("NotFound") => Failure::NotInRange,
        Some("InProgress") => Failure::Other("Already joining a network"),
        Some("Aborted") => Failure::Other("Joining was stopped"),
        Some("PermissionDenied" | "AccessDenied") => Failure::Other("Not allowed to change Wi-Fi"),
        _ => Failure::Other("iwd could not join it"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn signal_maps_to_percent() {
        assert_eq!(super::strength(-10_000), 0);
        assert_eq!(super::strength(-7_500), 50);
        assert_eq!(super::strength(-4_000), 100);
    }
}
