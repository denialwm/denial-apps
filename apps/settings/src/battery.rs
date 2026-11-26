//! The battery through UPower (`org.freedesktop.UPower` on the system bus).
//! Its devices are read whole at start and again after every change it
//! signals. Only batteries that report how much energy they hold when full
//! count: a firmware battery that reports a level and nothing else (the Ace
//! 3's battery manager says 50% and charging, always) is left out.
//!
//! Whether a battery can hold its charge with the charger attached, and
//! does, is read from its `charge_behaviour` in sysfs with each reading:
//! UPower does not carry it. Settings writes it where the system lets the
//! session do so (Phonewave's udev rule on a phone); elsewhere the switch
//! does not show.

use std::collections::HashMap;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
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
const SUPPLIES: &str = "/sys/class/power_supply";

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

#[derive(Clone, Debug)]
pub struct ChargeLimit {
    pub path: OwnedObjectPath,
    pub name: String,
    pub enabled: bool,
    pub start: Option<u32>,
    pub end: Option<u32>,
    pub firmware_optimized: bool,
}

/// A battery whose driver can hold its charge with the charger attached:
/// the device then runs from the charger and the battery is left as it is.
#[derive(Clone, Debug)]
pub struct ChargeHold {
    /// Its name under `/sys/class/power_supply`.
    pub supply: String,
    pub held: bool,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    /// None on mains power only.
    pub battery: Option<Battery>,
    /// Whether a charger is connected.
    pub plugged: bool,
    pub charge_limits: Vec<ChargeLimit>,
    pub charge_holds: Vec<ChargeHold>,
}

/// One UPower device's properties.
struct Device {
    path: OwnedObjectPath,
    properties: HashMap<String, OwnedValue>,
}

impl Device {
    fn get<T: TryFrom<OwnedValue>>(&self, name: &str) -> Option<T> {
        self.properties
            .get(name)
            .and_then(|value| value.try_clone().ok()?.try_into().ok())
    }

    fn number(&self, name: &str) -> f64 {
        self.get(name).unwrap_or(0.0)
    }
}

impl Device {
    fn charge_limit(&self) -> Option<ChargeLimit> {
        if self.get::<u32>("Type") != Some(BATTERY)
            || self.get("PowerSupply") != Some(true)
            || self.get("IsPresent") == Some(false)
            || self.get("ChargeThresholdSupported") != Some(true)
        {
            return None;
        }
        let settings = self
            .get::<u32>("ChargeThresholdSettingsSupported")
            .unwrap_or(0);
        let threshold = |bit, key| {
            (settings & bit != 0)
                .then(|| self.get::<u32>(key))
                .flatten()
                .filter(|&percentage| percentage <= 100)
        };
        let name = self
            .get::<String>("Model")
            .filter(|name| !name.trim().is_empty())
            .or_else(|| self.get("NativePath"))
            .unwrap_or_default();
        Some(ChargeLimit {
            path: self.path.clone(),
            name,
            enabled: self.get("ChargeThresholdEnabled").unwrap_or(false),
            start: threshold(1_u32, "ChargeStartThreshold"),
            end: threshold(2_u32, "ChargeEndThreshold"),
            firmware_optimized: settings & 4 != 0,
        })
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
        devices.push(Device {
            path,
            properties: reply.body().deserialize()?,
        });
    }
    let plugged = devices.iter().any(|device| {
        device.get::<u32>("Type") == Some(LINE_POWER)
            && device.get("PowerSupply").unwrap_or(false)
            && device.get("Online").unwrap_or(false)
    });
    Ok(State {
        battery: battery(&devices),
        plugged,
        charge_limits: devices.iter().filter_map(Device::charge_limit).collect(),
        charge_holds: charge_holds(),
    })
}

