//! The system's pages: Date and time with its time zones, Storage, the apps
//! installed and whether Android apps run, and the language. Each reads
//! what it shows when it opens; the clock again every second while it
//! shows.

use std::time::Duration;

use denial_look::words::{self, tr};
use denial_look_gpui::{Icon, dp};
use gpui::{
    Context, Div, Focusable, ObjectFit, SharedString, Stateful, StyledImage, Window, div, img,
    prelude::*, px,
};
use serde_json::{Value, json};

use super::parts::{ROW_GAP, ROW_PAD};
use super::{Dialog, Layer, Screen, Settings};
use crate::catalog::{self, Item, Section};
use crate::{about, apps, clock, denial, droidloom, language};

/// An app's icon in the installed apps, in dp.
pub(super) const APP_ICON: f32 = 36.0;

impl Settings {
    /// Reads the clock every second while Date and time or the time zones
    /// show: the rows say the time, and the network may change it.
    pub(super) fn follow_clock(&mut self, cx: &mut Context<Self>) {
        if !self.screen.is_page("time") && self.screen != Screen::Zones {
            self.reading_clock = None;
            return;
        }
        if self.reading_clock.is_some() {
            return;
        }
        self.reading_clock = Some(cx.spawn(async move |this, cx| {
            loop {
                let state = clock::read().await;
                let taken = this.update(cx, |view, cx| {
                    view.clock = state;
                    cx.notify();
                });
                if taken.is_err() {
                    return;
                }
                cx.background_executor().timer(Duration::from_secs(1)).await;
            }
        }));
    }

