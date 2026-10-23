//! A page: its sections as islands of setting rows, a chip over each titled
//! one, and the controls the rows hold: toggles, sliders and segmented
//! choices. A setting the phone cannot change yet shows faint and still.

use std::rc::Rc;

use denial_look::mobile::TOUCH;
use denial_look::nested_radius;
use denial_look_gpui::{Icon, control_fill, control_radius, dp, icon, island, say, solid};
use gpui::{
    Context, DispatchPhase, Div, FontWeight, HitboxBehavior, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, SharedString, Stateful, TouchDragEvent, TouchPhase,
    canvas, div, prelude::*, px, relative,
};

use super::parts::{ROW_GAP, ROW_PAD, TILE};
use super::{Layer, OnPick, Settings};
use crate::catalog::{Item, Kind, Section};

/// A setting that cannot be changed is drawn at this opacity.
pub(super) const FAINT: f32 = 0.5;

/// What a slider does with each new level, and whether the finger has let
/// go.
pub(super) type OnSlide = Rc<dyn Fn(&mut Settings, f32, bool, &mut Context<Settings>)>;

/// A slider's knob, in dp; the track runs between its centre's ends.
const KNOB: f32 = 24.0;
const TRACK: f32 = 6.0;

impl Settings {
    /// The page's islands, top to bottom: each section's chip, then its
    /// island.
    pub(super) fn page(
        &self,
        sections: &'static [Section],
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let mut parts = Vec::new();
        for section in sections {
            if let Some(title) = section.title {
                parts.push(self.chip_row(title));
            }
            parts.push(self.section(section, layer, cx));
        }
        parts
    }

    fn section(&self, section: &'static Section, layer: Layer, cx: &mut Context<Self>) -> Div {
        if let Some(said) = section.pending {
            return island(&self.look).child(
                self.frame(
                    section.items.first().map_or("pending", |item| item.key),
                    (true, true),
                )
                .flex()
                .items_center()
                .gap(dp(ROW_GAP))
                .child(self.tile(Icon::Info, TILE))
                .child(
                    div()
                        .text_color(solid(self.look.palette.secondary))
                        .child(said),
                ),
            );
        }
        let items: Vec<&'static Item> = section
            .items
            .iter()
            .filter(|item| self.applies(item))
            .collect();
        let count = items.len();
        items
            .into_iter()
            .enumerate()
            .fold(island(&self.look), |island, (ix, item)| {
                island
                    .when(ix > 0, |island| island.child(self.separator(0.)))
                    .child(self.setting(item, ix == 0, ix + 1 == count, layer, cx))
            })
    }

    /// A setting row's frame, rounded as its island is when first or last,
    /// so its press fills the island's corner.
    fn frame(&self, key: &'static str, (first, last): (bool, bool)) -> Stateful<Div> {
        let radius = px(self.look.group_radius());
        div()
            .id(key)
            .px(dp(ROW_PAD))
            .py(dp(12.))
            .when(first, |row| row.rounded_t(radius))
            .when(last, |row| row.rounded_b(radius))
    }

    /// A setting row's line: its icon tile, title and detail.
    fn line(&self, glyph: Option<Icon>, title: SharedString, detail: Option<SharedString>) -> Div {
        div()
            .flex()
            .items_center()
            .gap(dp(ROW_GAP))
            .children(glyph.map(|glyph| self.tile(glyph, TILE)))
            .child(self.text_column(title, detail))
    }

    /// A row with a level from 0 to 1: its percent beside the title, a
    /// slider under it. `slide` takes each new level, and whether the
    /// finger has let go.
    #[expect(clippy::too_many_arguments)]
    pub(super) fn level_row(
        &self,
        key: &'static str,
        glyph: Option<Icon>,
        title: &'static str,
        detail: Option<SharedString>,
        level: f32,
        live: bool,
        ends: (bool, bool),
        cx: &mut Context<Self>,
        slide: OnSlide,
    ) -> Stateful<Div> {
        self.frame(key, ends)
            .flex()
            .flex_col()
            .gap(dp(4.))
            .child(
                self.line(glyph, title.into(), detail).child(
                    div()
                        .flex_none()
                        .text_color(solid(self.look.palette.secondary))
                        .child(format!("{}%", (level * 100.0).round())),
                ),
            )
            .child(self.slider(key, level, live && self.dialog.is_none(), cx, slide))
    }

