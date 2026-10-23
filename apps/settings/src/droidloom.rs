//! Android through Droidloom. Whether it runs is Droidloom's user unit,
//! which starts with the session while the user's
//! `graphical-session.target.wants` links it. The link points at the unit
//! in /usr/lib, as Droidloom's updater expects; `systemctl enable` would pin
//! the release the unit comes from. How it runs, and its apps, come from
//! droidloomd's control socket: one JSON request, one JSON answer.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value, json};
use zbus::Connection;

const UNIT: &str = "droidloom.service";
const INSTALLED: &str = "/usr/lib/systemd/user/droidloom.service";
const SOCKET: &str = "/run/droidloom/control.sock";
const SPEC: &str = "/etc/droidloom/cell.json";
const RELEASE: &str = "/usr/lib/droidloom/last-update.json";
/// droidloomd answers one request at a time, and opening an app may wait
/// for Android to start.
const ANSWER: Duration = Duration::from_secs(150);
const MAX_ANSWER: u64 = 8 << 20;

/// How Android runs, while it does.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    /// Apps can be opened.
    pub ready: bool,
    pub up: Duration,
    /// How long it took until apps could be opened.
    pub ready_in: Option<Duration>,
    pub memory: Option<u64>,
    pub swap: Option<u64>,
    /// Processor time used since it started, in microseconds.
    pub cpu_usec: Option<u64>,
    pub processes: Option<u64>,
    pub threads: Option<u64>,
    /// The processes Android runs for apps.
    pub app_processes: Option<u64>,
    /// Android's version: "17".
    pub android: Option<String>,
    /// The model and the name Android gives apps.
    pub model: Option<String>,
    pub name: Option<String>,
}

/// Android, as droidloomd sees it.
#[derive(Clone, Debug)]
pub enum Cell {
    Running(Stats),
    /// Not running; why it ended, when it did not end on request.
    Stopped(Option<String>),
    /// droidloomd does not run.
    Absent,
}

/// Droidloom's user unit, as the user's service manager sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Unit {
    #[default]
    Stopped,
    Starting,
    Running,
    Stopping,
    /// Its last start, or its run, ended in an error.
    Failed,
}

/// The unit's state, and its last run, which names its output in the
/// journal. A start refused for starting too often is no run of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnitState {
    pub unit: Unit,
    pub invocation: [u8; 16],
}

/// One Android app, as Android describes it.
#[derive(Clone, Debug, Default)]
pub struct Details {
    pub version: String,
    /// Part of Android itself: it cannot be uninstalled.
    pub system: bool,
    /// Milliseconds since the epoch.
    pub installed_ms: i64,
    pub updated_ms: i64,
    pub app_bytes: u64,
    pub data_bytes: u64,
    pub cache_bytes: u64,
}

/// What can be done to an Android app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Stop,
    Uninstall,
    ClearCache,
    ClearData,
    /// Opens Android's own settings for it: permissions, notifications.
    Settings,
}

impl Action {
    fn wire(self) -> &'static str {
        match self {
            Action::Stop => "stop",
            Action::Uninstall => "uninstall",
            Action::ClearCache => "clear_cache",
            Action::ClearData => "clear_data",
            Action::Settings => "settings",
        }
    }
}

/// Asks droidloomd, and waits for its answer; blocks.
fn ask(request: Value) -> Result<Value, String> {
    let mut stream =
        UnixStream::connect(SOCKET).map_err(|_| "Droidloom does not run".to_owned())?;
    let answer = (|| {
        stream.set_read_timeout(Some(ANSWER))?;
        stream.write_all(&serde_json::to_vec(&request)?)?;
        stream.shutdown(std::net::Shutdown::Write)?;
        let mut text = String::new();
        (&mut stream).take(MAX_ANSWER).read_to_string(&mut text)?;
        Ok::<_, std::io::Error>(text)
    })()
    .map_err(|error| error.to_string())?;
    let answer: Value = serde_json::from_str(answer.trim()).map_err(|error| error.to_string())?;
    if answer["ok"] == true {
        Ok(answer)
    } else {
        Err(answer["message"]
            .as_str()
            .unwrap_or("Droidloom did not answer")
            .to_owned())
    }
}

