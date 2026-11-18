//! Plugins: the pieces the desktop is built from. The page shows what
//! Denial's plugin manager says (`crate::plugins`) and sends it what the
//! user asks; a build goes on in the manager when Settings closes.

use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use denial_look::words::{fill, tr};
use denial_look_gpui::{Icon, island, say, solid};
use gpui::{Context, Div, Focusable, SharedString, Task, Window, div, prelude::*, px};

use super::parts::{Fill, ROW_PAD};
use super::{Dialog, Layer, Settings};
use crate::plugins::{self, Entry, Line, Listing, Plugin, Summary, Work};

/// How often the manager is read: while it works, and otherwise.
const WORKING: Duration = Duration::from_millis(600);
const IDLE: Duration = Duration::from_secs(5);
/// The lines of a failed request's log the page shows: its end.
const LOG_LINES: usize = 60;
/// The requests the recent activity shows: the newest.
const HISTORY: usize = 10;

#[derive(Default)]
pub(super) struct Plugins {
    summary: Option<Summary>,
    /// The plugins, and what the manager says of the selection and of the
    /// community list.
    listing: Listing,
    /// Why the manager cannot be read.
    problem: Option<String>,
    following: Option<Task<()>>,
    /// The plugins whose switch is being saved.
    saving: HashSet<String>,
    /// A request was sent, and the manager has not been read since.
    sending: bool,
    /// The failure whose details show, by its job, and its log once read.
    opened: Option<(String, Option<String>)>,
    /// The failure the user put away, by its job.
    dismissed: Option<String>,
    /// Plugin support was asked to prepare itself, once.
    prepared: bool,
    /// The recent activity shows, and so is read.
    history_shown: bool,
    /// The requests the manager remembers; None until read.
    history: Option<Result<Vec<Entry>, String>>,
    /// The request of the activity whose reason shows, by its job.
    history_opened: Option<String>,
}

/// What runs, in Settings' words.
fn desktop(line: &Line) -> (SharedString, Option<SharedString>) {
    match line.code.as_str() {
        "standard" => (tr("The standard desktop is running").into(), None),
        "plugins" => (
            tr("Your plugins are running").into(),
            Some(titles(&line.plugins).into()),
        ),
        "other_plugins" => (tr("Plugins built elsewhere are running").into(), None),
        "unreachable" => (tr("Denial does not answer").into(), None),
        _ => (line.text.clone().into(), None),
    }
}

