//! Denial's plugins, through `denial-plugins`, the plugin manager of the
//! Denial that runs. The manager decides what runs, why, what failed and
//! what can be done now; Settings shows its answer and works nothing out
//! (Denial's docs/PLUGIN_MANAGER.md, section 23). Every sentence comes
//! with a code: Settings words the codes it knows, and shows the
//! manager's English for the rest.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;

/// The contract Settings reads.
const CONTRACT: u64 = 1;

/// The plugin manager, as the session names it or as installed; None on a
/// phone without it, which then has no Plugins page.
pub fn command() -> Option<PathBuf> {
    let named = std::env::var_os("DENIAL_PLUGINS_BINARY").map(PathBuf::from);
    named.filter(|path| path.is_file()).or_else(|| {
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|folder| folder.join("denial-plugins"))
            .find(|path| path.is_file())
    })
}

/// A sentence of the manager's, and the code that names it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Line {
    pub code: String,
    pub text: String,
    /// The plugins it is about: the ones running, or the ones in conflict.
    pub plugins: Vec<String>,
    pub feature: Option<String>,
    pub version: Option<String>,
    pub message: Option<String>,
    pub expected: Option<String>,
    pub installed: Option<String>,
    pub required: Option<String>,
}

/// The request that runs now.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Work {
    pub operation: String,
    pub label: String,
    pub step: Option<u32>,
    pub steps: Option<u32>,
    /// When this step began, in UTC.
    pub started: Option<String>,
}

/// The last request, which failed.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Failure {
    pub job: String,
    pub code: String,
    pub text: String,
    /// The message it ended with.
    pub detail: String,
}

/// What can be offered now.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Can {
    pub change: bool,
    pub apply: bool,
    pub undo: bool,
    pub off: bool,
    pub switch_now: bool,
}

/// What `denial-plugins status` says.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Summary {
    contract: u64,
    pub desktop: Line,
    pub why: Vec<Line>,
    pub issues: Vec<Line>,
    /// What the last build took in besides the selected plugins.
    pub included: Vec<Included>,
    pub setup: Line,
    pub work: Option<Work>,
    pub failure: Option<Failure>,
    pub next: Option<Line>,
    pub can: Can,
}

/// A plugin a build took in because a selected one needs it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Included {
    pub name: String,
    pub title: String,
    /// From a selected plugin down to the one that needs it.
    pub through: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Plugin {
    pub name: String,
    pub title: String,
    pub description: Option<String>,
    /// builtin, local or git.
    pub kind: Option<String>,
    /// The folder or the link it comes from.
    pub from: Option<String>,
    pub selected: bool,
    /// False for a plugin of the community list that was never added.
    pub installed: bool,
}

/// The list of plugins others published.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Community {
    /// Whether this Denial has such a list at all.
    pub configured: bool,
    /// When it was last read, in UTC.
    pub updated: Option<String>,
}

/// What `denial-plugins list` says.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Listing {
    pub plugins: Vec<Plugin>,
    /// Whether the selection is exactly the plugins Denial comes with.
    pub default_selection: bool,
    pub community: Community,
    /// Why the community list could not be loaded.
    pub errors: Vec<String>,
}

/// A request the manager remembers.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Entry {
    pub job: String,
    pub operation: String,
    /// waiting, working, done, failed or stopped.
    pub state: String,
    /// When it ended or last said something, in UTC.
    pub when: String,
    /// Of one that failed: why, as a failure says it.
    pub code: String,
    pub text: String,
    pub detail: String,
}

/// A plugin package found in a folder or behind a link.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Found {
    pub name: String,
    pub path: String,
}

/// Runs the manager and reads its JSON answer; an error is what it said.
fn ask(arguments: &[&str]) -> Result<Value, String> {
    let command = command().ok_or("The plugin manager is not installed")?;
    let output = Command::new(command)
        .arg("--json")
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        // Its last line says why; the ones before are progress.
        let why = said
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix("denial-plugins: "))
            .unwrap_or_else(|| said.trim());
        return Err(why.to_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
}

