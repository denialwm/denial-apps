//! Denial through its control socket (deniald, Denial's
//! docs/protocol/control-v1.md): the panel's brightness and refresh rate,
//! the media volume, and Denial's settings document, which holds the
//! appearance, the screen timeout and the other settings Denial keeps. One
//! request per connection; the document is followed by a subscription that
//! streams it on every change.

use std::path::PathBuf;
use std::time::Duration;

use async_channel::Sender;
use async_io::Async;
use futures_lite::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde_json::{Value, json};
use std::os::unix::net::UnixStream;

const RETRY: Duration = Duration::from_secs(2);
/// How long a new refresh rate waits to be kept before Denial undoes it.
pub const KEEP: Duration = Duration::from_secs(15);

/// The phone's panel.
#[derive(Clone, Debug)]
pub struct Output {
    pub monitor_id: i64,
    pub name: String,
    /// The current refresh rate, and the ones its resolution offers, in
    /// millihertz.
    pub rate: u32,
    pub rates: Vec<u32>,
}

/// What Denial's settings document says about the display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub dark: bool,
    /// Seconds until the idle screen turns off; None for never.
    pub screen_off: Option<u32>,
}

impl Prefs {
    pub fn of(document: &Value) -> Self {
        let power = &document["power"];
        Prefs {
            dark: document["appearance"]["colorSchemePreference"] != "preferLight",
            screen_off: (power["idleDpmsEnabled"] != false).then(|| {
                power["idleDpmsTimeoutSeconds"]
                    .as_u64()
                    .unwrap_or(SCREEN_OFF) as u32
            }),
        }
    }
}

/// The value at `path` in Denial's settings document, its keys joined by
/// dots: `notifications.doNotDisturb`. Null when it is not there.
pub fn at<'a>(document: &'a Value, path: &str) -> &'a Value {
    path.split('.').fold(document, |value, key| &value[key])
}

/// Puts `value` at `path`, making the objects on the way.
pub fn put(document: &mut Value, path: &str, value: Value) {
    let target = path.split('.').fold(document, |value, key| {
        if !value.is_object() {
            *value = Value::Object(Default::default());
        }
        &mut value[key]
    });
    *target = value;
}

/// Why Denial said no, as it put it.
pub struct Refusal(pub String);

impl From<std::io::Error> for Refusal {
    fn from(error: std::io::Error) -> Self {
        Refusal(error.to_string())
    }
}

impl From<serde_json::Error> for Refusal {
    fn from(error: serde_json::Error) -> Self {
        Refusal(error.to_string())
    }
}

/// The way to deniald.
#[derive(Clone)]
pub struct Link {
    socket: PathBuf,
}

