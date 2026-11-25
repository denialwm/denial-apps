//! Physical keyboard, mouse and touchpad preferences managed by deniald.

use std::rc::Rc;
use std::time::Duration;

use denial_look::words::fill;
use denial_look::{INSET, Look};
use denial_look_gpui::input::TextInput;
use denial_look_gpui::{Icon, icon, island, solid};
use gpui::{Context, Div, Entity, SharedString, Task, div, prelude::*, px};

use super::parts::{ICON, ROW_PAD};
use super::{Layer, Settings};
use crate::catalog::SliderUnit;
use crate::denial::{KeyboardLayout, KeyboardSettings, KeyboardSnapshot, PointerSettings, Refusal};

pub(super) struct Input {
    pub(super) layouts: Entity<TextInput>,
    pub(super) options: Entity<TextInput>,
    keyboard: Option<KeyboardSnapshot>,
    repeat_delay: u32,
    repeat_rate: u32,
    active: Option<String>,
    pointer: Option<PointerSettings>,
    reading: Option<Task<()>>,
    busy: bool,
}

impl Input {
    pub(super) fn new(look: &Look, cx: &mut Context<Settings>) -> Self {
        Self {
            layouts: cx.new(|cx| TextInput::new("us, us:dvorak", look, 1, cx)),
            options: cx.new(|cx| TextInput::new("grp:alt_shift_toggle", look, 1, cx)),
            keyboard: None,
            repeat_delay: 600,
            repeat_rate: 25,
            active: None,
            pointer: None,
            reading: None,
            busy: false,
        }
    }

    fn take_keyboard(&mut self, snapshot: KeyboardSnapshot, cx: &mut Context<Settings>) {
        let settings = &snapshot.settings;
        self.layouts.update(cx, |input, cx| {
            let layouts = settings
                .layouts
                .iter()
                .map(layout_token)
                .collect::<Vec<_>>();
            input.set_text(layouts.join(", "), cx);
        });
        self.options.update(cx, |input, cx| {
            input.set_text(settings.options.join(", "), cx)
        });
        self.repeat_delay = settings.repeat_delay_ms;
        self.repeat_rate = settings.repeat_rate_hz;
        self.active = active_layout(&snapshot);
        self.keyboard = Some(snapshot);
    }
}

/// Whether `a` and `b` set the keyboard up alike; a layout's shown name
/// is not part of that.
fn same_keyboard(a: &KeyboardSettings, b: &KeyboardSettings) -> bool {
    let layouts = |settings: &KeyboardSettings| {
        settings
            .layouts
            .iter()
            .map(layout_token)
            .collect::<Vec<_>>()
    };
    layouts(a) == layouts(b)
        && a.options == b.options
        && a.repeat_delay_ms == b.repeat_delay_ms
        && a.repeat_rate_hz == b.repeat_rate_hz
}

fn layout_token(layout: &KeyboardLayout) -> String {
    if layout.variant.is_empty() {
        layout.layout.clone()
    } else {
        format!("{}:{}", layout.layout, layout.variant)
    }
}

fn active_layout(snapshot: &KeyboardSnapshot) -> Option<String> {
    snapshot
        .settings
        .layouts
        .get(snapshot.active_layout)
        .map(|layout| {
            if layout.display_name.is_empty() {
                layout_token(layout)
            } else {
                layout.display_name.clone()
            }
        })
}

/// The touchpad's click methods as deniald names them, in the order of
/// `CLICK_METHOD_NAMES`.
const CLICK_METHODS: [&str; 2] = ["clickfinger", "buttonAreas"];
const CLICK_METHOD_NAMES: [&str; 2] = ["Two-finger press", "Corner press"];

fn tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

impl Settings {
    pub(super) fn follow_input(&mut self, cx: &mut Context<Self>) {
        if !self.screen.is_page("input") {
            self.input.reading = None;
            return;
        }
        if self.input.reading.is_some() {
            return;
        }
        self.input.keyboard = None;
        self.input.active = None;
        self.input.pointer = None;
        let link = self.denial.link.clone();
        self.input.reading = Some(cx.spawn(async move |this, cx| {
            loop {
                let (keyboard, pointer) =
                    futures_lite::future::zip(link.keyboard(), link.pointer()).await;
                let taken = this.update(cx, |view, cx| {
                    if view.input.busy || view.sliding.is_some() {
                        return;
                    }
                    if let Ok(keyboard) = keyboard {
                        match view.input.keyboard.as_mut() {
                            None => view.input.take_keyboard(keyboard, cx),
                            Some(previous) => {
                                if keyboard.settings == previous.settings {
                                    previous.revision = previous.revision.max(keyboard.revision);
                                }
                                view.input.active = active_layout(&keyboard);
                            }
                        }
                    }
                    if let Ok(pointer) = pointer
                        && view
                            .input
                            .pointer
                            .as_ref()
                            .is_none_or(|current| pointer.revision >= current.revision)
                    {
                        view.input.pointer = Some(pointer);
                    }
                    cx.notify();
                });
                if taken.is_err() {
                    return;
                }
                cx.background_executor().timer(Duration::from_secs(2)).await;
            }
        }));
    }

