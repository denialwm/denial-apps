//! Android through Droidloom. Whether it runs is Droidloom's user unit,
//! which starts with the session while the user's
//! `graphical-session.target.wants` links it. The link points at the unit
//! in /usr/lib, as Droidloom's updater expects; `systemctl enable` would pin
//! the release the unit comes from. How it runs, and its apps, come from
//! droidloomd's control socket: one JSON request, one JSON answer.
//!
//! The unit does not start until Droidloom is set up for the account
//! (`ConditionPathExists=/etc/droidloom/cell.json`): systemd skips it
//! without a word. A start from here sets Droidloom up first where it is
//! not, as `droidloomctl start` does, and waits for systemd to finish the
//! start, so it ends with Droidloom running or with why it is not.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zbus::Connection;

const UNIT: &str = "droidloom.service";
const INSTALLED: &str = "/usr/lib/systemd/user/droidloom.service";
const SOCKET: &str = "/run/droidloom/control.sock";
const SPEC: &str = "/etc/droidloom/cell.json";
const RELEASE: &str = "/usr/lib/droidloom/last-update.json";
/// Droidloom's setup helper: `prepare` sets Droidloom up for the calling
/// account where it is not, asking for an administrator through polkit.
const HELPER: &str = "/usr/lib/droidloom/droidloom-package-helper";
/// The longest systemd works on a start of the unit: its TimeoutStartSec,
/// 150 s, and the start of droidloomd before it.
const START: Duration = Duration::from_secs(170);
const JOB_POLL: Duration = Duration::from_millis(250);
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
    /// Stopped because systemd skipped its last start: Droidloom was not
    /// set up then.
    pub skipped: bool,
}

/// Why a start leaves nothing running: the unit's condition.
pub const NOT_SET_UP: &str = "Droidloom is not set up for this account yet";

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
        Ok(reply) => reply
            .body()
            .deserialize()
            .map_err(|error| error.to_string())?,
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
    // Before any start the result is false too, but with no time to it.
    let condition: bool = unit.get_property("ConditionResult").await.unwrap_or(true);
    let checked: u64 = unit
        .get_property("ConditionTimestampMonotonic")
        .await
        .unwrap_or_default();
    let unit = match active.as_str() {
        "active" | "reloading" | "refreshing" => Unit::Running,
        "activating" => Unit::Starting,
        "deactivating" => Unit::Stopping,
        "failed" => Unit::Failed,
        _ => Unit::Stopped,
    };
    Ok(UnitState {
        unit,
        invocation: invocation.try_into().unwrap_or_default(),
        skipped: unit == Unit::Stopped && !condition && checked > 0,
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
        .args([
            "--user",
            "--quiet",
            "--no-pager",
            "--output=cat",
            "--lines=1",
        ])
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

/// The OpenGL driver Android games start with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamesDriver {
    /// ANGLE, over the Vulkan driver.
    Angle,
    /// Mesa's own OpenGL driver.
    Mesa,
}

/// Asks Droidloom which OpenGL driver games start with; Android must be
/// running.
pub fn games_driver() -> Result<GamesDriver, String> {
    let answer = ask(json!({"command": "games_driver"}))?;
    match answer["games_driver"].as_str() {
        Some("angle") => Ok(GamesDriver::Angle),
        Some("mesa") => Ok(GamesDriver::Mesa),
        _ => Err("Droidloom did not say the driver".to_owned()),
    }
}

/// Starts games on `driver` from now on; open games keep theirs until they
/// start again.
pub fn set_games_driver(driver: GamesDriver) -> Result<(), String> {
    let driver = match driver {
        GamesDriver::Angle => "angle",
        GamesDriver::Mesa => "mesa",
    };
    ask(json!({"command": "set_games_driver", "driver": driver})).map(drop)
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

/// Whether Droidloom is set up: its setup wrote the cell's specification.
pub fn set_up() -> bool {
    Path::new(SPEC).exists()
}

/// Sets Droidloom up for this account where it is not, or no longer fits
/// the installed release or the GPUs, as `droidloomctl start` does before it
/// starts: the helper asks for an administrator through polkit, whose agent
/// in the session shows the dialog. Blocks until it is done.
pub fn prepare() -> Result<(), String> {
    let output = Command::new(HELPER)
        .arg("prepare")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("{HELPER}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(setup_failure(&String::from_utf8_lossy(&output.stderr)))
    }
}

/// Why setup failed, from what the helper said: the first of its "failed"
/// lines, which is the setup's own when pkexec ran it, else its last line.
fn setup_failure(said: &str) -> String {
    const FAILED: &str = "Droidloom package setup failed: ";
    said.lines()
        .find_map(|line| line.strip_prefix(FAILED))
        .or_else(|| said.lines().rev().find(|line| !line.trim().is_empty()))
        .map_or_else(
            || "Droidloom's setup did not finish".to_owned(),
            |why| why.trim().to_owned(),
        )
}

/// Starts Droidloom's unit, afresh with `restart` (its apps close and open
/// again on a tap), and waits for systemd to finish the start: Ok while the
/// unit runs, from where Android boots; Err why it does not.
pub async fn start(restart: bool) -> Result<(), String> {
    let bus = Connection::session()
        .await
        .map_err(|error| error.to_string())?;
    // The unit or its link may be new since the manager read them.
    manager(&bus, "Reload", &()).await?;
    // A start that failed before leaves the unit refusing more.
    let _ = manager(&bus, "ResetFailedUnit", &(UNIT,)).await;
    let method = if restart { "RestartUnit" } else { "StartUnit" };
    let job: zbus::zvariant::OwnedObjectPath = bus
        .call_method(
            Some("org.freedesktop.systemd1"),
            "/org/freedesktop/systemd1",
            Some("org.freedesktop.systemd1.Manager"),
            method,
            &(UNIT, "replace"),
        )
        .await
        .map_err(|error| error.to_string())?
        .body()
        .deserialize()
        .map_err(|error| error.to_string())?;
    let asked = Instant::now();
    while job_pending(&bus, &job).await {
        if asked.elapsed() > START {
            return Err(format!(
                "Droidloom did not start within {} seconds",
                START.as_secs()
            ));
        }
        async_io::Timer::after(JOB_POLL).await;
    }
    let state = unit().await?;
    match state.unit {
        Unit::Running | Unit::Starting => Ok(()),
        Unit::Failed => {
            Err(failure(state).unwrap_or_else(|| "Droidloom failed to start".to_owned()))
        }
        _ if state.skipped => Err(NOT_SET_UP.to_owned()),
        _ => Err("Droidloom stopped while it started".to_owned()),
    }
}

/// Whether systemd still works on `job`: it forgets a job once it is done.
async fn job_pending(bus: &Connection, job: &zbus::zvariant::OwnedObjectPath) -> bool {
    bus.call_method(
        Some("org.freedesktop.systemd1"),
        job.as_str(),
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &("org.freedesktop.systemd1.Job", "State"),
    )
    .await
    .is_ok()
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

/// The densities Droidloom gives Android apps by the screen they are on, in
/// dots per inch per unit of the screen's scale: one for every screen, and
/// one for some screens by connector. Droidloom watches the file
/// (`$XDG_CONFIG_HOME/droidloom/density-v1.json`), so open apps follow.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Densities {
    pub every: Option<f32>,
    pub outputs: std::collections::BTreeMap<String, f32>,
}

fn densities_file() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|config| config.join("droidloom/density-v1.json"))
}

