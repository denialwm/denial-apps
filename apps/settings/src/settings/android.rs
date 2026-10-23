//! Android's screens: the page with the switch, how Android runs, and its
//! apps; and one app, which opens, stops, clears and uninstalls there.
//! Everything goes through droidloomd (`crate::droidloom`), whose calls
//! block, so they run in the background. While a screen shows, or the home
//! screen with Android's line, the page reads how Android runs every two
//! seconds, and Droidloom's unit with it; when the unit failed, its last
//! words in the journal say why.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use denial_look::words::{fill, tr};
use denial_look_gpui::{Icon, dp, island, solid};
use gpui::{
    Context, Div, FontWeight, ObjectFit, SharedString, Stateful, StyledImage, Task, div, img,
    prelude::*,
};

use super::parts::{Fill, ROW_GAP, ROW_PAD};
use super::system::APP_ICON;
use super::{Dialog, Layer, Screen, Settings};
use crate::apps::{self, App};
use crate::catalog::Section;
use crate::droidloom::{self, Action, Cell, Density, Details, Stats, Unit, UnitState};
use crate::{about, clock};

/// How often the page reads how Android runs.
const READ_EVERY: Duration = Duration::from_secs(2);

/// The sizes Android apps can be drawn at, as shares of the density that
/// draws them as large as Denial's own apps: Android's 160 dpi times the
/// scale Denial gives windows.
const SIZES: [&str; 4] = ["Small", "Default", "Large", "Larger"];
const SIZE_SCALES: [f32; 4] = [0.85, 1.0, 1.15, 1.3];

/// Android's density at the size `option` of `SIZES`.
fn size_dpi(scale: f32, option: usize) -> Option<u32> {
    let share = SIZE_SCALES.get(option)?;
    Some((160.0 * scale * share).round() as u32)
}

/// Android as Settings knows it.
#[derive(Default)]
pub(super) struct Android {
    /// How it runs; None while it does not.
    stats: Option<Stats>,
    /// Why it ended, while it does not run.
    ended: Option<String>,
    /// Droidloom's unit; None while the service manager does not answer.
    unit: Option<UnitState>,
    /// Why the unit failed, for the failure it describes.
    failure: Option<(UnitState, Option<String>)>,
    /// Read once at least since a screen opened.
    read: bool,
    /// The share of the whole processor it used since the last reading.
    cpu: Option<f32>,
    last_cpu: Option<(Instant, u64)>,
    release: Option<String>,
    /// The Android apps in the launcher.
    apps: Vec<App>,
    /// What Android said about each app, by package, or why it did not.
    details: HashMap<String, Result<Details, String>>,
    /// The app being acted on, and what its screen says meanwhile.
    busy: Option<(String, &'static str)>,
    /// Android is being restarted from here.
    restarting: bool,
    /// The display's density, read once Android is ready while its page
    /// shows; None until then.
    density: Option<Density>,
    reading_density: bool,
    reading: Option<Task<()>>,
}

impl Screen {
    fn shows_android(&self) -> bool {
        self.is_page("android") || matches!(self, Screen::AndroidApp { .. })
    }

    /// Shows whether Android runs: its screens, and its line on the home
    /// screen.
    fn follows_android(&self) -> bool {
        self.shows_android() || *self == Screen::Home
    }
}

impl Settings {
    /// Reads Android's apps when its page opens, an app's details when its
    /// screen does, and how Android runs while either shows.
    pub(super) fn follow_android(&mut self, cx: &mut Context<Self>) {
        if !self.screen.follows_android() || !droidloom::installed() {
            self.android.reading = None;
            self.android.read = false;
            return;
        }
        if self.screen.follows_android() {
            self.installed.droidloom = droidloom::starts();
        }
        if self.screen.is_page("android") {
            self.android.release = droidloom::release();
            self.android.density = None;
            self.read_android_apps(cx);
        }
        if let Screen::AndroidApp { package } = &self.screen {
            self.read_details(package.to_string(), cx);
        }
        if self.android.reading.is_some() {
            return;
        }
        self.android.reading = Some(cx.spawn(async move |this, cx| {
            loop {
                let unit = cx.background_spawn(droidloom::unit()).await.ok();
                let cell = cx.background_spawn(async { droidloom::cell() }).await;
                if this
                    .update(cx, |view, cx| view.take_reading(unit, cell, cx))
                    .is_err()
                {
                    return;
                }
                cx.background_executor().timer(READ_EVERY).await;
            }
        }));
    }

