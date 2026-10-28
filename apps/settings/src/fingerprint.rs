//! The fingers fprintd knows for this user on its default reader: listing
//! them, adding one and removing one.
//!
//! Listing asks fprintd. Adding and removing go through Denial's fingerprint
//! session, `deniald --fingerprint-settings`, which sudo starts as root for
//! this one command without a password: the lock PIN opens it, since
//! fingerprints never unlock without one, and the session keeps the PIN for
//! readers that want it proven again for each finger added. The PIN goes
//! on the session's input, never in its arguments, environment or logs.
//! While the phone is locked Denial holds the reader to unlock it, and it
//! is busy here.
//!
//! An optical reader under the screen lights itself for each touch, but
//! nothing marks it while the phone is unlocked: Settings rings it, where
//! Denial's `/etc/denial/fingerprint.json` places it on the panel.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Lines, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};
use zbus::Connection;
use zbus::zvariant::OwnedObjectPath;

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

/// What Denial's fingerprint session says once it is open.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// The fingers on the reader, after each change.
    Fingers(Vec<String>),
    /// How adding the finger goes.
    Progress(Progress),
    /// A finger the reader would not remove, and why.
    Refused(String),
    /// The session ended; the PIN opens another.
    Ended,
}

/// What came of opening the session.
pub enum Opened {
    /// Open, with the fingers on the reader.
    Open(Session, Vec<String>),
    WrongPin,
    /// Too many wrong PINs: the PIN is taken again after these seconds.
    Wait(u32),
    /// No lock PIN is set.
    NoPin,
    /// The session did not start, or said nothing it should.
    Failed,
}

/// Denial's fingerprint session, open while this lives.
pub struct Session {
    input: ChildStdin,
    /// What the session says, until it ends.
    pub events: async_channel::Receiver<Event>,
}

impl Session {
    /// Starts the session and gives it `pin`. Blocks until it answers.
    pub fn open(pin: &str) -> Opened {
        if pin.is_empty() || pin.contains(['\n', '\r', '\0']) {
            return Opened::WrongPin;
        }
        // The compositor that started Settings names itself, and sudo's
        // rule allows only Denial's own binary.
        let binary = std::env::var_os("DENIAL_COMPOSITOR_BINARY")
            .filter(|binary| !binary.is_empty())
            .unwrap_or_else(|| "deniald".into());
        let Ok(mut child) = Command::new("sudo")
            .args(["-n", "--"])
            .arg(binary)
            .arg("--fingerprint-settings")
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| eprintln!("settings: starting the fingerprint session: {error}"))
        else {
            return Opened::Failed;
        };
        let (Some(mut input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
            return Opened::Failed;
        };
        let mut lines = BufReader::new(output).lines();
        let opened = loop {
            let Some(said) = lines
                .next()
                .and_then(Result::ok)
                .and_then(|line| serde_json::from_str::<Value>(&line).ok())
            else {
                break Opened::Failed;
            };
            match said["event"].as_str() {
                Some("pin") => {
                    if writeln!(input, "{pin}").is_err() {
                        break Opened::Failed;
                    }
                }
                Some("ready") => {
                    let (sender, events) = async_channel::unbounded();
                    std::thread::spawn(move || follow(child, lines, sender));
                    return Opened::Open(Session { input, events }, fingers(&said));
                }
                Some("wrong-pin") => break Opened::WrongPin,
                Some("rate-limited") => {
                    let ms = said["retry_ms"].as_u64().unwrap_or(0);
                    break Opened::Wait(ms.div_ceil(1000).max(1) as u32);
                }
                Some("pin-not-configured") => break Opened::NoPin,
                _ => break Opened::Failed,
            }
        };
        drop(input);
        let _ = child.wait();
        opened
    }

    /// Adds `finger`; `events` follow it. False when the session is gone.
    pub fn add(&mut self, finger: &str) -> bool {
        self.send(json!({"command": "enroll", "finger": finger}))
    }

    /// Removes `finger`; the fingers follow on `events`.
    pub fn remove(&mut self, finger: &str) -> bool {
        self.send(json!({"command": "delete", "finger": finger}))
    }

    /// Stops adding a finger.
    pub fn cancel(&mut self) {
        self.send(json!({"command": "cancel"}));
    }

