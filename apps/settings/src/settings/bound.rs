//! The settings on the made-up and Denial-backed pages, where their values
//! live (`catalog::Source`): reading each for its row and changing it, the
//! media volume, and the settings Settings works out itself: the screen
//! lock, the lock's delay and the accent.

use denial_look::words::fill;
use denial_look_gpui::say;
use std::rc::Rc;
use std::time::Duration;

use gpui::{AppContext, Context, Div, Focusable, SharedString, Window, prelude::*};
use serde_json::{Value, json};

use super::{Dialog, Layer, Screen, Settings};
use crate::catalog::{self, ACCENTS, Item, Kind, LOCK_DELAYS, SliderUnit, Source};
use crate::denial::{self, Refusal};
use crate::fingerprint::{self, Opened};
use crate::lock::{self, Outcome};
use crate::wallpaper;

/// The media slider's key while the finger holds it.
const MEDIA: &str = "sound.media";

impl Settings {
    /// The value at `path` in Denial's document, when it is there.
    pub(super) fn kept(&self, path: &str) -> Option<&Value> {
        let document = self.denial.document.as_ref()?;
        Some(denial::at(document, path)).filter(|value| !value.is_null())
    }

    /// Whether a toggle is on.
    pub(super) fn on(&self, item: &Item) -> bool {
        let Kind::Toggle(start) = item.kind else {
            return false;
        };
        match item.source {
            Source::Denial(path) => self.kept(path).and_then(Value::as_bool).unwrap_or(start),
            Source::Own => self.own_on(item),
            Source::Made => self.values.on(item),
            _ => start,
        }
    }

    /// A slider's level, 0 to 1.
    pub(super) fn level(&self, item: &Item) -> f32 {
        let Kind::Slider(start) = item.kind else {
            return 0.0;
        };
        match item.source {
            Source::Range(path, low, high) => self
                .kept(path)
                .and_then(Value::as_f64)
                .map_or(start, |value| {
                    ((value as f32 - low) / (high - low)).clamp(0.0, 1.0)
                }),
            Source::Volume | Source::SameAsVolume => self.denial.volume.unwrap_or(0.0),
            Source::Made => self.values.level(item),
            _ => start,
        }
    }

    pub(super) fn level_label(&self, item: &Item, level: f32) -> SharedString {
        let (Some(unit), Source::Range(_, min, max)) = (item.slider_unit, item.source) else {
            return format!("{:.0}%", level * 100.0).into();
        };
        let value = min + level * (max - min);
        match unit {
            SliderUnit::Percent => format!("{:.0}%", value * 100.0),
            SliderUnit::Pixels => format!("{value:.1} px"),
            SliderUnit::Degrees => format!("{value:.0}°"),
            SliderUnit::Decimal => format!("{value:.2}"),
            SliderUnit::Multiplier => format!("{value:.2}×"),
            SliderUnit::Count => format!("{value:.0}"),
        }.into()
    }

    /// The option a choice or segmented setting is on; None when its value
    /// is none of them.
    pub(super) fn picked(&self, item: &Item) -> Option<usize> {
        let (Kind::Segmented(_, start) | Kind::Choice(_, start)) = item.kind else {
            return None;
        };
        match item.source {
            Source::Words(path, words) => match self.kept(path).and_then(Value::as_str) {
                Some(word) => words.iter().position(|&each| each == word),
                None => Some(start),
            },
            Source::Own => self.own_picked(item),
            Source::Made => Some(self.values.picked(item)),
            _ => Some(start),
        }
    }

    /// Whether a row shows now: some only go with one option of another,
    /// or with a toggle off (0) or on (1).
    pub(super) fn applies(&self, item: &Item) -> bool {
        // The editor is hosted by the mobile shell. Window size alone
        // also matches large desktop windows, whose shell ignores the action.
        if item.key == "style.lockLayout" {
            let (width, height) = self.window_pixels;
            return std::env::var("DENIAL_SHELL_PROFILE").as_deref() == Ok("mobile")
                && width.min(height) as f32 / self.scale >= 600.0;
        }
        item.only.is_none_or(|(key, options)| {
            catalog::find(key)
                .and_then(|each| match each.kind {
                    Kind::Toggle(_) => Some(usize::from(self.on(each))),
                    _ => self.picked(each),
                })
                .is_some_and(|picked| options.contains(&picked))
        })
    }