/// What runs, why, and what can be done. An older manager, which says none
/// of it this way, is an error that names itself.
pub fn status() -> Result<Summary, String> {
    summary(&ask(&["status"])?)
}

fn summary(status: &Value) -> Result<Summary, String> {
    let summary = &status["summary"];
    if summary["contract"].as_u64() != Some(CONTRACT) {
        return Err(OLD.to_owned());
    }
    Summary::deserialize(summary).map_err(|error| error.to_string())
}

/// What `status` fails with on a Denial older than this page.
pub const OLD: &str =
    "This Denial's plugin manager is older than Settings. Update Denial to manage plugins here.";

/// Every plugin the manager knows, by name, and what it says of the
/// selection and of the community list.
pub fn list() -> Result<Listing, String> {
    Listing::deserialize(ask(&["list"])?).map_err(|error| error.to_string())
}

/// The requests the manager remembers, the newest first.
pub fn history() -> Result<Vec<Entry>, String> {
    let mut answer = ask(&["history"])?;
    Vec::<Entry>::deserialize(answer["entries"].take()).map_err(|error| error.to_string())
}

/// Makes the saved selection the plugins Denial comes with, and only
/// them. What was turned off stays in the list.
pub fn defaults() -> Result<(), String> {
    ask(&["defaults"]).map(drop)
}

/// Turns a plugin on or off in the saved selection. Nothing changes on
/// screen until the selection is applied.
pub fn select(name: &str, on: bool) -> Result<(), String> {
    ask(&[if on { "add" } else { "remove" }, name]).map(drop)
}

/// Starts a request that goes on by itself: `status` then reports it as
/// its work, and as its failure should it fail.
pub fn submit(request: &[&str]) -> Result<(), String> {
    let mut arguments = vec!["submit"];
    arguments.extend(request);
    ask(&arguments).map(drop)
}

/// Prepares plugin support in the background, where it is not yet.
pub fn prepare() -> Result<(), String> {
    ask(&["bootstrap"]).map(drop)
}

/// Asks a rebuild that waits for a pause to switch the desktop now.
pub fn switch_now() -> Result<(), String> {
    ask(&["rebuild-now"]).map(drop)
}

/// A folder is given by its path; anything else is a Git link.
pub fn is_folder(location: &str) -> bool {
    location.starts_with('/') || location.starts_with('~') || location.starts_with('.')
}

/// `location` as the manager takes it: a folder without its `~`.
pub fn located(location: &str) -> String {
    let location = location.trim();
    match (location.strip_prefix('~'), std::env::var("HOME")) {
        (Some(rest), Ok(home)) if rest.is_empty() || rest.starts_with('/') => {
            format!("{home}{rest}")
        }
        _ => location.to_owned(),
    }
}

/// The plugin packages in a folder or behind a link.
pub fn inspect(location: &str) -> Result<Vec<Found>, String> {
    let mut arguments = vec!["inspect", location];
    if is_folder(location) {
        arguments.push("--local");
    }
    Vec::<Found>::deserialize(ask(&arguments)?).map_err(|error| error.to_string())
}

/// Adds the package at `path` of `location` to the selection.
pub fn add(location: &str, path: &str) -> Result<(), String> {
    let mut request = vec!["add", location, "--path", path];
    if is_folder(location) {
        request.push("--local");
    }
    submit(&request)
}

/// What a request printed while it ran.
pub fn log(job: &str) -> Result<String, String> {
    let answer = ask(&["job-details", job])?;
    Ok(answer["log"].as_str().unwrap_or_default().trim().to_owned())
}

/// Seconds since `started`, a UTC time as the manager writes it
/// (`2026-11-18T10:00:00.123456Z`).
pub fn seconds_since(started: &str) -> Option<u64> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    now.checked_sub(epoch(started)?)
}

