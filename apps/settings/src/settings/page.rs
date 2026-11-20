//! A page: its sections as islands of setting rows, a chip over each titled
//! one, and the controls the rows hold: toggles, sliders and segmented
//! choices. A setting the phone cannot change yet shows faint and still.

use std::rc::Rc;

use denial_look::mobile::TOUCH;
use denial_look::nested_radius;
use denial_look_gpui::{Icon, control_fill, control_radius, icon, island, say, solid};
use gpui::{
    Context, DispatchPhase, Div, ElementId, FontWeight, HitboxBehavior, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, SharedString, Stateful, TouchDragEvent, TouchPhase, Transformation,
    canvas, div, prelude::*, px, relative,
};

use super::parts::{CARD_INSET, ROW_GAP, ROW_PAD, TILE};
use super::{Layer, OnPick, Settings};
use crate::catalog::{Item, Kind, Section};

/// A setting that cannot be changed is drawn at this opacity.
pub(super) const FAINT: f32 = 0.5;

/// Around and between the options of a segmented choice.
const SEGMENT_GAP: f32 = 3.0;

/// What a slider does with each new level, and whether the finger has let
/// go.
pub(super) type OnSlide = Rc<dyn Fn(&mut Settings, f32, bool, &mut Context<Settings>)>;

/// A slider's bar, as the shell's sliders are: filled in the accent up to
/// its handle, a thin line that stays `HANDLE_PAD` inside the bar's ends.
const BAR: f32 = 26.0;
const HANDLE: f32 = 3.0;
const HANDLE_PAD: f32 = 6.0;
/// Between the fill's end and the handle.
const HANDLE_GAP: f32 = 2.0;

impl Settings {
    /// The page's islands, top to bottom: each section's chip, then its
    /// island. Sections under a disclosure are one card: its row, and once
    /// opened, each section as a panel inside the card under it.
    pub(super) fn page(
        &self,
        sections: &'static [Section],
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let mut parts = Vec::new();
        for group in sections.chunk_by(|a, b| a.disclosure == b.disclosure) {
            let visible = group.iter().filter(|section| {
                section.pending.is_some() || section.items.iter().any(|item| self.applies(item))
            });
            let disclosure = group[0].disclosure;
            if let Some(title) = disclosure {
                if visible.clone().next().is_none() {
                    continue;
                }
                let expanded = self.expanded_sections.contains(title);
                let chevron = self.chevron().when(expanded, |icon| {
                    icon.with_transformation(Transformation::rotate(gpui::Radians(
                        std::f32::consts::FRAC_PI_2,
                    )))
                });
                let header = self
                    .row(title, None, title, None, (true, !expanded))
                    .role(gpui::Role::Button)
                    .aria_expanded(expanded)
                    .child(chevron);
                let header = self.live(header, layer.live(), cx, move |this, _, cx| {
                    if !this.expanded_sections.remove(title) {
                        this.expanded_sections.insert(title);
                    }
                    this.follow_cursors(cx);
                    cx.notify();
                });
                let card = island(&self.look).child(header);
                if !expanded {
                    parts.push(card);
                    continue;
                }
                let mut opened = div()
                    .flex()
                    .flex_col()
                    .gap(px(CARD_INSET))
                    .px(px(CARD_INSET))
                    .pb(px(CARD_INSET));
                self.nested.set(true);
                for section in visible {
                    if let Some(named) = section.title
                        && named != title
                    {
                        opened = opened.child(self.caption(named).px(px(CARD_INSET)).pt(px(4.)));
                    }
                    if section.title == Some("Lock screen") && title == "Lock screen" {
                        opened = opened.child(self.lock_screen_preview());
                    }
                    opened = opened.child(self.section_in(self.panel(), section, layer, cx));
                }
                self.nested.set(false);
                parts.push(card.child(self.appearing((title, 0usize), opened)));
                continue;
            }
            for section in visible {
                if let Some(title) = section.title {
                    parts.push(self.chip_row(title));
                }
                parts.push(self.section_in(island(&self.look), section, layer, cx));
            }
        }
        parts
    }

