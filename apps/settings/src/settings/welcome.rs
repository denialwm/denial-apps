//! Welcome: the tour a desktop opens on its first start. Five steps in one
//! window: a greeting, the shortcuts as they are bound now, the window
//! layout, the look, and how to support Denial. The layout and the look
//! change the desktop at once, through the same settings their pages use.
//!
//! Finishing the tour, or leaving it, records that it is done
//! (`crate::welcome`) and closes the app.

use denial_look::INSET;
use denial_look::words::{Language, language};
use denial_look_gpui::{Icon, color, control_fill, control_radius, icon, island, say, solid};
use gpui::{Context, Div, FontWeight, SharedString, Stateful, div, prelude::*, px};
use serde_json::{Value, json};

use super::Settings;
use super::parts::{Fill, ROW_PAD};
use super::phone::{GITHUB_SPONSORS, REPOSITORY, SPONSOR_PAGE, SPONSOR_PAGE_CHINESE};
use super::shortcuts::{action_label, list};
use crate::denial::{self, Appearance};

/// The steps, as the strip at the top names them.
const STEPS: [&str; 5] = ["Welcome", "Shortcuts", "Layout", "Appearance", "Support"];

/// Each step's scroll position, by its own key.
const SCROLLS: [&str; 5] = [
    "welcome-start",
    "welcome-shortcuts",
    "welcome-layout",
    "welcome-appearance",
    "welcome-support",
];

/// The widest a step's column gets, and how wide the wordmark is.
const COLUMN: f32 = 760.0;
const WORDMARK: f32 = 240.0;

/// A command at the bottom is this wide at most.
const COMMAND: f32 = 240.0;

/// What a window does with the others, and what it is called in the settings.
const LAYOUTS: [(&str, Icon, &str, &str); 4] = [
    (
        "stacking",
        Icon::Copy,
        "Stacking",
        "Place and overlap windows freely.",
    ),
    (
        "dwindle",
        Icon::Gridlines,
        "Tiling",
        "Windows tile into a balanced, branching layout.",
    ),
    (
        "scrolling",
        Icon::Layout,
        "Scrolling",
        "A flowing strip of windows with room to keep going.",
    ),
    (
        "screens",
        Icon::FitScreen,
        "Full screen",
        "Every app on its own screen. Swipe between them with three fingers.",
    ),
];

const MATERIALS: [(&str, &str); 3] = [("glass", "Glass"), ("blur", "Blur"), ("off", "None")];

/// The shortcuts every desktop needs first.
const ESSENTIALS: [&str; 6] = [
    "closeWindow",
    "minimizeWindow",
    "openOverview",
    "windowSwitcher",
    "toggleMaximize",
    "toggleFullscreen",
];

const WORKSPACES: [&str; 4] = [
    "previousWorkspace",
    "nextWorkspace",
    "moveToPreviousWorkspace",
    "moveToNextWorkspace",
];

const TILING: [&str; 12] = [
    "resizeShrinkWidth",
    "resizeGrowWidth",
    "resizeShrinkHeight",
    "resizeGrowHeight",
    "swapLeft",
    "swapRight",
    "swapUp",
    "swapDown",
    "focusLeft",
    "focusRight",
    "focusUp",
    "focusDown",
];

/// Where the tour is.
#[derive(Default)]
pub(super) struct Welcome {
    step: usize,
    /// Leaving at the first step asks once before it does.
    leaving: bool,
    /// Why finishing did not close the tour.
    error: Option<&'static str>,
}

/// One line of a shortcuts list: what it does and the keys bound to it.
struct Binding {
    label: SharedString,
    keys: Vec<SharedString>,
}