fn epoch(time: &str) -> Option<u64> {
    let number = |range: std::ops::Range<usize>| time.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !time.ends_with('Z') || !(1..=12).contains(&month) {
        return None;
    }
    // Days from 1970 of a civil date (Howard Hinnant's algorithm).
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year.rem_euclid(400);
    let of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let days = era * 146_097 + of_era * 365 + of_era / 4 - of_era / 100 + of_year - 719_468;
    u64::try_from(days * 86_400 + hour * 3_600 + minute * 60 + second).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_what_the_manager_says() {
        let status = json!({"summary": {
            "contract": 1,
            "desktop": {"code": "plugins", "text": "Your plugins are running: a, b", "plugins": ["a", "b"]},
            "selected": ["a", "b"],
            "why": [{"code": "rebuild_update", "text": "Denial changed.", "version": "0.6.4"}],
            "issues": [{"code": "too_many_providers", "text": "Desktop is provided by a and b.", "plugins": ["a", "b"], "feature": "Desktop"}],
            "included": [{"name": "denial_clock_core", "title": "Clock Core", "through": ["a"]}],
            "setup": {"code": "missing_dart", "text": "Dart 3.13.2 was not found.", "expected": "3.13.2"},
            "work": {"job": "1", "operation": "apply", "label": "Compiling your desktop", "step": 3, "steps": 5, "started": "2026-11-18T10:00:00Z"},
            "failure": null,
            "next": {"code": "wait", "text": "Wait for it."},
            "can": {"change": false, "apply": false, "undo": true, "off": true, "switch_now": false},
            "added_later": 7,
        }});
        let summary = summary(&status).unwrap();
        assert_eq!(summary.desktop.code, "plugins");
        assert_eq!(summary.desktop.plugins, ["a", "b"]);
        assert_eq!(summary.why[0].version.as_deref(), Some("0.6.4"));
        assert_eq!(summary.issues[0].feature.as_deref(), Some("Desktop"));
        assert_eq!(summary.included[0].through, ["a"]);
        assert_eq!(summary.setup.expected.as_deref(), Some("3.13.2"));
        let work = summary.work.unwrap();
        assert_eq!((work.step, work.steps), (Some(3), Some(5)));
        assert!(summary.failure.is_none());
        assert!(summary.can.undo && !summary.can.apply);
    }

    #[test]
    fn reads_the_list_and_the_history() {
        let listing = Listing::deserialize(json!({
            "contract": 1,
            "plugins": [
                {"name": "denial_clock", "title": "Clock", "kind": "builtin", "selected": true, "installed": true, "default": true},
                {"name": "weather", "title": "Weather", "kind": "git", "from": "https://example.org/weather.git", "installed": false},
            ],
            "default_selection": true,
            "community": {"configured": true, "updated": "2026-11-18T10:00:00Z"},
            "errors": ["no network"],
        }))
        .unwrap();
        assert!(listing.default_selection && listing.community.configured);
        assert!(listing.plugins[0].installed && !listing.plugins[1].installed);
        assert_eq!(listing.errors, ["no network"]);
        // A manager from before these were said.
        let old = Listing::deserialize(json!({"plugins": [{"name": "a"}], "errors": []})).unwrap();
        assert!(!old.default_selection && !old.community.configured);

        let entries = Vec::<Entry>::deserialize(json!([
            {"job": "4", "operation": "apply", "state": "failed", "when": "2026-11-18T12:00:00Z",
             "code": "did_not_start", "text": "They did not start.", "detail": "The shell showed nothing."},
            {"job": "1", "operation": "apply", "state": "done", "when": "2026-11-18T10:00:00Z"},
        ]))
        .unwrap();
        assert_eq!(entries[0].code, "did_not_start");
        assert!(entries[1].detail.is_empty());
    }

    #[test]
    fn an_older_manager_names_itself() {
        let old = json!({"summary": {"desktop": "The standard desktop is running."}});
        assert_eq!(summary(&old).unwrap_err(), OLD);
        assert_eq!(summary(&json!({})).unwrap_err(), OLD);
    }

    #[test]
    fn times_and_places_are_read() {
        assert_eq!(epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch("2026-11-18T10:00:00.123456Z"), Some(1_794_996_000));
        assert_eq!(epoch("2024-02-29T23:59:59Z"), Some(1_709_251_199));
        assert_eq!(epoch("yesterday"), None);
        assert!(is_folder("/home/me/plugin") && is_folder("~/plugin"));
        assert!(!is_folder("https://example.org/plugin.git"));
    }
}
