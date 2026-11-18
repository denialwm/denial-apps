//! Welcome's name on the session bus and the record that its tour is done.
//!
//! `settings --welcome` is Welcome: the same program as its own app,
//! `org.denialwm.Welcome`. Denial starts it with `--autostart` on every
//! start of a desktop session, and it opens only until the tour was
//! finished or left once. The launcher's entry always opens it.

use std::path::PathBuf;
use std::{env, fs, io, process};

pub const NAME: &str = "org.denialwm.Welcome";

/// An XDG base directory: `variable` when it names an absolute path, else
/// `fallback` under the home directory.
fn base(variable: &str, fallback: &str) -> Option<PathBuf> {
    env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| Some(PathBuf::from(env::var_os("HOME")?).join(fallback)))
}

/// The record that the tour is done. It is not a setting: resetting the
/// look must not open Welcome again. Removing it does.
fn marker() -> Option<PathBuf> {
    Some(base("XDG_CONFIG_HOME", ".config")?.join("denial/welcome"))
}

/// Where the first Welcome kept its record.
fn old_marker() -> Option<PathBuf> {
    Some(base("XDG_STATE_HOME", ".local/state")?.join("denial/welcome-completed"))
}

/// Whether the tour was finished or left before. A record in the old place
/// counts, and moves to the new one.
pub fn completed() -> bool {
    if marker().is_some_and(|marker| marker.exists()) {
        return true;
    }
    if old_marker().is_some_and(|marker| marker.exists()) {
        if let Err(error) = complete() {
            eprintln!("settings: moving Welcome's record: {error}");
        }
        return true;
    }
    false
}

/// Records that the tour is done, whole or not at all.
pub fn complete() -> io::Result<()> {
    let marker = marker().ok_or_else(|| io::Error::other("no home directory"))?;
    if let Some(parent) = marker.parent() {
        fs::create_dir_all(parent)?;
    }
    let draft = marker.with_extension(format!("{}.tmp", process::id()));
    let written = fs::write(&draft, "1\n").and_then(|()| fs::rename(&draft, &marker));
    if written.is_err() {
        let _ = fs::remove_file(&draft);
    }
    written?;
    if let Some(old) = old_marker() {
        let _ = fs::remove_file(old);
    }
    Ok(())
}