/// The batteries that can hold their charge and that this session may
/// switch, by name.
fn charge_holds() -> Vec<ChargeHold> {
    let Ok(entries) = std::fs::read_dir(SUPPLIES) else {
        return Vec::new();
    };
    let mut holds: Vec<ChargeHold> = entries
        .flatten()
        .filter_map(|entry| {
            let folder = entry.path();
            let kind = std::fs::read_to_string(folder.join("type")).ok()?;
            if kind.trim() != "Battery" {
                return None;
            }
            let file = folder.join("charge_behaviour");
            let behaviour = std::fs::read_to_string(&file).ok()?;
            if !writable(&file) {
                return None;
            }
            Some(ChargeHold {
                supply: entry.file_name().into_string().ok()?,
                held: holding(&behaviour)?,
            })
        })
        .collect();
    holds.sort_by(|a, b| a.supply.cmp(&b.supply));
    holds
}

/// From a `charge_behaviour` such as `auto [inhibit-charge]`: whether the
/// charge is held, or None when holding it is not offered.
fn holding(behaviour: &str) -> Option<bool> {
    let mut offered = false;
    let mut chosen = None;
    for each in behaviour.split_whitespace() {
        let name = each.trim_matches(['[', ']']);
        if name == "inhibit-charge" {
            offered = true;
        }
        if each.starts_with('[') {
            chosen = Some(name);
        }
    }
    offered.then_some(chosen == Some("inhibit-charge"))
}

/// Whether this process may write a file.
fn writable(file: &std::path::Path) -> bool {
    CString::new(file.as_os_str().as_bytes())
        .is_ok_and(|path| unsafe { libc::access(path.as_ptr(), libc::W_OK) } == 0)
}

/// Holds a battery's charge or lets it charge.
pub fn set_charge_hold(supply: &str, held: bool) -> Result<(), String> {
    if supply.is_empty() || supply.starts_with('.') || supply.contains('/') {
        return Err(format!("{supply} is not a power supply"));
    }
    let file = std::path::Path::new(SUPPLIES).join(supply).join("charge_behaviour");
    std::fs::write(&file, if held { "inhibit-charge" } else { "auto" })
        .map_err(|error| error.to_string())
}

pub async fn set_charge_limit(path: &OwnedObjectPath, enabled: bool) -> zbus::Result<()> {
    let bus = Connection::system().await?;
    bus.call_method(
        Some(NAME),
        path,
        Some(DEVICE),
        "EnableChargeThreshold",
        &(enabled,),
    )
    .await
    .map(drop)
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

#[cfg(test)]
#[test]
fn charge_limits_only_use_supported_levels_on_present_system_batteries() {
    let mut device = Device {
        path: OwnedObjectPath::try_from("/org/freedesktop/UPower/devices/battery_BAT0").unwrap(),
        properties: [
            ("Type".to_owned(), OwnedValue::from(BATTERY)),
            ("PowerSupply".to_owned(), OwnedValue::from(true)),
            (
                "ChargeThresholdSupported".to_owned(),
                OwnedValue::from(true),
            ),
            ("ChargeThresholdEnabled".to_owned(), OwnedValue::from(true)),
            (
                "ChargeThresholdSettingsSupported".to_owned(),
                OwnedValue::from(6_u32),
            ),
            ("ChargeStartThreshold".to_owned(), OwnedValue::from(50_u32)),
            ("ChargeEndThreshold".to_owned(), OwnedValue::from(80_u32)),
        ]
        .into(),
    };
    let limit = device.charge_limit().unwrap();
    assert!(limit.enabled);
    assert_eq!(limit.start, None);
    assert_eq!(limit.end, Some(80));
    assert!(limit.firmware_optimized);
    device
        .properties
        .insert("ChargeEndThreshold".into(), OwnedValue::from(u32::MAX));
    assert_eq!(device.charge_limit().unwrap().end, None);
    device
        .properties
        .insert("IsPresent".into(), OwnedValue::from(false));
    assert!(device.charge_limit().is_none());
}

#[cfg(test)]
#[test]
fn a_hold_is_read_from_the_charge_behaviour() {
    assert_eq!(holding("[auto] inhibit-charge"), Some(false));
    assert_eq!(holding("auto [inhibit-charge]"), Some(true));
    assert_eq!(holding("auto inhibit-charge [force-discharge]"), Some(false));
    assert_eq!(holding("[auto] force-discharge"), None);
    assert_eq!(holding(""), None);
}
