//! Developer: the Flutter shell run from its source, reloaded live, and
//! built into an optimized shell; and the way back to a shell that works.
//! Denial's compositor does all of it (`crate::developer`); the page shows
//! its state, read every second while it shows.

use std::rc::Rc;
use std::time::Duration;

use denial_look::words::{fill, tr};
use denial_look_gpui::{Icon, island, say};
use futures_lite::{FutureExt, future};
use gpui::{ClipboardItem, Context, Div, Focusable, SharedString, Task, Window, prelude::*, px};

use super::parts::{Fill, ROW_PAD};
use super::{Dialog, Layer, Screen, Settings};
use crate::catalog;
use crate::denial::{Link, Refusal};
use crate::developer::{self, Ask, LIVE, OFFICIAL, State};

const READ_EVERY: Duration = Duration::from_secs(1);
/// How long Settings waits for Denial's answer as it starts.
const START_WAIT: Duration = Duration::from_millis(300);

#[derive(Default)]
pub(super) struct Developer {
    state: Option<State>,
    following: Option<Task<()>>,
    /// Something was asked of Denial, and it has not answered yet.
    asking: bool,
    /// An editable copy of the shell is being made.
    setting_up: bool,
    /// Why making it failed.
    setup_error: Option<String>,
}

impl Developer {
    /// What Denial has for a developer as Settings starts, waited for a
    /// moment only: the page is listed, and opens by its name, from then.
    pub(super) fn at_start() -> Self {
        let link = Link::new();
        let state = future::block_on(async { developer::read(&link).await.ok() }.or(async {
            async_io::Timer::after(START_WAIT).await;
            None
        }));
        catalog::set_developer(
            developer::can_set_up() || state.as_ref().is_some_and(State::offers),
        );
        Developer {
            state,
            ..Developer::default()
        }
    }
}

/// The shell that runs, as the page names it.
fn mode(state: &State) -> &'static str {
    match state.active_mode.as_str() {
        "official_optimized" => "Official optimized",
        "custom_optimized" => "Custom optimized",
        "live_development" => "Live development",
        _ => "Connecting",
    }
}

impl Settings {
    /// Reads the shell's state every second while the Developer page
    /// shows, and once whenever the home screen does: the page is listed
    /// only where Denial has something for a developer.
    pub(super) fn follow_developer(&mut self, cx: &mut Context<Self>) {
        let shown = self.screen.is_page("developer");
        if !shown {
            self.developer.following = None;
        }
        if !(shown || self.screen == Screen::Home) || self.developer.following.is_some() {
            return;
        }
        let link = self.denial.link.clone();
        let reading = cx.spawn(async move |this, cx| {
            loop {
                let state = developer::read(&link).await.ok();
                let keep = this.update(cx, |view, cx| {
                    catalog::set_developer(
                        developer::can_set_up() || state.as_ref().is_some_and(State::offers),
                    );
                    if view.developer.state != state {
                        view.developer.state = state;
                        cx.notify();
                    }
                    view.screen.is_page("developer")
                });
                if !matches!(keep, Ok(true)) {
                    return;
                }
                cx.background_executor().timer(READ_EVERY).await;
            }
        });
        if shown {
            self.developer.following = Some(reading);
        } else {
            reading.detach();
        }
    }