/// `denial_top_bar` reads "Top Bar", as the manager titles it.
fn title(name: &str) -> String {
    name.strip_prefix("denial_")
        .unwrap_or(name)
        .split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut letters = word.chars();
            letters
                .next()
                .map(|first| first.to_uppercase().chain(letters).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn titles(names: &[String]) -> String {
    names
        .iter()
        .map(|name| title(name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A reason, an issue or the setup, in Settings' words: by its code, and
/// the manager's own English for a code Settings does not know.
fn words(line: &Line) -> String {
    let value = |value: &Option<String>| value.clone().unwrap_or_default();
    match line.code.as_str() {
        "selection_changed" => tr("Your selection changed since they were built.").to_owned(),
        "engine_differs" => {
            tr("They were built for another engine than this session runs, and loaded anyway.")
                .to_owned()
        }
        "starting" => tr("They are still starting.").to_owned(),
        "rebuild_update" => match &line.version {
            Some(version) => fill(
                "Denial {version} is installed. Your plugins have to be built again.",
                &[("version", version)],
            ),
            None => {
                tr("Denial changed since your plugins were built. They have to be built again.")
                    .to_owned()
            }
        },
        "rebuild_engine" => {
            tr("Your plugins did not start on this session's engine. They have to be built again.")
                .to_owned()
        }
        "denial_said" => fill(
            "Denial said: {message}",
            &[("message", &value(&line.message))],
        ),
        "session_old" => {
            tr("This session's Denial is older than its plugin manager. Log out and back in once.")
                .to_owned()
        }
        "too_many_providers" => fill(
            "{feature} is provided by {plugins}. Keep one of them.",
            &[
                ("feature", &say(value(&line.feature))),
                ("plugins", &titles(&line.plugins)),
            ],
        ),
        "missing_provider" => fill(
            "Nothing selected provides {feature}. Turn on a plugin that does.",
            &[("feature", &say(value(&line.feature)))],
        ),
        "invalid_declaration" => fill(
            "{plugins} describes itself wrongly and cannot be built.",
            &[("plugins", &titles(&line.plugins))],
        ),
        "missing_dart" => fill(
            "Building plugins needs Dart {version}. Install it, then open this page again.",
            &[("version", &value(&line.expected))],
        ),
        "incompatible_dart" => fill(
            "Dart {installed} is installed, but building plugins needs {required}.",
            &[
                ("installed", &value(&line.installed)),
                ("required", &value(&line.required)),
            ],
        ),
        "preparing" => tr("Plugin support is being prepared.").to_owned(),
        _ => line.text.clone(),
    }
}

/// Why a request failed, in Settings' words: by its code, and the
/// manager's own English for a code Settings does not know.
fn failed(code: &str, text: &str) -> SharedString {
    match code {
        "did_not_start" => {
            tr("Your plugins were built but did not start. Denial kept the desktop it was running.")
        }
        "conflict" => tr("Several plugins provide the same feature. Keep one, then apply again."),
        "two_panels" => tr("More than one desktop panel is selected. Keep one, then apply again."),
        "missing" => {
            tr("A feature the desktop needs is missing. Turn on a plugin that provides it, then apply again.")
        }
        "declaration" => tr("A plugin describes itself wrongly. The details name it."),
        "download" => tr("A download failed. Check the connection and apply again."),
        "log_in_again" => tr("Log out and back into Denial once, then apply again."),
        "other_denial" => tr("Your plugins were built for another Denial. Apply builds them again."),
        "stopped" => tr("The last change was stopped before it finished."),
        "failed" => tr("The last change failed. The details say why."),
        _ => return text.to_owned().into(),
    }
    .into()
}

/// What a request was asked to do, in Settings' words.
fn asked(operation: &str) -> SharedString {
    tr(match operation {
        "apply" => "Apply",
        "update" => "Update plugins",
        "plan" => "Check the selection",
        "undo" | "revert" => "Undo",
        "off" | "restore" => "Standard desktop",
        "add" | "enable" => "Add a plugin",
        "remove" => "Turn a plugin off",
        "defaults" => "Use Denial defaults",
        "refresh-catalog" => "Refresh community plugins",
        "initialize" | "prepare" => "Prepare plugin support",
        "rebuild" => "Rebuild after a Denial update",
        other => return other.to_owned().into(),
    })
    .into()
}

/// How a request stands or ended, in Settings' words.
fn stands(state: &str) -> SharedString {
    tr(match state {
        "done" => "Completed",
        "failed" => "Failed",
        "stopped" => "Stopped",
        "working" => "In progress",
        "waiting" => "Waiting",
        other => return other.to_owned().into(),
    })
    .into()
}

/// How long ago `seconds` ago was.
fn ago(seconds: u64) -> String {
    let count = |text, count: u64| fill(text, &[("n", &count.to_string())]);
    match seconds {
        0..60 => tr("just now").to_owned(),
        60..3_600 => count("{n} min ago", seconds / 60),
        3_600..86_400 => count("{n} h ago", seconds / 3_600),
        _ => count("{n} d ago", seconds / 86_400),
    }
}

/// The running step and how long it has taken: "Step 3 of 5 · 0:42".
fn progress(work: &Work) -> Option<SharedString> {
    let step = match (work.step, work.steps) {
        (Some(step), Some(steps)) => Some(fill(
            "Step {step} of {steps}",
            &[("step", &step.to_string()), ("steps", &steps.to_string())],
        )),
        _ => None,
    };
    let clock = work
        .started
        .as_deref()
        .and_then(plugins::seconds_since)
        .map(|seconds| format!("{}:{:02}", seconds / 60, seconds % 60));
    match (step, clock) {
        (Some(step), Some(clock)) => Some(format!("{step} · {clock}").into()),
        (Some(said), None) | (None, Some(said)) => Some(said.into()),
        (None, None) => None,
    }
}

impl Settings {
    /// Reads the manager while the Plugins page shows: often while it
    /// works, every few seconds otherwise.
    pub(super) fn follow_plugins(&mut self, cx: &mut Context<Self>) {
        if !self.screen.is_page("plugins") {
            self.plugins.following = None;
            return;
        }
        if self.plugins.following.is_some() {
            return;
        }
        let prepare = !std::mem::replace(&mut self.plugins.prepared, true);
        self.plugins.following = Some(cx.spawn(async move |this, cx| {
            if prepare {
                // It returns at once; what it starts shows as the work.
                let _ = cx.background_spawn(async { plugins::prepare() }).await;
            }
            loop {
                // The activity is read only while the user looks at it.
                let Ok(history) = this.update(cx, |view, _| view.plugins.history_shown) else {
                    return;
                };
                let read = cx
                    .background_spawn(async move {
                        (
                            plugins::status(),
                            plugins::list(),
                            history.then(plugins::history),
                        )
                    })
                    .await;
                let Ok(working) = this.update(cx, |view, cx| view.take_plugins(read, cx)) else {
                    return;
                };
                cx.background_executor()
                    .timer(if working { WORKING } else { IDLE })
                    .await;
            }
        }));
    }

    /// Reads the manager now, after something was asked of it.
    fn read_plugins(&mut self, cx: &mut Context<Self>) {
        self.plugins.following = None;
        self.follow_plugins(cx);
    }

    /// Takes what the manager said; whether it is working.
    fn take_plugins(
        &mut self,
        (status, list, history): (
            Result<Summary, String>,
            Result<Listing, String>,
            Option<Result<Vec<Entry>, String>>,
        ),
        cx: &mut Context<Self>,
    ) -> bool {
        let state = &mut self.plugins;
        state.sending = false;
        match status {
            Ok(summary) => {
                state.problem = None;
                state.summary = Some(summary);
            }
            Err(problem) => {
                state.problem = Some(problem);
                state.summary = None;
            }
        }
        if let Ok(list) = list {
            state.listing = list;
        }
        if state.history_shown && history.is_some() {
            state.history = history;
        }
        // The details of a failure that is over close with it.
        let failure = state
            .summary
            .as_ref()
            .and_then(|summary| summary.failure.as_ref())
            .map(|failure| failure.job.as_str());
        if state
            .opened
            .as_ref()
            .is_some_and(|(job, _)| Some(job.as_str()) != failure)
        {
            state.opened = None;
        }
        cx.notify();
        state
            .summary
            .as_ref()
            .is_some_and(|summary| summary.work.is_some())
    }

    /// A line of text in an island, in the secondary colour, or the error
    /// colour when `wrong`.
    pub(super) fn island_text(&self, text: impl Into<SharedString>, wrong: bool) -> Div {
        let ink = if wrong {
            self.look.palette.error
        } else {
            self.look.palette.secondary
        };
        div()
            .px(px(ROW_PAD))
            .pb(px(12.))
            .text_size(px(14.))
            .text_color(solid(ink))
            .child(text.into())
    }

    pub(super) fn plugins_page(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let state = &self.plugins;
        let Some(summary) = &state.summary else {
            let (glyph, said): (Icon, SharedString) = match &state.problem {
                Some(problem) if problem == plugins::OLD => (
                    Icon::Update,
                    tr("This Denial's plugin manager is older than Settings. Update Denial to manage plugins here.").into(),
                ),
                Some(problem) => (Icon::Alert, problem.clone().into()),
                None => (Icon::Progress, tr("Reading your plugins…").into()),
            };
            return vec![island(&self.look).child(self.row(
                "plugins-reading",
                Some(glyph),
                said,
                None,
                (true, true),
            ))];
        };
        let live = layer.live() && !state.sending;
        let can = summary.can;
        let mut parts = vec![self.plugins_status(summary, live, cx)];

        if matches!(
            summary.setup.code.as_str(),
            "missing_dart" | "incompatible_dart" | "tools" | "preparing"
        ) {
            let glyph = if summary.setup.code == "preparing" {
                Icon::Progress
            } else {
                Icon::Alert
            };
            parts.push(island(&self.look).child(self.row(
                "plugins-setup",
                Some(glyph),
                words(&summary.setup),
                None,
                (true, true),
            )));
        }
        if !summary.issues.is_empty() {
            parts.push(self.chip_row(tr("Before you apply")));
            parts.push(self.rows_island(
                summary.issues.iter().enumerate(),
                0.,
                |(ix, issue), ends| {
                    self.row(
                        SharedString::from(format!("plugins-issue-{ix}")),
                        Some(Icon::Alert),
                        words(issue),
                        None,
                        ends,
                    )
                },
            ));
        }

        // The plugins the user has, and the ones the community list offers.
        let (mine, offered): (Vec<&Plugin>, Vec<&Plugin>) = state
            .listing
            .plugins
            .iter()
            .partition(|plugin| plugin.installed || plugin.selected);
        if !mine.is_empty() {
            parts.push(self.chip_row(tr("Your desktop is built from")));
            parts.push(self.rows_island(mine.into_iter(), 0., |plugin, ends| {
                self.plugin_row(plugin, ends, live && can.change, cx)
            }));
        }
        if !summary.included.is_empty() {
            parts.push(self.chip_row(tr("Included automatically")));
            parts.push(
                self.rows_island(summary.included.iter(), 0., |plugin, ends| {
                    let name = if plugin.title.is_empty() {
                        title(&plugin.name)
                    } else {
                        plugin.title.clone()
                    };
                    // The selected plugin that needs it, and the way down.
                    let through = (!plugin.through.is_empty()).then(|| {
                        let through: Vec<String> =
                            plugin.through.iter().map(|name| title(name)).collect();
                        fill("Needed by {plugins}", &[("plugins", &through.join(" → "))]).into()
                    });
                    self.row(
                        SharedString::from(format!("plugin-included:{}", plugin.name)),
                        None,
                        name,
                        through,
                        ends,
                    )
                }),
            );
        }
        if !offered.is_empty() {
            parts.push(self.chip_row(tr("From the community")));
            parts.push(self.rows_island(offered.into_iter(), 0., |plugin, ends| {
                self.plugin_row(plugin, ends, live && can.change, cx)
            }));
        }

        parts.push(self.chip_row(tr("More plugins")));
        let community = &state.listing.community;
        let read: SharedString = if !state.listing.errors.is_empty() {
            tr("The community list could not be loaded").into()
        } else {
            match community
                .updated
                .as_deref()
                .and_then(plugins::seconds_since)
            {
                Some(seconds) => fill("Last read {when}", &[("when", &ago(seconds))]).into(),
                None => tr("Read the list of plugins others have published").into(),
            }
        };
        let mut more: Vec<(&'static str, Icon, &'static str, SharedString, bool)> = vec![
            (
                "plugins-add",
                Icon::Plus,
                "Add a plugin",
                tr("From a folder on this computer, or a Git link").into(),
                can.change,
            ),
            (
                "plugins-update",
                Icon::Update,
                "Update plugins",
                tr("Fetch the newest version of every plugin added by link, and apply").into(),
                can.apply,
            ),
        ];
        if community.configured {
            more.push((
                "plugins-refresh",
                Icon::Retry,
                "Refresh community plugins",
                read,
                can.change,
            ));
        }
        more.push((
            "plugins-defaults",
            Icon::Star,
            "Use Denial defaults",
            tr("Select the plugins Denial comes with, and only them").into(),
            can.change && !state.listing.default_selection,
        ));
        parts.push(self.rows_island(
            more.into_iter(),
            0.,
            |(id, glyph, name, detail, can), ends| {
                let row = self
                    .row(id, Some(glyph), tr(name), Some(detail), ends)
                    .child(self.chevron());
                let live = live && can;
                self.live(
                    row.when(!live, |row| row.opacity(0.5)),
                    live,
                    cx,
                    move |this, window, cx| match id {
                        "plugins-add" => this.ask_plugin(window, cx),
                        "plugins-update" => this.request_plugins(&["apply", "--update"], cx),
                        "plugins-refresh" => this.request_plugins(&["refresh-catalog"], cx),
                        _ => this.use_plugin_defaults(cx),
                    },
                )
            },
        ));
        parts.push(self.plugins_history(layer.live(), cx));
        parts
    }

    /// A plugin of the list, with the switch that selects it.
    fn plugin_row(
        &self,
        plugin: &Plugin,
        ends: (bool, bool),
        live: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        // Where a plugin of the user's own comes from says which copy
        // this is.
        let from = plugin
            .from
            .as_deref()
            .filter(|_| plugin.kind.as_deref() != Some("builtin"));
        let detail = match (
            plugin
                .description
                .as_deref()
                .filter(|said| !said.is_empty()),
            from,
        ) {
            (Some(description), Some(from)) => Some(format!("{description}\n{from}")),
            (Some(said), None) | (None, Some(said)) => Some(said.to_owned()),
            (None, None) => None,
        };
        let name = if plugin.title.is_empty() {
            title(&plugin.name)
        } else {
            plugin.title.clone()
        };
        let row = self
            .row(
                SharedString::from(format!("plugin:{}", plugin.name)),
                None,
                name,
                detail.map(SharedString::from),
                ends,
            )
            .child(self.toggle(plugin.selected));
        let live = live && !self.plugins.saving.contains(&plugin.name);
        let (name, on) = (plugin.name.clone(), !plugin.selected);
        self.live(
            row.when(!live, |row| row.opacity(0.5)),
            live,
            cx,
            move |this, _, cx| this.select_plugin(name.clone(), on, cx),
        )
    }

    /// What was asked of the manager lately and how it ended, behind a
    /// row that shows and hides it.
    fn plugins_history(&self, live: bool, cx: &mut Context<Self>) -> Div {
        let state = &self.plugins;
        let head = self
            .row(
                "plugins-history",
                Some(Icon::Recent),
                tr("Recent activity"),
                None,
                (true, !state.history_shown),
            )
            .child(self.value(tr(if state.history_shown { "Hide" } else { "Show" })));
        let history = island(&self.look)
            .child(self.live(head, live, cx, |this, _, cx| this.show_plugin_history(cx)));
        if !state.history_shown {
            return history;
        }
        let entries = match &state.history {
            None => return history.child(self.island_text(tr("Reading…"), false)),
            Some(Err(problem)) => return history.child(self.island_text(problem.clone(), true)),
            Some(Ok(entries)) if entries.is_empty() => {
                return history.child(self.island_text(tr("Nothing was asked yet."), false));
            }
            Some(Ok(entries)) => &entries[..entries.len().min(HISTORY)],
        };
        let mut rows = div();
        for (ix, entry) in entries.iter().enumerate() {
            let over = matches!(entry.state.as_str(), "failed" | "stopped");
            let open = over && state.history_opened.as_deref() == Some(entry.job.as_str());
            let glyph = match entry.state.as_str() {
                "done" => Icon::Delivered,
                "failed" | "stopped" => Icon::Alert,
                _ => Icon::Progress,
            };
            let stands = stands(&entry.state);
            let detail = match plugins::seconds_since(&entry.when) {
                Some(seconds) => format!("{stands} · {}", ago(seconds)).into(),
                None => stands,
            };
            let row = self.row(
                SharedString::from(format!("plugins-entry:{}", entry.job)),
                Some(glyph),
                asked(&entry.operation),
                Some(detail),
                (false, ix + 1 == entries.len() && !open),
            );
            rows = rows.child(self.separator(0.));
            if !over {
                rows = rows.child(row);
                continue;
            }
            let job = entry.job.clone();
            rows = rows.child(self.live(
                row.child(self.value(tr(if open { "Hide" } else { "Details" }))),
                live,
                cx,
                move |this, _, cx| {
                    let opened = &mut this.plugins.history_opened;
                    *opened = (opened.as_deref() != Some(job.as_str())).then(|| job.clone());
                    cx.notify();
                },
            ));
            if open {
                // "It failed" is said by the row; a known reason says more.
                let mut reason = div();
                if !matches!(entry.code.as_str(), "failed" | "stopped") {
                    reason =
                        reason.child(self.island_text(failed(&entry.code, &entry.text), false));
                }
                if !entry.detail.is_empty() {
                    reason = reason.child(self.island_text(entry.detail.clone(), true));
                }
                rows = rows.child(self.appearing(("plugins-entry-reason", ix), reason));
            }
        }
        history.child(self.appearing("plugins-history-rows", rows))
    }

    /// Shows or hides the recent activity; shown, it is read at once and
    /// with every reading of the manager after.
    fn show_plugin_history(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.plugins;
        state.history_shown = !state.history_shown;
        state.history = None;
        state.history_opened = None;
        cx.notify();
        if self.plugins.history_shown {
            self.read_plugins(cx);
        }
    }

    /// The island that says what runs and why, what is being done about
    /// it, and what failed, over the buttons that change it.
    fn plugins_status(&self, summary: &Summary, live: bool, cx: &mut Context<Self>) -> Div {
        let state = &self.plugins;
        let can = summary.can;
        let (running, plugins) = desktop(&summary.desktop);
        let mut status = island(&self.look).child(self.row(
            "plugins-desktop",
            Some(Icon::Computer),
            running,
            plugins,
            (true, false),
        ));
        for line in &summary.why {
            status = status.child(self.island_text(words(line), false));
        }
        if let Some(work) = &summary.work {
            status = status.child(self.separator(0.)).child(self.row(
                "plugins-work",
                Some(Icon::Progress),
                say(work.label.clone()),
                progress(work),
                (false, false),
            ));
        }
        let failure = summary
            .failure
            .as_ref()
            .filter(|failure| state.dismissed.as_deref() != Some(failure.job.as_str()));
        if let Some(failure) = failure {
            let open = state
                .opened
                .as_ref()
                .filter(|(job, _)| *job == failure.job)
                .map(|(_, log)| log);
            let row = self
                .row(
                    "plugins-failure",
                    Some(Icon::Alert),
                    tr("The last change failed"),
                    Some(failed(&failure.code, &failure.text)),
                    (false, false),
                )
                .child(self.value(tr(if open.is_some() { "Hide" } else { "Details" })));
            let job = failure.job.clone();
            status = status.child(self.separator(0.)).child(self.live(
                row,
                live,
                cx,
                move |this, _, cx| this.open_plugin_failure(job.clone(), cx),
            ));
            if let Some(log) = open {
                let mut details = div();
                if !failure.detail.is_empty() {
                    details = details.child(self.island_text(failure.detail.clone(), true));
                }
                let log: SharedString = match log {
                    None => tr("Reading what it printed…").into(),
                    Some(log) if log.is_empty() => tr("It printed nothing.").into(),
                    Some(log) => log.clone().into(),
                };
                let job = failure.job.clone();
                details = details
                    .child(self.island_text(log, false).text_size(px(12.)))
                    .child(
                        self.buttons([self.button(
                            "plugins-dismiss",
                            tr("Put away"),
                            Fill::Control,
                            live,
                            cx,
                            move |this, _, cx| {
                                this.plugins.dismissed = Some(job.clone());
                                this.plugins.opened = None;
                                cx.notify();
                            },
                        )])
                        .px(px(ROW_PAD))
                        .pb(px(12.)),
                    );
                status = status.child(self.appearing("plugins-failure-details", details));
            }
        }

        // Apply is the main thing to do when the manager says so.
        let apply = summary
            .next
            .as_ref()
            .is_some_and(|next| next.code == "apply");
        let mut buttons = vec![self.button(
            "plugins-apply",
            tr("Apply"),
            if apply { Fill::Accent } else { Fill::Control },
            live && can.apply,
            cx,
            |this, _, cx| this.request_plugins(&["apply"], cx),
        )];
        if can.switch_now {
            buttons.push(self.button(
                "plugins-switch",
                tr("Switch now"),
                Fill::Accent,
                live,
                cx,
                |this, _, cx| this.switch_plugins_now(cx),
            ));
        }
        buttons.push(self.button(
            "plugins-undo",
            tr("Undo"),
            Fill::Control,
            live && can.undo,
            cx,
            |this, _, cx| this.request_plugins(&["undo"], cx),
        ));
        buttons.push(self.button(
            "plugins-off",
            tr("Standard desktop"),
            Fill::Control,
            live && can.off,
            cx,
            |this, _, cx| this.request_plugins(&["off"], cx),
        ));
        status.child(
            self.buttons(buttons)
                .px(px(ROW_PAD))
                .pt(px(4.))
                .pb(px(ROW_PAD)),
        )
    }

    /// Turns a plugin on or off in the saved selection.
    fn select_plugin(&mut self, name: String, on: bool, cx: &mut Context<Self>) {
        if !self.plugins.saving.insert(name.clone()) {
            return;
        }
        // The switch moves at once; the manager's answer follows.
        if let Some(plugin) = self
            .plugins
            .listing
            .plugins
            .iter_mut()
            .find(|plugin| plugin.name == name)
        {
            plugin.selected = on;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let asked = name.clone();
            let saved = cx
                .background_spawn(async move { plugins::select(&asked, on) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.plugins.saving.remove(&name);
                if let Err(error) = saved {
                    eprintln!("settings: selecting {name}: {error}");
                    view.note(error, cx);
                }
                view.read_plugins(cx);
            });
        })
        .detach();
    }

    /// Sends the manager a request that goes on by itself, and reads it
    /// again at once: the request is then its work.
    fn request_plugins(&mut self, request: &'static [&'static str], cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.plugins.sending, true) {
            return;
        }
        self.plugins.dismissed = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let sent = cx
                .background_spawn(async move { plugins::submit(request) })
                .await;
            let _ = this.update(cx, |view, cx| {
                if let Err(error) = sent {
                    eprintln!("settings: plugins {request:?}: {error}");
                    view.plugins.sending = false;
                    view.note(error, cx);
                }
                view.read_plugins(cx);
            });
        })
        .detach();
    }

    /// Makes the saved selection the plugins Denial comes with.
    fn use_plugin_defaults(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.plugins.sending, true) {
            return;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let done = cx.background_spawn(async { plugins::defaults() }).await;
            let _ = this.update(cx, |view, cx| {
                match done {
                    Ok(()) => view.note(
                        tr("Denial's own plugins are selected. Apply to use them."),
                        cx,
                    ),
                    Err(error) => {
                        eprintln!("settings: selecting Denial's plugins: {error}");
                        view.plugins.sending = false;
                        view.note(error, cx);
                    }
                }
                view.read_plugins(cx);
            });
        })
        .detach();
    }

    fn switch_plugins_now(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let asked = cx.background_spawn(async { plugins::switch_now() }).await;
            let _ = this.update(cx, |view, cx| {
                if let Err(error) = asked {
                    view.note(error, cx);
                }
                view.read_plugins(cx);
            });
        })
        .detach();
    }

    /// Shows or hides a failure's details, reading its log the first time.
    fn open_plugin_failure(&mut self, job: String, cx: &mut Context<Self>) {
        if self.plugins.opened.take().is_some() {
            return cx.notify();
        }
        self.plugins.opened = Some((job.clone(), None));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let asked = job.clone();
            let log = cx
                .background_spawn(async move { plugins::log(&asked) })
                .await
                .unwrap_or_else(|error| error);
            // Its end says what went wrong; the start is the routine.
            let lines: Vec<&str> = log.lines().collect();
            let log = lines[lines.len().saturating_sub(LOG_LINES)..].join("\n");
            let _ = this.update(cx, |view, cx| {
                if let Some((opened, read)) = &mut view.plugins.opened
                    && *opened == job
                {
                    *read = Some(log);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Asks where a plugin to add is.
    fn ask_plugin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.output_field
            .update(cx, |field, cx| field.set_text(String::new(), cx));
        self.show(
            Dialog::TextEdit {
                title: "Add a plugin",
                detail: "A folder on this computer, or a Git link. A plugin becomes part of your desktop: add code you trust.",
                valid: Rc::new(|text| !text.trim().is_empty()),
                save: Rc::new(|this, text, cx| this.find_plugin(plugins::located(&text), cx)),
            },
            cx,
        );
        window.focus(&self.output_field.focus_handle(cx), cx);
    }

    /// Looks for plugin packages at `location`: the only one is added, and
    /// several are offered to pick from.
    fn find_plugin(&mut self, location: String, cx: &mut Context<Self>) {
        self.note(tr("Looking for plugins there…"), cx);
        cx.spawn(async move |this, cx| {
            let asked = location.clone();
            let found = cx
                .background_spawn(async move { plugins::inspect(&asked) })
                .await;
            let _ = this.update(cx, |view, cx| match found {
                Err(error) => {
                    eprintln!("settings: looking for plugins in {location}: {error}");
                    view.note(error, cx);
                }
                Ok(found) if found.is_empty() => view.note(tr("No plugin was found there"), cx),
                Ok(found) if found.len() == 1 => {
                    view.add_plugin(location, found[0].path.clone(), cx)
                }
                Ok(found) => view.show(
                    Dialog::Pick {
                        title: tr("Which plugin?").into(),
                        options: found
                            .iter()
                            .map(|found| title(&found.name).into())
                            .collect(),
                        // None is picked yet.
                        picked: usize::MAX,
                        pick: Rc::new(move |this, choice, _, cx| {
                            if let Some(found) = found.get(choice) {
                                this.add_plugin(location.clone(), found.path.clone(), cx);
                            }
                        }),
                    },
                    cx,
                ),
            });
        })
        .detach();
    }

    /// Adds the package at `path` of `location` to the selection; the
    /// manager fetches it, and the list shows it turned on.
    fn add_plugin(&mut self, location: String, path: String, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.plugins.sending, true) {
            return;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let added = cx
                .background_spawn(async move { plugins::add(&location, &path) })
                .await;
            let _ = this.update(cx, |view, cx| {
                match added {
                    Ok(()) => view.note(tr("Added. Apply to use it."), cx),
                    Err(error) => {
                        view.plugins.sending = false;
                        view.note(error, cx);
                    }
                }
                view.read_plugins(cx);
            });
        })
        .detach();
    }

    /// Plugins' line on the home screen: what runs.
    pub(super) fn plugins_summary(&self) -> Option<SharedString> {
        let summary = self.plugins.summary.as_ref()?;
        Some(desktop(&summary.desktop).0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_as_titles() {
        assert_eq!(title("denial_top_bar"), "Top Bar");
        assert_eq!(title("my_notes"), "My Notes");
        assert_eq!(
            titles(&["denial_clock".to_owned(), "denial_desktop".to_owned()]),
            "Clock, Desktop"
        );
    }

    #[test]
    fn a_code_settings_does_not_know_keeps_the_managers_words() {
        let line = Line {
            code: "added_next_year".into(),
            text: "Something new.".into(),
            ..Line::default()
        };
        assert_eq!(words(&line), "Something new.");
        assert_eq!(
            failed("added_next_year", "It failed anew.").as_ref(),
            "It failed anew."
        );
        assert_eq!(asked("added_next_year").as_ref(), "added_next_year");
        assert_eq!(stands("added_next_year").as_ref(), "added_next_year");
    }

    #[test]
    fn a_time_past_reads_in_its_largest_unit() {
        assert_eq!(ago(59), "just now");
        assert_eq!(ago(60), "1 min ago");
        assert_eq!(ago(3_599), "59 min ago");
        assert_eq!(ago(7_300), "2 h ago");
        assert_eq!(ago(200_000), "2 d ago");
    }

    #[test]
    fn a_step_shows_with_its_number() {
        let work = Work {
            step: Some(3),
            steps: Some(5),
            ..Work::default()
        };
        assert_eq!(progress(&work).unwrap().as_ref(), "Step 3 of 5");
        assert_eq!(progress(&Work::default()), None);
    }
}
