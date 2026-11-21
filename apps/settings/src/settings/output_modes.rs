//! Staged display configuration. Only Apply submits a complete configuration;
//! deniald restores the previous configuration unless it is kept.

mod arrangement;
mod input_deck;

use std::rc::Rc;
use std::time::Duration;

use denial_look::words::fill;
use denial_look_gpui::{Icon, island};
use gpui::{Context, Div, Focusable, SharedString, Stateful, Task, Window, prelude::*};

use super::monitor::percent;
use super::panel::hertz;
use super::{Dialog, Layer, Settings};
use crate::denial::{ConfiguredOutput, InputDeckStatus, OutputConfiguration, OutputMode, Refusal};

/// Where Denial keeps the shell's own scale on each display, by output name.
const SHELL_SCALES: &str = "appearance.shellScales";

/// The shell sizes offered beside the default.
const SHELL_SIZES: [f64; 8] = [1.0, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 3.0];

/// The shell's own scale on `output` until one is set, as the shell takes
/// it: larger on a built-in panel whose scale came from its density, where
/// the desktop's sizes would come out about half as large as on a monitor.
fn automatic_shell_size(output: &ConfiguredOutput) -> f64 {
    if output.name.starts_with("DSI-") && output.scale > 1.0 {
        1.75
    } else {
        1.0
    }
}

#[derive(Default)]
pub(super) struct OutputModes {
    snapshot: Option<OutputConfiguration>,
    draft: Option<OutputConfiguration>,
    selected: Option<String>,
    reading: Option<Task<()>>,
    applying: bool,
    arrangement: arrangement::Arrangement,
    input_deck: Option<InputDeckStatus>,
}

impl OutputModes {
    fn configuration(&self) -> Option<&OutputConfiguration> {
        self.draft.as_ref().or(self.snapshot.as_ref())
    }

    fn selected(&self) -> Option<&ConfiguredOutput> {
        let configuration = self.configuration()?;
        let selected = self
            .selected
            .as_ref()
            .or(configuration.primary_output.as_ref());
        configuration
            .outputs
            .iter()
            .find(|output| Some(&output.name) == selected)
            .or_else(|| configuration.outputs.iter().find(|output| output.enabled))
            .or_else(|| configuration.outputs.first())
    }

    fn editing(&self) -> bool {
        !self.applying
            && self.configuration().is_some_and(|state| {
                state.capabilities.apply && state.pending_confirmation.is_none()
            })
    }
}

const TRANSFORMS: &[(&str, &str)] = &[
    ("normal", "Normal"),
    ("90", "90°"),
    ("180", "180°"),
    ("270", "270°"),
    ("flipped", "Flipped"),
    ("flipped-90", "Flipped 90°"),
    ("flipped-180", "Flipped 180°"),
    ("flipped-270", "Flipped 270°"),
];

const AXES: &[(&str, &str)] = &[
    ("auto", "Automatic"),
    ("horizontal", "Horizontal"),
    ("vertical", "Vertical"),
];

fn option_label(options: &[(&str, &str)], current: &str) -> SharedString {
    options
        .iter()
        .find(|(value, _)| *value == current)
        .map_or(current, |(_, label)| *label)
        .to_owned()
        .into()
}

fn resolution(mode: OutputMode) -> SharedString {
    format!("{} × {}", mode.width, mode.height).into()
}

