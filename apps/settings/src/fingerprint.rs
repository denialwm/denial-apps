//! The fingers fprintd knows for this user on its default reader: listing
//! them, adding one and removing one.
//!
//! Adding and removing claim the reader for the time it takes, each on a
//! bus connection of its own: when Settings stops waiting, the connection
//! closes and fprintd lets the reader go. While the phone is locked Denial
//! holds the reader to unlock it, and it is busy here.
//!
//! An optical reader under the screen lights itself for each touch, but
//! nothing marks it while the phone is unlocked: Settings rings it, where
//! Denial's `/etc/denial/fingerprint.json` places it on the panel.

use std::collections::HashMap;
use std::path::PathBuf;

use futures_lite::StreamExt;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{Connection, MatchRule, MessageStream};

const NAME: &str = "net.reactivated.Fprint";
const DEVICE: &str = "net.reactivated.Fprint.Device";

/// fprintd's slots for fingers, taken in this order as fingerprints are
/// added. fprintd knows only these ten; the names people give fingerprints
/// are Settings' own, kept by slot in `names()`.
pub const SLOTS: [&str; 10] = [
    "right-thumb",
    "right-index-finger",
    "right-middle-finger",
    "right-ring-finger",
    "right-little-finger",
    "left-thumb",
    "left-index-finger",
    "left-middle-finger",
    "left-ring-finger",
    "left-little-finger",
];

/// The longest name a fingerprint takes, in characters.
pub const NAME_LENGTH: usize = 40;

fn names_file() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .map(|state| state.join("denial/settings/fingerprints.json"))
}