impl Settings {
    /// The whole Welcome window: the steps' strip, the step shown, and the
    /// commands under it.
    pub(super) fn welcome_layout(&mut self, width: f32, cx: &mut Context<Self>) -> Div {
        let step = self.welcome.as_ref().map_or(0, |welcome| welcome.step);
        let scroll = self.scrolls.entry(SCROLLS[step]).or_default().clone();
        let column = (width - 2.0 * INSET).min(COLUMN);
        let parts = match step {
            0 => self.welcome_start(),
            1 => self.welcome_shortcuts(),
            2 => self.welcome_windows(cx),
            3 => self.welcome_look(cx),
            _ => self.welcome_support(cx),
        };
        let page = div()
            .w_full()
            .max_w(px(column))
            .mx_auto()
            .flex()
            .flex_col()
            .gap(px(INSET))
            .children(parts);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .px(px(INSET))
                    .pt(px(INSET))
                    .child(self.welcome_steps(step)),
            )
            .child(self.marked(
                "welcome-pane",
                &scroll,
                Self::wide_scroll(SCROLLS[step], &scroll, INSET).child(page),
            ))
            .child(
                div()
                    .flex_none()
                    .px(px(INSET))
                    .pb(px(INSET))
                    .child(self.welcome_commands(step, cx)),
            )
    }

    /// The strip of steps: each a numbered circle over its name, the one
    /// shown in the accent, the ones done marked.
    fn welcome_steps(&self, current: usize) -> Div {
        let accent = solid(self.look.accent);
        let secondary = solid(self.look.palette.secondary);
        STEPS.iter().enumerate().fold(
            island(&self.look).px(px(ROW_PAD)).py(px(12.)).flex(),
            |strip, (ix, name)| {
                let circle = div()
                    .size(px(30.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD);
                let circle = if ix == current {
                    circle
                        .bg(accent)
                        .text_color(solid(self.look.on_accent))
                        .child((ix + 1).to_string())
                } else if ix < current {
                    circle.bg(control_fill(&self.look)).child(icon(
                        Icon::Read,
                        px(16.),
                        solid(self.look.accent_text),
                    ))
                } else {
                    circle
                        .bg(control_fill(&self.look))
                        .text_color(secondary)
                        .child((ix + 1).to_string())
                };
                strip.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(6.))
                        .child(circle)
                        .child(
                            div()
                                .max_w_full()
                                .truncate()
                                .text_size(px(13.))
                                .when(ix == current, |name| name.font_weight(FontWeight::SEMIBOLD))
                                .when(ix != current, |name| name.text_color(secondary))
                                .child(say(*name)),
                        ),
                )
            },
        )
    }

    /// A step's opening: what it is about, large, and a line under it.
    fn welcome_intro(&self, title: &'static str, line: &'static str) -> Div {
        div()
            .px(px(ROW_PAD))
            .pt(px(8.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(26.))
                    .font_weight(FontWeight::BOLD)
                    .child(say(title)),
            )
            .child(
                div()
                    .text_size(px(15.))
                    .text_color(solid(self.look.palette.secondary))
                    .child(say(line)),
            )
    }

    /// A line of plain words between the islands.
    fn welcome_line(&self, line: impl Into<SharedString>) -> Div {
        div()
            .px(px(ROW_PAD))
            .text_size(px(14.))
            .text_color(solid(self.look.palette.secondary))
            .child(say(line))
    }

    fn welcome_start(&self) -> Vec<Div> {
        vec![
            div()
                .pt(px(48.))
                .pb(px(24.))
                .flex()
                .justify_center()
                .child(self.wordmark(WORDMARK)),
            self.welcome_intro(
                "Welcome to Denial",
                "Make yourself at home. We will walk through the essential shortcuts, help \
                 you choose a window layout, and make the desktop feel like yours. You can \
                 change these preferences later in Settings.",
            ),
        ]
    }

    /// The keys bound to `action` now. None of Denial's defaults stand in
    /// for a binding the user removed.
    fn welcome_binding(snapshot: &Value, action: &str) -> Binding {
        let label = match action {
            "closeWindow" => say("Kill / close the focused window"),
            _ => say(action_label(action)),
        };
        let keys = list(snapshot, "shortcuts")
            .iter()
            .filter(|binding| {
                binding["target"]["type"] == "denialAction" && binding["target"]["action"] == action
            })
            .filter_map(|binding| binding["shortcut"].as_str())
            .map(|keys| SharedString::from(keys.to_owned()))
            .collect();
        Binding { label, keys }
    }

    /// `bindings` in an island, each with its keys at the row's end.
    fn welcome_bindings(&self, group: &'static str, bindings: Vec<Binding>) -> Div {
        self.rows_island(
            bindings.into_iter().enumerate(),
            0.,
            |(ix, binding), ends| {
                let keys = div()
                    .flex_none()
                    .max_w(gpui::relative(0.6))
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .gap(px(6.))
                    .children(binding.keys.iter().map(|keys| {
                        div()
                            .px(px(10.))
                            .py(px(5.))
                            .rounded(control_radius(&self.look))
                            .bg(control_fill(&self.look))
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(keys.clone())
                    }))
                    .when(binding.keys.is_empty(), |keys| {
                        keys.child(self.value("Not assigned"))
                    });
                self.row(
                    SharedString::from(format!("welcome-{group}-{ix}")),
                    None,
                    binding.label,
                    None,
                    ends,
                )
                .child(keys)
            },
        )
    }

    fn welcome_shortcuts(&self) -> Vec<Div> {
        let snapshot = self.shortcuts.snapshot.as_ref();
        // Moving and resizing with the pointer are built in, not bound.
        let mut essentials = vec![
            Binding {
                label: say("Resize a window"),
                keys: vec![say("Super + right-button drag")],
            },
            Binding {
                label: say("Move a window"),
                keys: vec![say("Super + left-button drag")],
            },
        ];
        if let Some(snapshot) = snapshot {
            essentials.extend(
                ESSENTIALS
                    .iter()
                    .map(|action| Self::welcome_binding(snapshot, action)),
            );
        }
        let mut parts = vec![
            self.welcome_intro(
                "A different desktop. A few keys to make it yours.",
                "These are your current bindings. You can change them later in Settings → \
                 Shortcuts.",
            ),
            self.chip_row("Desktop essentials"),
            self.welcome_bindings("essentials", essentials),
            self.welcome_line(
                "Hold Super (the Windows key) and drag anywhere on a window. These pointer \
                 gestures are built in.",
            ),
        ];
        let Some(snapshot) = snapshot else {
            parts.push(self.notice_row("Waiting for shortcuts"));
            return parts;
        };
        let count = self
            .kept("layout.workspaceCount")
            .and_then(Value::as_u64)
            .unwrap_or(4)
            .min(9);
        let numbered = (1..=count).flat_map(|number| {
            [
                format!("switchWorkspace{number}"),
                format!("moveToWorkspace{number}"),
            ]
        });
        let workspaces = WORKSPACES
            .iter()
            .map(|action| Self::welcome_binding(snapshot, action))
            .chain(numbered.map(|action| Self::welcome_binding(snapshot, &action)))
            .collect();
        parts.push(self.chip_row("Workspaces"));
        parts.push(self.welcome_bindings("workspaces", workspaces));
        if self.kept("layout.workspacesEnabled") == Some(&Value::Bool(false)) {
            parts.push(self.welcome_line(
                "Workspaces are currently disabled. Enable them in Settings → Layout to use \
                 these bindings.",
            ));
        }
        parts.push(self.chip_row("Tiling and keyboard control"));
        parts.push(
            self.welcome_bindings(
                "tiling",
                TILING
                    .iter()
                    .map(|action| Self::welcome_binding(snapshot, action))
                    .collect(),
            ),
        );
        parts
    }

    /// A row that picks one of a few: marked when it is the one in use.
    fn welcome_choice(
        &self,
        id: &'static str,
        glyph: Option<Icon>,
        title: &'static str,
        detail: Option<&'static str>,
        chosen: bool,
        ends: (bool, bool),
        cx: &mut Context<Self>,
        choose: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let mark = div()
            .flex_none()
            .size(px(24.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center();
        let mark = if chosen {
            mark.bg(solid(self.look.accent)).child(icon(
                Icon::Read,
                px(14.),
                solid(self.look.on_accent),
            ))
        } else {
            mark.border_2()
                .border_color(color(self.look.palette.foreground, 0.25))
        };
        let row = self
            .row(id, glyph, title, detail.map(say), ends)
            .child(mark);
        self.live(row, true, cx, move |this, _, cx| choose(this, cx))
    }

    fn welcome_windows(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let layout = self
            .kept("layout.windowLayout")
            .and_then(Value::as_str)
            .unwrap_or("stacking")
            .to_owned();
        let choices = self.rows_island(
            LAYOUTS.iter(),
            0.,
            |&(value, glyph, title, detail), ends| {
                self.welcome_choice(
                    value,
                    Some(glyph),
                    title,
                    Some(detail),
                    layout == value,
                    ends,
                    cx,
                    move |this, cx| {
                        this.change(
                            "Windows",
                            move |document| {
                                denial::put(document, "layout.windowLayout", json!(value))
                            },
                            cx,
                        );
                    },
                )
            },
        );
        vec![
            self.welcome_intro(
                "Make room for the way you work.",
                "Try a choice. Your desktop changes immediately.",
            ),
            choices,
        ]
    }

    fn welcome_look(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let material = self
            .kept("appearance.transparencyMode")
            .and_then(Value::as_str)
            .unwrap_or("glass")
            .to_owned();
        // On glass the tint is the theme; elsewhere the colour scheme is.
        let light = if material == "glass" {
            self.kept("appearance.glass.appearance") == Some(&json!("light"))
        } else {
            self.kept("appearance.colorSchemePreference") == Some(&json!("preferLight"))
        };
        let materials = self.rows_island(MATERIALS.iter(), 0., |&(value, title), ends| {
            self.welcome_choice(
                value,
                None,
                title,
                None,
                material == value,
                ends,
                cx,
                move |this, cx| {
                    this.change(
                        "Surfaces",
                        move |document| {
                            denial::put(document, "appearance.transparencyMode", json!(value))
                        },
                        cx,
                    );
                },
            )
        });
        let themes = [
            ("welcome-light", Icon::Display, "Light", Appearance::Light),
            ("welcome-dark", Icon::Moon, "Dark", Appearance::Dark),
        ];
        let themes = self.rows_island(themes.into_iter(), 0., |(id, glyph, title, theme), ends| {
            self.welcome_choice(
                id,
                Some(glyph),
                title,
                None,
                light == (theme == Appearance::Light),
                ends,
                cx,
                move |this, cx| {
                    this.change(
                        "Appearance",
                        move |document| denial::set_appearance(document, theme),
                        cx,
                    );
                },
            )
        });
        vec![
            self.welcome_intro(
                "A desktop that feels like you.",
                "Try a choice. Your desktop changes immediately.",
            ),
            self.chip_row("Window material"),
            materials,
            self.chip_row("Theme"),
            themes,
        ]
    }

    fn welcome_support(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let heart = div()
            .size(px(64.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(solid(self.look.accent))
            .child(icon(Icon::Heart, px(30.), solid(self.look.on_accent)));
        let give = self.buttons([
            self.button(
                "welcome-donate",
                "Donate",
                Fill::Accent,
                true,
                cx,
                |_, _, cx| {
                    // The sponsor page has one address for each language.
                    cx.open_url(match language() {
                        Language::Chinese => SPONSOR_PAGE_CHINESE,
                        _ => SPONSOR_PAGE,
                    })
                },
            ),
            self.button(
                "welcome-sponsors",
                "GitHub Sponsors",
                Fill::Control,
                true,
                cx,
                |_, _, cx| cx.open_url(GITHUB_SPONSORS),
            ),
        ]);
        let card = island(&self.look)
            .p(px(28.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(24.))
            .child(heart)
            .child(div().w_full().max_w(px(2.0 * COMMAND)).child(give))
            .child(
                div()
                    .w_full()
                    .max_w(px(2.0 * COMMAND))
                    .flex()
                    .child(self.button(
                        "welcome-repository",
                        "Explore Denial on GitHub",
                        Fill::Control,
                        true,
                        cx,
                        |_, _, cx| cx.open_url(REPOSITORY),
                    )),
            );
        vec![
            div()
                .pt(px(24.))
                .pb(px(8.))
                .flex()
                .justify_center()
                .child(self.wordmark(WORDMARK)),
            self.welcome_intro(
                "Help Denial become what comes next.",
                "Denial is built with care, curiosity, and the belief that a desktop can be \
                 different. If it feels like home, your support helps keep it growing.",
            ),
            card,
            self.welcome_line(
                "Always optional. Thank you for giving Denial a place on your desktop.",
            )
            .text_center(),
        ]
    }

    /// The commands under a step: back on the left, onward on the right.
    /// Leaving at the first step asks first, in the same place.
    fn welcome_commands(&self, step: usize, cx: &mut Context<Self>) -> Div {
        let welcome = self.welcome.as_ref();
        let command =
            |button: Stateful<Div>| div().flex_1().max_w(px(COMMAND)).flex().child(button);
        if welcome.is_some_and(|welcome| welcome.leaving) {
            return island(&self.look)
                .p(px(ROW_PAD))
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .child(div().flex_1().min_w(px(200.)).child(say(
                    "The setup won't appear again, you can open it from your launcher.",
                )))
                .child(div().w(px(2.0 * COMMAND)).max_w_full().child(self.buttons([
                    self.button(
                        "welcome-stay",
                        "Cancel",
                        Fill::Control,
                        true,
                        cx,
                        |this, _, cx| this.welcome_leaving(false, cx),
                    ),
                    self.button(
                        "welcome-leave",
                        "Exit",
                        Fill::Accent,
                        true,
                        cx,
                        |this, _, cx| this.finish_welcome(cx),
                    ),
                ])));
        }
        let last = STEPS.len() - 1;
        let back = if step == 0 {
            self.button(
                "welcome-exit",
                "Exit setup",
                Fill::Control,
                true,
                cx,
                |this, _, cx| this.welcome_leaving(true, cx),
            )
        } else {
            self.button(
                "welcome-previous",
                "Previous",
                Fill::Control,
                true,
                cx,
                |this, _, cx| this.welcome_back(cx),
            )
        };
        let onward = match step {
            0 => "Get started",
            step if step == last => "Start using Denial",
            _ => "Next",
        };
        let onward = self.button(
            "welcome-next",
            onward,
            Fill::Accent,
            true,
            cx,
            move |this, _, cx| {
                if step == last {
                    this.finish_welcome(cx);
                } else {
                    this.welcome_to(step + 1, cx);
                }
            },
        );
        let error = welcome
            .and_then(|welcome| welcome.error)
            .map(|error| self.notice_row(error));
        div()
            .flex()
            .flex_col()
            .gap(px(INSET))
            .children(error)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .gap(px(16.))
                    .child(command(back))
                    .child(command(onward)),
            )
    }

    fn welcome_to(&mut self, step: usize, cx: &mut Context<Self>) {
        if let Some(welcome) = &mut self.welcome {
            welcome.step = step.min(STEPS.len() - 1);
            welcome.leaving = false;
            welcome.error = None;
            cx.notify();
        }
    }

    /// A step back; at the first one, the question whether to leave goes
    /// away.
    pub(super) fn welcome_back(&mut self, cx: &mut Context<Self>) {
        let step = self.welcome.as_ref().map_or(0, |welcome| welcome.step);
        self.welcome_to(step.saturating_sub(1), cx);
    }

    fn welcome_leaving(&mut self, leaving: bool, cx: &mut Context<Self>) {
        if let Some(welcome) = &mut self.welcome {
            welcome.leaving = leaving;
            cx.notify();
        }
    }

    /// Records that the tour is done and closes Welcome. What it changed is
    /// already saved: every choice goes to Denial as it is made.
    fn finish_welcome(&mut self, cx: &mut Context<Self>) {
        match crate::welcome::complete() {
            Ok(()) => cx.quit(),
            Err(error) => {
                log::error!("Welcome could not record that it is done: {error}");
                if let Some(welcome) = &mut self.welcome {
                    welcome.leaving = false;
                    welcome.error = Some("Could not save setup completion. Please try again.");
                    cx.notify();
                }
            }
        }
    }
}