impl Settings {
    pub(super) fn follow_output_modes(&mut self, cx: &mut Context<Self>) {
        if !self.screen.is_page("displays") {
            self.output_modes.reading = None;
            self.output_modes.draft = None;
            self.output_modes.input_deck = None;
            self.output_modes.arrangement = arrangement::Arrangement::default();
            return;
        }
        if self.output_modes.reading.is_some() {
            return;
        }
        let link = self.denial.link.clone();
        self.output_modes.reading = Some(cx.spawn(async move |this, cx| {
            loop {
                let (configuration, input_deck) = futures_lite::future::zip(
                    link.output_configuration(),
                    link.input_deck_status(),
                )
                .await;
                if this
                    .update(cx, |view, cx| {
                        let input_deck = input_deck.ok();
                        if view.output_modes.input_deck != input_deck {
                            view.output_modes.input_deck = input_deck;
                            cx.notify();
                        }
                        if view.output_modes.applying {
                            return;
                        }
                        match configuration {
                            Ok(configuration) => view.take_output_snapshot(configuration, cx),
                            Err(_) if view.output_modes.draft.is_none() => {
                                view.output_modes.snapshot = None;
                                cx.notify();
                            }
                            Err(_) => {}
                        }
                    })
                    .is_err()
                {
                    return;
                }
                cx.background_executor().timer(Duration::from_secs(2)).await;
            }
        }));
    }

    fn take_output_snapshot(&mut self, snapshot: OutputConfiguration, cx: &mut Context<Self>) {
        if self
            .output_modes
            .snapshot
            .as_ref()
            .is_some_and(|previous| previous.serial != snapshot.serial)
        {
            self.output_modes.arrangement = arrangement::Arrangement::default();
            if self.output_modes.draft.take().is_some() {
                self.note("Displays changed. Pending edits were discarded.", cx);
            }
            // A picker opened against the old state cannot edit the new topology.
            if self.screen.is_page("displays")
                && matches!(
                    self.dialog,
                    Some((
                        Dialog::Pick { .. }
                            | Dialog::OutputScale { .. }
                            | Dialog::OutputPosition { .. },
                        _
                    ))
                )
            {
                self.dialog = None;
            }
        }
        self.output_modes.snapshot = Some(snapshot);
        cx.notify();
    }