/// The names given to fingerprints, by fprintd's slot.
pub fn names() -> HashMap<String, String> {
    names_file()
        .and_then(|file| std::fs::read(file).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Keeps `names`, written beside the file and renamed over it.
pub fn save_names(names: &HashMap<String, String>) -> std::io::Result<()> {
    let file = names_file().ok_or(std::io::ErrorKind::NotFound)?;
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let temporary = file.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(names)?)?;
    std::fs::rename(&temporary, &file)
}

/// Where the sensor is on the panel, as fractions of its width and height:
/// left, top, width, height. None for a reader not under the screen.
pub fn sensor() -> Option<[f32; 4]> {
    let place: serde_json::Value =
        serde_json::from_slice(&std::fs::read("/etc/denial/fingerprint.json").ok()?).ok()?;
    let number = |key: &str| place[key].as_f64().map(|value| value as f32);
    let (panel_width, panel_height) = (number("panel_width")?, number("panel_height")?);
    (panel_width > 0.0 && panel_height > 0.0).then_some([
        number("x")? / panel_width,
        number("y")? / panel_height,
        number("width")? / panel_width,
        number("height")? / panel_height,
    ])
}

/// How adding a finger goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Progress {
    /// The reader takes up to `of` touches; `done` are in. Readers may
    /// count a touch as more than one, and finish early.
    Touches {
        done: u32,
        of: u32,
    },
    /// The last touch did not count, and why.
    Again(&'static str),
    Added,
    Failed(String),
}

async fn device(bus: &Connection) -> zbus::Result<OwnedObjectPath> {
    bus.call_method(
        Some(NAME),
        "/net/reactivated/Fprint/Manager",
        Some("net.reactivated.Fprint.Manager"),
        "GetDefaultDevice",
        &(),
    )
    .await?
    .body()
    .deserialize()
}

/// The fingers on the reader, in fprintd's names; None without a reader or
/// fprintd.
pub async fn enrolled() -> Option<Vec<String>> {
    let bus = Connection::system().await.ok()?;
    let device = device(&bus).await.ok()?;
    let listed = bus
        .call_method(
            Some(NAME),
            &device,
            Some(DEVICE),
            "ListEnrolledFingers",
            &(""),
        )
        .await;
    match listed {
        Ok(reply) => reply.body().deserialize::<Vec<String>>().ok(),
        Err(error) if fprint_error(&error) == Some("NoEnrolledPrints") => Some(Vec::new()),
        Err(error) => {
            eprintln!("settings: asking fprintd for fingers: {error}");
            None
        }
    }
}

/// The name of fprintd's error, after its prefix.
fn fprint_error(error: &zbus::Error) -> Option<&str> {
    match error {
        zbus::Error::MethodError(name, _, _) => {
            name.as_str().strip_prefix("net.reactivated.Fprint.Error.")
        }
        _ => None,
    }
}

/// What a row says when the reader would not do something.
fn refusal(error: &zbus::Error) -> String {
    match fprint_error(error) {
        Some("AlreadyInUse") => "The reader is busy".to_owned(),
        Some("PermissionDenied") => "Not allowed to change fingerprints".to_owned(),
        Some("NoSuchDevice") => "No fingerprint reader".to_owned(),
        _ => format!("The reader failed: {error}"),
    }
}

/// Claims the reader on a connection of its own, which lets it go when it
/// closes.
async fn claim() -> zbus::Result<(Connection, OwnedObjectPath)> {
    let bus = Connection::system().await?;
    let device = device(&bus).await?;
    bus.call_method(Some(NAME), &device, Some(DEVICE), "Claim", &(""))
        .await?;
    Ok((bus, device))
}

/// Removes `finger` from the reader.
pub async fn remove(finger: &str) -> Result<(), String> {
    let (bus, device) = claim().await.map_err(|error| refusal(&error))?;
    let removed = bus
        .call_method(
            Some(NAME),
            &device,
            Some(DEVICE),
            "DeleteEnrolledFinger",
            &(finger),
        )
        .await;
    let _ = bus
        .call_method(Some(NAME), &device, Some(DEVICE), "Release", &())
        .await;
    match removed {
        // fprintd forgets the finger even when the reader keeps it.
        Err(error) if fprint_error(&error) == Some("PrintsNotDeletedFromDevice") => Ok(()),
        removed => removed.map(drop).map_err(|error| refusal(&error)),
    }
}

/// Adds `finger`, saying each step on `progress` until it is added or
/// fails. Dropping the future stops it.
pub async fn add(finger: &str, progress: &async_channel::Sender<Progress>) {
    if let Err(failed) = adding(finger, progress).await {
        let _ = progress.send(Progress::Failed(failed)).await;
    }
}

async fn adding(finger: &str, progress: &async_channel::Sender<Progress>) -> Result<(), String> {
    let (bus, device) = claim().await.map_err(|error| refusal(&error))?;
    let of = bus
        .call_method(
            Some(NAME),
            &device,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(DEVICE, "num-enroll-stages"),
        )
        .await
        .ok()
        .and_then(|reply| reply.body().deserialize::<OwnedValue>().ok())
        .and_then(|value| i32::try_from(value).ok())
        .and_then(|stages| u32::try_from(stages).ok())
        .unwrap_or(0);
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(NAME)
        .and_then(|rule| rule.path(device.as_ref()))
        .and_then(|rule| rule.interface(DEVICE))
        .and_then(|rule| rule.member("EnrollStatus"))
        .map_err(|error| error.to_string())?
        .build();
    let mut statuses = MessageStream::for_match_rule(rule, &bus, Some(16))
        .await
        .map_err(|error| refusal(&error))?;
    bus.call_method(Some(NAME), &device, Some(DEVICE), "EnrollStart", &(finger))
        .await
        .map_err(|error| refusal(&error))?;
    let _ = progress.send(Progress::Touches { done: 0, of }).await;
    let mut done = 0;
    let outcome = loop {
        let Some(Ok(status)) = statuses.next().await else {
            break Err("fprintd stopped".to_owned());
        };
        let Ok((result, finished)) = status.body().deserialize::<(String, bool)>() else {
            continue;
        };
        let step = match result.as_str() {
            "enroll-completed" => break Ok(()),
            "enroll-stage-passed" => {
                done += 1;
                Progress::Touches { done, of }
            }
            "enroll-retry-scan" => Progress::Again("Touch the sensor again"),
            "enroll-swipe-too-short" | "enroll-finger-not-centered" => {
                Progress::Again("Put your finger in the middle of the sensor")
            }
            "enroll-remove-and-retry" => Progress::Again("Lift your finger, then touch again"),
            "enroll-duplicate" => break Err("This fingerprint is already added".to_owned()),
            "enroll-data-full" => {
                break Err("The reader has no room for another fingerprint".to_owned());
            }
            "enroll-disconnected" => break Err("The reader went away".to_owned()),
            // Some readers say no more than this, for a finger already
            // added or a full reader too.
            _ if finished => {
                break Err(
                    "The reader could not add this fingerprint. Try again, or another finger"
                        .to_owned(),
                );
            }
            _ => continue,
        };
        let _ = progress.send(step).await;
    };
    let _ = bus
        .call_method(Some(NAME), &device, Some(DEVICE), "EnrollStop", &())
        .await;
    let _ = bus
        .call_method(Some(NAME), &device, Some(DEVICE), "Release", &())
        .await;
    outcome?;
    let _ = progress.send(Progress::Added).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lists the fingers on a phone: `cargo test -- --ignored --nocapture
    /// lists_fingers`, as the session's user.
    #[test]
    #[ignore]
    fn lists_fingers() {
        println!("{:?}", futures_lite::future::block_on(enrolled()));
    }
}
