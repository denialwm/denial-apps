//! Output selection for brightness, dimming and the desktop system bar.

use std::rc::Rc;
use std::time::Duration;

use denial_look::words::fill;
use denial_look_gpui::{Icon, island};
use gpui::{Context, Div, Task, prelude::*};
use serde_json::{Value, json};

use super::{Dialog, Layer, Settings};
use crate::denial::{self, DisplayTarget, DisplayTargets, Refusal};

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Display,
    Layout,
}

#[derive(Clone, Copy)]
enum Control {
    Brightness,
    Dimming,
}

#[derive(Default)]
pub(super) struct DisplayControls {
    targets: Option<DisplayTargets>,
    selected: Option<i64>,
    levels: Option<(i64, Option<f32>, Option<f32>)>,
    deck: Option<String>,
    page: Option<Page>,
    task: Option<Task<()>>,
    writing: bool,
    revision: u64,
}

impl DisplayControls {
    fn selected(&self) -> Option<&DisplayTarget> {
        let targets = self.targets.as_ref()?;
        targets
            .outputs
            .iter()
            .find(|output| Some(output.id) == self.selected)
            .or_else(|| {
                targets
                    .outputs
                    .iter()
                    .find(|output| Some(output.name.as_str()) == targets.primary.as_deref())
            })
            .or_else(|| targets.outputs.first())
    }
}

fn system_bar_selection(
    targets: &DisplayTargets,
    deck: Option<&str>,
    document: &Value,
) -> Vec<String> {
    let eligible = || {
        targets
            .outputs
            .iter()
            .filter(|output| Some(output.name.as_str()) != deck)
    };
    let saved = denial::at(document, "layout.systemBarOutputs").as_array();
    let mut selection: Vec<_> = eligible()
        .filter(|output| {
            saved.is_some_and(|saved| saved.iter().any(|name| name == output.name.as_str()))
        })
        .map(|output| output.name.clone())
        .collect();
    if selection.is_empty()
        && let Some(output) = eligible()
            .find(|output| Some(output.name.as_str()) == targets.primary.as_deref())
            .or_else(|| eligible().next())
    {
        selection.push(output.name.clone());
    }
    selection
}

