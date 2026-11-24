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

#[derive(serde::Deserialize)]
pub struct AudioStream {
    pub id: u32,
    pub name: String,
    pub level: f32,
    pub muted: bool,
}

#[derive(PartialEq, Eq, serde::Deserialize)]
pub struct InputDeckStatus {
    pub touch_output: Option<String>,
    #[serde(default)]
    pub touch_outputs: Vec<String>,
    pub available: bool,
    pub active_output: Option<String>,
}

impl InputDeckStatus {
    pub fn selected_output<'a>(&'a self, preferred: &'a str) -> Option<&'a str> {
        self.touch_outputs.iter().find(|name| *name == preferred).map(String::as_str)
            .or(self.touch_output.as_deref())
    }
}

#[derive(Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct KeyboardLayout {
    pub layout: String,
    pub variant: String,
    #[serde(default, skip_serializing)]
    pub display_name: String,
}

#[derive(Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct KeyboardSettings {
    pub layouts: Vec<KeyboardLayout>,
    pub options: Vec<String>,
    #[serde(rename(serialize = "repeatDelayMs"))]
    pub repeat_delay_ms: u32,
    #[serde(rename(serialize = "repeatRateHz"))]
    pub repeat_rate_hz: u32,
}

impl Default for KeyboardSettings {
    fn default() -> Self {
        Self {
            layouts: vec![KeyboardLayout {
                layout: "us".into(),
                variant: String::new(),
                display_name: String::new(),
            }],
            options: Vec::new(),
            repeat_delay_ms: 600,
            repeat_rate_hz: 25,
        }
    }
}

#[derive(serde::Deserialize)]
pub struct KeyboardSnapshot {
    pub revision: u64,
    pub active_layout: usize,
    #[serde(flatten)]
    pub settings: KeyboardSettings,
}