    fn send(&mut self, command: Value) -> bool {
        writeln!(self.input, "{command}").is_ok()
    }
}

fn fingers(said: &Value) -> Vec<String> {
    said["fingers"]
        .as_array()
        .map(|fingers| {
            fingers
                .iter()
                .filter_map(Value::as_str)
                .filter(|finger| SLOTS.contains(finger))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Reads the open session until it ends, then lets it go.
fn follow(
    mut child: Child,
    lines: Lines<BufReader<ChildStdout>>,
    events: async_channel::Sender<Event>,
) {
    for line in lines.map_while(Result::ok) {
        let Ok(said) = serde_json::from_str::<Value>(&line) else {
            break;
        };
        if let Some(event) = event(&said)
            && events.send_blocking(event).is_err()
        {
            break;
        }
    }
    let _ = events.send_blocking(Event::Ended);
    let _ = child.wait();
}

/// What a line from the session means here, if anything.
fn event(said: &Value) -> Option<Event> {
    let failed = |why: &str| Some(Event::Progress(Progress::Failed(why.to_owned())));
    match said["event"].as_str()? {
        "ready" => Some(Event::Fingers(fingers(said))),
        "expired" => Some(Event::Ended),
        "enrollment" => {
            let count = |key: &str| said[key].as_u64().unwrap_or(0) as u32;
            let touches = Progress::Touches {
                done: count("completed"),
                of: count("total"),
            };
            let step = match said["status"].as_str()? {
                "started" | "enroll-stage-passed" => touches,
                "enroll-completed" => Progress::Added,
                "enroll-retry-scan" => Progress::Again("Touch the sensor again"),
                "enroll-swipe-too-short" | "enroll-finger-not-centered" => {
                    Progress::Again("Put your finger in the middle of the sensor")
                }
                "enroll-remove-and-retry" => Progress::Again("Lift your finger, then touch again"),
                "enroll-duplicate" => return failed("This fingerprint is already added"),
                "enroll-data-full" => {
                    return failed("The reader has no room for another fingerprint");
                }
                "enroll-disconnected" => return failed("The reader went away"),
                // Some readers say no more than this, for a finger already
                // added or a full reader too.
                "enroll-failed" | "enroll-unknown-error" => {
                    return failed(
                        "The reader could not add this fingerprint. Try again, or another finger",
                    );
                }
                _ => return None,
            };
            Some(Event::Progress(step))
        }
        "error" => match said["code"].as_str()? {
            "delete-failed" => Some(Event::Refused("The reader would not remove it".to_owned())),
            "already-enrolled" => failed("This fingerprint is already added"),
            "wrong-pin" | "rate-limited" | "pin-authorization-failed" | "pin-not-configured" => {
                failed("The reader did not take the PIN")
            }
            _ => failed("The reader could not add this fingerprint. Try again, or another finger"),
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_lines_mean_events() {
        let event = |line: Value| event(&line);
        assert_eq!(
            event(json!({"event": "ready", "fingers": ["right-thumb", "nose"]})),
            Some(Event::Fingers(vec!["right-thumb".to_owned()]))
        );
        assert_eq!(
            event(
                json!({"event": "enrollment", "status": "enroll-stage-passed", "completed": 2, "total": 10})
            ),
            Some(Event::Progress(Progress::Touches { done: 2, of: 10 }))
        );
        assert_eq!(
            event(
                json!({"event": "enrollment", "status": "enroll-completed", "completed": 10, "total": 10})
            ),
            Some(Event::Progress(Progress::Added))
        );
        assert_eq!(
            event(json!({"event": "enrollment", "status": "cancelled"})),
            None
        );
        assert!(matches!(
            event(json!({"event": "error", "code": "enrollment-failed"})),
            Some(Event::Progress(Progress::Failed(_)))
        ));
        assert!(matches!(
            event(json!({"event": "error", "code": "delete-failed"})),
            Some(Event::Refused(_))
        ));
        assert_eq!(event(json!({"event": "expired"})), Some(Event::Ended));
    }

    /// Lists the fingers on a phone: `cargo test -- --ignored --nocapture
    /// lists_fingers`, as the session's user.
    #[test]
    #[ignore]
    fn lists_fingers() {
        println!("{:?}", futures_lite::future::block_on(enrolled()));
    }
}