impl Settings {
    pub(super) fn follow_display_controls(&mut self, cx: &mut Context<Self>) {
        let page = if self.screen.is_page("display") {
            Page::Display
        } else if self.screen.is_page("layout") {
            Page::Layout
        } else {
            self.display_controls.task = None;
            self.display_controls.page = None;
            return;
        };
        if self.display_controls.page == Some(page) && self.display_controls.task.is_some() {
            return;
        }
        if self.display_controls.page != Some(page) && page == Page::Display {
            self.display_controls.levels = None;
        }
        self.display_controls.page = Some(page);
        let link = self.denial.link.clone();
        self.display_controls.task = Some(cx.spawn(async move |this, cx| {
            loop {
                let targets = link.display_targets().await.ok();
                let deck = if page == Page::Layout {
                    link.input_deck_output().await
                } else {
                    None
                };
                let Ok((target, revision, panel, cached)) = this.update(cx, |view, cx| {
                    let controls = &mut view.display_controls;
                    controls.targets = targets;
                    controls.deck = deck;
                    let target = controls.selected().cloned();
                    let cached = controls
                        .levels
                        .filter(|(id, ..)| target.as_ref().is_some_and(|target| target.id == *id));
                    cx.notify();
                    (
                        target,
                        controls.revision,
                        view.uses_panel_brightness(),
                        cached,
                    )
                }) else {
                    return;
                };
                if page == Page::Display
                    && let Some(output) = target
                {
                    // Socket brightness reads also emit the shell's brightness
                    // HUD. Sample the hardware only when selecting an output.
                    let read_brightness = async {
                        if let Some((_, brightness, _)) = cached {
                            brightness
                        } else if panel {
                            None
                        } else {
                            link.hardware_brightness(&output).await.ok()
                        }
                    };
                    let (brightness, dimming) =
                        futures_lite::future::zip(read_brightness, link.software_dimming(&output))
                            .await;
                    if this
                        .update(cx, |view, cx| {
                            let controls = &mut view.display_controls;
                            if controls.revision == revision
                                && !controls.writing
                                && view.sliding.is_none()
                                && controls
                                    .selected()
                                    .is_some_and(|target| target.id == output.id)
                            {
                                controls.levels = Some((output.id, brightness, dimming));
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                cx.background_executor().timer(Duration::from_secs(3)).await;
            }
        }));
    }

    pub(super) fn uses_panel_brightness(&self) -> bool {
        self.denial.brightness.is_some()
            && self.denial.output.as_ref().is_some_and(|panel| {
                self.display_controls.selected().map_or_else(
                    || self.display_controls.targets.is_none(),
                    |target| target.id == panel.monitor_id,
                )
            })
    }

    pub(super) fn display_level_parts(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let Some(output) = self.display_controls.selected() else {
            return Vec::new();
        };
        let live = layer.live();
        let row = self
            .row(
                "brightness-display",
                Some(Icon::Display),
                "Display",
                None,
                (true, false),
            )
            .child(self.value(output.label.clone()))
            .child(self.chevron());
        let mut group = island(&self.look).child(self.live(row, live, cx, |this, _, cx| {
            this.choose_brightness_display(cx)
        }));
        let panel = self.uses_panel_brightness();
        if panel && let Some(row) = self.panel_brightness_row(live, cx) {
            group = group.child(self.separator(0.)).child(row);
        }
        if let Some((_, brightness, dimming)) = self
            .display_controls
            .levels
            .filter(|(id, ..)| *id == output.id)
        {
            if let Some(brightness) = brightness.filter(|_| !panel) {
                group = group
                    .child(self.separator(0.))
                    .child(self.display_level_slider(
                        output,
                        Control::Brightness,
                        brightness,
                        live,
                        cx,
                    ));
            } else if !panel {
                group = group.child(self.separator(0.)).child(self.row(
                    "brightness-unavailable",
                    None,
                    "Hardware brightness unavailable",
                    None,
                    (false, false),
                ));
            }
            if let Some(dimming) = dimming {
                group = group
                    .child(self.separator(0.))
                    .child(self.display_level_slider(output, Control::Dimming, dimming, live, cx));
            }
        }
        vec![self.chip_row("Display brightness"), group]
    }

    fn choose_brightness_display(&mut self, cx: &mut Context<Self>) {
        let Some(targets) = self.display_controls.targets.as_ref() else {
            return;
        };
        let ids: Vec<_> = targets.outputs.iter().map(|output| output.id).collect();
        let picked = self
            .display_controls
            .selected()
            .and_then(|selected| ids.iter().position(|&id| id == selected.id))
            .unwrap_or(ids.len());
        self.show(
            Dialog::Pick {
                title: "Display brightness".into(),
                options: targets
                    .outputs
                    .iter()
                    .map(|output| output.label.clone().into())
                    .collect(),
                picked,
                pick: Rc::new(move |this, option, _, cx| {
                    this.display_controls.selected = ids.get(option).copied();
                    this.display_controls.levels = None;
                    this.display_controls.task = None;
                    this.follow_display_controls(cx);
                    cx.notify();
                }),
            },
            cx,
        );
    }

    fn display_level_slider(
        &self,
        output: &DisplayTarget,
        control: Control,
        level: f32,
        live: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let (key, title, minimum) = match control {
            // A monitor at 0 is at its dimmest and still lit; a built-in
            // panel's backlight at 0 may be off, so that one stops at 1%.
            Control::Brightness => (
                "hardware-brightness",
                "Hardware brightness",
                if crate::denial::internal(&output.name) {
                    0.01
                } else {
                    0.0
                },
            ),
            Control::Dimming => ("software-dimming", "Software dimming", 0.25),
        };
        let output = output.clone();
        let level = level.clamp(minimum, 1.0);
        self.slider_row(
            (key, output.id as u64),
            None,
            title,
            None,
            format!("{:.0}%", level * 100.0).into(),
            (level - minimum) / (1.0 - minimum),
            live && !self.display_controls.writing,
            (false, false),
            cx,
            Rc::new(move |this, value, done, cx| {
                let level = minimum + value.clamp(0.0, 1.0) * (1.0 - minimum);
                this.adjust_display_level(&output, control, level, done, cx);
            }),
        )
    }

    fn adjust_display_level(
        &mut self,
        output: &DisplayTarget,
        control: Control,
        level: f32,
        done: bool,
        cx: &mut Context<Self>,
    ) {
        let controls = &mut self.display_controls;
        let Some((id, brightness, dimming)) = &mut controls.levels else {
            return;
        };
        if *id != output.id || controls.writing {
            return;
        }
        match control {
            Control::Brightness => *brightness = Some(level),
            Control::Dimming => *dimming = Some(level),
        }
        cx.notify();
        if !done {
            return;
        }
        controls.revision += 1;
        controls.writing = true;
        let link = self.denial.link.clone();
        let output = output.clone();
        cx.spawn(async move |this, cx| {
            let percent = (level * 100.0).round() as u8;
            let result = match control {
                Control::Brightness => link.set_hardware_brightness(&output, percent).await,
                Control::Dimming => link.set_software_dimming(&output, percent).await,
            };
            let _ = this.update(cx, |view, cx| {
                view.display_controls.writing = false;
                if let Err(Refusal(why)) = result {
                    view.display_controls.levels = None;
                    view.note(
                        fill(
                            "Could not change display brightness: {why}",
                            &[("why", &why)],
                        ),
                        cx,
                    );
                }
                if view.screen.is_page("display") {
                    view.display_controls.task = None;
                    view.follow_display_controls(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn system_bar_displays(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let Some(targets) = self.display_controls.targets.as_ref() else {
            return Vec::new();
        };
        let Some(document) = self.denial.document.as_ref() else {
            return Vec::new();
        };
        let deck = self.display_controls.deck.as_deref();
        let selected = system_bar_selection(targets, deck, document);
        if selected.is_empty() {
            return Vec::new();
        }
        let eligible: Vec<_> = targets
            .outputs
            .iter()
            .filter(|output| Some(output.name.as_str()) != deck)
            .collect();
        let group = self.rows_island(eligible.into_iter(), 0.0, |output, ends| {
            let checked = selected.contains(&output.name);
            let row = self
                .row(
                    ("bar-output", output.id as u64),
                    Some(Icon::Display),
                    output.label.clone(),
                    None,
                    ends,
                )
                .child(self.toggle(checked));
            let name = output.name.clone();
            self.live(
                row,
                layer.live() && (!checked || selected.len() > 1),
                cx,
                move |this, _, cx| {
                    this.set_bar_display(&name, !checked, cx);
                },
            )
        });
        vec![self.chip_row("System bar displays"), group]
    }

    fn set_bar_display(&mut self, name: &str, enabled: bool, cx: &mut Context<Self>) {
        let Some(targets) = self.display_controls.targets.as_ref() else {
            return;
        };
        let deck = self.display_controls.deck.as_deref();
        if !targets
            .outputs
            .iter()
            .any(|output| output.name == name && Some(name) != deck)
        {
            return;
        }
        let targets = DisplayTargets {
            outputs: targets.outputs.clone(),
            primary: targets.primary.clone(),
        };
        let deck = self.display_controls.deck.clone();
        let name = name.to_owned();
        self.change(
            "System bar displays",
            move |document| {
                let mut selected = system_bar_selection(&targets, deck.as_deref(), document);
                match (enabled, selected.iter().position(|output| output == &name)) {
                    (true, None) => selected.push(name.clone()),
                    (false, Some(index)) if selected.len() > 1 => {
                        selected.remove(index);
                    }
                    _ => return,
                }
                denial::put(document, "layout.systemBarOutputs", json!(selected));
            },
            cx,
        );
    }
}