    fn own_picked(&self, item: &Item) -> Option<usize> {
        match item.key {
            "security.lock" => self
                .lock
                .pin_length
                .map(|length| if length > 0 { 0 } else { 1 }),
            // Locking on, by default, then the delay; off is Never.
            "security.after" => {
                if self.kept("power.idleLockEnabled") == Some(&json!(false)) {
                    return Some(LOCK_DELAYS.len());
                }
                let seconds = self
                    .kept("power.screenOffLockDelaySeconds")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                LOCK_DELAYS
                    .iter()
                    .position(|&(_, delay)| u64::from(delay) == seconds)
            }
            "language.system" => Some(self.language_picked()),
            "style.accent" => {
                if self.kept("appearance.accentSource") != Some(&json!("custom")) {
                    return Some(0);
                }
                let argb = self.kept("appearance.customAccentColor")?.as_u64()?;
                ACCENTS
                    .iter()
                    .position(|&(_, accent)| u64::from(accent) == argb)
                    .map(|ix| ix + 1)
            }
            _ => None,
        }
    }

    /// What a choice, or a row that opens a screen, shows under its title.
    pub(super) fn shown(&self, item: &Item) -> SharedString {
        if item.key == "style.cursorTheme" {
            let selected = self.current_cursor();
            return if selected == crate::cursor_theme::DEFAULT { "Bibata Modern Ice".into() } else { "Imported theme".into() };
        }
        if item.key == "style.customAccent" {
            let argb = self.kept("appearance.customAccentColor").and_then(Value::as_u64).unwrap_or(0xFF6B3F73);
            return format!("#{:06X}", argb & 0xFFFFFF).into();
        }
        if let Some(value) = self.idle_power_value(item.key) {
            return value;
        }
        if item.key == "security.fingerprint" {
            return match self.fingers.enrolled.as_ref().map(Vec::len) {
                _ if self.lock.pin_length == Some(0) => "Needs a PIN".into(),
                _ if !self.on(&catalog::FINGERPRINT_UNLOCK) => "Off".into(),
                None => "No reader".into(),
                Some(0) => "No fingers".into(),
                Some(1) => "1 finger".into(),
                Some(fingers) => fill("{count} fingers", &[("count", &fingers.to_string())]).into(),
            };
        }
        if item.key == "style.font" {
            return self.kept("appearance.fontFamily")
                .and_then(Value::as_str)
                .filter(|family| !family.is_empty())
                .unwrap_or("System default")
                .into();
        }
        if item.key == "style.wallpaper" {
            return wallpaper::name(
                self.wallpapers
                    .current
                    .as_deref()
                    .unwrap_or(wallpaper::DENIAL),
            )
            .into();
        }
        if item.key == "monitor.wallpaper" {
            return wallpaper::name(
                self.wallpapers
                    .monitor
                    .as_deref()
                    .unwrap_or(wallpaper::DENIAL),
            )
            .into();
        }
        if let Some(shown) = self.own_shown(item) {
            return shown;
        }
        let Kind::Choice(options, _) = item.kind else {
            return SharedString::default();
        };
        match (self.picked(item), item.key) {
            (Some(ix), _) => options[ix].into(),
            (None, "security.lock") => "Reading…".into(),
            (None, "style.accent") => "Custom".into(),
            (None, _) => "Not set".into(),
        }
    }

    /// The line under a row's title.
    pub(super) fn detail(&self, item: &Item) -> Option<SharedString> {
        item.detail.map(SharedString::from)
    }