    fn take_reading(&mut self, unit: Option<UnitState>, cell: Cell, cx: &mut Context<Self>) {
        let (stats, ended) = match cell {
            Cell::Running(stats) => (Some(stats), None),
            Cell::Stopped(why) => (None, why),
            Cell::Absent => (None, None),
        };
        let android = &mut self.android;
        android.ended = ended;
        android.unit = unit;
        if let Some(state) = unit.filter(|state| state.unit == Unit::Failed)
            && android.failure.as_ref().is_none_or(|(known, _)| *known != state)
        {
            android.failure = Some((state, None));
            cx.spawn(async move |this, cx| {
                let why = cx
                    .background_spawn(async move { droidloom::failure(state) })
                    .await;
                let _ = this.update(cx, |view, cx| {
                    if let Some((known, said)) = &mut view.android.failure
                        && *known == state
                    {
                        *said = why;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        let now = Instant::now();
        let used = stats.as_ref().and_then(|stats| stats.cpu_usec);
        android.cpu = match (android.last_cpu, used) {
            (Some((then, before)), Some(used)) if used >= before => {
                let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
                let wall = now.duration_since(then).as_micros() as f32 * cores as f32;
                (wall > 0.0).then(|| (used - before) as f32 / wall)
            }
            _ => None,
        };
        android.last_cpu = used.map(|used| (now, used));
        if stats.as_ref().is_some_and(|stats| stats.ready)
            || unit.is_some_and(|state| state.unit == Unit::Failed)
        {
            android.restarting = false;
        }
        android.stats = stats;
        android.read = true;
        let ready = android.stats.as_ref().is_some_and(|stats| stats.ready);
        if !ready {
            android.density = None;
        } else if android.density.is_none()
            && !android.reading_density
            && self.screen.is_page("android")
        {
            self.read_density(cx);
        }
        cx.notify();
    }

    fn read_density(&mut self, cx: &mut Context<Self>) {
        self.android.reading_density = true;
        cx.spawn(async move |this, cx| {
            let density = cx.background_spawn(async { droidloom::density() }).await;
            let _ = this.update(cx, |view, cx| {
                view.android.reading_density = false;
                view.android.density = density.ok();
                cx.notify();
            });
        })
        .detach();
    }

    /// Draws Android apps at the size picked from `SIZES`.
    fn pick_android_size(&mut self, option: usize, cx: &mut Context<Self>) {
        let (Some(density), Some(dpi)) = (self.android.density, size_dpi(self.scale, option)) else {
            return;
        };
        let before = density.dpi;
        self.android.density = Some(Density { dpi, ..density });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let set = cx
                .background_spawn(async move { droidloom::set_density(dpi) })
                .await;
            let _ = this.update(cx, |view, cx| {
                if let Err(why) = set {
                    if let Some(density) = &mut view.android.density {
                        density.dpi = before;
                    }
                    view.note(fill("Android said: {why}", &[("why", &why)]), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn read_android_apps(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let apps = cx.background_spawn(async { apps::read() }).await;
            let _ = this.update(cx, |view, cx| {
                view.android.apps = apps
                    .into_iter()
                    .filter(|app| app.android.is_some())
                    .collect();
                cx.notify();
            });
        })
        .detach();
    }

    fn read_details(&mut self, package: String, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let asked = package.clone();
            let details = cx
                .background_spawn(async move { droidloom::details(&asked) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.android.details.insert(package, details);
                cx.notify();
            });
        })
        .detach();
    }

    /// Droidloom failed to start or stopped on an error.
    fn android_failed(&self) -> bool {
        let android = &self.android;
        !android.restarting
            && (android.unit.is_some_and(|state| state.unit == Unit::Failed)
                || android.ended.is_some())
    }

    /// Why Android does not run, when it failed and the reason is known.
    fn android_failure(&self) -> Option<&str> {
        if !self.android_failed() {
            return None;
        }
        let android = &self.android;
        android.ended.as_deref().or_else(|| {
            android
                .failure
                .as_ref()
                .filter(|(state, _)| Some(*state) == android.unit)
                .and_then(|(_, said)| said.as_deref())
        })
    }

    /// What Android does now, in a few words.
    fn android_state(&self) -> &'static str {
        let android = &self.android;
        let unit = android.unit.map(|state| state.unit);
        match &android.stats {
            _ if android.restarting => "Restarting…",
            _ if self.android_failed() => "Failed",
            Some(stats) if stats.ready => "Ready",
            Some(_) => "Starting…",
            None if unit == Some(Unit::Starting) => "Starting…",
            None if unit == Some(Unit::Stopping) => "Stopping…",
            None if self.installed.droidloom => "Stopped",
            None => "Off",
        }
    }

    /// The Android page: the switch, how Android runs and its apps.
    pub(super) fn android_page(
        &self,
        sections: &'static [Section],
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let mut parts = self.page(sections, layer, cx);
        if !droidloom::installed() || !self.android.read {
            return parts;
        }
        let android = &self.android;
        let live = layer.live();
        let running = android.stats.is_some();
        if running || self.installed.droidloom {
            let version = android
                .stats
                .as_ref()
                .and_then(|stats| stats.android.clone())
                .map_or_else(
                    || tr("Android").to_owned(),
                    |version| fill("Android {version}", &[("version", &version)]),
                );
            let restart = self.button(
                "restart-android",
                if running { "Restart" } else { "Start" },
                Fill::Control,
                live && !android.restarting,
                cx,
                move |this, _, cx| {
                    if running {
                        this.ask_restart_android(cx)
                    } else {
                        this.restart_android(cx)
                    }
                },
            );
            let failure = self.android_failure().map(|why| {
                div()
                    .px(dp(ROW_PAD))
                    .pb(dp(ROW_GAP))
                    .text_color(solid(self.look.palette.secondary))
                    .child(SharedString::from(why.to_owned()))
            });
            parts.push(
                island(&self.look)
                    .child(self.row(
                        "android-state",
                        Some(Icon::Android),
                        version,
                        Some(tr(self.android_state()).into()),
                        (true, false),
                    ))
                    .children(failure)
                    .child(
                        div()
                            .px(dp(ROW_PAD))
                            .pb(dp(ROW_PAD))
                            .child(self.buttons([restart])),
                    ),
            );
        }
        if let Some(density) = android.density {
            let picked = (0..SIZES.len())
                .find(|option| size_dpi(self.scale, *option) == Some(density.dpi))
                .unwrap_or(SIZES.len());
            parts.push(self.chip_row("Display"));
            parts.push(island(&self.look).child(self.options_row(
                "android.size",
                "Display size",
                Some(fill("{dpi} dpi", &[("dpi", &density.dpi.to_string())]).into()),
                &SIZES,
                picked,
                live,
                (true, true),
                cx,
                Rc::new(|this, option, _, cx| this.pick_android_size(option, cx)),
            )));
        }
        if let Some(stats) = &android.stats {
            let mut rows: Vec<(&str, String)> = vec![("Running for", about::up_time(stats.up))];
            if let Some(ready_in) = stats.ready_in {
                rows.push((
                    "Started in",
                    fill(
                        "{seconds} s",
                        &[("seconds", &format!("{:.1}", ready_in.as_secs_f32()))],
                    ),
                ));
            }
            if let Some(memory) = stats.memory {
                rows.push(("Memory", about::bytes(memory)));
            }
            if let Some(swap) = stats.swap.filter(|swap| *swap > 0) {
                rows.push(("Swapped out", about::bytes(swap)));
            }
            if let Some(cpu) = android.cpu {
                rows.push(("Processor", format!("{:.0}%", cpu * 100.0)));
            }
            let counts = [
                ("Processes", stats.processes),
                ("App processes", stats.app_processes),
                ("Threads", stats.threads),
            ];
            rows.extend(
                counts
                    .into_iter()
                    .filter_map(|(title, count)| Some((title, count?.to_string()))),
            );
            parts.push(self.chip_row("Now"));
            parts.push(self.value_rows(rows));

            // What apps learn about the phone, and what runs them.
            let mut rows: Vec<(&str, String)> = Vec::new();
            if let Some(model) = &stats.model {
                rows.push(("Model", model.clone()));
            }
            if let Some(name) = &stats.name {
                rows.push(("Device name", name.clone()));
            }
            if let Some(release) = &android.release {
                rows.push(("Droidloom", release.clone()));
            }
            if !rows.is_empty() {
                parts.push(self.chip_row("Shown to apps"));
                parts.push(self.value_rows(rows));
            }
        }
        if android.apps.is_empty() {
            return parts;
        }
        parts.push(self.chip_row("Apps"));
        parts.push(self.rows_island(
            android.apps.iter(),
            ROW_PAD + APP_ICON + ROW_GAP,
            |app, ends| {
                let package: SharedString = app
                    .android
                    .as_ref()
                    .map(|android| android.package.clone())
                    .unwrap_or_default()
                    .into();
                let row = self.app_row(app, ends).child(self.chevron());
                self.live(row, live, cx, move |this, window, cx| {
                    let package = package.clone();
                    this.open(Screen::AndroidApp { package }, window, cx)
                })
            },
        ));
        parts
    }

    /// Titled rows, each with its value.
    fn value_rows(&self, rows: Vec<(&'static str, String)>) -> Div {
        self.rows_island(rows.into_iter(), 0., |(title, value), ends| {
            self.row(title, None, tr(title), None, ends)
                .child(self.value(value))
        })
    }

    /// Android's line under its title on the home screen.
    pub(super) fn android_summary(&self) -> Option<SharedString> {
        droidloom::installed().then(|| {
            let summary = match (self.installed.droidloom, self.android_failed()) {
                (false, _) => "Off",
                (true, false) => "On",
                (true, true) => "On – failed",
            };
            summary.into()
        })
    }

    /// The app with `package`, while the launcher lists it.
    pub(super) fn android_app(&self, package: &str) -> Option<&App> {
        self.android.apps.iter().find(|app| {
            app.android
                .as_ref()
                .is_some_and(|android| android.package == package)
        })
    }

    /// One Android app: open or stop it, its storage, what it is, and
    /// uninstalling it.
    pub(super) fn android_app_screen(
        &self,
        package: &SharedString,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let Some(app) = self.android_app(package) else {
            return vec![self.chip_row("This app is no longer installed")];
        };
        let android = &self.android;
        let ready = android.stats.as_ref().is_some_and(|stats| stats.ready);
        let busy = android
            .busy
            .as_ref()
            .filter(|(busy, _)| busy == package.as_ref())
            .map(|(_, doing)| *doing);
        let live = layer.live() && ready && android.busy.is_none();
        let details = android.details.get(package.as_ref());
        let status: SharedString = match (busy, details) {
            (Some(doing), _) => tr(doing).into(),
            (None, Some(Ok(details))) if !details.version.is_empty() => {
                fill("Version {version}", &[("version", &details.version)]).into()
            }
            (None, Some(Ok(_))) => SharedString::default(),
            (None, Some(Err(why))) => why.clone().into(),
            (None, None) if ready => tr("Reading…").into(),
            (None, None) => tr("Android does not run").into(),
        };
        let open = {
            let app = app.clone();
            self.button(
                "open",
                "Open",
                Fill::Accent,
                live,
                cx,
                move |this, _, cx| this.open_android_app(app.clone(), cx),
            )
        };
        let stop = self.app_button(
            package,
            "stop",
            "Stop",
            Fill::Control,
            live,
            cx,
            |this, package, cx| this.android_action(package, Action::Stop, "Stopping…", cx),
        );
        let picture = match &app.icon {
            Some(path) => img(path.clone())
                .size_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => self.tile(Icon::Android, 56.).into_any_element(),
        };
        let hero = island(&self.look)
            .flex()
            .flex_col()
            .items_center()
            .gap(dp(6.))
            .p(dp(ROW_PAD))
            .pt(dp(22.))
            .child(div().size(dp(56.)).child(picture))
            .child(
                div()
                    .pt(dp(6.))
                    .text_size(dp(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(app.name.clone()),
            )
            .child(
                div()
                    .text_color(solid(self.look.palette.secondary))
                    .child(status),
            )
            .child(self.buttons([open, stop]).w_full().pt(dp(12.)));
        let mut parts = vec![hero];

        let Some(Ok(details)) = details else {
            return parts;
        };
        let sizes = [
            ("App", details.app_bytes),
            ("Data", details.data_bytes),
            ("Cache", details.cache_bytes),
        ];
        let clear_cache = self.app_button(
            package,
            "clear-cache",
            "Clear cache",
            Fill::Control,
            live && details.cache_bytes > 0,
            cx,
            |this, package, cx| this.android_action(package, Action::ClearCache, "Clearing…", cx),
        );
        let clear_data = self.app_button(
            package,
            "clear-data",
            "Clear storage",
            Fill::Danger,
            live,
            cx,
            |this, package, cx| this.ask_clear_android_app(package, cx),
        );
        parts.push(self.chip_row("Storage"));
        parts.push(
            island(&self.look)
                .children(sizes.into_iter().enumerate().map(|(ix, (title, bytes))| {
                    div()
                        .when(ix > 0, |row| row.child(self.separator(0.)))
                        .child(
                            self.row(title, None, tr(title), None, (ix == 0, false))
                                .child(self.value(about::bytes(bytes))),
                        )
                }))
                .child(
                    div()
                        .px(dp(ROW_PAD))
                        .pb(dp(ROW_PAD))
                        .child(self.buttons([clear_cache, clear_data])),
                ),
        );

        let mut about_rows: Vec<(&str, String)> = Vec::new();
        if !details.version.is_empty() {
            about_rows.push(("Version", details.version.clone()));
        }
        if details.installed_ms > 0 {
            about_rows.push(("Installed", clock::day_of(details.installed_ms)));
        }
        if details.updated_ms > details.installed_ms {
            about_rows.push(("Updated", clock::day_of(details.updated_ms)));
        }
        about_rows.push(("Package", package.to_string()));
        parts.push(self.chip_row("About"));
        parts.push(
            self.rows_island(about_rows.into_iter(), 0., |(title, value), ends| {
                self.row(title, None, tr(title), None, ends)
                    .child(self.value(value))
            }),
        );

        let more = self
            .row(
                "android-settings",
                Some(Icon::Settings),
                "Permissions and notifications",
                Some(tr("In Android's settings").into()),
                (true, true),
            )
            .child(self.chevron());
        let package_owned = package.to_string();
        parts.push(
            island(&self.look).child(self.live(more, live, cx, move |this, _, cx| {
                this.android_action(package_owned.clone(), Action::Settings, "Opening…", cx)
            })),
        );

        if !details.system {
            let uninstall = self.app_button(
                package,
                "uninstall",
                "Uninstall",
                Fill::Danger,
                live,
                cx,
                |this, package, cx| this.ask_uninstall_android_app(package, cx),
            );
            parts.push(
                island(&self.look)
                    .p(dp(ROW_PAD))
                    .child(self.buttons([uninstall])),
            );
        }
        parts
    }

    /// A button on an app's screen that acts on its package.
    #[allow(clippy::too_many_arguments)]
    fn app_button(
        &self,
        package: &SharedString,
        id: &'static str,
        label: &'static str,
        fill: Fill,
        live: bool,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, String, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let package = package.to_string();
        self.button(id, label, fill, live, cx, move |this, _, cx| {
            action(this, package.clone(), cx)
        })
    }

    fn open_android_app(&mut self, app: App, cx: &mut Context<Self>) {
        let Some(android) = app.android.clone() else {
            return;
        };
        self.android.busy = Some((android.package.clone(), "Opening…"));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let opened = cx
                .background_spawn(async move { droidloom::open(&android) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.android.busy = None;
                if let Err(why) = opened {
                    view.note(
                        fill(
                            "Could not open {name}: {why}",
                            &[("name", &app.name), ("why", &why)],
                        ),
                        cx,
                    );
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Does `action` to `package`, its screen saying `doing` meanwhile, then
    /// reads it again; after an uninstall, goes back to the page.
    fn android_action(
        &mut self,
        package: String,
        action: Action,
        doing: &'static str,
        cx: &mut Context<Self>,
    ) {
        self.android.busy = Some((package.clone(), doing));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let asked = package.clone();
            let done = cx
                .background_spawn(async move { droidloom::act(&asked, action) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.android.busy = None;
                match done {
                    Err(why) => view.note(fill("Android said: {why}", &[("why", &why)]), cx),
                    Ok(()) if action == Action::Uninstall => {
                        view.android.details.remove(&package);
                        view.android.apps.retain(|app| {
                            app.android
                                .as_ref()
                                .is_none_or(|android| android.package != package)
                        });
                        view.note(tr("Uninstalled"), cx);
                    }
                    Ok(()) => {
                        if matches!(action, Action::ClearCache | Action::ClearData) {
                            view.read_details(package.clone(), cx);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn ask_uninstall_android_app(&mut self, package: String, cx: &mut Context<Self>) {
        let name = self
            .android_app(&package)
            .map_or_else(|| package.clone(), |app| app.name.clone());
        self.show(
            Dialog::Confirm {
                title: fill("Uninstall {name}?", &[("name", &name)]).into(),
                consequence: "The app and everything it keeps go.".into(),
                action: "Uninstall",
                confirm: Rc::new(move |this, window, cx| {
                    let back = Self::parent(&this.screen);
                    this.open_at(back, true, window, cx);
                    this.android_action(package.clone(), Action::Uninstall, "Uninstalling…", cx)
                }),
            },
            cx,
        );
    }

    fn ask_clear_android_app(&mut self, package: String, cx: &mut Context<Self>) {
        let name = self
            .android_app(&package)
            .map_or_else(|| package.clone(), |app| app.name.clone());
        self.show(
            Dialog::Confirm {
                title: fill("Clear the storage of {name}?", &[("name", &name)]).into(),
                consequence: "Its files, accounts and settings go, as if just installed.".into(),
                action: "Clear",
                confirm: Rc::new(move |this, _, cx| {
                    this.android_action(package.clone(), Action::ClearData, "Clearing…", cx)
                }),
            },
            cx,
        );
    }

    fn ask_restart_android(&mut self, cx: &mut Context<Self>) {
        self.show(
            Dialog::Confirm {
                title: "Restart Android?".into(),
                consequence: "Android apps close, and open again on a tap.".into(),
                action: "Restart",
                confirm: Rc::new(|this, _, cx| this.restart_android(cx)),
            },
            cx,
        );
    }

    fn restart_android(&mut self, cx: &mut Context<Self>) {
        self.android.restarting = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let restarted = droidloom::restart().await;
            let _ = this.update(cx, |view, cx| {
                if let Err(why) = restarted {
                    view.android.restarting = false;
                    view.note(
                        tr("Could not restart Android: {why}").replace("{why}", &why),
                        cx,
                    );
                }
                cx.notify();
            });
        })
        .detach();
    }
}