    pub(super) fn output_modes_page(
        &self,
        width: f32,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let Some(configuration) = self.output_modes.configuration() else {
            return vec![self.notice_row("Waiting for display configuration")];
        };
        let Some(output) = self.output_modes.selected() else {
            return vec![self.notice_row("No connected displays")];
        };
        let live = layer.live();
        let edit = live && self.output_modes.editing();
        let capabilities = configuration.capabilities;
        let selected = self.output_option(
            "output-select",
            "Display",
            output.label().into(),
            live && !self.output_modes.applying,
            cx,
            |this, _, cx| this.choose_config_output(cx),
        );
        let primary_label = configuration
            .primary_output
            .as_ref()
            .and_then(|name| {
                configuration
                    .outputs
                    .iter()
                    .find(|output| &output.name == name)
            })
            .map_or_else(|| "Automatic".into(), |output| output.label().into());
        let primary = self.output_option(
            "output-primary",
            "Primary display",
            primary_label,
            edit,
            cx,
            |this, _, cx| this.choose_primary_output(cx),
        );
        let mut individual = island(&self.look).child(selected);
        let can_toggle = !output.enabled
            || configuration
                .outputs
                .iter()
                .filter(|each| each.enabled)
                .count()
                > 1;
        if capabilities.enable {
            let name = output.name.clone();
            let enabled = output.enabled;
            let row = self
                .row(
                    "output-enabled",
                    None,
                    "Enable display",
                    None,
                    (false, false),
                )
                .child(self.toggle(enabled));
            individual = individual.child(self.separator(0.)).child(self.live(
                row,
                edit && can_toggle,
                cx,
                move |this, _, cx| {
                    this.set_output_enabled(&name, !enabled, cx);
                },
            ));
        }
        if output.enabled {
            if capabilities.position {
                individual = individual
                    .child(self.separator(0.))
                    .child(self.output_option(
                        "output-position",
                        "Display position",
                        format!("{}, {}", output.x, output.y).into(),
                        edit,
                        cx,
                        |this, window, cx| this.choose_output_position(window, cx),
                    ));
            }
            if capabilities.mode
                && let Some(mode) = output.mode()
            {
                individual = individual
                    .child(self.separator(0.))
                    .child(self.output_option(
                        "output-resolution",
                        "Resolution",
                        resolution(mode),
                        edit,
                        cx,
                        |this, _, cx| this.choose_resolution(cx),
                    ))
                    .child(self.separator(0.))
                    .child(self.output_option(
                        "output-refresh",
                        "Refresh rate",
                        hertz(mode.refresh_millihz),
                        edit,
                        cx,
                        |this, _, cx| this.choose_output_refresh(cx),
                    ));
            }
            if capabilities.scale {
                individual = individual
                    .child(self.separator(0.))
                    .child(self.output_option(
                        "output-scale",
                        "Scale",
                        percent(output.scale),
                        edit,
                        cx,
                        |this, _, cx| this.choose_output_scale(cx),
                    ));
            }
            individual = individual
                .child(self.separator(0.))
                .child(self.output_option(
                    "output-shell-size",
                    "Shell size",
                    self.shell_size_label(output),
                    edit,
                    cx,
                    |this, _, cx| this.choose_shell_size(cx),
                ));
            if capabilities.transform {
                individual = individual
                    .child(self.separator(0.))
                    .child(self.output_option(
                        "output-rotation",
                        "Rotation",
                        option_label(TRANSFORMS, &output.transform),
                        edit,
                        cx,
                        |this, _, cx| this.choose_output_words("Rotation", TRANSFORMS, false, cx),
                    ));
            }
            if capabilities.scrolling_layout_axis {
                individual = individual
                    .child(self.separator(0.))
                    .child(self.output_option(
                        "output-axis",
                        "Scrolling layout axis",
                        option_label(
                            AXES,
                            output.scrolling_layout_axis.as_deref().unwrap_or("auto"),
                        ),
                        edit,
                        cx,
                        |this, _, cx| {
                            this.choose_output_words("Scrolling layout axis", AXES, true, cx)
                        },
                    ));
            }
            if capabilities.adaptive_sync && output.adaptive_sync_supported {
                let name = output.name.clone();
                let enabled = output.adaptive_sync;
                let row = self
                    .row(
                        "output-adaptive",
                        None,
                        "Variable refresh rate",
                        None,
                        (false, false),
                    )
                    .child(self.toggle(enabled));
                individual = individual.child(self.separator(0.)).child(self.live(
                    row,
                    edit,
                    cx,
                    move |this, _, cx| {
                        this.edit_output(&name, |output| output.adaptive_sync = !enabled, cx);
                    },
                ));
            }
        }
        // What is done to the page's changes sits at the end of its first
        // heading: reading again always, and once something is changed,
        // dropping it. Carrying it out is in the window's corner.
        let mut actions = vec![self.action(
            "output-refresh-state",
            Icon::Flip,
            "Refresh",
            false,
            live && !self.output_modes.applying,
            cx,
            |this, _, cx| {
                this.output_modes.reading = None;
                this.follow_output_modes(cx);
            },
        )];
        if self.output_modes.draft.is_some() {
            actions.push(self.action(
                "output-discard",
                Icon::Close,
                "Discard",
                false,
                edit,
                cx,
                |this, _, cx| {
                    this.output_modes.draft = None;
                    this.output_modes.arrangement = arrangement::Arrangement::default();
                    cx.notify();
                },
            ));
        }
        let mut parts = Vec::new();
        // Every connected display has a place, so an off one can be chosen.
        let arranged = configuration.outputs.len() > 1;
        if !arranged {
            parts.push(self.heading_row("Displays", actions));
        } else {
            parts.push(self.heading_row("Display arrangement", actions));
            parts.push(self.output_arrangement(
                configuration,
                &output.name,
                width,
                live,
                edit && capabilities.position,
                cx,
            ));
        }
        parts.extend([island(&self.look).child(primary), individual]);
        parts
    }

    fn output_option(
        &self,
        id: &'static str,
        label: &'static str,
        value: SharedString,
        live: bool,
        cx: &mut Context<Self>,
        act: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let row = self
            .row(
                id,
                Some(Icon::Display).filter(|_| id == "output-select"),
                label,
                None,
                (true, false),
            )
            .child(self.value(value))
            .child(self.chevron());
        self.live(row, live, cx, act)
    }