    /// A row with a few short options side by side under its title. `pick`
    /// takes the option tapped.
    #[expect(clippy::too_many_arguments)]
    pub(super) fn options_row(
        &self,
        key: &'static str,
        title: &'static str,
        detail: Option<SharedString>,
        options: &'static [&'static str],
        picked: usize,
        live: bool,
        ends: (bool, bool),
        cx: &mut Context<Self>,
        pick: OnPick,
    ) -> Stateful<Div> {
        self.frame(key, ends)
            .flex()
            .flex_col()
            .gap(dp(10.))
            .child(self.line(None, title.into(), detail))
            .child(self.segmented(options, picked, live, cx, pick))
    }

    /// One setting's row.
    pub(super) fn setting(
        &self,
        item: &'static Item,
        first: bool,
        last: bool,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let changes = item.source.changes();
        let live = layer.live() && changes;
        let palette = &self.look.palette;
        let detail = self.detail(item);
        let row = self
            .frame(item.key, (first, last))
            .when(!changes, |row| row.opacity(FAINT));
        let line =
            |title: SharedString, detail: Option<SharedString>| self.line(item.icon, title, detail);
        match item.kind {
            Kind::Toggle(_) => {
                let row =
                    row.child(line(item.title.into(), detail).child(self.toggle(self.on(item))));
                self.live(row, live, cx, move |this, _, cx| this.flip(item, cx))
            }
            Kind::Slider(_) => self
                .level_row(
                    item.key,
                    item.icon,
                    item.title,
                    detail,
                    self.level(item),
                    live,
                    (first, last),
                    cx,
                    Rc::new(move |this, level, done, cx| this.slide(item, level, done, cx)),
                )
                .when(!changes, |row| row.opacity(FAINT)),
            Kind::Segmented(options, _) => self
                .options_row(
                    item.key,
                    item.title,
                    detail,
                    options,
                    self.picked(item).unwrap_or(options.len()),
                    live,
                    (first, last),
                    cx,
                    Rc::new(move |this, option, window, cx| {
                        this.pick_option(item, option, window, cx)
                    }),
                )
                .when(!changes, |row| row.opacity(FAINT)),
            Kind::Choice(..) | Kind::Opens => {
                let row = row.child(line(item.title.into(), Some(self.shown(item))).child(icon(
                    Icon::Chevron,
                    dp(20.),
                    solid(palette.secondary),
                )));
                self.live(row, live, cx, move |this, window, cx| {
                    if matches!(item.kind, Kind::Opens) {
                        this.act(item, window, cx)
                    } else {
                        this.choose(item, cx)
                    }
                })
            }
            Kind::Info(value) => row.child(
                line(item.title.into(), detail).child(
                    div()
                        .flex_none()
                        .max_w(relative(0.5))
                        .text_color(solid(palette.secondary))
                        .child(say(value)),
                ),
            ),
            Kind::Action(_) => {
                let row = row.child(
                    line(item.title.into(), detail)
                        .text_color(solid(self.look.accent_text))
                        .font_weight(FontWeight::SEMIBOLD),
                );
                self.live(row, live, cx, move |this, window, cx| {
                    this.act(item, window, cx)
                })
            }
        }
    }

    /// A slider across the row: a faint track, the accent up to the level,
    /// and the knob. A touch on it claims the drag, so the page does not
    /// scroll under the finger.
    fn slider(
        &self,
        key: &'static str,
        level: f32,
        live: bool,
        cx: &mut Context<Self>,
        slide: OnSlide,
    ) -> Div {
        let travel = div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(dp(KNOB / 2.))
            .right(dp(KNOB / 2.))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(dp((TOUCH - TRACK) / 2.))
                    .h(dp(TRACK))
                    .rounded_full()
                    .bg(control_fill(&self.look))
                    .child(
                        div()
                            .h_full()
                            .w(relative(level))
                            .rounded_full()
                            .bg(solid(self.look.accent)),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(relative(level))
                    .ml(-dp(KNOB / 2.))
                    .top(dp((TOUCH - KNOB) / 2.))
                    .size(dp(KNOB))
                    .rounded_full()
                    .bg(solid(self.look.accent)),
            );
        let slider = div().relative().h(dp(TOUCH)).child(travel);
        if !live {
            return slider;
        }
        let view = cx.weak_entity();
        slider.child(
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                move |bounds, hitbox, window, _| {
                    let pad = dp(KNOB / 2.);
                    let level_at =
                        move |x: Pixels| (x - bounds.left() - pad) / (bounds.size.width - pad * 2.);
                    let touch_view = view.clone();
                    let touch_hitbox = hitbox.clone();
                    let (touch_slide, down_slide, move_slide) =
                        (slide.clone(), slide.clone(), slide.clone());
                    window.on_mouse_event(move |event: &TouchDragEvent, phase, window, cx| {
                        if phase != DispatchPhase::Bubble {
                            return;
                        }
                        let started = event.phase == TouchPhase::Started;
                        if started && !touch_hitbox.is_hovered(window) {
                            return;
                        }
                        let done = matches!(event.phase, TouchPhase::Ended | TouchPhase::Cancelled);
                        let taken = touch_view.update(cx, |this, cx| {
                            if started {
                                this.sliding = Some(key);
                            } else if this.sliding != Some(key) {
                                return false;
                            }
                            if done {
                                this.sliding = None;
                            }
                            touch_slide(this, level_at(event.position.x), done, cx);
                            true
                        });
                        if started && taken.unwrap_or(false) {
                            // Claims the touch: the page does not scroll.
                            window.prevent_default();
                            cx.stop_propagation();
                        }
                    });
                    let down_view = view.clone();
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                        if phase.bubble()
                            && event.button == MouseButton::Left
                            && hitbox.is_hovered(window)
                        {
                            let _ = down_view.update(cx, |this, cx| {
                                this.sliding = Some(key);
                                down_slide(this, level_at(event.position.x), false, cx);
                            });
                        }
                    });
                    let move_view = view.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                        if phase.bubble() && event.pressed_button == Some(MouseButton::Left) {
                            let _ = move_view.update(cx, |this, cx| {
                                if this.sliding == Some(key) {
                                    move_slide(this, level_at(event.position.x), false, cx);
                                }
                            });
                        }
                    });
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                        if phase.bubble() && event.button == MouseButton::Left {
                            let _ = view.update(cx, |this, cx| {
                                if this.sliding == Some(key) {
                                    this.sliding = None;
                                    slide(this, level_at(event.position.x), true, cx);
                                }
                            });
                        }
                    });
                },
            )
            .absolute()
            .inset_0(),
        )
    }

    /// The options side by side, the picked one in the accent.
    fn segmented(
        &self,
        options: &'static [&'static str],
        picked: usize,
        live: bool,
        cx: &mut Context<Self>,
        pick: OnPick,
    ) -> Div {
        let radius = px(nested_radius(control_radius(&self.look).as_f32(), 3.0));
        let segments = options.iter().enumerate().map(|(ix, option)| {
            let segment = div()
                .id(ix)
                .flex_1()
                .min_w_0()
                .h(dp(36.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(radius)
                .truncate()
                .font_weight(FontWeight::SEMIBOLD)
                .child(say(*option));
            if ix == picked {
                segment
                    .bg(solid(self.look.accent))
                    .text_color(solid(self.look.on_accent))
            } else if live {
                let pick = pick.clone();
                self.tappable(segment, cx, move |this, window, cx| {
                    pick(this, ix, window, cx)
                })
            } else {
                segment
            }
        });
        div()
            .flex()
            .p(dp(3.))
            .gap(dp(3.))
            .rounded(control_radius(&self.look))
            .bg(control_fill(&self.look))
            .children(segments)
    }
}