    pub(super) fn input_page(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let mut parts = vec![
            self.heading_row("Physical keyboard", self.keyboard_actions(layer, cx)),
            self.keyboard_island(layer, cx),
        ];
        let Some(pointer) = self.input.pointer.as_ref() else {
            return parts;
        };
        let live = layer.live() && !self.input.busy;
        if pointer.has_mouse {
            let acceleration = self
                .row(
                    "mouse-acceleration",
                    None,
                    "Mouse acceleration",
                    None,
                    (false, pointer.mouse_natural_scroll_enabled.is_none()),
                )
                .child(self.toggle(pointer.mouse_acceleration_enabled));
            let wheel = pointer.mouse_natural_scroll_enabled.map(|enabled| {
                self.row(
                    "mouse-natural",
                    None,
                    "Natural scrolling",
                    None,
                    (false, true),
                )
                .child(self.toggle(enabled))
            });
            parts.push(self.chip_row("Mouse"));
            parts.push(
                island(&self.look)
                    .child(self.pointer_slider(
                        "mouse-speed",
                        "Pointer speed",
                        pointer.mouse_speed,
                        (-1.0, 1.0),
                        true,
                        SliderUnit::Percent,
                        live,
                        cx,
                        |state| &mut state.mouse_speed,
                    ))
                    .child(self.separator(0.))
                    .child(self.live(acceleration, live, cx, |this, _, cx| {
                        this.edit_pointer(
                            true,
                            |state| {
                                state.mouse_acceleration_enabled = !state.mouse_acceleration_enabled
                            },
                            cx,
                        );
                    }))
                    .when_some(wheel, |island, wheel| {
                        island.child(self.separator(0.)).child(self.live(
                            wheel,
                            live,
                            cx,
                            |this, _, cx| {
                                this.edit_pointer(
                                    true,
                                    |state| {
                                        state.mouse_natural_scroll_enabled = state
                                            .mouse_natural_scroll_enabled
                                            .map(|enabled| !enabled)
                                    },
                                    cx,
                                );
                            },
                        ))
                    }),
            );
        }
        if pointer.has_touchpad {
            let tap = self
                .row("touchpad-tap", None, "Tap to click", None, (true, false))
                .child(self.toggle(pointer.tap_to_click_enabled));
            let click_method = pointer.touchpad_click_method.as_deref().map(|method| {
                self.options_row(
                    "touchpad-click",
                    "Right click",
                    None,
                    &CLICK_METHOD_NAMES,
                    // None picked for a method this app does not know.
                    CLICK_METHODS
                        .iter()
                        .position(|&known| known == method)
                        .unwrap_or(CLICK_METHODS.len()),
                    live,
                    (false, false),
                    cx,
                    Rc::new(|this, option, _, cx| {
                        let Some(&method) = CLICK_METHODS.get(option) else {
                            return;
                        };
                        this.edit_pointer(
                            false,
                            |state| state.touchpad_click_method = Some(method.to_owned()),
                            cx,
                        );
                    }),
                )
            });
            let acceleration = pointer.touchpad_acceleration_enabled.map(|enabled| {
                self.row(
                    "touchpad-acceleration",
                    None,
                    "Pointer acceleration",
                    None,
                    (false, false),
                )
                .child(self.toggle(enabled))
            });
            let natural = self
                .row(
                    "touchpad-natural",
                    None,
                    "Natural scrolling",
                    None,
                    (false, true),
                )
                .child(self.toggle(pointer.natural_scroll_enabled));
            parts.push(self.chip_row("Touchpad"));
            parts.push(
                island(&self.look)
                    .child(self.live(tap, live, cx, |this, _, cx| {
                        this.edit_pointer(
                            false,
                            |state| state.tap_to_click_enabled = !state.tap_to_click_enabled,
                            cx,
                        );
                    }))
                    .when_some(click_method, |island, click_method| {
                        island.child(self.separator(0.)).child(click_method)
                    })
                    .when_some(pointer.touchpad_speed, |island, speed| {
                        island.child(self.separator(0.)).child(self.pointer_slider(
                            "touchpad-speed",
                            "Pointer speed",
                            speed,
                            (-1.0, 1.0),
                            false,
                            SliderUnit::Percent,
                            live,
                            cx,
                            |state| state.touchpad_speed.get_or_insert(0.0),
                        ))
                    })
                    .when_some(acceleration, |island, acceleration| {
                        island.child(self.separator(0.)).child(self.live(
                            acceleration,
                            live,
                            cx,
                            |this, _, cx| {
                                this.edit_pointer(
                                    false,
                                    |state| {
                                        state.touchpad_acceleration_enabled = state
                                            .touchpad_acceleration_enabled
                                            .map(|enabled| !enabled)
                                    },
                                    cx,
                                );
                            },
                        ))
                    })
                    .child(self.separator(0.))
                    .child(self.pointer_slider(
                        "touchpad-scroll",
                        "Touchpad scroll speed",
                        pointer.scroll_speed_factor,
                        (0.05, 5.0),
                        false,
                        SliderUnit::Multiplier,
                        live,
                        cx,
                        |state| &mut state.scroll_speed_factor,
                    ))
                    .child(self.separator(0.))
                    .child(self.pointer_slider(
                        "touchpad-swipe",
                        "Scrolling-layout swipe speed",
                        pointer.scrolling_layout_swipe_speed_factor,
                        (0.25, 4.0),
                        false,
                        SliderUnit::Multiplier,
                        live,
                        cx,
                        |state| &mut state.scrolling_layout_swipe_speed_factor,
                    ))
                    .child(self.separator(0.))
                    .child(self.live(natural, live, cx, |this, _, cx| {
                        this.edit_pointer(
                            false,
                            |state| state.natural_scroll_enabled = !state.natural_scroll_enabled,
                            cx,
                        );
                    })),
            );
        }
        parts
    }