    fn output_picker<T: PartialEq + 'static>(
        &mut self,
        title: &'static str,
        choices: Vec<(T, SharedString)>,
        current: T,
        cx: &mut Context<Self>,
        pick: impl Fn(&mut Self, &T, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if choices.is_empty() {
            return;
        }
        let picked = choices
            .iter()
            .position(|(value, _)| value == &current)
            .unwrap_or(choices.len());
        self.show(
            Dialog::Pick {
                title: title.into(),
                options: choices.iter().map(|(_, label)| label.clone()).collect(),
                picked,
                pick: Rc::new(move |this, option, window, cx| {
                    if let Some((value, _)) = choices.get(option)
                        && value != &current
                    {
                        pick(this, value, window, cx);
                    }
                }),
            },
            cx,
        );
    }

    fn choose_config_output(&mut self, cx: &mut Context<Self>) {
        let Some(configuration) = self.output_modes.configuration() else {
            return;
        };
        let Some(selected) = self.output_modes.selected() else {
            return;
        };
        let choices = configuration
            .outputs
            .iter()
            .map(|output| (output.name.clone(), output.label().into()))
            .collect();
        self.output_picker(
            "Display",
            choices,
            selected.name.clone(),
            cx,
            |this, selected, _, cx| {
                this.output_modes.selected = Some(selected.clone());
                cx.notify();
            },
        );
    }

    fn choose_primary_output(&mut self, cx: &mut Context<Self>) {
        let Some(configuration) = self.output_modes.configuration() else {
            return;
        };
        let choices = std::iter::once((None, "Automatic".into()))
            .chain(
                configuration
                    .outputs
                    .iter()
                    .filter(|output| output.enabled)
                    .map(|output| (Some(output.name.clone()), output.label().into())),
            )
            .collect();
        self.output_picker(
            "Primary display",
            choices,
            configuration.primary_output.clone(),
            cx,
            |this, primary, _, cx| {
                this.edit_outputs(
                    |configuration| configuration.primary_output = primary.clone(),
                    cx,
                );
            },
        );
    }

    fn choose_resolution(&mut self, cx: &mut Context<Self>) {
        let Some(output) = self.output_modes.selected() else {
            return;
        };
        let Some(current) = output.mode() else {
            return;
        };
        let mut modes = output.modes.clone();
        modes.sort_by_key(|mode| {
            std::cmp::Reverse((
                mode.width,
                mode.height,
                mode.refresh_millihz == current.refresh_millihz,
                mode.preferred,
                mode.refresh_millihz,
            ))
        });
        modes.dedup_by_key(|mode| (mode.width, mode.height));
        let choices = modes
            .iter()
            .map(|mode| ((mode.width, mode.height), resolution(*mode)))
            .collect();
        let name = output.name.clone();
        self.output_picker(
            "Resolution",
            choices,
            (current.width, current.height),
            cx,
            move |this, size, _, cx| {
                let mode = modes
                    .iter()
                    .copied()
                    .find(|mode| (mode.width, mode.height) == *size);
                this.edit_output(&name, |output| output.current_mode = mode, cx);
            },
        );
    }

    fn choose_output_refresh(&mut self, cx: &mut Context<Self>) {
        let Some(output) = self.output_modes.selected() else {
            return;
        };
        let Some(current) = output.mode() else {
            return;
        };
        let mut modes = output.modes.clone();
        modes.retain(|mode| (mode.width, mode.height) == (current.width, current.height));
        modes.sort_by_key(|mode| std::cmp::Reverse(mode.refresh_millihz));
        modes.dedup_by_key(|mode| mode.refresh_millihz);
        let choices = modes
            .iter()
            .map(|mode| {
                let rate = mode.refresh_millihz;
                let label = if modes.iter().any(|other| {
                    other.refresh_millihz != rate
                        && (other.refresh_millihz + 500) / 1000 == (rate + 500) / 1000
                }) {
                    format!("{:.3} Hz", f64::from(rate) / 1000.0).into()
                } else {
                    hertz(rate)
                };
                (rate, label)
            })
            .collect();
        let name = output.name.clone();
        self.output_picker(
            "Refresh rate",
            choices,
            current.refresh_millihz,
            cx,
            move |this, rate, _, cx| {
                let mode = modes
                    .iter()
                    .copied()
                    .find(|mode| mode.refresh_millihz == *rate);
                this.edit_output(&name, |output| output.current_mode = mode, cx);
            },
        );
    }