    pub(super) fn flip(&mut self, item: &'static Item, cx: &mut Context<Self>) {
        let on = !self.on(item);
        match item.source {
            Source::Denial(path) => self.change(
                item.title,
                move |document| denial::put(document, path, json!(on)),
                cx,
            ),
            Source::Own => self.own_flip(item, cx),
            Source::Made => {
                self.values.set(item, on);
                cx.notify();
            }
            _ => {}
        }
    }

    /// Moves a slider to `level` while it is held; `done` keeps it. Denial's
    /// settings change when the finger lets go; the volume follows it.
    pub(super) fn slide(
        &mut self,
        item: &'static Item,
        level: f32,
        done: bool,
        cx: &mut Context<Self>,
    ) {
        let level = (level.clamp(0.0, 1.0) * 100.0).round() / 100.0;
        match item.source {
            Source::Range(path, low, high) => {
                let value = low + level * (high - low);
                let value = if matches!(item.slider_unit, Some(SliderUnit::Count)) {
                    json!(value.round() as u32)
                } else {
                    json!((value * 1000.0).round() / 1000.0)
                };
                let edit = move |document: &mut Value| denial::put(document, path, value.clone());
                if done {
                    self.change(item.title, edit, cx);
                } else if let Some(document) = self.denial.document.as_mut() {
                    edit(document);
                    cx.notify();
                }
            }
            Source::Volume => self.set_volume(level, cx),
            Source::Made => {
                self.values.set_unsaved(item, level);
                if done {
                    self.values.save();
                }
                cx.notify();
            }
            _ => {}
        }
    }

