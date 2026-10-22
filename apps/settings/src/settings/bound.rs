//! The settings on the made-up and Denial-backed pages, where their values
//! live (`catalog::Source`): reading each for its row and changing it, the
//! media volume, and the settings Settings works out itself: the screen
//! lock, the lock's delay and the accent.

use denial_look::words::fill;
use denial_look_gpui::say;
use std::rc::Rc;
use std::time::Duration;

use gpui::{AppContext, Context, Focusable, SharedString, Window};
use serde_json::{Value, json};

use super::{Dialog, Screen, Settings};
use crate::catalog::{self, ACCENTS, Item, Kind, LOCK_DELAYS, Source};
use crate::denial::{self, Refusal};
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
        if item.key == "security.fingerprint" {
            return match self.fingers.enrolled.as_ref().map(Vec::len) {
                _ if !self.on(&catalog::FINGERPRINT_UNLOCK) => "Off".into(),
                None => "No reader".into(),
                Some(0) => "No fingers".into(),
                Some(1) => "1 finger".into(),
                Some(fingers) => fill("{count} fingers", &[("count", &fingers.to_string())]).into(),
            };
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
                let value = json!(((low + level * (high - low)) * 1000.0).round() / 1000.0);
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
                "security.lock" if option == 0 => self.ask_lock_pin(None, window, cx),
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
            Kind::Action(said) => self.note(said, cx),
            Kind::Opens if item.key == "style.wallpaper" => {
                self.open(Screen::Wallpapers, window, cx)
            }
            Kind::Opens if item.key == "security.fingerprint" => {
                self.open(Screen::Fingers, window, cx)
            }
            Kind::Opens => self.own_open(item, window, cx),
            _ => {}
        }
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
                let Ok(link) = this.update(cx, |view, _| view.denial.link.clone()) else {
                    return;
                };
                let volume = link.volume().await.ok();
                let taken = this.update(cx, |view, cx| {
                    // The finger's level stands until it lets go.
                    if view.sliding != Some(MEDIA) && !view.denial.volume_sending {
                        view.denial.volume = volume;
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

    /// Asks for the account password and a new PIN, saying `error` when
    /// asked again.
    fn ask_lock_pin(
        &mut self,
        error: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_secrets(cx);
        self.show(Dialog::LockPin { error }, cx);
        window.focus(&self.password.focus_handle(cx), cx);
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

    /// Sets the lock PIN to `pin`, or removes it when None, once `password`
    /// is the account's; asks again after a wrong one.
    pub(super) fn change_pin(
        &mut self,
        password: String,
        pin: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.lock.busy {
            return;
        }
        self.lock.busy = true;
        let length = pin.as_ref().map_or(0, |pin| pin.len() as u32);
        let setting = pin.is_some();
        cx.spawn_in(window, async move |this, cx| {
            let outcome = cx
                .background_spawn(async move {
                    match &pin {
                        Some(pin) => lock::set_pin(&password, pin),
                        None => lock::clear_pin(&password),
                    }
                })
                .await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.lock.busy = false;
                match outcome {
                    Outcome::Saved | Outcome::Cleared => {
                        view.lock.pin_length = Some(length);
                        view.dismiss(window, cx);
                        view.note(
                            if setting {
                                "The lock screen asks for the PIN"
                            } else {
                                "The lock screen asks for the password"
                            },
                            cx,
                        );
                    }
                    Outcome::WrongPassword if setting => {
                        view.ask_lock_pin(Some("Wrong password"), window, cx)
                    }
                    Outcome::WrongPassword => {
                        view.ask_clear_pin(Some("Wrong password"), window, cx)
                    }
                    Outcome::InvalidPin => {
                        view.ask_lock_pin(Some("Use 4 to 12 digits"), window, cx)
                    }
                    Outcome::Failed => {
                        view.dismiss(window, cx);
                        view.note("Could not change the screen lock", cx);
                    }
                }
            });
        })
        .detach();
    }
}