/// How Android runs, or why it does not. Cheap: droidloomd only reads files
/// for it.
pub fn cell() -> Cell {
    let answer = match ask(json!({"command": "stats"})) {
        Ok(answer) => answer,
        Err(_) => return Cell::Absent,
    };
    let Some(stats) = answer.get("stats") else {
        // droidloomd says "cell is stopped" when nothing ended it.
        let why = answer["message"]
            .as_str()
            .filter(|message| *message != "cell is stopped")
            .map(str::to_owned);
        return Cell::Stopped(why);
    };
    let number = |key: &str| stats[key].as_u64();
    let text = |key: &str| stats[key].as_str().map(str::to_owned);
    Cell::Running(Stats {
        ready: stats["ready"] == true,
        up: Duration::from_millis(number("uptime_ms").unwrap_or(0)),
        ready_in: number("ready_ms").map(Duration::from_millis),
        memory: number("memory_bytes"),
        swap: number("swap_bytes"),
        cpu_usec: number("cpu_usec"),
        processes: number("processes"),
        threads: number("threads"),
        app_processes: number("app_processes"),
        android: text("android_version"),
        model: text("model"),
        name: text("device_name"),
    })
}

/// The state of Droidloom's unit; Stopped while the manager has not
/// loaded it.
pub async fn unit() -> Result<UnitState, String> {
    let bus = Connection::session()
        .await
        .map_err(|error| error.to_string())?;
    let path: zbus::zvariant::OwnedObjectPath = match bus
        .call_method(
            Some("org.freedesktop.systemd1"),
            "/org/freedesktop/systemd1",
            Some("org.freedesktop.systemd1.Manager"),
            "GetUnit",
            &(UNIT,),
        )
        .await
    {
        Ok(reply) => reply.body().deserialize().map_err(|error| error.to_string())?,
        Err(_) => return Ok(UnitState::default()),
    };
    let unit = zbus::proxy::Builder::<zbus::Proxy>::new(&bus)
        .destination("org.freedesktop.systemd1")
        .and_then(|builder| builder.path(path))
        .and_then(|builder| builder.interface("org.freedesktop.systemd1.Unit"))
        .map_err(|error| error.to_string())?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .map_err(|error| error.to_string())?;
    let active: String = unit
        .get_property("ActiveState")
        .await
        .map_err(|error| error.to_string())?;
    let invocation: Vec<u8> = unit.get_property("InvocationID").await.unwrap_or_default();
    Ok(UnitState {
        unit: match active.as_str() {
            "active" | "reloading" | "refreshing" => Unit::Running,
            "activating" => Unit::Starting,
            "deactivating" => Unit::Stopping,
            "failed" => Unit::Failed,
            _ => Unit::Stopped,
        },
        invocation: invocation.try_into().unwrap_or_default(),
    })
}

/// What Droidloom said last in the run of `state`: why it failed. Blocks.
pub fn failure(state: UnitState) -> Option<String> {
    let invocation: String = state
        .invocation
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let output = std::process::Command::new("journalctl")
        .args(["--user", "--quiet", "--no-pager", "--output=cat", "--lines=1"])
        .arg(format!("_SYSTEMD_USER_UNIT={UNIT}"))
        .arg(format!("_SYSTEMD_INVOCATION_ID={invocation}"))
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let line = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    // Droidloom's tools name themselves first: "droidloomctl: …".
    let said = match line.split_once(": ") {
        Some((tool, said)) if tool.starts_with("droidloom") && !tool.contains(' ') => said,
        _ => &line,
    };
    (!said.is_empty()).then(|| said.to_owned())
}

/// The density Android draws its display at, and the one it started with,
/// in dots per inch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Density {
    pub dpi: u32,
    pub initial: u32,
}

/// Asks Android for its display's density; it must be running.
pub fn density() -> Result<Density, String> {
    let answer = ask(json!({"command": "display"}))?;
    let display = &answer["display"];
    let dpi = |key: &str| {
        display[key]
            .as_u64()
            .and_then(|dpi| u32::try_from(dpi).ok())
            .ok_or_else(|| "Droidloom did not say the density".to_owned())
    };
    Ok(Density {
        dpi: dpi("dpi")?,
        initial: dpi("initial_dpi")?,
    })
}