    pub(super) fn pick_option(
        &mut self,
        item: &'static Item,
        option: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match item.source {
            Source::Words(path, words) => {
                let word = words[option];
                self.change(
                    item.title,
                    move |document| denial::put(document, path, json!(word)),
                    cx,
                );
            }
            Source::Made => {
                self.values.set(item, option);
                cx.notify();
            }
            Source::Own => match item.key {
                "security.lock" if option == 0 && self.lock.pin_length.unwrap_or(0) > 0 => {
                    self.ask_change_pin(None, window, cx)
                }
                "security.lock" if option == 0 => self.ask_lock_pin(None, false, window, cx),
                "security.lock" if self.lock.pin_length.unwrap_or(0) > 0 => {
                    self.ask_clear_pin(None, window, cx)
                }
                "security.after" => {
                    let seconds = LOCK_DELAYS.get(option).map(|&(_, seconds)| seconds);
                    self.change(
                        item.title,
                        move |document| {
                            denial::put(
                                document,
                                "power.idleLockEnabled",
                                json!(seconds.is_some()),
                            );
                            if let Some(seconds) = seconds {
                                denial::put(
                                    document,
                                    "power.screenOffLockDelaySeconds",
                                    json!(seconds),
                                );
                            }
                        },
                        cx,
                    );
                }
                "language.system" => self.set_language(option, cx),
                "style.accent" => {
                    let accent = option.checked_sub(1).map(|ix| ACCENTS[ix].1);
                    self.change(
                        item.title,
                        move |document| match accent {
                            Some(argb) => {
                                denial::put(document, "appearance.accentSource", json!("custom"));
                                denial::put(document, "appearance.customAccentColor", json!(argb));
                            }
                            None => {
                                denial::put(document, "appearance.accentSource", json!("wallpaper"))
                            }
                        },
                        cx,
                    );
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// Shows the options of a choice in a dialog.
    pub(super) fn choose(&mut self, item: &'static Item, cx: &mut Context<Self>) {
        let Kind::Choice(options, _) = item.kind else {
            return;
        };
        self.show(
            Dialog::Pick {
                title: item.title.into(),
                options: options.iter().map(|&option| option.into()).collect(),
                picked: self.picked(item).unwrap_or(options.len()),
                pick: Rc::new(move |this, option, window, cx| {
                    this.pick_option(item, option, window, cx)
                }),
            },
            cx,
        );
    }

    pub(super) fn act(&mut self, item: &'static Item, window: &mut Window, cx: &mut Context<Self>) {
        match item.kind {
            Kind::Action(_) if item.key == "style.resetGlass" => {
                self.change(item.title, |document| {
                    for (page, _, item) in catalog::items() {
                        if page.id != "style" || item.slider_unit.is_none() || item.key == "style.appPanelOpacity" {
                            continue;
                        }
                        if let (Kind::Slider(start), Source::Range(path, min, max)) = (&item.kind, item.source)
                            && path.starts_with("appearance.glass.")
                        {
                            let value = ((min + *start * (max - min)) * 1000.0).round() / 1000.0;
                            denial::put(document, path, json!(value));
                        }
                    }
                }, cx);
            }
            Kind::Action(_) if item.key == "layout.reset" => {
                self.change(item.title, denial::reset_desktop_layout, cx);
            }
            Kind::Action(_) if item.key == "overlays.reset" => {
                self.change(item.title, denial::reset_overlays, cx);
            }
            Kind::Action(_) if item.key == "style.resetMotion" => {
                self.change(item.title, |document| {
                    denial::put(document, "animations.durationScale", json!(1.0));
                    denial::put(document, "animations.panelTravel", json!(32.0));
                    denial::put(document, "animations.windowCloseEffect", json!("implode"));
                    denial::put(document, "animations.animateLockScreen", json!(true));
                }, cx);
            }
            Kind::Action(_) if item.key == "power.reset" => {
                self.reset_idle_power(cx);
            }
            Kind::Action(_) if item.key == "language.reset" => self.use_system_language(true, cx),
            Kind::Action(_) if item.key == "style.resetAccent" => self.change(item.title, |document| {
                denial::put(document, "appearance.customAccentColor", json!(0xFF6B3F73_u32));
            }, cx),
            Kind::Action(_) if item.key == "style.reapplyTheme" => self.change(item.title, |_| {}, cx),
            Kind::Action(_) if item.key == "style.resetAppearance" => self.change(item.title, |document| {
                denial::put(document, "appearance", json!({
                    "colorSchemePreference": "preferDark",
                    "accentSource": "custom", "customAccentColor": 0xFF6B3F73_u32,
                    "fontFamily": "", "cornerRadiusScale": 0.5, "monitorCornerRadiusScale": 0.5,
                    "panelOpacity": 0.5, "cardOpacity": 0.4421052631578947,
                    "transparencyMode": "glass", "backdropBlurLevel": "good",
                    "backdropBlurOpacityThreshold": 0.19, "glass": {},
                    "applicationThemingEnabled": true, "focusedWindowBorderEnabled": true,
                    "roundedWindowFrame": false, "windowCornerRadiusScale": 0.5,
                    "focusedWindowOpacity": 1, "unfocusedWindowOpacity": 1,
                    "cursorThemeId": crate::cursor_theme::DEFAULT, "allowClientCursorSurfaces": true,
                }));
            }, cx),
            Kind::Action(_) if item.key == "style.resetLock" => self.change(item.title, |document| {
                denial::put(document, "lockScreen", json!({
                    "useSystemWallpaper": true, "wallpaper": "", "dimAmount": 0.24,
                    "blurRadius": 8, "clockScale": 1, "showSystemStatus": true,
                    "fingerprintUnlock": true, "callsApp": "org.denialwm.Calls",
                    "tabletPadSide": "right", "tabletClockX": 0.5,
                    "tabletClockY": 0.27, "tabletClockScale": 1.25
                }));
            }, cx),
            Kind::Opens if item.key == "style.customAccent" => {
                let initial = self.shown(item).to_string();
                self.output_field.update(cx, |field, cx| field.set_text(initial, cx));
                self.show(Dialog::TextEdit {
                    title: "Custom accent colour",
                    detail: "Enter a six-digit hex colour such as #6B3F73.",
                    valid: Rc::new(|text| parse_rgb(text).is_some()),
                    save: Rc::new(|this, text, cx| {
                        if let Some(rgb) = parse_rgb(&text) {
                            this.change("Custom accent colour", move |document| {
                                denial::put(document, "appearance.accentSource", json!("custom"));
                                denial::put(document, "appearance.customAccentColor", json!(0xFF000000 | rgb));
                            }, cx);
                        }
                    }),
                }, cx);
                window.focus(&self.output_field.focus_handle(cx), cx);
            }
            Kind::Action(_) if item.key == "style.cursorImport" => self.import_cursor(cx),
            Kind::Opens if item.key == "style.cursorTheme" => self.choose_cursor(false, cx),
            Kind::Opens if item.key == "style.cursorRemove" => self.choose_cursor(true, cx),
            Kind::Action(said) => self.note(said, cx),
            Kind::Opens if item.key.starts_with("power.") => self.choose_idle_power(item, cx),
            Kind::Opens if item.key == "style.font" => self.choose_font(cx),
            Kind::Opens if item.key == "style.wallpaper" => {
                self.open(Screen::Wallpapers, window, cx)
            }
            Kind::Opens if item.key == "monitor.wallpaper" => {
                self.open(Screen::MonitorWallpapers, window, cx)
            }
            Kind::Opens if item.key == "security.fingerprint" => self.open_fingers(window, cx),
            Kind::Opens if item.key == "security.pin" => self.ask_change_pin(None, window, cx),
            Kind::Opens => self.own_open(item, window, cx),
            _ => {}
        }
    }

    fn choose_font(&mut self, cx: &mut Context<Self>) {
        let mut fonts = cx.text_system().all_font_names();
        fonts.retain(|family| {
            !family.is_empty()
                && family.chars().count() <= 128
                && !family.chars().any(char::is_control)
        });
        fonts.sort_unstable();
        fonts.dedup();
        let current = self.kept("appearance.fontFamily")
            .and_then(Value::as_str)
            .unwrap_or("");
        let picked = if current.is_empty() {
            0
        } else {
            fonts.iter().position(|family| family == current).map_or(fonts.len() + 1, |index| index + 1)
        };
        let options = std::iter::once("System default".into())
            .chain(fonts.iter().map(|value| SharedString::from(value.clone())))
            .collect();
        self.show(Dialog::Pick {
            title: "Font family".into(),
            options,
            picked,
            pick: Rc::new(move |this, index, _, cx| {
                let font = index.checked_sub(1)
                    .and_then(|index| fonts.get(index))
                    .cloned()
                    .unwrap_or_default();
                this.change("Font family", move |document| {
                    denial::put(document, "appearance.fontFamily", json!(font));
                }, cx);
            }),
        }, cx);
    }

    /// Changes Denial's settings document with `edit`: here at once, then in
    /// Denial, which streams the document back.
    pub(super) fn change(
        &mut self,
        what: &'static str,
        edit: impl Fn(&mut Value) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self.denial.document.as_mut() else {
            return self.note("Denial does not answer", cx);
        };
        edit(document);
        cx.notify();
        let link = self.denial.link.clone();
        cx.spawn(async move |this, cx| {
            if let Err(Refusal(said)) = link.edit(&edit).await {
                let _ = this.update(cx, |view, cx| {
                    view.note(
                        fill(
                            "Could not change {what}: {why}",
                            &[
                                ("what", &say(what).to_lowercase()),
                                ("why", &say(said.as_str())),
                            ],
                        ),
                        cx,
                    )
                });
            }
        })
        .detach();
    }

    /// Moves the media volume to `level`; Denial follows, one request at a
    /// time, ending on the last level asked.
    fn set_volume(&mut self, level: f32, cx: &mut Context<Self>) {
        self.denial.volume = Some(level);
        self.denial.volume_wanted = Some((level * 100.0).round() as u8);
        cx.notify();
        if self.denial.volume_sending {
            return;
        }
        self.denial.volume_sending = true;
        let link = self.denial.link.clone();
        cx.spawn(async move |this, cx| {
            let mut sent = None;
            loop {
                let asked = this
                    .update(cx, |view, _| {
                        let wanted = view.denial.volume_wanted;
                        if wanted.is_none() || wanted == sent {
                            view.denial.volume_sending = false;
                            return None;
                        }
                        view.denial.volume_serial =
                            view.denial.volume_serial.wrapping_add(1).max(1);
                        wanted.map(|percent| (percent, view.denial.volume_serial))
                    })
                    .ok()
                    .flatten();
                let Some((percent, serial)) = asked else {
                    return;
                };
                if let Err(Refusal(said)) = link.set_volume(percent, serial).await {
                    eprintln!("settings: setting the volume: {said}");
                }
                sent = Some(percent);
            }
        })
        .detach();
    }

    /// The shade and the volume keys change the volume too, so the Sound
    /// page reads it again every second.
    pub(super) fn application_audio(&self, layer: Layer, cx: &mut Context<Self>) -> Div {
        let live = layer.live();
        let streams = self.denial.audio_streams.as_deref().unwrap_or_default();
        let mut group = denial_look_gpui::island(&self.look);
        if streams.is_empty() {
            let label = if self.denial.audio_streams.is_some() {
                "No apps playing audio"
            } else {
                "Application audio unavailable"
            };
            group = group.child(self.row("audio-status", None, label, None, (true, false)));
        }
        for stream in streams {
            let id = stream.id;
            let shown = format!("{:.0}%", stream.level * 100.0);
            group = group.child(self.slider_row(
                ("audio-stream", id),
                None,
                SharedString::from(stream.name.clone()),
                stream.muted.then(|| "Muted".into()),
                shown.into(),
                stream.level.clamp(0.0, 1.0),
                live,
                (false, false),
                cx,
                Rc::new(move |this, level, done, cx| this.adjust_audio_stream(id, level, done, cx)),
            ));
        }
        group
    }

    /// Reads the apps' volumes again, from the heading over them.
    pub(super) fn audio_actions(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        vec![self.action(
            "refresh-audio",
            denial_look_gpui::Icon::Flip,
            "Refresh audio",
            false,
            layer.live(),
            cx,
            |this, _, cx| {
                this.reading_volume = None;
                this.follow_volume(cx);
            },
        )]
    }

    fn adjust_audio_stream(&mut self, id: u32, level: f32, done: bool, cx: &mut Context<Self>) {
        let level = level.clamp(0.0, 1.0);
        let Some(stream) = self.denial.audio_streams.as_mut()
            .and_then(|streams| streams.iter_mut().find(|stream| stream.id == id))
        else {
            return;
        };
        stream.level = level;
        cx.notify();
        if !done {
            return;
        }
        self.denial.audio_stream_revision += 1;
        self.denial.audio_stream_writes += 1;
        let link = self.denial.link.clone();
        cx.spawn(async move |this, cx| {
            let result = link.set_audio_stream(id, (level * 100.0).round() as u8).await;
            let _ = this.update(cx, |view, cx| {
                view.denial.audio_stream_writes -= 1;
                if let Err(Refusal(why)) = result {
                    view.note(fill("Could not change {what}: {why}", &[
                        ("what", &say("Application audio").to_lowercase()),
                        ("why", &why),
                    ]), cx);
                }
                cx.notify();
            });
        }).detach();
    }

    pub(super) fn follow_volume(&mut self, cx: &mut Context<Self>) {
        if !self.screen.is_page("sound") {
            self.reading_volume = None;
            return;
        }
        if self.reading_volume.is_some() {
            return;
        }
        self.reading_volume = Some(cx.spawn(async move |this, cx| {
            loop {
                let Ok((link, revision)) = this.update(cx, |view, _| {
                    (view.denial.link.clone(), view.denial.audio_stream_revision)
                }) else {
                    return;
                };
                let (volume, streams) = futures_lite::future::zip(link.volume(), link.audio_streams()).await;
                let taken = this.update(cx, |view, cx| {
                    if view.denial.audio_stream_revision == revision
                        && view.denial.audio_stream_writes == 0
                        && view.sliding.is_none()
                    {
                        view.denial.audio_streams = streams.ok();
                        cx.notify();
                    }
                    // The finger's level stands until it lets go.
                    if view.sliding != Some(MEDIA.into()) && !view.denial.volume_sending {
                        view.denial.volume = volume.ok();
                        cx.notify();
                    }
                });
                if taken.is_err() {
                    return;
                }
                cx.background_executor().timer(Duration::from_secs(1)).await;
            }
        }));
    }

    /// Reads the lock PIN's length and the enrolled fingers.
    pub(super) fn read_lock(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let length = cx.background_spawn(async { lock::pin_length() }).await;
            let _ = this.update(cx, |view, cx| {
                view.lock.pin_length = length;
                cx.notify();
            });
        })
        .detach();
        self.read_fingers(cx);
    }

    /// The fingerprints, behind the PIN: asked for, or set first when there
    /// is none, since fingerprints never unlock without one. The PIN opens
    /// Denial's fingerprint session; the password only sets a first PIN.
    pub(super) fn open_fingers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.lock.pin_length {
            Some(0) => self.ask_lock_pin(None, true, window, cx),
            Some(_) => self.ask_pin_dialog(Dialog::CheckPin { error: None }, window, cx),
            None => self.note("The screen lock does not answer", cx),
        }
    }

    /// Asks for the account password and a new PIN, twice; `fingers` when
    /// the fingerprints open after it. Says `error` when asked again.
    pub(super) fn ask_lock_pin(
        &mut self,
        error: Option<&'static str>,
        fingers: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_secrets(cx);
        self.show(Dialog::LockPin { error, fingers }, cx);
        window.focus(&self.password.focus_handle(cx), cx);
    }

    pub(super) fn ask_change_pin(
        &mut self,
        error: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ask_pin_dialog(Dialog::ChangePin { error }, window, cx);
    }

    /// Shows `dialog`, which starts with the current PIN.
    fn ask_pin_dialog(&mut self, dialog: Dialog, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_secrets(cx);
        self.show(dialog, cx);
        window.focus(&self.current_pin.focus_handle(cx), cx);
    }

    fn ask_clear_pin(
        &mut self,
        error: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_secrets(cx);
        self.show(Dialog::ClearPin { error }, cx);
        window.focus(&self.password.focus_handle(cx), cx);
    }

    /// Hands `request` to Denial's PIN helper, then does what follows it, or
    /// asks again after a wrong password or PIN.
    pub(super) fn send_pin(
        &mut self,
        request: PinRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.lock.busy {
            return;
        }
        self.lock.busy = true;
        cx.notify();
        let after = request.after();
        cx.spawn_in(window, async move |this, cx| {
            let (outcome, opened) = cx.background_spawn(async move { request.send() }).await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.lock.busy = false;
                view.pin_sent(after, outcome, window, cx);
                if let Some(opened) = opened {
                    view.session_opened(opened, window, cx);
                }
            });
        })
        .detach();
    }

    /// Opens Denial's fingerprint session with `pin`, then the fingerprints.
    pub(super) fn open_session(
        &mut self,
        pin: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.lock.busy {
            return;
        }
        self.lock.busy = true;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let opened = cx
                .background_spawn(async move { fingerprint::Session::open(&pin) })
                .await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.lock.busy = false;
                view.session_opened(opened, window, cx);
            });
        })
        .detach();
    }

    fn session_opened(&mut self, opened: Opened, window: &mut Window, cx: &mut Context<Self>) {
        let again = |view: &mut Self, error, window: &mut Window, cx: &mut Context<Self>| {
            view.ask_pin_dialog(Dialog::CheckPin { error: Some(error) }, window, cx)
        };
        match opened {
            Opened::Open(session, enrolled) => {
                self.dismiss(window, cx);
                self.follow_session(session, enrolled, cx);
                if self.screen != Screen::Fingers {
                    self.open(Screen::Fingers, window, cx);
                }
            }
            Opened::WrongPin => again(self, "Wrong PIN", window, cx),
            Opened::Wait(seconds) => {
                self.dismiss(window, cx);
                self.note(
                    fill(
                        "Too many tries. Wait {count} s",
                        &[("count", &seconds.to_string())],
                    ),
                    cx,
                );
            }
            Opened::NoPin => {
                self.lock.pin_length = Some(0);
                self.ask_lock_pin(None, true, window, cx);
            }
            Opened::Failed => {
                self.dismiss(window, cx);
                self.note("Fingerprints cannot change now", cx);
            }
        }
    }

    fn pin_sent(
        &mut self,
        after: After,
        outcome: Outcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let wrong = match outcome {
            Outcome::WrongPassword => "Wrong password",
            Outcome::WrongPin => "Wrong PIN",
            Outcome::InvalidPin => "Use 6 digits",
            Outcome::Failed => {
                self.dismiss(window, cx);
                self.note("Could not change the screen lock", cx);
                return;
            }
            Outcome::Saved | Outcome::Cleared => {
                self.dismiss(window, cx);
                match after {
                    After::Set { length, fingers } => {
                        self.lock.pin_length = Some(length);
                        // With `fingers` the session opens next, with this PIN.
                        if !fingers {
                            self.note("The lock screen asks for the PIN", cx);
                        }
                    }
                    After::Change { length } => {
                        self.lock.pin_length = Some(length);
                        self.note("The PIN is changed", cx);
                    }
                    After::Clear => {
                        self.lock.pin_length = Some(0);
                        self.note(
                            if self
                                .fingers
                                .enrolled
                                .as_ref()
                                .is_some_and(|fingers| !fingers.is_empty())
                            {
                                "Fingerprints stop unlocking until a PIN is set"
                            } else {
                                "The lock screen asks for the password"
                            },
                            cx,
                        );
                    }
                }
                return;
            }
        };
        match after {
            After::Set { fingers, .. } => self.ask_lock_pin(Some(wrong), fingers, window, cx),
            After::Change { .. } => self.ask_change_pin(Some(wrong), window, cx),
            After::Clear => self.ask_clear_pin(Some(wrong), window, cx),
        }
    }
}