    /// `section`'s rows in `holder`: an island, or a panel inside a card.
    fn section_in(
        &self,
        holder: Div,
        section: &'static Section,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Div {
        if let Some(said) = section.pending {
            return holder.child(
                self.frame(
                    section.items.first().map_or("pending", |item| item.key),
                    (true, true),
                )
                .flex()
                .items_center()
                .gap(px(ROW_GAP))
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
            .fold(holder, |holder, (ix, item)| {
                holder
                    .when(ix > 0, |holder| holder.child(self.separator(0.)))
                    .child(self.setting(item, ix == 0, ix + 1 == count, layer, cx))
            })
    }

    /// A setting row's frame, rounded as its island is when first or last,
    /// so its press fills the island's corner.
    fn frame(&self, key: impl Into<ElementId>, (first, last): (bool, bool)) -> Stateful<Div> {
        let radius = self.row_radius();
        div()
            .id(key)
            .px(px(ROW_PAD))
            .py(px(12.))
            .when(first, |row| row.rounded_t(radius))
            .when(last, |row| row.rounded_b(radius))
    }

    /// A setting row's line: its icon tile, title and detail.
    fn line(&self, glyph: Option<Icon>, title: SharedString, detail: Option<SharedString>) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(ROW_GAP))
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
        let shown = format!("{}%", (level * 100.0).round());
        self.slider_row(
            key,
            glyph,
            title,
            detail,
            shown.into(),
            level,
            live,
            ends,
            cx,
            slide,
        )
    }