    pub(super) fn developer_page(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let Some(state) = &self.developer.state else {
            return vec![island(&self.look).child(self.row(
                "developer-waiting",
                Some(Icon::Progress),
                tr("Waiting for native runtime status…"),
                None,
                (true, true),
            ))];
        };
        let developer = &self.developer;
        let live = layer.live() && !developer.asking;
        let idle = live && !state.busy();
        let mut parts = Vec::new();

        // What runs, and the switch to live development.
        let status = if state.status.is_empty() {
            tr("Waiting for native runtime status…").to_owned()
        } else {
            state.status.clone()
        };
        let mut runtime = island(&self.look).child(
            self.row(
                "developer-runtime",
                Some(Icon::Computer),
                tr(mode(state)),
                Some(status.into()),
                (true, false),
            )
            .children(
                state
                    .progress_basis_points
                    .map(|points| self.value(format!("{}%", (points.min(10_000) + 50) / 100))),
            ),
        );
        if !state.error.is_empty() {
            runtime = runtime.child(self.island_text(state.error.clone(), true));
        }
        let can_switch = idle
            && (state.live() || (state.developer_components_available && state.workspace_valid));
        let enable = !state.live();
        let switch = self
            .row(
                "developer-live",
                None,
                tr("Enable live UI development"),
                Some(
                    tr("Run the selected workspace with the Dart VM service available for VSCodium and Flutter tooling.")
                        .into(),
                ),
                (false, false),
            )
            .child(self.toggle(state.live()));
        parts.push(
            runtime
                .child(self.separator(0.))
                .child(self.live(
                    switch.when(!can_switch, |row| row.opacity(0.5)),
                    can_switch,
                    cx,
                    move |this, _, cx| this.ask_developer(Ask::Live(enable), cx),
                ))
                .child(self.island_text(
                    tr("Live development uses a JIT Flutter engine and debug checks. Frame pacing and game performance will be lower until you return to an optimized build."),
                    false,
                )),
        );

        // The source it runs from.
        let workspace_free = idle
            && !developer.setting_up
            && state.active_mode != LIVE
            && state.desired_mode != LIVE;
        parts.push(self.chip_row(tr("Source workspace")));
        let folder = self
            .row(
                "developer-workspace",
                Some(Icon::Storage),
                tr("Flutter source workspace"),
                Some(if state.workspace.is_empty() {
                    tr("Select a Flutter project containing pubspec.yaml and lib/main.dart.").into()
                } else {
                    state.workspace.clone().into()
                }),
                (true, false),
            )
            .child(self.value(tr(if state.workspace_valid {
                "Ready"
            } else {
                "Needs setup"
            })))
            .child(self.chevron());
        let can_set_up = developer::can_set_up();
        let setup = self.row(
            "developer-setup",
            Some(Icon::New),
            tr("Create and start editable UI"),
            Some(
                tr(if developer.setting_up {
                    "Preparing the editable UI. The shell will switch automatically when it is ready…"
                } else if !can_set_up {
                    "Install denial-ui-development to enable automatic setup."
                } else {
                    "Clone the version-matched Denial source from GitHub into ~/DenialUI, prepare it with the pinned toolchain, and enter live development."
                })
                .into(),
            ),
            (false, developer.setup_error.is_none()),
        );
        let can_start = workspace_free && can_set_up;
        let mut workspace = island(&self.look)
            .child(self.live(
                folder.when(!workspace_free, |row| row.opacity(0.5)),
                workspace_free,
                cx,
                |this, window, cx| this.ask_workspace(window, cx),
            ))
            .child(self.separator(0.))
            .child(self.live(
                setup.when(!can_start, |row| row.opacity(0.5)),
                can_start,
                cx,
                |this, _, cx| this.set_up_workspace(cx),
            ));
        if let Some(error) = &developer.setup_error {
            workspace = workspace.child(self.island_text(error.clone(), true));
        }
        parts.push(workspace);

        // While it runs live.
        let running = idle && state.active_mode == LIVE;
        let can_follow = idle && state.auto_reload_supported && state.workspace_valid;
        let follow = !state.auto_reload;
        let auto = self
            .row(
                "developer-auto-reload",
                None,
                tr("Reload when files change"),
                Some(
                    tr("Let Denial watch the workspace and reload successful source changes. Available when the native tooling bridge is installed.")
                        .into(),
                ),
                (true, false),
            )
            .child(self.toggle(state.auto_reload));
        parts.push(self.chip_row(tr("Live session")));
        parts.push(
            island(&self.look)
                .child(self.live(
                    auto.when(!can_follow, |row| row.opacity(0.5)),
                    can_follow,
                    cx,
                    move |this, _, cx| this.ask_developer(Ask::AutoReload(follow), cx),
                ))
                .child(
                    self.buttons([
                        self.button(
                            "developer-reload",
                            tr("Hot reload"),
                            Fill::Accent,
                            running && state.can_hot_reload,
                            cx,
                            |this, _, cx| this.ask_developer(Ask::Reload, cx),
                        ),
                        self.button(
                            "developer-restart",
                            tr("Hot restart"),
                            Fill::Control,
                            running && state.can_hot_restart,
                            cx,
                            |this, _, cx| this.ask_developer(Ask::Restart, cx),
                        ),
                    ])
                    .px(px(ROW_PAD))
                    .pt(px(4.))
                    .pb(px(ROW_PAD)),
                ),
        );

        // An optimized shell from the workspace, and the ways back.
        parts.push(self.chip_row(tr("Build & recovery")));
        let ways = [
            (
                "developer-build",
                Icon::Update,
                "Build & activate optimized",
                "Promote the current workspace to an optimized shell.",
                idle && state.can_build_optimized,
                Ask::Build,
            ),
            (
                "developer-revert",
                Icon::Recent,
                "Revert last working",
                "Return to the last custom shell that worked, without restarting the session.",
                idle && state.can_revert,
                Ask::Revert,
            ),
            (
                "developer-restore",
                Icon::Retry,
                "Restore official UI",
                "Return to the shell Denial was installed with.",
                idle && state.active_mode != OFFICIAL,
                Ask::Restore,
            ),
        ];
        parts.push(self.rows_island(
            ways.into_iter(),
            0.,
            |(id, glyph, name, detail, can, ask), ends| {
                let row = self.row(id, Some(glyph), tr(name), Some(tr(detail).into()), ends);
                self.live(
                    row.when(!can, |row| row.opacity(0.5)),
                    can,
                    cx,
                    move |this, _, cx| this.ask_developer(ask.clone(), cx),
                )
            },
        ));

        // How an editor attaches, and what the shell reported.
        parts.push(self.chip_row(tr("Connection & diagnostics")));
        let mut diagnostics = island(&self.look).child(
            self.row(
                "developer-generation",
                Some(Icon::Info),
                tr("Generation"),
                None,
                (true, false),
            )
            .child(self.value(state.generation.to_string())),
        );
        if !state.vm_service_uri.is_empty() {
            let uri = state.vm_service_uri.clone();
            let service = self.row(
                "developer-vm-service",
                Some(Icon::Copy),
                tr("Dart VM service"),
                Some(uri.clone().into()),
                (false, false),
            );
            diagnostics = diagnostics
                .child(self.separator(0.))
                .child(self.live(service, layer.live(), cx, move |this, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(uri.clone()));
                    this.note(tr("Copied"), cx);
                }))
                .child(self.island_text(
                    tr("Open the Flutter shell workspace in VSCodium, choose “Attach to Denial live UI” in Run and Debug, then save changed Dart files to reload the desktop. Native compositor changes require a normal rebuild."),
                    false,
                ));
        }
        if state.diagnostics.is_empty() {
            diagnostics = diagnostics.child(self.separator(0.)).child(
                self.island_text(tr("No diagnostics reported."), false)
                    .pt(px(12.)),
            );
        }
        for (ix, diagnostic) in state.diagnostics.iter().enumerate() {
            let glyph = match diagnostic.severity.as_str() {
                "information" => Icon::Info,
                _ => Icon::Alert,
            };
            diagnostics = diagnostics.child(self.separator(0.)).child(self.row(
                SharedString::from(format!("developer-diagnostic-{ix}")),
                Some(glyph),
                diagnostic.message.clone(),
                diagnostic.location().map(SharedString::from),
                (false, ix + 1 == state.diagnostics.len()),
            ));
        }
        parts.push(diagnostics);
        parts
    }

    /// Asks Denial to change how the shell runs. Its answer is the state
    /// once it accepted; the page follows the operation from there.
    fn ask_developer(&mut self, ask: Ask, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.developer.asking, true) {
            return;
        }
        cx.notify();
        let link = self.denial.link.clone();
        cx.spawn(async move |this, cx| {
            let answer = developer::ask(&link, &ask).await;
            let _ = this.update(cx, |view, cx| {
                view.developer.asking = false;
                match answer {
                    Ok(state) => view.developer.state = Some(state),
                    Err(Refusal(said)) => {
                        eprintln!("settings: {ask:?}: {said}");
                        view.note(fill("Denial said: {message}", &[("message", &said)]), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks for the folder of the shell's source.
    fn ask_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self
            .developer
            .state
            .as_ref()
            .map(|state| state.workspace.clone())
            .unwrap_or_default();
        self.output_field
            .update(cx, |field, cx| field.set_text(current, cx));
        self.show(
            Dialog::TextEdit {
                title: "Flutter source workspace",
                detail: "Select a Flutter project containing pubspec.yaml and lib/main.dart. Source changes can replace any shell UI that does not require a new native protocol.",
                valid: Rc::new(|text| crate::plugins::located(text).starts_with('/')),
                save: Rc::new(|this, text, cx| {
                    this.ask_developer(Ask::Workspace(crate::plugins::located(&text)), cx)
                }),
            },
            cx,
        );
        window.focus(&self.output_field.focus_handle(cx), cx);
    }

    /// Makes an editable copy of the shell and starts it live; it takes
    /// minutes, and goes on while other pages show.
    fn set_up_workspace(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.developer.setting_up, true) {
            return;
        }
        self.developer.setup_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let made = cx.background_spawn(async { developer::set_up() }).await;
            let _ = this.update(cx, |view, cx| {
                view.developer.setting_up = false;
                if let Err(error) = made {
                    eprintln!("settings: setting up the editable UI: {error}");
                    view.developer.setup_error = Some(error);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Developer's line on the home screen: the shell that runs.
    pub(super) fn developer_summary(&self) -> Option<SharedString> {
        let state = self.developer.state.as_ref()?;
        (state.active_mode != OFFICIAL).then(|| say(mode(state)))
    }
}
