//! The fingers fprintd knows for this user on its default reader: listing
//! them, adding one and removing one.
//!
//! Settings asks fprintd itself, as this user. The denial package's polkit
//! rule lets the user at the phone add and remove their own fingers; the
//! lock PIN guards them here, since fingerprints never unlock without one.
//! `deniald --lock-pin` checks it, and the PIN goes on its input, never in
//! its arguments, environment or logs. While the phone is locked Denial
//! holds the reader to unlock it, and it is busy here.
//!
//! A phone whose Gatekeeper keeps the PIN adds a finger only once the PIN
//! is proven to its reader, which needs root: there Denial's fingerprint
//! session, `deniald --fingerprint-settings`, does the same work, started
//! by sudo for this one command.
//!
//! An optical reader under the screen lights itself for each touch, but
//! nothing marks it while the phone is unlocked: Settings rings it, where
//! Denial's `/etc/denial/fingerprint.json` places it on the panel.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Lines, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use futures_lite::{StreamExt, future};
use serde_json::{Value, json};
use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, Proxy};

use crate::lock::{self, Checked};

const NAME: &str = "net.reactivated.Fprint";
const DEVICE: &str = "net.reactivated.Fprint.Device";

/// How long a session the PIN opened lasts.
const SESSION_LIMIT: Duration = Duration::from_secs(5 * 60);
/// How long one finger takes to add.
const ENROLL_LIMIT: Duration = Duration::from_secs(120);

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

/// What the fingerprint session says once it is open.
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

/// The fingerprint session, open while this lives.
pub struct Session {
    commands: Commands,
    /// What the session says, until it ends.
    pub events: async_channel::Receiver<Event>,
}

enum Commands {
    /// Settings' own session with fprintd.
    Fprintd(async_channel::Sender<Request>),
    /// Denial's root session, on a phone whose Gatekeeper keeps the PIN.
    Denial(ChildStdin),
}

enum Request {
    Add(String),
    Remove(String),
    Cancel,
}

impl Session {
    /// Checks `pin` and opens the session. Blocks until it answers.
    pub fn open(pin: &str) -> Opened {
        if pin.is_empty() || pin.contains(['\n', '\r', '\0']) {
            return Opened::WrongPin;
        }
        match lock::status() {
            None => Opened::Failed,
            Some(status) if status.length == 0 => Opened::NoPin,
            Some(status) if status.gatekeeper => open_denial(pin),
            Some(_) => match lock::check_pin(pin) {
                Checked::Confirmed => open_fprintd(),
                Checked::WrongPin => Opened::WrongPin,
                Checked::Wait(seconds) => Opened::Wait(seconds),
                Checked::Failed => Opened::Failed,
            },
        }
    }

    /// Adds `finger`; `events` follow it. False when the session is gone.
    pub fn add(&mut self, finger: &str) -> bool {
        self.send(Request::Add(finger.to_owned()))
    }

    /// Removes `finger`; the fingers follow on `events`.
    pub fn remove(&mut self, finger: &str) -> bool {
        self.send(Request::Remove(finger.to_owned()))
    }

    /// Stops adding a finger.
    pub fn cancel(&mut self) {
        self.send(Request::Cancel);
    }

    fn send(&mut self, request: Request) -> bool {
        match &mut self.commands {
            Commands::Fprintd(requests) => requests.try_send(request).is_ok(),
            Commands::Denial(input) => {
                let command = match request {
                    Request::Add(finger) => json!({"command": "enroll", "finger": finger}),
                    Request::Remove(finger) => json!({"command": "delete", "finger": finger}),
                    Request::Cancel => json!({"command": "cancel"}),
                };
                writeln!(input, "{command}").is_ok()
            }
        }
    }
}