fn read_densities() -> Value {
    densities_file()
        .and_then(|file| std::fs::read(file).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

pub fn densities() -> Densities {
    let file = read_densities();
    let number = |value: &Value| value.as_f64().map(|n| n as f32);
    Densities {
        every: number(&file["dpiPerScale"]),
        outputs: file["outputs"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(name, value)| Some((name.clone(), number(value)?)))
            .collect(),
    }
}

/// Sets the density for every screen, or with `output` for that screen
/// alone; None for a screen drops its own, so it takes every screen's.
pub fn set_densities(output: Option<&str>, per_scale: Option<f32>) -> std::io::Result<()> {
    let file = densities_file().ok_or(std::io::ErrorKind::NotFound)?;
    let mut densities = read_densities();
    match (output, per_scale) {
        (None, per_scale) => densities["dpiPerScale"] = json!(per_scale),
        (Some(name), Some(per_scale)) => {
            if !densities["outputs"].is_object() {
                densities["outputs"] = json!({});
            }
            densities["outputs"][name] = json!(per_scale);
        }
        (Some(name), None) => {
            if let Some(outputs) = densities["outputs"].as_object_mut() {
                outputs.remove(name);
            }
        }
    }
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let temporary = file.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(&densities)?)?;
    std::fs::rename(&temporary, &file)
}

/// Whether Droidloom starts with the session.
pub fn starts() -> bool {
    link().is_some_and(|link| link.symlink_metadata().is_ok())
}

/// Starts Droidloom with each session, or stops it now and leaves it
/// stopped. Starting it now is `start`'s, after `prepare`.
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
    if on {
        // The start that follows has the manager read the link.
        return Ok(());
    }
    let bus = Connection::session()
        .await
        .map_err(|error| error.to_string())?;
    // The user's manager reads the link again, for the next session.
    manager(&bus, "Reload", &()).await?;
    manager(&bus, "StopUnit", &(UNIT, "replace")).await
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

#[cfg(test)]
mod tests {
    use super::setup_failure;

    #[test]
    fn setup_failure_is_the_setup_s_own_reason() {
        // pkexec ran the setup, which failed: its own line comes first.
        let said = "Administrator authentication is needed to configure Droidloom for dmitry (UID 1000).\n\
            Droidloom package setup failed: Droidloom needs an AMD, Intel, NVIDIA, Qualcomm Adreno or Arm Mali GPU: none found\n\
            Droidloom package setup failed: the setup ended with status 1; Droidloom was not set up\n";
        assert_eq!(
            setup_failure(said),
            "Droidloom needs an AMD, Intel, NVIDIA, Qualcomm Adreno or Arm Mali GPU: none found"
        );
        // The dialog was dismissed: pkexec's words, then the helper's.
        let said = "Error executing command as another user: Request dismissed\n\
            Droidloom package setup failed: the administrator authentication was dismissed; Droidloom was not set up\n";
        assert_eq!(
            setup_failure(said),
            "the administrator authentication was dismissed; Droidloom was not set up"
        );
        // A helper that says something else: its last line.
        assert_eq!(setup_failure("one\ntwo\n\n"), "two");
        assert_eq!(setup_failure(""), "Droidloom's setup did not finish");
    }
}