    fn choose_output_words(
        &mut self,
        title: &'static str,
        values: &'static [(&'static str, &'static str)],
        axis: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(output) = self.output_modes.selected() else {
            return;
        };
        let name = output.name.clone();
        let current = if axis {
            output.scrolling_layout_axis.as_deref().unwrap_or("auto")
        } else {
            &output.transform
        };
        let choices = values
            .iter()
            .map(|(value, label)| (value.to_string(), (*label).into()))
            .collect();
        self.output_picker(
            title,
            choices,
            current.to_owned(),
            cx,
            move |this, value, _, cx| {
                this.edit_output(
                    &name,
                    |output| {
                        if axis {
                            output.scrolling_layout_axis = Some(value.clone());
                        } else {
                            output.transform = value.clone();
                        }
                    },
                    cx,
                );
            },
        );
    }

    /// The shell's own scale on `output`, as Denial keeps it: None until one
    /// is set, and the shell then takes its own default.
    fn shell_size(&self, output: &ConfiguredOutput) -> Option<f64> {
        self.kept(&format!("{SHELL_SCALES}.{}", output.name))
            .and_then(serde_json::Value::as_f64)
    }

    fn shell_size_label(&self, output: &ConfiguredOutput) -> SharedString {
        match self.shell_size(output) {
            Some(scale) => percent(scale),
            None => fill(
                "Default, {size}",
                &[("size", &percent(automatic_shell_size(output)))],
            )
            .into(),
        }
    }

    /// How large the shell's bar, panels, menus and window frames are on the
    /// selected display, beside its scale: Denial's
    /// `appearance.shellScales`, by output name. It takes effect at once and
    /// is not part of the configuration Apply submits.
    fn choose_shell_size(&mut self, cx: &mut Context<Self>) {
        let Some(output) = self.output_modes.selected() else {
            return;
        };
        let name = output.name.clone();
        let current = self.shell_size(output);
        let automatic = automatic_shell_size(output);
        let choices = std::iter::once((
            None,
            fill("Default, {size}", &[("size", &percent(automatic))]).into(),
        ))
        .chain(
            SHELL_SIZES
                .into_iter()
                .map(|scale| (Some(scale), percent(scale))),
        )
        .collect();
        self.output_picker(
            "Shell size",
            choices,
            current,
            cx,
            move |this, value, _, cx| {
                let (name, value) = (name.clone(), *value);
                this.change(
                    "Shell size",
                    move |document| match value {
                        Some(scale) => crate::denial::put(
                            document,
                            &format!("{SHELL_SCALES}.{name}"),
                            serde_json::json!(scale),
                        ),
                        None => {
                            if let Some(scales) = document
                                .pointer_mut("/appearance/shellScales")
                                .and_then(serde_json::Value::as_object_mut)
                            {
                                scales.remove(&name);
                            }
                        }
                    },
                    cx,
                );
            },
        );
    }

    fn choose_output_scale(&mut self, cx: &mut Context<Self>) {
        let Some(output) = self.output_modes.selected() else {
            return;
        };
        let name = output.name.clone();
        let current = output.scale;
        let choices = [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0]
            .into_iter()
            .map(|scale| (Some(scale), percent(scale)))
            .chain(std::iter::once((None, "Custom scale".into())))
            .collect();
        self.output_picker(
            "Scale",
            choices,
            Some(current),
            cx,
            move |this, value, window, cx| {
                if let Some(scale) = value {
                    this.edit_output(&name, |output| output.scale = *scale, cx);
                } else {
                    this.output_field.update(cx, |input, cx| {
                        input.set_text(format!("{:.2}", current * 100.0), cx)
                    });
                    this.show(Dialog::OutputScale { name: name.clone() }, cx);
                    window.focus(&this.output_field.focus_handle(cx), cx);
                }
            },
        );
    }

