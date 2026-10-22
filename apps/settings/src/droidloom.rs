//! Whether Android apps run: Droidloom's user unit, which starts with the
//! session while the user's `graphical-session.target.wants` links it. The
//! link points at the unit in /usr/lib, as Droidloom's updater expects;
//! `systemctl enable` would pin the release the unit comes from.

use std::path::PathBuf;

use zbus::Connection;

const UNIT: &str = "droidloom.service";
const INSTALLED: &str = "/usr/lib/systemd/user/droidloom.service";

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