    /// The keyboard's settings as the fields and sliders hold them.
    fn shown_keyboard(&self, cx: &gpui::App) -> KeyboardSettings {
        KeyboardSettings {
            layouts: tokens(self.input.layouts.read(cx).text())
                .map(|token| {
                    let (layout, variant) = token.split_once(':').unwrap_or((token, ""));
                    KeyboardLayout {
                        layout: layout.into(),
                        variant: variant.into(),
                        display_name: String::new(),
                    }
                })
                .collect(),
            options: tokens(self.input.options.read(cx).text())
                .map(str::to_owned)
                .collect(),
            repeat_delay_ms: self.input.repeat_delay,
            repeat_rate_hz: self.input.repeat_rate,
        }
    }

    /// Whether the fields and sliders hold something Denial has not been
    /// given, and whether it can be given now.
    pub(super) fn keyboard_changes(&self, cx: &gpui::App) -> Option<bool> {
        let applied = &self.input.keyboard.as_ref()?.settings;
        (!same_keyboard(&self.shown_keyboard(cx), applied)).then_some(!self.input.busy)
    }

    /// What is done to the keyboard's settings, at their heading's end:
    /// reading them again, which is always there, and going back to
    /// Denial's own once they differ from it. What the fields and sliders
    /// hold is carried out from the window's corner.
    fn keyboard_actions(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let live = layer.live() && !self.input.busy;
        let own = KeyboardSettings::default();
        let differs = self.input.keyboard.as_ref().is_some_and(|applied| {
            !same_keyboard(&applied.settings, &own)
                || !same_keyboard(&self.shown_keyboard(cx), &own)
        });
        let mut actions = vec![self.action(
            "keyboard-reload",
            Icon::Flip,
            "Reload",
            false,
            layer.live(),
            cx,
            |this, window, cx| {
                window.focus(&this.focus_handle, cx);
                this.input.reading = None;
                this.follow_input(cx);
                cx.notify();
            },
        )];
        if self.input.keyboard.is_some() {
            actions.push(self.action(
                "keyboard-reset",
                Icon::Retry,
                "Reset",
                false,
                live && differs,
                cx,
                |this, window, cx| {
                    window.focus(&this.focus_handle, cx);
                    this.apply_keyboard(true, cx);
                },
            ));
        }
        actions
    }