/// Opens Settings' own session with fprintd, once the PIN is checked.
fn open_fprintd() -> Opened {
    let opened = future::block_on(async {
        let bus = Connection::system().await?;
        let path = device(&bus).await?;
        let device = Proxy::new(&bus, NAME, path, DEVICE).await?;
        let fingers = list_fingers(&device).await?;
        zbus::Result::Ok((device, fingers))
    });
    let (device, fingers) = match opened {
        Ok(opened) => opened,
        Err(error) => {
            eprintln!("settings: opening fprintd: {error}");
            return Opened::Failed;
        }
    };
    let (requests, received) = async_channel::unbounded();
    let (sender, events) = async_channel::unbounded();
    std::thread::spawn(move || future::block_on(serve(device, received, sender)));
    Opened::Open(
        Session {
            commands: Commands::Fprintd(requests),
            events,
        },
        fingers,
    )
}

/// Does what Settings asks until it lets the session go or the session's
/// time is up.
async fn serve(
    device: Proxy<'static>,
    requests: async_channel::Receiver<Request>,
    events: async_channel::Sender<Event>,
) {
    let end = Instant::now() + SESSION_LIMIT;
    loop {
        let request = future::or(async { requests.recv().await.ok() }, async {
            async_io::Timer::at(end).await;
            None
        })
        .await;
        let event = match request {
            None => break,
            Some(Request::Cancel) => continue,
            Some(Request::Add(finger)) => {
                match add(&device, &finger, &requests, &events, end).await {
                    Ok(true) => None,
                    Ok(false) => break,
                    Err(error) => {
                        eprintln!("settings: adding a fingerprint: {error}");
                        Some(Event::Progress(Progress::Failed(
                        "The reader could not add this fingerprint. Try again, or another finger"
                            .to_owned(),
                    )))
                    }
                }
            }
            Some(Request::Remove(finger)) => remove(&device, &finger).await.err().map(|error| {
                eprintln!("settings: removing a fingerprint: {error}");
                Event::Refused("The reader would not remove it".to_owned())
            }),
        };
        if let Some(event) = event
            && events.send(event).await.is_err()
        {
            return;
        }
        match list_fingers(&device).await {
            Ok(fingers) => {
                if events.send(Event::Fingers(fingers)).await.is_err() {
                    return;
                }
            }
            Err(error) => {
                eprintln!("settings: asking fprintd for fingers: {error}");
                break;
            }
        }
    }
    let _ = events.send(Event::Ended).await;
}