/// Makes Android draw at `dpi` from now on; its apps redraw at once.
pub fn set_density(dpi: u32) -> Result<(), String> {
    ask(json!({"command": "set_dpi", "spec": SPEC, "dpi": dpi})).map(drop)
}

/// The release Droidloom runs, as its updater recorded it.
pub fn release() -> Option<String> {
    let record: Value = serde_json::from_str(&std::fs::read_to_string(RELEASE).ok()?).ok()?;
    record["release"].as_str().map(str::to_owned)
}

/// Asks Android about the app `package`.
pub fn details(package: &str) -> Result<Details, String> {
    let answer = ask(json!({
        "command": "application_details",
        "package": package,
        "user": 0,
    }))?;
    let app = &answer["application"];
    let number = |key: &str| app[key].as_u64().unwrap_or(0);
    Ok(Details {
        version: app["version"].as_str().unwrap_or_default().to_owned(),
        system: app["system"] == true,
        installed_ms: app["installed_ms"].as_i64().unwrap_or(0),
        updated_ms: app["updated_ms"].as_i64().unwrap_or(0),
        app_bytes: number("app_bytes"),
        data_bytes: number("data_bytes"),
        cache_bytes: number("cache_bytes"),
    })
}

/// Does `action` to the app `package`.
pub fn act(package: &str, action: Action) -> Result<(), String> {
    ask(json!({
        "command": "application_action",
        "package": package,
        "user": 0,
        "action": action.wire(),
    }))
    .map(drop)
}

/// Opens an Android app in its window.
pub fn open(app: &crate::apps::Android) -> Result<(), String> {
    let mut request = json!({
        "command": "launch",
        "spec": SPEC,
        "package": app.package,
        "user": 0,
    });
    if let Some(component) = &app.component {
        request["component"] = json!(component);
    }
    ask(request).map(drop)
}

/// Starts Android afresh: its apps close, and open again on a tap.
pub async fn restart() -> Result<(), String> {
    let bus = Connection::session()
        .await
        .map_err(|error| error.to_string())?;
    let _ = manager(&bus, "ResetFailedUnit", &(UNIT,)).await;
    manager(&bus, "RestartUnit", &(UNIT, "replace")).await
}

/// The link that starts Droidloom with the session.
fn link() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|config| {
            config
                .join("systemd/user/graphical-session.target.wants")
                .join(UNIT)
        })
}

pub fn installed() -> bool {
    std::path::Path::new(INSTALLED).exists()
}

/// Whether Droidloom starts with the session.
pub fn starts() -> bool {
    link().is_some_and(|link| link.symlink_metadata().is_ok())
}

/// Starts Droidloom now and with each session, or stops it and leaves it
/// stopped.
pub async fn set(on: bool) -> Result<(), String> {
    let link = link().ok_or("No home folder")?;
    if on {
        if let Some(folder) = link.parent() {
            std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
        }
        match std::os::unix::fs::symlink(INSTALLED, &link) {
            Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => {
                return Err(error.to_string());
            }
            _ => {}
        }
    } else {
        match std::fs::remove_file(&link) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.to_string());
            }
            _ => {}
        }
    }
    let bus = Connection::session()
        .await
        .map_err(|error| error.to_string())?;
    // The user's manager reads the link again, for the next session.
    manager(&bus, "Reload", &()).await?;
    if on {
        // A start that failed before leaves the unit refusing more.
        let _ = manager(&bus, "ResetFailedUnit", &(UNIT,)).await;
        manager(&bus, "StartUnit", &(UNIT, "replace")).await
    } else {
        manager(&bus, "StopUnit", &(UNIT, "replace")).await
    }
}

async fn manager<B>(bus: &Connection, method: &str, body: &B) -> Result<(), String>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    bus.call_method(
        Some("org.freedesktop.systemd1"),
        "/org/freedesktop/systemd1",
        Some("org.freedesktop.systemd1.Manager"),
        method,
        body,
    )
    .await
    .map(drop)
    .map_err(|error| error.to_string())
}
