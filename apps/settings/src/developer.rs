//! The Flutter shell as Denial runs it, for who develops it: the packaged
//! shell, a custom optimized one, or the source run live with hot reload.
//! Denial's compositor owns all of it and answers on its control socket
//! (`ui.*`, Denial's docs/protocol/control-v1.md), so it stays reachable
//! when a custom shell shows nothing.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::denial::{Link, Refusal};

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Diagnostic {
    /// information, warning or error.
    pub severity: String,
    pub message: String,
    pub path: String,
    pub line: u32,
    pub column: u32,
}

impl Diagnostic {
    /// Where it is in the source: `lib/main.dart:12:3`, or nothing.
    pub fn location(&self) -> Option<String> {
        match (self.path.is_empty(), self.line) {
            (true, _) => None,
            (false, 0) => Some(self.path.clone()),
            (false, line) => Some(format!("{}:{line}:{}", self.path, self.column)),
        }
    }
}

/// What `ui.get` answers.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct State {
    /// official_optimized, custom_optimized or live_development.
    pub active_mode: String,
    pub desired_mode: String,
    /// idle, or what Denial is doing to the shell.
    pub operation: String,
    pub developer_components_available: bool,
    pub workspace_valid: bool,
    pub auto_reload: bool,
    pub auto_reload_supported: bool,
    pub can_hot_reload: bool,
    pub can_hot_restart: bool,
    pub can_build_optimized: bool,
    pub can_revert: bool,
    pub generation: u64,
    pub workspace: String,
    pub vm_service_uri: String,
    pub status: String,
    pub error: String,
    pub diagnostics: Vec<Diagnostic>,
    /// How far the operation is, in hundredths of a percent, when it can
    /// tell.
    pub progress_basis_points: Option<u32>,
}

pub const OFFICIAL: &str = "official_optimized";
pub const LIVE: &str = "live_development";

impl State {
    pub fn busy(&self) -> bool {
        self.operation != "idle"
    }

    /// Live development runs, or Denial is on its way to it.
    pub fn live(&self) -> bool {
        self.active_mode == LIVE || self.desired_mode == LIVE
    }

    /// Whether the phone or computer has anything for a developer: live
    /// development installed or set up, or a shell other than the packaged
    /// one to leave or go back to.
    pub fn offers(&self) -> bool {
        self.developer_components_available
            || self.workspace_valid
            || self.can_build_optimized
            || self.can_revert
            || (!self.active_mode.is_empty() && self.active_mode != OFFICIAL)
    }
}

fn state(answer: Value) -> Result<State, Refusal> {
    Ok(State::deserialize(answer)?)
}

/// The shell's state now.
pub async fn read(link: &Link) -> Result<State, Refusal> {
    state(link.ui("ui.get", None).await?)
}

/// What a developer asks of the shell.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    Workspace(String),
    Live(bool),
    AutoReload(bool),
    Reload,
    Restart,
    Build,
    Revert,
    Restore,
}

impl Ask {
    fn method(&self) -> (&'static str, Option<Value>) {
        match self {
            Ask::Workspace(path) => ("ui.workspace.set", Some(json!({"path": path}))),
            Ask::Live(true) => ("ui.live.enable", None),
            Ask::Live(false) => ("ui.live.disable", None),
            Ask::AutoReload(enabled) => ("ui.auto_reload.set", Some(json!({"enabled": enabled}))),
            Ask::Reload => ("ui.reload", None),
            Ask::Restart => ("ui.restart", None),
            Ask::Build => ("ui.build", None),
            Ask::Revert => ("ui.revert", None),
            Ask::Restore => ("ui.restore", None),
        }
    }
}

/// Asks Denial; its answer is the state once it accepted, not once it is
/// done: the operation shows in the state until then.
pub async fn ask(link: &Link, ask: &Ask) -> Result<State, Refusal> {
    let (method, params) = ask.method();
    state(link.ui(method, params).await?)
}

fn tool(variable: &str, name: &str) -> Option<PathBuf> {
    let named = std::env::var_os(variable).map(PathBuf::from);
    named.filter(|path| path.is_file()).or_else(|| {
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|folder| folder.join(name))
            .find(|path| path.is_file())
    })
}

/// Whether an editable copy of the shell can be made here: Denial's
/// control tool and its development tool (package `denial-ui-development`)
/// are both installed.
pub fn can_set_up() -> bool {
    tool("DENIAL_CONTROL_TOOL", "denialctl").is_some()
        && tool("DENIAL_DEVELOPMENT_TOOL", "denial-ui").is_some()
}

/// Makes an editable copy of the shell's source and starts it live. It
/// fetches and builds, so it takes minutes; an error is what the tool said.
pub fn set_up() -> Result<(), String> {
    let control =
        tool("DENIAL_CONTROL_TOOL", "denialctl").ok_or("Denial's control tool is not installed")?;
    let output = Command::new(control)
        .args(["--json", "ui", "setup"])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let said = |bytes: &[u8]| String::from_utf8_lossy(bytes).trim().to_owned();
    let (error, printed) = (said(&output.stderr), said(&output.stdout));
    Err(if !error.is_empty() {
        error
    } else if !printed.is_empty() {
        printed
    } else {
        format!("denialctl ended with {}", output.status)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_state_denial_answers() {
        let answer = json!({
            "acknowledged_request_id": 7,
            "active_mode": "official_optimized",
            "desired_mode": "live_development",
            "operation": "switching_runtime",
            "developer_components_available": true,
            "workspace_valid": true,
            "auto_reload": true,
            "auto_reload_supported": false,
            "can_revert": true,
            "generation": 4,
            "workspace": "/home/me/DenialUI/dart_shell",
            "vm_service_uri": "",
            "status": "Starting the live shell",
            "error": "",
            "progress_basis_points": 2500,
            "diagnostics": [
                {"severity": "error", "message": "Expected ';'", "path": "lib/main.dart", "line": 12, "column": 3},
                {"severity": "warning", "message": "Slow frame", "path": "", "line": 0, "column": 0},
            ],
            "session_activity": {"locked": false},
        });
        let Ok(state) = state(answer) else {
            panic!("the state was not read");
        };
        assert!(state.busy() && state.live() && state.offers());
        assert_eq!(state.progress_basis_points, Some(2500));
        assert_eq!(
            state.diagnostics[0].location().as_deref(),
            Some("lib/main.dart:12:3")
        );
        assert_eq!(state.diagnostics[1].location(), None);
    }

    #[test]
    fn a_phone_with_the_packaged_shell_offers_nothing() {
        let state = State {
            active_mode: OFFICIAL.into(),
            desired_mode: OFFICIAL.into(),
            operation: "idle".into(),
            ..State::default()
        };
        assert!(!state.offers() && !state.busy() && !state.live());
    }

    #[test]
    fn asks_name_their_methods() {
        assert_eq!(Ask::Live(true).method().0, "ui.live.enable");
        assert_eq!(Ask::Live(false).method().0, "ui.live.disable");
        assert_eq!(
            Ask::Workspace("/w".into()).method(),
            ("ui.workspace.set", Some(json!({"path": "/w"})))
        );
        assert_eq!(
            Ask::AutoReload(false).method().1,
            Some(json!({"enabled": false}))
        );
    }
}