    pub(super) fn custom_output_scale(&self, cx: &Context<Self>) -> Option<f64> {
        let text = self
            .output_field
            .read(cx)
            .text()
            .trim()
            .trim_end_matches('%')
            .trim();
        let percent: f64 = text.parse().ok()?;
        (25.0..=800.0)
            .contains(&percent)
            .then(|| (percent * 1.2).round() / 120.0)
    }

    pub(super) fn set_custom_output_scale(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(scale) = self.custom_output_scale(cx) {
            self.dismiss(window, cx);
            self.edit_output(name, |output| output.scale = scale, cx);
        }
    }

    fn set_output_enabled(&mut self, name: &str, enabled: bool, cx: &mut Context<Self>) {
        self.edit_outputs(
            |configuration| {
                if !enabled
                    && configuration
                        .outputs
                        .iter()
                        .filter(|output| output.enabled)
                        .count()
                        <= 1
                {
                    return;
                }
                if let Some(output) = configuration
                    .outputs
                    .iter_mut()
                    .find(|output| output.name == name)
                {
                    output.enabled = enabled;
                    output.powered = enabled;
                    if !enabled && configuration.primary_output.as_deref() == Some(name) {
                        configuration.primary_output = None;
                    }
                }
            },
            cx,
        );
    }

    fn edit_output(
        &mut self,
        name: &str,
        edit: impl FnOnce(&mut ConfiguredOutput),
        cx: &mut Context<Self>,
    ) {
        self.edit_outputs(
            |configuration| {
                if let Some(output) = configuration
                    .outputs
                    .iter_mut()
                    .find(|output| output.name == name)
                {
                    edit(output);
                }
            },
            cx,
        );
    }

    fn edit_outputs(
        &mut self,
        edit: impl FnOnce(&mut OutputConfiguration),
        cx: &mut Context<Self>,
    ) {
        if !self.output_modes.editing() {
            return;
        }
        let Some(mut draft) = self
            .output_modes
            .draft
            .take()
            .or_else(|| self.output_modes.snapshot.clone())
        else {
            return;
        };
        edit(&mut draft);
        let dirty = self.output_modes.snapshot.as_ref().is_some_and(|saved| {
            draft.outputs != saved.outputs || draft.primary_output != saved.primary_output
        });
        self.output_modes.draft = dirty.then_some(draft);
        cx.notify();
    }

    /// Whether the displays hold changes Denial has not been given, and
    /// whether they can be given now.
    pub(super) fn output_changes(&self) -> Option<bool> {
        self.output_modes
            .draft
            .is_some()
            .then(|| self.output_modes.editing())
    }

    pub(super) fn apply_output_modes(&mut self, cx: &mut Context<Self>) {
        if !self.output_modes.editing() {
            return;
        }
        let Some(draft) = self.output_modes.draft.clone() else {
            return;
        };
        self.output_modes.applying = true;
        self.output_modes.reading = None;
        let link = self.denial.link.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let applied = link.apply_output_configuration(&draft).await;
            let _ = this.update(cx, |view, cx| {
                view.output_modes.applying = false;
                match applied {
                    Ok(snapshot) => {
                        let token = snapshot.pending_confirmation.map(|pending| pending.token);
                        view.output_modes.draft = None;
                        view.take_output_snapshot(snapshot, cx);
                        if let Some(token) = token {
                            view.ask_keep(
                                token,
                                "this display configuration".into(),
                                "the previous configuration".into(),
                                cx,
                            );
                        }
                    }
                    Err(Refusal(why)) => view.note(
                        fill(
                            "Could not apply display configuration: {why}",
                            &[("why", &why)],
                        ),
                        cx,
                    ),
                }
                view.follow_output_modes(cx);
                cx.notify();
            });
        })
        .detach();
    }
}