#[derive(Clone, serde::Deserialize)]
pub struct PointerSettings {
    pub revision: u64,
    pub has_mouse: bool,
    pub has_touchpad: bool,
    pub mouse_speed: f64,
    pub mouse_acceleration_enabled: bool,
    /// The wheel's own natural scrolling; None from a Denial that predates it,
    /// which would refuse the field.
    #[serde(default)]
    pub mouse_natural_scroll_enabled: Option<bool>,
    pub tap_to_click_enabled: bool,
    pub natural_scroll_enabled: bool,
    pub scroll_speed_factor: f64,
    pub scrolling_layout_swipe_speed_factor: f64,
    /// The touchpad's pointer speed, acceleration and click method
    /// (`clickfinger` or `buttonAreas`); None from a Denial that predates
    /// them, which would refuse the fields.
    #[serde(default)]
    pub touchpad_speed: Option<f64>,
    #[serde(default)]
    pub touchpad_acceleration_enabled: Option<bool>,
    #[serde(default)]
    pub touchpad_click_method: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct DisplayTarget {
    pub id: i64,
    pub name: String,
    pub label: String,
}

pub struct DisplayTargets {
    pub outputs: Vec<DisplayTarget>,
    pub primary: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct OutputMode {
    pub width: u32,
    pub height: u32,
    pub refresh_millihz: u32,
    #[serde(default, skip_serializing)]
    pub preferred: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct ConfiguredOutput {
    pub name: String,
    pub description: String,
    pub connected: bool,
    pub enabled: bool,
    pub powered: bool,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    /// The scale Denial gives the display while none is set; None from a
    /// Denial that does not tell.
    #[serde(default)]
    pub default_scale: Option<f64>,
    pub transform: String,
    pub scrolling_layout_axis: Option<String>,
    #[serde(default)]
    pub adaptive_sync_supported: bool,
    pub adaptive_sync: bool,
    pub current_mode: Option<OutputMode>,
    pub modes: Vec<OutputMode>,
}

impl ConfiguredOutput {
    pub fn mode(&self) -> Option<OutputMode> {
        self.current_mode.or_else(|| self.modes.iter().copied().find(|mode| mode.preferred))
            .or_else(|| self.modes.first().copied())
    }

    pub fn label(&self) -> String {
        if self.description.is_empty() || self.description == self.name {
            self.name.clone()
        } else {
            format!("{} ({})", self.description, self.name)
        }
    }

    fn apply_payload(&self) -> Result<Value, Refusal> {
        let mode = self.mode().ok_or_else(|| Refusal(format!("{} has no display mode", self.name)))?;
        Ok(json!({
            "name": self.name,
            "enabled": self.enabled,
            "powered": self.enabled && self.powered,
            "x": self.x,
            "y": self.y,
            "mode": mode,
            "scale": self.scale,
            "transform": self.transform,
            "scrolling_layout_axis": self.scrolling_layout_axis,
            "adaptive_sync": self.adaptive_sync,
        }))
    }
}

#[derive(Clone, Copy, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct OutputCapabilities {
    pub apply: bool,
    pub persistent: bool,
    pub enable: bool,
    pub position: bool,
    pub mode: bool,
    pub scale: bool,
    pub transform: bool,
    pub scrolling_layout_axis: bool,
    pub adaptive_sync: bool,
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
pub struct OutputConfirmation {
    pub token: u64,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct OutputConfiguration {
    pub serial: u64,
    pub capabilities: OutputCapabilities,
    pub primary_output: Option<String>,
    pub outputs: Vec<ConfiguredOutput>,
    pub pending_confirmation: Option<OutputConfirmation>,
}

/// The phone's panel.
#[derive(Clone, Debug)]
pub struct Output {
    pub monitor_id: i64,
    pub name: String,
    /// The current refresh rate, and the ones its resolution offers, in
    /// millihertz.
    pub rate: u32,
    pub rates: Vec<u32>,
    /// Device pixels per logical pixel, and the scale Denial gives the panel
    /// while none is set.
    pub scale: f64,
    pub default_scale: Option<f64>,
    /// Its mode, in pixels as it is turned now, wide by high.
    pub pixels: (u32, u32),
}

/// A monitor beside the phone, as Denial shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    /// Its connector: `DP-1`.
    pub name: String,
    /// Device pixels per logical pixel, and the scale Denial gives the
    /// monitor while none is set.
    pub scale: f64,
    pub default_scale: Option<f64>,
    /// Its mode, in pixels, wide by high.
    pub size: (u32, u32),
    /// The same as it is turned now.
    pub pixels: (u32, u32),
    /// The phone's own panel's scale, beside it.
    pub panel_scale: f64,
    /// The current refresh rate, and the ones its resolution offers, in
    /// millihertz.
    pub rate: u32,
    pub rates: Vec<u32>,
}

/// A screen's current refresh rate and the ones its resolution offers, in
/// millihertz. Rates that round to the same hertz show as one: the current
/// one, else the highest.
fn rates(output: &Value) -> (u32, Vec<u32>) {
    let current = &output["current_mode"];
    let rate = current["refresh_millihz"].as_u64().unwrap_or(0) as u32;
    let mut rates: Vec<u32> = output["modes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|mode| mode["width"] == current["width"] && mode["height"] == current["height"])
        .filter_map(|mode| mode["refresh_millihz"].as_u64())
        .map(|rate| rate as u32)
        .collect();
    rates.sort_unstable_by(|a, b| b.cmp(a));
    rates.dedup();
    let hertz = |rate: u32| (rate + 500) / 1000;
    let mut shown: Vec<u32> = Vec::with_capacity(rates.len());
    for each in rates {
        match shown.last_mut() {
            Some(last) if hertz(*last) == hertz(each) => {
                if each == rate {
                    *last = each;
                }
            }
            _ => shown.push(each),
        }
    }
    shown.reverse();
    (rate, shown)
}

/// A screen's current mode in pixels, wide by high as it is turned now.
fn pixels(output: &Value) -> (u32, u32) {
    let mode = &output["current_mode"];
    let side = |key: &str| mode[key].as_u64().unwrap_or(0) as u32;
    turned(
        (side("width"), side("height")),
        output["transform"].as_str().unwrap_or("normal"),
    )
}

/// The pixels a screen shows in `transform`: a quarter turn swaps them.
pub fn turned(pixels: (u32, u32), transform: &str) -> (u32, u32) {
    if transform.ends_with("90") || transform.ends_with("270") {
        (pixels.1, pixels.0)
    } else {
        pixels
    }
}

/// Whether the screen named `name` is built in: the phone's own panel.
pub fn internal(name: &str) -> bool {
    ["DSI-", "eDP-", "LVDS-"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// What Denial's settings document says about the display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub appearance: Appearance,
    /// Seconds until the idle screen turns off; None for never.
    pub screen_off: Option<u32>,
}

impl Prefs {
    pub fn of(document: &Value) -> Self {
        let power = &document["power"];
        Prefs {
            appearance: match document["appearance"]["colorSchemePreference"].as_str() {
                Some("preferLight") => Appearance::Light,
                Some("noPreference") => Appearance::NoPreference,
                _ => Appearance::Dark,
            },
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
        self.ask_with_error(method, params, false).await
    }

    async fn ask_with_error(
        &self,
        method: &str,
        params: Option<Value>,
        detailed: bool,
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
        let answer = answer(&mut answers, detailed).await?;
        Ok((answer, answers))
    }

    async fn call(&self, method: &str, params: Option<Value>) -> Result<Value, Refusal> {
        Ok(self.ask(method, params).await?.0)
    }

    /// One of Denial's Flutter UI methods (`ui.get`, `ui.reload`, …): the UI
    /// state after it, or why Denial refused, in its words.
    pub async fn ui(&self, method: &str, params: Option<Value>) -> Result<Value, Refusal> {
        Ok(self.ask_with_error(method, params, true).await?.0)
    }

    pub async fn keyboard(&self) -> Result<KeyboardSnapshot, Refusal> {
        Ok(serde_json::from_value(self.call("settings.keyboard.get", None).await?)?)
    }

    pub async fn set_keyboard(
        &self,
        revision: u64,
        keyboard: &KeyboardSettings,
    ) -> Result<KeyboardSnapshot, Refusal> {
        let (applied, _) = self.ask_with_error("settings.keyboard.apply", Some(json!({
            "expected_revision": revision,
            "keyboard": keyboard,
        })), true).await?;
        Ok(serde_json::from_value(applied)?)
    }

    pub async fn pointer(&self) -> Result<PointerSettings, Refusal> {
        Ok(serde_json::from_value(self.call("settings.input.get", None).await?)?)
    }

    pub async fn set_pointer(&self, settings: &PointerSettings, mouse: bool) -> Result<PointerSettings, Refusal> {
        let (method, params) = if mouse {
            let mut fields = json!({
                "speed": settings.mouse_speed,
                "accelerationEnabled": settings.mouse_acceleration_enabled,
            });
            if let Some(natural) = settings.mouse_natural_scroll_enabled {
                fields["naturalScrollEnabled"] = natural.into();
            }
            ("settings.mouse.apply", json!({
                "expected_revision": settings.revision,
                "mouse": fields,
            }))
        } else {
            let mut fields = json!({
                "tapToClickEnabled": settings.tap_to_click_enabled,
                "naturalScrollEnabled": settings.natural_scroll_enabled,
                "scrollSpeedFactor": settings.scroll_speed_factor,
                "scrollingLayoutSwipeSpeedFactor": settings.scrolling_layout_swipe_speed_factor,
            });
            if let Some(speed) = settings.touchpad_speed {
                fields["speed"] = speed.into();
            }
            if let Some(acceleration) = settings.touchpad_acceleration_enabled {
                fields["accelerationEnabled"] = acceleration.into();
            }
            if let Some(method) = &settings.touchpad_click_method {
                fields["clickMethod"] = method.as_str().into();
            }
            ("settings.touchpad.apply", json!({
                "expected_revision": settings.revision,
                "touchpad": fields,
            }))
        };
        Ok(serde_json::from_value(self.call(method, Some(params)).await?)?)
    }

    pub async fn display_targets(&self) -> Result<DisplayTargets, Refusal> {
        let result = self.call("outputs.get", None).await?;
        let outputs = result["outputs"].as_array().into_iter().flatten()
            .filter(|output| output["enabled"] == true && output["connected"] == true)
            .filter_map(|output| {
                let id = output["monitor_id"].as_i64().filter(|&id| id > 0)?;
                let name = output["name"].as_str()?;
                let description = output["description"].as_str().filter(|description| !description.is_empty());
                let label = match description {
                    Some(description) if description != name => format!("{description} ({name})"),
                    _ => name.to_owned(),
                };
                Some(DisplayTarget { id, name: name.to_owned(), label })
            }).collect();
        Ok(DisplayTargets {
            outputs,
            primary: result["primary_output"].as_str().map(str::to_owned),
        })
    }

    pub async fn input_deck_output(&self) -> Option<String> {
        self.input_deck_status().await.ok()?.active_output
    }

    pub async fn input_deck_status(&self) -> Result<InputDeckStatus, Refusal> {
        let mut input = self.call("settings.input.get", None).await?;
        Ok(serde_json::from_value(input["input_deck"].take())?)
    }

    pub async fn hardware_brightness(&self, output: &DisplayTarget) -> Result<f32, Refusal> {
        let result = self.call("brightness.get", Some(json!({
            "monitor_id": output.id,
            "connector": output.name,
        }))).await?;
        result["level"].as_f64().map(|level| level.clamp(0.0, 1.0) as f32)
            .ok_or_else(|| Refusal("Brightness unavailable".to_owned()))
    }

    pub async fn set_hardware_brightness(&self, output: &DisplayTarget, percent: u8) -> Result<(), Refusal> {
        self.call("brightness.set", Some(json!({
            "monitor_id": output.id,
            "connector": output.name,
            "percent": percent,
        }))).await.map(drop)
    }

    pub async fn software_dimming(&self, output: &DisplayTarget) -> Option<f32> {
        let result = self.call("software_dimming.get", Some(json!({"monitor_id": output.id}))).await.ok()?;
        if result["supported"] != true {
            return None;
        }
        result["level"].as_f64().map(|level| level.clamp(0.0, 1.0) as f32)
    }

    pub async fn set_software_dimming(&self, output: &DisplayTarget, percent: u8) -> Result<(), Refusal> {
        self.call("software_dimming.set", Some(json!({
            "monitor_id": output.id,
            "percent": percent,
        }))).await.map(drop)
    }

    /// App-managed wallpaper surfaces on the selected or primary output.
    /// None means the compositor could not report the scene.
    pub async fn wallpaper_status(&self, output: Option<&str>) -> Option<Vec<String>> {
        let outputs = self.call("outputs.get", None).await.ok()?;
        let active = outputs["outputs"].as_array()?.iter().filter(|output| {
            output["enabled"] == true && output["connected"] == true
        });
        let name = output.or_else(|| outputs["primary_output"].as_str());
        let selected = match name {
            Some(name) => active.clone().find(|output| output["name"] == name),
            None => active.clone().find(|output| {
                internal(output["name"].as_str().unwrap_or_default())
            }),
        };
        let monitor_id = selected.or_else(|| active.into_iter().next())?["monitor_id"].as_i64()?;
        let status = self.call("wallpaper.status", None).await.ok()?;
        if status["available"] != true {
            return None;
        }
        let mut apps: Vec<_> = status["surfaces"]
            .as_array()?
            .iter()
            .filter(|surface| surface["monitor_id"].as_i64() == Some(monitor_id))
            .map(|surface| surface["app_id"].as_str().unwrap_or_default().to_owned())
            .collect();
        apps.sort();
        apps.dedup();
        Some(apps)
    }

    /// The phone's panel, with its refresh rates: the enabled internal
    /// screen, else the first enabled one.
    pub async fn output(&self) -> Result<Option<Output>, Refusal> {
        let outputs = self.call("outputs.get", None).await?;
        let shown: Vec<&Value> = outputs["outputs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|output| output["enabled"] == true && output["connected"] == true)
            .collect();
        let panel = shown
            .iter()
            .find(|output| internal(output["name"].as_str().unwrap_or_default()))
            .or(shown.first());
        Ok(panel.map(|output| {
            let (rate, rates) = rates(output);
            Output {
                monitor_id: output["monitor_id"].as_i64().unwrap_or(0),
                name: output["name"].as_str().unwrap_or_default().to_owned(),
                rate,
                rates,
                scale: output["scale"].as_f64().unwrap_or(1.0),
                default_scale: output["default_scale"].as_f64(),
                pixels: pixels(output),
            }
        }))
    }

    /// The monitor plugged in beside the phone: the first enabled screen
    /// that is not built in. None without one.
    pub async fn monitor(&self) -> Result<Option<Monitor>, Refusal> {
        let outputs = self.call("outputs.get", None).await?;
        let shown: Vec<&Value> = outputs["outputs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|output| output["enabled"] == true && output["connected"] == true)
            .collect();
        let built_in = |output: &&&Value| internal(output["name"].as_str().unwrap_or_default());
        let panel_scale = shown
            .iter()
            .find(built_in)
            .and_then(|panel| panel["scale"].as_f64())
            .unwrap_or(1.0);
        Ok(shown.iter().find(|output| !built_in(output)).map(|output| {
            let mode = &output["current_mode"];
            let (rate, rates) = rates(output);
            Monitor {
                name: output["name"].as_str().unwrap_or_default().to_owned(),
                scale: output["scale"].as_f64().unwrap_or(1.0),
                default_scale: output["default_scale"].as_f64(),
                size: (
                    mode["width"].as_u64().unwrap_or(0) as u32,
                    mode["height"].as_u64().unwrap_or(0) as u32,
                ),
                pixels: pixels(output),
                panel_scale,
                rate,
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

    /// Asks the shell to edit the tablet lock screen's layout in place.
    pub async fn shortcut_request(&self, method: &str, params: Option<Value>) -> Result<Value, Refusal> {
        self.ask_with_error(method, params, true).await.map(|(answer, _)| answer)
    }

    pub async fn edit_lock_screen(&self) -> Result<(), Refusal> {
        self.call("shell.lockScreen.edit", None).await.map(drop)
    }

    /// Denial's version: the release installed, or the build's identity.
    pub async fn version(&self) -> Result<String, Refusal> {
        let version = self.call("denial.version", None).await?;
        version["version"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| Refusal("Denial did not say its version".to_owned()))
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

    pub async fn audio_streams(&self) -> Result<Vec<AudioStream>, Refusal> {
        let result = self.call("audio.streams.get", None).await?;
        Ok(serde_json::from_value(result["streams"].clone())?)
    }

    pub async fn set_audio_stream(&self, id: u32, percent: u8) -> Result<(), Refusal> {
        self.call(
            "audio.stream.set",
            Some(json!({"stream_id": id, "percent": percent})),
        ).await.map(drop)
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

    pub async fn output_configuration(&self) -> Result<OutputConfiguration, Refusal> {
        let mut configuration: OutputConfiguration =
            serde_json::from_value(self.call("outputs.get", None).await?)?;
        configuration.outputs.retain(|output| output.connected);
        Ok(configuration)
    }

    pub async fn apply_output_configuration(
        &self,
        configuration: &OutputConfiguration,
    ) -> Result<OutputConfiguration, Refusal> {
        let outputs: Vec<Value> = configuration.outputs.iter()
            .filter(|output| output.connected)
            .map(ConfiguredOutput::apply_payload)
            .collect::<Result<_, _>>()?;
        let (applied, _) = self.ask_with_error("outputs.apply", Some(json!({
            "serial": configuration.serial,
            "primary_output": configuration.primary_output,
            "persistent": configuration.capabilities.persistent,
            "confirmation_timeout_milliseconds": KEEP.as_millis() as u64,
            "outputs": outputs,
        })), true).await?;
        let mut applied: OutputConfiguration = serde_json::from_value(applied)?;
        applied.outputs.retain(|output| output.connected);
        Ok(applied)
    }

    /// Changes the refresh rate, keeping all other connected output settings.
    pub async fn try_rate(&self, name: &str, rate: u32) -> Result<u64, Refusal> {
        self.try_output(name, |output| {
            let mut mode = output.mode().ok_or_else(|| Refusal("Display mode unavailable".into()))?;
            mode.refresh_millihz = rate;
            output.current_mode = Some(mode);
            Ok(())
        }).await
    }

    pub async fn try_scale(&self, name: &str, scale: f64) -> Result<u64, Refusal> {
        self.try_output(name, |output| {
            output.scale = scale;
            Ok(())
        }).await
    }

    async fn try_output(
        &self,
        name: &str,
        change: impl FnOnce(&mut ConfiguredOutput) -> Result<(), Refusal>,
    ) -> Result<u64, Refusal> {
        let mut configuration = self.output_configuration().await?;
        let output = configuration.outputs.iter_mut().find(|output| output.name == name)
            .ok_or_else(|| Refusal("Display disconnected".into()))?;
        change(output)?;
        self.apply_output_configuration(&configuration).await?.pending_confirmation
            .map(|confirmation| confirmation.token)
            .ok_or_else(|| Refusal("Denial kept no way back".into()))
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
async fn answer(answers: &mut BufReader<Async<UnixStream>>, detailed: bool) -> Result<Value, Refusal> {
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
        let reason = if detailed {
            answer["error"]["message"].as_str().unwrap_or(code)
        } else {
            code
        };
        Err(Refusal(reason.to_owned()))
    }
}

/// Denial's own timeout when its settings name none: 15 minutes.
const SCREEN_OFF: u64 = 15 * 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Appearance {
    Dark,
    Light,
    NoPreference,
}

pub fn set_appearance(document: &mut Value, appearance: Appearance) {
    let scheme = match appearance {
        Appearance::Dark => "preferDark",
        Appearance::Light => "preferLight",
        Appearance::NoPreference => "noPreference",
    };
    put(document, "appearance.colorSchemePreference", json!(scheme));
    // Dark and light also set the glass appearance, as the mobile picker
    // has always done. No preference leaves the shell glass tint alone.
    match appearance {
        Appearance::Dark => put(document, "appearance.glass.appearance", json!("dark")),
        Appearance::Light => put(document, "appearance.glass.appearance", json!("light")),
        Appearance::NoPreference => {}
    }
}

/// Restores the desktop layout defaults without discarding native or future
/// settings in the same section.
pub fn reset_desktop_layout(document: &mut Value) {
    let defaults = json!({
        "windowLayout": "stacking",
        "scrollingLayoutPreserveSwapSizes": true,
        "scrollingLayoutWheelSpeed": 1.0,
        "scrollingLayoutWheelUpDirection": "left",
        "workspacesEnabled": true,
        "workspaceCount": 4,
        "workspaceSwitchingOrientation": "horizontal",
        "systemBarSide": "top",
        "systemBarOutputs": [],
        "systemBarThickness": 33,
        "maximizePadding": 8,
        "minimizedWindowPlacement": "offscreen",
        "clipboardTrayEdge": "bottom",
        "clipboardTrayExtent": 160,
    });
    for (key, value) in defaults.as_object().into_iter().flatten() {
        put(document, &format!("layout.{key}"), value.clone());
    }
}

pub fn reset_overlays(document: &mut Value) {
    for (surface, anchor, width, height, margin) in [
        ("launcher", "center", 730, 200, 8),
        ("dashboard", "bottomLeft", 420, 740, 8),
        ("notifications", "bottomRight", 410, 640, 10),
        ("systemHud", "bottomCenter", 350, 74, 10),
    ] {
        for (key, value) in [
            ("anchor", json!(anchor)),
            ("width", json!(width)),
            ("height", json!(height)),
            ("margin", json!(margin)),
            ("hoverTriggerEnabled", json!(true)),
        ] {
            put(document, &format!("overlays.{surface}.{key}"), value);
        }
    }
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
                let next = document(&answer(&mut answers, false).await?)?;
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
                set_appearance(document, before.appearance);
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
    set_appearance(&mut document, Appearance::Light);
    assert_eq!(at(&document, "appearance.glass.appearance"), "light");
    assert_eq!(Prefs::of(&document).appearance, Appearance::Light);
    set_appearance(&mut document, Appearance::NoPreference);
    assert_eq!(Prefs::of(&document).appearance, Appearance::NoPreference);
    assert_eq!(at(&document, "appearance.glass.appearance"), "light");
}

#[cfg(test)]
#[test]
fn rates_that_sound_alike_show_once() {
    let mode = |rate: u32| json!({"width": 2560, "height": 1440, "refresh_millihz": rate});
    let output = json!({
        "current_mode": mode(59_951),
        "modes": [
            mode(240_001), mode(60_000), mode(59_951), mode(144_000),
            {"width": 1920, "height": 1080, "refresh_millihz": 75_000},
        ],
    });
    assert_eq!(rates(&output), (59_951, vec![59_951, 144_000, 240_001]));
}

#[cfg(test)]
#[test]
fn a_quarter_turn_swaps_a_screens_pixels() {
    let output = |transform: &str| {
        json!({
            "current_mode": {"width": 3392, "height": 2400, "refresh_millihz": 120_000},
            "transform": transform,
        })
    };
    assert_eq!(pixels(&output("normal")), (3392, 2400));
    assert_eq!(pixels(&output("180")), (3392, 2400));
    assert_eq!(pixels(&output("90")), (2400, 3392));
    assert_eq!(pixels(&output("flipped-270")), (2400, 3392));
    assert_eq!(pixels(&json!({})), (0, 0));
}