impl Link {
    /// Denial's control socket, as deniald exports it to what it starts.
    pub fn new() -> Self {
        let socket = std::env::var_os("DENIAL_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let runtime = std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_default();
                PathBuf::from(runtime).join("denial/control.sock")
            });
        Link { socket }
    }

    /// Sends `method` and reads its first answer, keeping the connection
    /// for more when `stream` wants them.
    async fn ask(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> Result<(Value, BufReader<Async<UnixStream>>), Refusal> {
        let mut connection = Async::<UnixStream>::connect(&self.socket).await?;
        let mut request = json!({"version": 1, "id": 1, "method": method});
        if let Some(params) = params {
            request["params"] = params;
        }
        let mut line = serde_json::to_vec(&request)?;
        line.push(b'\n');
        connection.write_all(&line).await?;
        let mut answers = BufReader::new(connection);
        let answer = answer(&mut answers).await?;
        Ok((answer, answers))
    }

    async fn call(&self, method: &str, params: Option<Value>) -> Result<Value, Refusal> {
        Ok(self.ask(method, params).await?.0)
    }

    /// The first enabled panel, with its refresh rates.
    pub async fn output(&self) -> Result<Option<Output>, Refusal> {
        let outputs = self.call("outputs.get", None).await?;
        Ok(outputs["outputs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|output| output["enabled"] == true && output["connected"] == true)
            .map(|output| {
                let current = &output["current_mode"];
                let mut rates: Vec<u32> = output["modes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|mode| {
                        mode["width"] == current["width"] && mode["height"] == current["height"]
                    })
                    .filter_map(|mode| mode["refresh_millihz"].as_u64())
                    .map(|rate| rate as u32)
                    .collect();
                rates.sort_unstable();
                rates.dedup();
                Output {
                    monitor_id: output["monitor_id"].as_i64().unwrap_or(0),
                    name: output["name"].as_str().unwrap_or_default().to_owned(),
                    rate: current["refresh_millihz"].as_u64().unwrap_or(0) as u32,
                    rates,
                }
            }))
    }

    pub async fn set_brightness(&self, output: &Output, percent: u8) -> Result<(), Refusal> {
        self.call(
            "brightness.set",
            Some(json!({
                "monitor_id": output.monitor_id,
                "connector": output.name,
                "percent": percent,
            })),
        )
        .await?;
        Ok(())
    }

    /// The media volume, 0 to 1. Answered with Denial's level rather than
    /// asked of PipeWire, as the shade's slider is.
    pub async fn volume(&self) -> Result<f32, Refusal> {
        let audio = self.call("audio.get", None).await?;
        audio["level"]
            .as_f64()
            .map(|level| (level as f32).clamp(0.0, 1.0))
            .ok_or_else(|| Refusal("Denial has no volume".to_owned()))
    }

    /// Sets the media volume; `serial` tells this request's answer apart.
    pub async fn set_volume(&self, percent: u8, serial: u32) -> Result<(), Refusal> {
        self.call(
            "audio.set",
            Some(json!({"percent": percent, "request_serial": serial})),
        )
        .await
        .map(drop)
    }

    /// Changes Denial's settings document with `edit`, again on a fresh copy
    /// when another change got there first.
    pub async fn edit(&self, edit: impl Fn(&mut Value)) -> Result<(), Refusal> {
        let mut tries = 3;
        loop {
            let read = self.call("settings.document.get", None).await?;
            let mut document: Value =
                serde_json::from_str(read["document"].as_str().unwrap_or_default())?;
            edit(&mut document);
            let applied = self
                .call(
                    "settings.document.apply",
                    Some(json!({
                        "expected_revision": read["revision"],
                        "document": serde_json::to_string(&document)?,
                    })),
                )
                .await;
            tries -= 1;
            match applied {
                Err(Refusal(code)) if code == "conflict" && tries > 0 => continue,
                applied => return applied.map(drop),
            }
        }
    }

    /// Moves `output` to `rate` until it is kept: Denial goes back by itself
    /// after `KEEP`. Answers the change's token.
    pub async fn try_rate(&self, output: &Output, rate: u32) -> Result<u64, Refusal> {
        let snapshot = self.call("outputs.get", None).await?;
        let outputs: Vec<Value> = snapshot["outputs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|each| each["connected"] == true)
            .map(|each| {
                let mut mode = each["current_mode"].clone();
                if each["name"] == output.name.as_str() {
                    mode["refresh_millihz"] = json!(rate);
                }
                json!({
                    "name": each["name"],
                    "enabled": each["enabled"],
                    "powered": each["powered"],
                    "x": each["x"],
                    "y": each["y"],
                    "mode": {
                        "width": mode["width"],
                        "height": mode["height"],
                        "refresh_millihz": mode["refresh_millihz"],
                    },
                    "scale": each["scale"],
                    "transform": each["transform"],
                    "adaptive_sync": each["adaptive_sync"],
                })
            })
            .collect();
        let applied = self
            .call(
                "outputs.apply",
                Some(json!({
                    "serial": snapshot["serial"],
                    "persistent": snapshot["capabilities"]["persistent"],
                    "confirmation_timeout_milliseconds": KEEP.as_millis() as u64,
                    "outputs": outputs,
                })),
            )
            .await?;
        applied["pending_confirmation"]["token"]
            .as_u64()
            .ok_or_else(|| Refusal("Denial kept no way back".to_owned()))
    }

    pub async fn keep(&self, token: u64) -> Result<(), Refusal> {
        self.call("outputs.confirm", Some(json!({"token": token})))
            .await
            .map(drop)
    }

    pub async fn undo(&self, token: u64) -> Result<(), Refusal> {
        self.call("outputs.rollback", Some(json!({"token": token})))
            .await
            .map(drop)
    }
}

/// The panel's brightness, 0 to 1, as its backlight reads now: the level
/// deniald works in. Read here rather than through `brightness.get`, which
/// Denial's shell takes for a change and answers with its brightness popup.
pub fn backlight() -> Option<f32> {
    let priority = |kind: &str| match kind.trim() {
        "raw" => 3,
        "platform" => 2,
        "firmware" => 1,
        _ => 0,
    };
    let read =
        |path: PathBuf| -> Option<u32> { std::fs::read_to_string(path).ok()?.trim().parse().ok() };
    let backlight = std::fs::read_dir("/sys/class/backlight")
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .max_by_key(|path| {
            priority(&std::fs::read_to_string(path.join("type")).unwrap_or_default())
        })?;
    let maximum = read(backlight.join("max_brightness")).filter(|&maximum| maximum > 0)?;
    let current =
        read(backlight.join("actual_brightness")).or_else(|| read(backlight.join("brightness")))?;
    Some(current.min(maximum) as f32 / maximum as f32)
}

/// One answer's result, or what went wrong: an error answer is refused
/// with its code, so callers can tell a conflict.
async fn answer(answers: &mut BufReader<Async<UnixStream>>) -> Result<Value, Refusal> {
    let mut line = String::new();
    if answers.read_line(&mut line).await? == 0 {
        return Err(Refusal("deniald closed the connection".to_owned()));
    }
    let mut answer: Value = serde_json::from_str(&line)?;
    if answer["ok"] == true {
        Ok(answer["result"].take())
    } else {
        let code = answer["error"]["code"].as_str().unwrap_or("unknown");
        eprintln!(
            "settings: deniald said {code}: {}",
            answer["error"]["message"]
        );
        Err(Refusal(code.to_owned()))
    }
}

/// Denial's own timeout when its settings name none: 15 minutes.
const SCREEN_OFF: u64 = 15 * 60;

/// The dark or light appearance. The shell takes it from the glass
/// appearance while it draws glass, apps from the colour scheme: both move.
pub fn set_dark(document: &mut Value, dark: bool) {
    let (scheme, glass) = if dark {
        ("preferDark", "dark")
    } else {
        ("preferLight", "light")
    };
    put(document, "appearance.colorSchemePreference", json!(scheme));
    put(document, "appearance.glass.appearance", json!(glass));
}

pub fn set_screen_off(document: &mut Value, seconds: Option<u32>) {
    put(document, "power.idleDpmsEnabled", json!(seconds.is_some()));
    if let Some(seconds) = seconds {
        put(document, "power.idleDpmsTimeoutSeconds", json!(seconds));
    }
}

/// The document in a settings snapshot.
fn document(snapshot: &Value) -> Result<Value, Refusal> {
    Ok(serde_json::from_str(
        snapshot["document"].as_str().unwrap_or_default(),
    )?)
}

/// Follows Denial's settings document for as long as `updates` is heard,
/// or None while deniald does not answer.
pub async fn follow(link: Link, updates: Sender<Option<Value>>) {
    loop {
        let followed = async {
            let (first, mut answers) = link.ask("settings.document.subscribe", None).await?;
            let mut said = document(&first)?;
            if updates.send(Some(said.clone())).await.is_err() {
                return Ok(());
            }
            loop {
                let next = document(&answer(&mut answers).await?)?;
                if next != said {
                    said = next;
                    if updates.send(Some(said.clone())).await.is_err() {
                        return Ok(());
                    }
                }
            }
        };
        let followed: Result<(), Refusal> = followed.await;
        if updates.is_closed() {
            return;
        }
        if let Err(Refusal(error)) = followed {
            eprintln!("settings: following Denial's settings: {error}");
        }
        let _ = updates.send(None).await;
        async_io::Timer::after(RETRY).await;
    }
}

/// Reads the display through deniald on a phone and prints it: `cargo test
/// -- --ignored --nocapture reads_denial`.
#[cfg(test)]
#[test]
#[ignore]
fn reads_denial() {
    futures_lite::future::block_on(async {
        let link = Link::new();
        let output = link.output().await.ok().flatten().unwrap();
        println!("{output:#?}");
        println!("brightness {:?}", backlight());
        let (first, _) = link
            .ask("settings.document.subscribe", None)
            .await
            .ok()
            .unwrap();
        println!("{:#?}", Prefs::of(&document(&first).ok().unwrap()));
        println!("volume {:?}", link.volume().await.ok());
    });
}

/// Writes the display's settings back as they are, through deniald on a
/// phone: `cargo test -- --ignored --nocapture writes_denial`.
#[cfg(test)]
#[test]
#[ignore]
fn writes_denial() {
    futures_lite::future::block_on(async {
        let link = Link::new();
        let output = link.output().await.ok().flatten().unwrap();
        let level = backlight().unwrap();
        let (first, _) = link
            .ask("settings.document.subscribe", None)
            .await
            .ok()
            .unwrap();
        let before = Prefs::of(&document(&first).ok().unwrap());
        assert!(
            link.set_brightness(&output, (level * 100.0).round() as u8)
                .await
                .is_ok()
        );
        let written = link
            .edit(|document| {
                set_dark(document, before.dark);
                set_screen_off(document, before.screen_off);
            })
            .await;
        assert!(written.is_ok());
        let (after, _) = link
            .ask("settings.document.subscribe", None)
            .await
            .ok()
            .unwrap();
        assert_eq!(Prefs::of(&document(&after).ok().unwrap()), before);
        println!("wrote {before:?} and brightness {level} back");
    });
}

#[cfg(test)]
#[test]
fn paths_reach_into_the_document() {
    let mut document = json!({"power": {"idleDpmsEnabled": true}});
    put(&mut document, "notifications.doNotDisturb", json!(true));
    put(&mut document, "power.screenOffLockDelaySeconds", json!(5));
    assert_eq!(at(&document, "notifications.doNotDisturb"), &json!(true));
    assert_eq!(at(&document, "power.idleDpmsEnabled"), &json!(true));
    assert_eq!(at(&document, "power.screenOffLockDelaySeconds"), &json!(5));
    assert!(at(&document, "sound.keyboardVibration").is_null());
    set_dark(&mut document, false);
    assert_eq!(at(&document, "appearance.glass.appearance"), "light");
    assert!(!Prefs::of(&document).dark);
}
