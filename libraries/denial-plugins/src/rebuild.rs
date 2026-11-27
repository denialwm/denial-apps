//! Rebuilding the composition deniald last confirmed, which a Denial update
//! turned away (PLUGIN_MANAGER.md, section 21). deniald runs the packaged
//! shell meanwhile.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::store::{Store, validate_id};
use crate::tools::source_identity;
use crate::util::{Map, Result, content_key, fail, is_word, name_of, obj, text};

/// The installed build tools do not belong to the installed Denial.
pub const BUILD_KIT_MISMATCH: &str = "These plugin build tools belong to another Denial than the one this session runs. Use the plugin manager that came with it, then try again.";

pub struct Target {
    /// The candidate deniald last confirmed.
    pub candidate: String,
    pub bundle: String,
    /// The installed release, or a development build identity.
    pub version: String,
    /// The installed shell's source identity; None for a development bundle.
    pub installed_source: Option<Value>,
}

impl Target {
    /// What deniald waits for, or None when it waits for nothing.
    pub fn from_native(store: &Store, native: &Map) -> Result<Option<Self>> {
        let waiting = obj(native.get("plugin_rebuild").unwrap_or(&Value::Null));
        let Some(bundle) = text(waiting, "bundle") else {
            return Ok(None);
        };
        let path = Path::new(bundle);
        let candidate = path.parent().map(name_of).unwrap_or_default();
        if name_of(path) != "bundle"
            || !is_word(&candidate)
            || !store
                .file(&format!("candidates/{candidate}/plan.json"))
                .exists()
        {
            return fail(
                "The plugins Denial was running were not built by this plugin manager. Open Plugins and apply your selection again.",
            );
        }
        if text(
            &store.read(&format!("candidates/{candidate}/build.json"))?,
            "bundle",
        ) != Some(bundle)
        {
            return fail(
                "The plugins that were running can no longer be rebuilt. Open Plugins and apply your selection again.",
            );
        }
        Ok(Some(Self {
            candidate,
            bundle: bundle.to_owned(),
            version: text(waiting, "version").unwrap_or("").to_owned(),
            installed_source: waiting
                .get("installed_source")
                .filter(|source| source.is_object())
                .cloned(),
        }))
    }
}

/// A tagged release, which has release notes.
pub fn is_release(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether deniald still waits for candidate `base` to be rebuilt.
pub fn waits_for_rebuild_of(store: &Store, native: &Map, base: &str) -> Result<bool> {
    validate_id(base)?;
    let waiting = obj(native.get("plugin_rebuild").unwrap_or(&Value::Null));
    let Some(bundle) = text(waiting, "bundle") else {
        return Ok(false);
    };
    Ok(text(
        &store.read(&format!("candidates/{base}/build.json"))?,
        "bundle",
    ) == Some(bundle))
}

/// Tools of another Denial would build what deniald turns away again:
/// compare before compiling anything.
pub fn require_matching_kit(runtime: &Path, target: &Target) -> Result<()> {
    let Some(installed) = &target.installed_source else {
        return Ok(());
    };
    if content_key(&source_identity(runtime)?) != content_key(installed) {
        return fail(BUILD_KIT_MISMATCH);
    }
    Ok(())
}

/// How long to wait before asking deniald again, or None at a quiet moment
/// to switch the shell: unlocked, no input for `idle`, no keyboard capture
/// by the shell, nothing visible inhibiting idle (a playing video), no
/// runtime transition.
pub fn until_quiet_moment(native: &Map, idle: Duration) -> Option<Duration> {
    let activity = native.get("session_activity")?.as_object()?;
    if text(native, "operation") != Some("idle") {
        return Some(Duration::from_secs(1));
    }
    if activity.get("locked") == Some(&Value::Bool(true)) {
        return Some(Duration::from_secs(2));
    }
    if activity.get("shell_captures_keyboard") == Some(&Value::Bool(true)) {
        return Some(Duration::from_secs(1));
    }
    if activity.get("idle_inhibited") == Some(&Value::Bool(true)) {
        return Some(Duration::from_secs(5));
    }
    let quiet = activity.get("input_idle_ms")?.as_i64()?;
    let remaining = idle.as_millis() as i64 - quiet;
    // Ask again just after the threshold would pass without more input.
    (remaining > 0).then(|| Duration::from_millis(remaining as u64 + 100))
}

/// Asks a waiting rebuild to switch now; Plugins offers it while it waits.
pub fn switch_now_request(store: &Store) -> PathBuf {
    store.file("rebuild-now")
}

/// Waits for a quiet moment while deniald still waits for `base`, or until
/// someone asks to switch now. False when the user meanwhile chose the
/// packaged shell or another composition. A session that stays away ends the
/// wait with an error.
pub fn wait_for_quiet_moment(
    store: &Store,
    status: &dyn Fn() -> Map,
    base: &str,
    progress: &dyn Fn(&str),
    sleep: &dyn Fn(Duration),
) -> Result<bool> {
    let request = switch_now_request(store);
    let requested = || request.exists();
    // Asking deniald starts a process; the user's request costs nothing.
    let pause = |duration: Duration| {
        let slice = Duration::from_millis(250);
        let mut left = duration;
        while !left.is_zero() {
            if requested() {
                return;
            }
            let step = left.min(slice);
            sleep(step);
            left -= step;
        }
    };
    let mut announced = false;
    let mut unavailable = 0;
    loop {
        let native = status();
        if native.get("available") == Some(&Value::Bool(false)) {
            unavailable += 1;
            if unavailable > 30 {
                return fail(
                    "Denial is not available. Your plugins will be rebuilt at your next login.",
                );
            }
            pause(Duration::from_secs(1));
            continue;
        }
        unavailable = 0;
        if !waits_for_rebuild_of(store, &native, base)? {
            return Ok(false);
        }
        let wait = if requested() {
            None
        } else {
            until_quiet_moment(&native, Duration::from_millis(2500))
        };
        let Some(wait) = wait else {
            return Ok(true);
        };
        if !announced {
            progress("Waiting for a pause to switch");
            announced = true;
        }
        pause(wait);
    }
}