    fn keyboard_island(&self, layer: Layer, cx: &mut Context<Self>) -> Div {
        let live = layer.live() && !self.input.busy;
        if self.input.keyboard.is_none() {
            return island(&self.look).child(self.row(
                "keyboard-unavailable",
                None,
                "Waiting for Denial input settings",
                None,
                (true, true),
            ));
        }
        let field = |label, input: &Entity<TextInput>| {
            div()
                .px(px(ROW_PAD))
                .pt(px(INSET))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(denial_look_gpui::say(label))
                .child(div().flex().child(self.field(
                    icon(Icon::Keyboard, px(ICON), solid(self.look.palette.secondary)),
                    input,
                )))
        };
        let delay = self.input.repeat_delay;
        let rate = self.input.repeat_rate;
        island(&self.look)
            .child(self.row(
                "active-keyboard",
                Some(Icon::Keyboard),
                "Active keyboard layout",
                self.input.active.clone().map(SharedString::from),
                (true, false),
            ))
            .child(field(
                "XKB layouts (layout:variant, comma-separated)",
                &self.input.layouts,
            ))
            .child(field("XKB options (comma-separated)", &self.input.options))
            .child(
                div()
                    .px(px(ROW_PAD))
                    .pt(px(INSET))
                    .text_color(solid(self.look.palette.secondary))
                    .child(denial_look_gpui::say(
                        "Super+Space switches layouts. Add Shift to go back.",
                    )),
            )
            .child(self.slider_row(
                "keyboard-delay",
                None,
                "Key repeat delay",
                None,
                format!("{delay} ms").into(),
                (delay as f32 - 100.0) / 4900.0,
                live,
                (false, false),
                cx,
                Rc::new(|this, level, _, cx| {
                    this.input.repeat_delay =
                        100 + (level.clamp(0.0, 1.0) * 98.0).round() as u32 * 50;
                    cx.notify();
                }),
            ))
            .child(self.slider_row(
                "keyboard-rate",
                None,
                "Key repeat rate",
                None,
                fill("{rate} Hz", &[("rate", &rate.to_string())]).into(),
                rate as f32 / 100.0,
                live,
                (false, false),
                cx,
                Rc::new(|this, level, _, cx| {
                    this.input.repeat_rate = (level.clamp(0.0, 1.0) * 100.0).round() as u32;
                    cx.notify();
                }),
            ))
    }

    pub(super) fn apply_keyboard(&mut self, reset: bool, cx: &mut Context<Self>) {
        let Some(snapshot) = self.input.keyboard.as_ref() else {
            return;
        };
        if self.input.busy {
            return;
        }
        let revision = snapshot.revision;
        let settings = if reset {
            KeyboardSettings::default()
        } else {
            let shown = self.shown_keyboard(cx);
            if shown.layouts.is_empty() || shown.layouts.len() > 8 {
                self.note("Enter between 1 and 8 keyboard layouts", cx);
                return;
            }
            shown
        };
        let link = self.denial.link.clone();
        self.input.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let applied = link.set_keyboard(revision, &settings).await;
            let _ = this.update(cx, |view, cx| {
                view.input.busy = false;
                match applied {
                    Ok(snapshot) => {
                        view.input.take_keyboard(snapshot, cx);
                        view.note("Keyboard settings applied", cx);
                    }
                    Err(error) => view.input_error(error, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    #[expect(clippy::too_many_arguments)]
    fn pointer_slider(
        &self,
        key: &'static str,
        label: &'static str,
        value: f64,
        (min, max): (f64, f64),
        mouse: bool,
        unit: SliderUnit,
        live: bool,
        cx: &mut Context<Self>,
        field: fn(&mut PointerSettings) -> &mut f64,
    ) -> gpui::Stateful<Div> {
        // The two units pointer settings use: libinput's speed, -1 to 1, as
        // -100% to 100%, and factors.
        let shown = match unit {
            SliderUnit::Percent => format!("{:.0}%", value * 100.0),
            _ => format!("{value:.2}×"),
        };
        self.slider_row(
            key,
            None,
            label,
            None,
            shown.into(),
            ((value - min) / (max - min)) as f32,
            live,
            (false, false),
            cx,
            Rc::new(move |this, level, done, cx| {
                let value = (min + f64::from(level.clamp(0.0, 1.0)) * (max - min)) * 100.0;
                let edit = move |state: &mut PointerSettings| *field(state) = value.round() / 100.0;
                if done {
                    this.edit_pointer(mouse, edit, cx);
                } else if let Some(state) = &mut this.input.pointer {
                    edit(state);
                    cx.notify();
                }
            }),
        )
    }

    fn edit_pointer(
        &mut self,
        mouse: bool,
        edit: impl FnOnce(&mut PointerSettings),
        cx: &mut Context<Self>,
    ) {
        let Some(mut settings) = self.input.pointer.clone() else {
            return;
        };
        if self.input.busy {
            return;
        }
        edit(&mut settings);
        self.input.busy = true;
        let link = self.denial.link.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let applied = link.set_pointer(&settings, mouse).await;
            let refreshed = if applied.is_err() {
                link.pointer().await.ok()
            } else {
                None
            };
            let _ = this.update(cx, |view, cx| {
                view.input.busy = false;
                match applied {
                    Ok(settings) => view.input.pointer = Some(settings),
                    Err(error) => {
                        view.input.pointer = refreshed;
                        view.input_error(error, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn input_error(&mut self, Refusal(error): Refusal, cx: &mut Context<Self>) {
        let message = if error == "conflict" {
            "Input settings changed. Reload before editing again.".to_owned()
        } else {
            fill("Could not apply input settings: {why}", &[("why", &error)])
        };
        self.note(message, cx);
    }
}