/// What goes to Denial's PIN helper.
pub(super) enum PinRequest {
    /// A first PIN, or one forgotten, behind the account password.
    Set {
        password: String,
        pin: String,
        fingers: bool,
    },
    Change {
        current: String,
        pin: String,
    },
    Clear {
        password: String,
    },
}

/// What follows a request once the helper says yes; no secrets.
#[derive(Clone, Copy)]
enum After {
    Set { length: u32, fingers: bool },
    Change { length: u32 },
    Clear,
}

impl PinRequest {
    fn after(&self) -> After {
        match self {
            PinRequest::Set { pin, fingers, .. } => After::Set {
                length: pin.len() as u32,
                fingers: *fingers,
            },
            PinRequest::Change { pin, .. } => After::Change {
                length: pin.len() as u32,
            },
            PinRequest::Clear { .. } => After::Clear,
        }
    }

    /// The helper's answer and, for a first PIN the fingerprints asked
    /// for, their session, opened with it.
    fn send(self) -> (Outcome, Option<Opened>) {
        match self {
            PinRequest::Set {
                password,
                pin,
                fingers,
            } => {
                let outcome = lock::set_pin(&password, &pin);
                let opened = (fingers && outcome == Outcome::Saved)
                    .then(|| fingerprint::Session::open(&pin));
                (outcome, opened)
            }
            PinRequest::Change { current, pin } => (lock::change_pin(&current, &pin), None),
            PinRequest::Clear { password } => (lock::clear_pin(&password), None),
        }
    }
}

fn parse_rgb(text: &str) -> Option<u32> {
    let text = text.trim().trim_start_matches('#');
    (text.len() == 6 && text.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| u32::from_str_radix(text, 16).ok()).flatten()
}