async fn list_fingers(device: &Proxy<'_>) -> zbus::Result<Vec<String>> {
    match device
        .call::<_, _, Vec<String>>("ListEnrolledFingers", &("",))
        .await
    {
        Ok(fingers) => Ok(fingers
            .into_iter()
            .filter(|f| SLOTS.contains(&f.as_str()))
            .collect()),
        Err(error) if fprint_error(&error) == Some("NoEnrolledPrints") => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

async fn remove(device: &Proxy<'_>, finger: &str) -> zbus::Result<()> {
    device.call::<_, _, ()>("Claim", &("",)).await?;
    let removed = match device
        .call::<_, _, ()>("DeleteEnrolledFinger", &(finger,))
        .await
    {
        // Gone from fprintd's store, kept by a reader that cannot forget.
        Err(error) if fprint_error(&error) == Some("PrintsNotDeletedFromDevice") => Ok(()),
        removed => removed,
    };
    let _ = device.call::<_, _, ()>("Release", &()).await;
    removed
}

/// Adds `finger`, saying how it goes. False when the session is over:
/// Settings let it go, or its time is up.
async fn add(
    device: &Proxy<'_>,
    finger: &str,
    requests: &async_channel::Receiver<Request>,
    events: &async_channel::Sender<Event>,
    end: Instant,
) -> zbus::Result<bool> {
    if list_fingers(device)
        .await?
        .iter()
        .any(|known| known == finger)
    {
        let _ = events
            .send(Event::Progress(Progress::Failed(
                "This fingerprint is already added".to_owned(),
            )))
            .await;
        return Ok(true);
    }
    device.call::<_, _, ()>("Claim", &("",)).await?;
    let added = enroll(device, finger, requests, events, end).await;
    let _ = device.call::<_, _, ()>("EnrollStop", &()).await;
    let _ = device.call::<_, _, ()>("Release", &()).await;
    added
}

async fn enroll(
    device: &Proxy<'_>,
    finger: &str,
    requests: &async_channel::Receiver<Request>,
    events: &async_channel::Sender<Event>,
    end: Instant,
) -> zbus::Result<bool> {
    let mut statuses = device.receive_signal("EnrollStatus").await?;
    let mut owner = device.receive_owner_changed().await?;
    let total = device
        .get_property::<i32>("num-enroll-stages")
        .await?
        .clamp(1, 100) as u32;
    device.call::<_, _, ()>("EnrollStart", &(finger,)).await?;
    let mut done = 0;
    let say = move |progress| async move { events.send(Event::Progress(progress)).await.is_ok() };
    if !say(Progress::Touches { done, of: total }).await {
        return Ok(false);
    }
    let deadline = (Instant::now() + ENROLL_LIMIT).min(end);
    enum Heard {
        Status(zbus::Message),
        Gone,
        Request(bool),
        Late,
    }
    loop {
        let heard = future::or(
            future::or(
                async { statuses.next().await.map_or(Heard::Gone, Heard::Status) },
                async {
                    owner.next().await;
                    Heard::Gone
                },
            ),
            future::or(
                async { Heard::Request(requests.recv().await.is_ok()) },
                async {
                    async_io::Timer::at(deadline).await;
                    Heard::Late
                },
            ),
        )
        .await;
        let message = match heard {
            Heard::Status(message) => message,
            Heard::Gone => {
                return Ok(say(Progress::Failed("The reader went away".to_owned())).await);
            }
            // Any request while a finger goes in stops it; a closed
            // channel ends the session.
            Heard::Request(open) => return Ok(open),
            Heard::Late => return Err(zbus::Error::Failure("no touch in time".to_owned())),
        };
        let (status, finished): (String, bool) = message.body().deserialize()?;
        if status == "enroll-stage-passed" {
            done = (done + 1).min(total);
        }
        if let Some(progress) = step(&status, done, total)
            && !say(progress).await
        {
            return Ok(false);
        }
        if finished {
            return Ok(true);
        }
    }
}

/// Opens Denial's root session and gives it `pin`.
fn open_denial(pin: &str) -> Opened {
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
                let session = Session {
                    commands: Commands::Denial(input),
                    events,
                };
                return Opened::Open(session, fingers(&said));
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

/// Reads Denial's open session until it ends, then lets it go.
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

/// What fprintd's enrollment `status` means here, after `done` of `total`
/// touches, if anything.
fn step(status: &str, done: u32, total: u32) -> Option<Progress> {
    let failed = |why: &str| Some(Progress::Failed(why.to_owned()));
    Some(match status {
        "started" | "enroll-stage-passed" => Progress::Touches { done, of: total },
        "enroll-completed" => Progress::Added,
        "enroll-retry-scan" => Progress::Again("Touch the sensor again"),
        "enroll-swipe-too-short" | "enroll-finger-not-centered" => {
            Progress::Again("Put your finger in the middle of the sensor")
        }
        "enroll-remove-and-retry" => Progress::Again("Lift your finger, then touch again"),
        "enroll-duplicate" => return failed("This fingerprint is already added"),
        "enroll-data-full" => return failed("The reader has no room for another fingerprint"),
        "enroll-disconnected" => return failed("The reader went away"),
        // Some readers say no more than this, for a finger already added
        // or a full reader too.
        "enroll-failed" | "enroll-unknown-error" => {
            return failed(
                "The reader could not add this fingerprint. Try again, or another finger",
            );
        }
        _ => return None,
    })
}

/// What a line from Denial's session means here, if anything.
fn event(said: &Value) -> Option<Event> {
    let failed = |why: &str| Some(Event::Progress(Progress::Failed(why.to_owned())));
    match said["event"].as_str()? {
        "ready" => Some(Event::Fingers(fingers(said))),
        "expired" => Some(Event::Ended),
        "enrollment" => {
            let count = |key: &str| said[key].as_u64().unwrap_or(0) as u32;
            step(said["status"].as_str()?, count("completed"), count("total")).map(Event::Progress)
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