    /// A row with a slider from 0 to 1 under its title, `shown` beside the
    /// title saying what the level means.
    #[expect(clippy::too_many_arguments)]
    pub(super) fn slider_row(
        &self,
        key: impl Into<ElementId>,
        glyph: Option<Icon>,
        title: impl Into<SharedString>,
        detail: Option<SharedString>,
        shown: SharedString,
        level: f32,
        live: bool,
        ends: (bool, bool),
        cx: &mut Context<Self>,
        slide: OnSlide,
    ) -> Stateful<Div> {
        let key = key.into();
        self.frame(key.clone(), ends)
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                self.line(glyph, title.into(), detail).child(
                    div()
                        .flex_none()
                        .text_color(solid(self.look.palette.secondary))
                        .child(say(shown)),
                ),
            )
            .child(self.slider(key, level, live && self.dialog.is_none(), cx, slide))
    }

    /// A row with a few short options side by side under its title. `pick`
    /// takes the option tapped. Without a title or detail the options are
    /// all of it: for an island whose chip already names them.
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
            .gap(px(10.))
            .children(
                (!title.is_empty() || detail.is_some())
                    .then(|| self.line(None, title.into(), detail)),
            )
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
        if item.key == "style.cursorTheme" {
            return self.cursor_cards(item.key, (first, last), live, cx);
        }
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
            Kind::Slider(_) => {
                let level = self.level(item);
                self.slider_row(
                    item.key,
                    item.icon,
                    item.title,
                    detail,
                    self.level_label(item, level),
                    level,
                    live,
                    (first, last),
                    cx,
                    Rc::new(move |this, level, done, cx| this.slide(item, level, done, cx)),
                )
                .when(!changes, |row| row.opacity(FAINT))
            }
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
                    px(20.),
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

    /// A slider across the row: a bar in the control fill, the accent up
    /// to the level, and the handle at its end. A touch on it claims the
    /// drag, so the page does not scroll under the finger.
    fn slider(
        &self,
        key: ElementId,
        level: f32,
        live: bool,
        cx: &mut Context<Self>,
        slide: OnSlide,
    ) -> Div {
        let level = level.clamp(0.0, 1.0);
        let radius = control_radius(&self.look);
        let top = px((TOUCH - BAR) / 2.);
        let bar = div()
            .absolute()
            .left_0()
            .right_0()
            .top(top)
            .h(px(BAR))
            .rounded(radius)
            .bg(control_fill(&self.look));
        // Where the handle's centre goes: the bar less `HANDLE_PAD` at each
        // end. The fill reaches from the bar's start to just before it.
        let travel = div()
            .absolute()
            .top(top)
            .h(px(BAR))
            .left(px(HANDLE_PAD))
            .right(px(HANDLE_PAD))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .w(relative(level))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(-px(HANDLE_PAD))
                            .right(px(HANDLE / 2. + HANDLE_GAP))
                            .rounded_l(radius)
                            .bg(solid(self.look.accent)),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(relative(level))
                    .ml(-px(HANDLE / 2.))
                    .top(px(4.))
                    .bottom(px(4.))
                    .w(px(HANDLE))
                    .rounded_full()
                    .bg(solid(self.look.palette.foreground)),
            );
        let slider = div().relative().h(px(TOUCH)).child(bar).child(travel);
        if !live {
            return slider;
        }
        let view = cx.weak_entity();
        slider.child(
            canvas(
                |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                move |bounds, hitbox, window, _| {
                    let pad = px(HANDLE_PAD);
                    let level_at =
                        move |x: Pixels| (x - bounds.left() - pad) / (bounds.size.width - pad * 2.);
                    let touch_view = view.clone();
                    let touch_hitbox = hitbox.clone();
                    let (touch_slide, down_slide, move_slide) =
                        (slide.clone(), slide.clone(), slide.clone());
                    let touch_key = key.clone();
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
                                this.sliding = Some(touch_key.clone());
                            } else if this.sliding.as_ref() != Some(&touch_key) {
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
                    let down_key = key.clone();
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                        if phase.bubble()
                            && event.button == MouseButton::Left
                            && hitbox.is_hovered(window)
                        {
                            let _ = down_view.update(cx, |this, cx| {
                                this.sliding = Some(down_key.clone());
                                down_slide(this, level_at(event.position.x), false, cx);
                            });
                        }
                    });
                    let move_view = view.clone();
                    let move_key = key.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                        if phase.bubble() && event.pressed_button == Some(MouseButton::Left) {
                            let _ = move_view.update(cx, |this, cx| {
                                if this.sliding.as_ref() == Some(&move_key) {
                                    move_slide(this, level_at(event.position.x), false, cx);
                                }
                            });
                        }
                    });
                    let up_key = key.clone();
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                        if phase.bubble() && event.button == MouseButton::Left {
                            let _ = view.update(cx, |this, cx| {
                                if this.sliding.as_ref() == Some(&up_key) {
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
    pub(super) fn segmented(
        &self,
        options: &'static [&'static str],
        picked: usize,
        live: bool,
        cx: &mut Context<Self>,
        pick: OnPick,
    ) -> Div {
        let radius = px(nested_radius(control_radius(&self.look).as_f32(), 3.0));
        let count = options.len().max(1) as f32;
        let accent = solid(self.look.accent);
        // The accent under the picked option; it slides when another is
        // picked. Its box is an option and the gap after it wide.
        let marker = (picked < options.len()).then(|| {
            div()
                .absolute()
                .top(px(SEGMENT_GAP))
                .bottom(px(SEGMENT_GAP))
                .left(px(SEGMENT_GAP))
                .right_0()
                .child(self.sprung("picked", picked as f32, move |at| {
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(relative(at.clamp(0.0, count - 1.0) / count))
                        .w(relative(1.0 / count))
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left_0()
                                .right(px(SEGMENT_GAP))
                                .rounded(radius)
                                .bg(accent),
                        )
                }))
        });
        let segments = options.iter().enumerate().map(|(ix, option)| {
            let segment = div()
                .id(ix)
                .flex_1()
                .min_w_0()
                .h(px(36.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(radius)
                .truncate()
                .font_weight(FontWeight::SEMIBOLD)
                .child(say(*option));
            if ix == picked {
                segment.text_color(solid(self.look.on_accent))
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
            .relative()
            .flex()
            .p(px(SEGMENT_GAP))
            .gap(px(SEGMENT_GAP))
            .rounded(control_radius(&self.look))
            .bg(control_fill(&self.look))
            .children(marker)
            .children(segments)
    }
}