    /// Reads how full the storage is at once, and counts the home folder
    /// meanwhile.
    pub(super) fn read_storage(&mut self, cx: &mut Context<Self>) {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_owned());
        self.storage.used = about::storage(&home);
        if self.storage.counting.is_some() {
            return;
        }
        self.storage.counting = Some(cx.spawn(async move |this, cx| {
            let home = cx.background_spawn(async { about::home_used() }).await;
            let _ = this.update(cx, |view, cx| {
                view.storage.home = home;
                view.storage.counting = None;
                cx.notify();
            });
        }));
    }

    /// Reads the apps.
    pub(super) fn read_apps(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let apps = cx.background_spawn(async { apps::read() }).await;
            let _ = this.update(cx, |view, cx| {
                view.installed.apps = apps;
                cx.notify();
            });
        })
        .detach();
    }

    /// Whether a toggle Settings works out itself is on.
    pub(super) fn own_on(&self, item: &Item) -> bool {
        match item.key {
            "time.auto" => self.clock.as_ref().is_some_and(|clock| clock.network),
            "android.run" => self.installed.droidloom,
            _ => false,
        }
    }

    pub(super) fn own_flip(&mut self, item: &'static Item, cx: &mut Context<Self>) {
        match item.key {
            "time.auto" => {
                let Some(state) = self.clock.as_mut() else {
                    return self.note(tr("The clock does not answer"), cx);
                };
                if !state.network && !state.can_network {
                    return self.note(tr("The phone has no service to set the time"), cx);
                }
                state.network = !state.network;
                let on = state.network;
                cx.notify();
                cx.spawn(async move |this, cx| {
                    if let Err(said) = clock::set_network(on).await {
                        let _ = this.update(cx, |view, cx| {
                            view.note(
                                tr("Could not change the clock: {why}").replace("{why}", &said),
                                cx,
                            )
                        });
                    }
                })
                .detach();
            }
            "android.run" => {
                if !droidloom::installed() {
                    return self.note(tr("Droidloom is not installed"), cx);
                }
                let on = !self.installed.droidloom;
                self.installed.droidloom = on;
                cx.notify();
                cx.spawn(async move |this, cx| {
                    let set = cx.background_spawn(droidloom::set(on)).await;
                    let _ = this.update(cx, |view, cx| {
                        view.installed.droidloom = droidloom::starts();
                        if let Err(said) = set {
                            view.note(
                                tr("Could not change Android apps: {why}").replace("{why}", &said),
                                cx,
                            );
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            _ => {}
        }
    }

    /// The language picked: Denial's, else the one the app shows.
    pub(super) fn language_picked(&self) -> usize {
        let kept = self.kept("localization.locale").and_then(Value::as_str);
        match kept.and_then(|word| {
            language::LANGUAGES
                .iter()
                .position(|language| language.denial == word)
        }) {
            Some(ix) => ix,
            None => usize::from(words::language() == words::Language::Chinese),
        }
    }

    /// Changes the language of Denial, of the apps it starts and of the
    /// system. Settings changes with Denial's settings document.
    pub(super) fn set_language(&mut self, option: usize, cx: &mut Context<Self>) {
        let Some(language) = language::LANGUAGES.get(option) else {
            return;
        };
        let (word, locale) = (language.denial, language.locale);
        self.change(
            "Language",
            move |document| {
                denial::put(document, "localization.locale", json!(word));
                denial::put(
                    document,
                    "applicationEnvironment.default.LANG",
                    json!(locale),
                );
            },
            cx,
        );
        cx.spawn(async move |this, cx| {
            if let Err(said) = language::set_system(locale).await {
                let _ = this.update(cx, |view, cx| {
                    view.note(
                        tr("Could not change the system's language: {why}").replace("{why}", &said),
                        cx,
                    )
                });
            }
        })
        .detach();
    }

    /// What a row Settings works out itself shows under its title.
    pub(super) fn own_shown(&self, item: &Item) -> Option<SharedString> {
        let now = clock::now();
        let twenty_four = self.on(catalog::find("time.24")?);
        match item.key {
            "time.zone" => Some(match &self.clock {
                Some(state) => tr_city(&state.zone).into(),
                None => tr("Reading…").into(),
            }),
            "time.date" => Some(clock::date_text(&now).into()),
            "time.time" => Some(clock::time_text(&now, twenty_four).into()),
            _ => None,
        }
    }

    /// Opens the time zones, or asks for the date or the time.
    pub(super) fn own_open(&mut self, item: &Item, window: &mut Window, cx: &mut Context<Self>) {
        match item.key {
            "time.zone" if self.clock.is_some() => self.open(Screen::Zones, window, cx),
            "time.zone" => self.note(tr("The clock does not answer"), cx),
            "time.date" => self.ask_clock(true, None, window, cx),
            "time.time" => self.ask_clock(false, None, window, cx),
            _ => {}
        }
    }

    /// Asks for the date, or the time, saying `error` when asked again.
    pub(super) fn ask_clock(
        &mut self,
        date: bool,
        error: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if error.is_none() {
            let now = clock::now();
            let typed = if date {
                clock::date_text(&now)
            } else {
                clock::time_text(&now, true)
            };
            self.clock_field
                .update(cx, |input, cx| input.set_text(typed, cx));
        }
        self.show(Dialog::Clock { date, error }, cx);
        window.focus(&self.clock_field.focus_handle(cx), cx);
    }

    /// Sets the clock to what the dialog's field says.
    pub(super) fn set_clock(&mut self, date: bool, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.clock_field.read(cx).text().to_owned();
        let seconds = if date {
            clock::with_date(&typed)
        } else {
            clock::with_time(&typed)
        };
        let Some(seconds) = seconds else {
            let error = if date { "Not a date" } else { "Not a time" };
            return self.ask_clock(date, Some(error), window, cx);
        };
        self.dismiss(window, cx);
        cx.spawn(async move |this, cx| {
            let set = clock::set_time(seconds).await;
            let _ = this.update(cx, |view, cx| {
                if let Err(said) = set {
                    view.note(
                        tr("Could not change the clock: {why}").replace("{why}", &said),
                        cx,
                    );
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The time zones to pick from, the phone's marked; one not among them
    /// comes first.
    pub(super) fn zones_screen(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let Some(state) = &self.clock else {
            return Vec::new();
        };
        let current = state.zone.clone();
        let mut zones: Vec<&str> = Vec::new();
        if !clock::ZONES.iter().any(|(_, zone)| *zone == current) {
            zones.push(&current);
        }
        zones.extend(clock::ZONES.iter().map(|(_, zone)| *zone));
        let rows = zones.into_iter().map(|zone| zone.to_owned());
        vec![
            self.rows_island(rows.collect::<Vec<_>>().into_iter(), 0., |zone, ends| {
                let picked = zone == current;
                let row = self
                    .row(
                        SharedString::from(zone.clone()),
                        None,
                        tr_city(&zone),
                        Some(zone.clone().into()),
                        ends,
                    )
                    .child(self.radio(picked));
                self.live(row, layer.live() && !picked, cx, move |this, window, cx| {
                    this.pick_zone(zone.clone(), window, cx)
                })
            }),
        ]
    }

    fn pick_zone(&mut self, zone: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(state) = self.clock.as_mut() {
            state.zone = zone.clone();
        }
        let back = Self::parent(&self.screen);
        self.open_at(back, true, window, cx);
        cx.spawn(async move |this, cx| {
            let set = clock::set_zone(&zone).await;
            let _ = this.update(cx, |view, cx| {
                if let Err(said) = set {
                    view.note(
                        tr("Could not change the time zone: {why}").replace("{why}", &said),
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    /// How full the storage is, then what fills it: the home folder, and
    /// the system, everything else.
    pub(super) fn storage_page(&self) -> Vec<Div> {
        let Some((used, all)) = self.storage.used else {
            return Vec::new();
        };
        let home = self.storage.home;
        let rows = [
            (Icon::Person, "Your files", "The home folder", home),
            (
                Icon::Settings,
                "System",
                "Everything else",
                home.map(|home| used.saturating_sub(home)),
            ),
        ];
        vec![
            self.storage_island(used, all),
            self.chip_row(tr("What uses it")),
            self.rows_island(
                rows.into_iter(),
                0.,
                |(glyph, title, detail, size), ends| {
                    let size = size.map_or_else(|| tr("Counting…").to_owned(), about::bytes);
                    self.row(title, Some(glyph), tr(title), Some(tr(detail).into()), ends)
                        .child(self.value(size))
                },
            ),
        ]
    }

    /// Storage's line on the home screen.
    pub(super) fn storage_summary(&self) -> Option<SharedString> {
        let (used, all) = self.storage.used.or(self.about.facts.as_ref()?.storage)?;
        Some(
            tr("{used} used of {all}")
                .replace("{used}", &about::bytes(used))
                .replace("{all}", &about::bytes(all))
                .into(),
        )
    }

    /// The Apps page: its settings, then the apps installed.
    pub(super) fn apps_page(
        &self,
        sections: &'static [Section],
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let mut parts = self.page(sections, layer, cx);
        let installed = &self.installed.apps;
        if !installed.is_empty() {
            parts.push(self.chip_row(tr("Installed apps")));
            parts.push(self.rows_island(
                installed.iter(),
                ROW_PAD + APP_ICON + ROW_GAP,
                |app, ends| self.app_row(app, ends),
            ));
        }
        parts
    }

    pub(super) fn app_row(&self, app: &apps::App, (first, last): (bool, bool)) -> Stateful<Div> {
        let radius = px(self.look.group_radius());
        let picture = match &app.icon {
            Some(path) => img(path.clone())
                .size_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => self.tile(Icon::Apps, APP_ICON).into_any_element(),
        };
        div()
            .id(SharedString::from(app.id.clone()))
            .flex()
            .items_center()
            .gap(dp(ROW_GAP))
            .px(dp(ROW_PAD))
            .py(dp(10.))
            .when(first, |row| row.rounded_t(radius))
            .when(last, |row| row.rounded_b(radius))
            .child(div().flex_none().size(dp(APP_ICON)).child(picture))
            .child(div().flex_1().min_w_0().truncate().child(app.name.clone()))
    }
}

/// A zone's city in the app's language.
fn tr_city(zone: &str) -> String {
    match clock::ZONES.iter().find(|(_, each)| *each == zone) {
        Some((city, _)) => tr(city).to_owned(),
        None => clock::city(zone),
    }
}
