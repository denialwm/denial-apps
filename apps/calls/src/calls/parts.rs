//! Pieces every screen is built from, as Messages has them: avatars,
//! buttons, long presses, rows of section islands, blocking states, the
//! action group under a lifted island, the top group's rows and the dialog.

use denial_look::words::tr;
use std::time::Instant;

use denial_contacts::dialable;
use denial_look::mobile::TOUCH;
use denial_look::{INSET, PRESSED_FILL, floating, lerp_rgb, motion, on, palette};
use denial_look_gpui::{
    Icon, color, control_fill, control_radius, dp, glass_group, icon, island, island_edge,
    island_fill, lifted, solid,
};
use gpui::{
    Context, Div, FontWeight, HighlightStyle, HitboxBehavior, LongPressEvent, MouseButton,
    ObjectFit, SharedString, Stateful, TouchPhase, Window, canvas, div, img, prelude::*, px,
};

use super::{Back, Calls, Confirm, Held, Layer, Motion};
use crate::data;

/// Size of an icon in a button, in dp.
pub(super) const ICON: f32 = 22.0;
/// Size of an icon in a chip or beside a row's detail, in dp.
pub(super) const SMALL_ICON: f32 = 14.0;
/// A row's padding from its island's edge, in dp.
pub(super) const ROW_PAD: f32 = 16.0;
/// A row's avatar, in dp.
pub(super) const ROW_AVATAR: f32 = 42.0;
/// Between a row's avatar and its text, in dp.
pub(super) const ROW_GAP: f32 = 12.0;

/// What a button looks like.
#[derive(Clone, Copy)]
pub(super) enum Fill {
    /// No fill until pressed: in a glass group or the action group.
    Clear,
    /// Clear, for an action that destroys something: it reads in the error
    /// colour.
    Danger,
    /// The control fill inside an island or group: the foreground, faint.
    Control,
    Accent,
    Destructive,
    /// The green of placing and answering a call.
    Call,
}

/// What a button shows.
#[derive(Clone, Copy)]
pub(super) enum Face {
    Icon(Icon),
    Label(&'static str),
    Both(Icon, &'static str),
}

/// Where a row sits in its section island: the first carries the island's
/// top corners and label, the last its bottom corners.
#[derive(Clone, Copy)]
pub(super) struct Segment {
    pub first: bool,
    pub last: bool,
}

impl Segment {
    /// Item `ix` of a list whose items fall into islands by `key`.
    pub fn of<K: PartialEq>(ix: usize, count: usize, key: impl Fn(usize) -> K) -> Self {
        Self {
            first: ix == 0 || key(ix - 1) != key(ix),
            last: ix + 1 == count || key(ix + 1) != key(ix),
        }
    }

    pub const ALONE: Segment = Segment {
        first: true,
        last: true,
    };
}

impl Calls {
    /// Contact `contact`'s photo, else initials on their colour, bare on the
    /// frost; for a number no one has, `number`'s.
    pub(super) fn avatar(&self, contact: Option<usize>, number: &str, diameter: f32) -> Div {
        let circle = div().flex_none().size(dp(diameter));
        if let Some(photo) = contact.and_then(|index| self.people.photo(index)) {
            return circle.child(
                img(photo)
                    .size_full()
                    .rounded_full()
                    .object_fit(ObjectFit::Cover),
            );
        }
        // Coloured by number as Messages colours its conversations, so a
        // person looks the same in both.
        let (name, key) = match contact {
            Some(index) => {
                let contact = &self.people.all()[index];
                let first = contact
                    .phones
                    .first()
                    .and_then(|phone| dialable(&phone.number));
                (
                    contact.name.as_str(),
                    first.unwrap_or_else(|| contact.name.clone()),
                )
            }
            None => (
                number,
                dialable(number).unwrap_or_else(|| number.to_owned()),
            ),
        };
        let fill = palette::avatar(&key);
        circle
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(solid(fill))
            .text_size(dp(diameter * 0.36))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(solid(on(fill)))
            .child(data::initials(name))
    }

    /// Shows the press on `element`, drawn with `fill`.
    pub(super) fn press(&self, element: Stateful<Div>, fill: Fill) -> Stateful<Div> {
        let foreground = self.look.palette.foreground;
        let pressed = match self.fill(fill) {
            Some(fill) => solid(lerp_rgb(fill, foreground, PRESSED_FILL)),
            None => color(foreground, PRESSED_FILL),
        };
        element.cursor_pointer().active(move |s| s.bg(pressed))
    }

    /// The opaque colour of `fill`; none for a clear button or a control,
    /// which takes the tint of the island or group under it.
    pub(super) fn fill(&self, fill: Fill) -> Option<u32> {
        match fill {
            Fill::Clear | Fill::Danger | Fill::Control => None,
            Fill::Accent => Some(self.look.accent),
            Fill::Destructive => Some(palette::DESTRUCTIVE),
            Fill::Call => Some(palette::CALL),
        }
    }

    /// A button inside an island or group.
    pub(super) fn button(
        &self,
        id: impl Into<gpui::ElementId>,
        face: Face,
        fill: Fill,
        live: bool,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let ink = match (self.fill(fill), fill) {
            (Some(fill), _) => on_fill(&self.look, fill),
            (None, Fill::Danger) => self.look.palette.error,
            (None, _) => self.look.palette.foreground,
        };
        let (glyph, label) = match face {
            Face::Icon(glyph) => (Some(glyph), None),
            Face::Label(label) => (None, Some(label)),
            Face::Both(glyph, label) => (Some(glyph), Some(label)),
        };
        let mut button = div()
            .id(id)
            .flex_none()
            .h(dp(TOUCH))
            .min_w(dp(TOUCH))
            .flex()
            .items_center()
            .justify_center()
            .gap(dp(8.))
            .rounded(control_radius(&self.look))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(solid(ink))
            .children(glyph.map(|glyph| icon(glyph, dp(ICON), solid(ink))))
            .children(label);
        if label.is_some() {
            button = button.px(dp(18.));
        }
        if let Some(fill) = self.fill(fill) {
            button = button.bg(solid(fill));
        } else if matches!(fill, Fill::Control) {
            button = button.bg(control_fill(&self.look));
        }
        if live {
            button = self
                .press(button, fill)
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    action(this, window, cx)
                }));
        }
        button
    }

    /// A round island of `diameter` dp in `fill`, such as Call, Answer and
    /// End, with the floating shadow.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn round(
        &self,
        id: &'static str,
        glyph: Icon,
        fill: Fill,
        diameter: f32,
        live: bool,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        self.button(id, Face::Icon(glyph), fill, live, cx, action)
            .size(dp(diameter))
            .rounded_full()
            .shadow(denial_look_gpui::shadow(&self.look))
    }

    /// Runs `action` on a long press of `element`, or on a right click with
    /// a mouse. The press claims the touch: it ends in no tap and scrolls
    /// nothing.
    pub(super) fn long_press(
        &self,
        element: Stateful<Div>,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + Clone + 'static,
    ) -> Stateful<Div> {
        let view = cx.weak_entity();
        let clicked = action.clone();
        element
            .relative()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    clicked(this, cx)
                }),
            )
            .child(
                canvas(
                    |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                    move |_, hitbox, window, _| {
                        window.on_mouse_event(move |event: &LongPressEvent, phase, window, cx| {
                            if phase.bubble()
                                && event.phase == TouchPhase::Started
                                && hitbox.is_hovered(window)
                            {
                                window.prevent_default();
                                cx.stop_propagation();
                                let action = action.clone();
                                let _ = view.update(cx, |this, cx| action(this, cx));
                            }
                        });
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
    }

    /// Lifts `held` on a long press.
    pub(super) fn holdable(
        &self,
        element: Stateful<Div>,
        held: Held,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.long_press(element, cx, move |this, cx| this.hold(held.clone(), cx))
    }

    /// The overlay's lift, 0 to 1, when it is `held`.
    pub(super) fn lift(&self, held: &Held, now: Instant) -> Option<f32> {
        match &self.overlay {
            Some((super::Overlay::Held(shown), lift)) if shown == held => Some(
                denial_look::ease_out_cubic(lift.segment(0, motion::LIFT).progress(now)),
            ),
            _ => None,
        }
    }

    /// The action group under a lifted island, fading in with the dim.
    pub(super) fn action_group(&self, now: Instant, buttons: Vec<Stateful<Div>>) -> Div {
        div().pt(px(INSET)).opacity(self.dim(now)).child(
            glass_group(&self.look)
                .flex()
                .flex_wrap()
                .gap(dp(4.))
                .p(dp(4.))
                .children(buttons),
        )
    }

    /// One row's share of a section island, smoke or the opaque `fill`: the
    /// label over the first row, a separator from the text column between
    /// rows. `lift` raises it off the frost.
    pub(super) fn segment(
        &self,
        segment: Segment,
        label: Option<String>,
        fill: Option<u32>,
        lift: Option<f32>,
        row: impl IntoElement,
    ) -> Div {
        let look = &self.look;
        let palette = &look.palette;
        let radius = px(look.group_radius());
        let (fill, edge) = match fill {
            Some(fill) => (solid(fill), None),
            None => (island_fill(look), island_edge(look)),
        };
        let mut island = div().bg(fill);
        if segment.first {
            island = island.rounded_t(radius);
        }
        if segment.last {
            island = island.rounded_b(radius);
        }
        if let Some(color) = edge {
            let edge = px(floating::EDGE_WIDTH);
            island = island
                .border_color(color)
                .border_l(edge)
                .border_r(edge)
                .when(segment.first, |island| island.border_t(edge))
                .when(segment.last, |island| island.border_b(edge));
        }
        if let Some(t) = lift {
            island = island.shadow(lifted(t));
        }
        div().when(segment.first, |item| item.pt(px(INSET))).child(
            island
                .children(label.map(|label| self.island_label(label)))
                .children((!segment.first).then(|| {
                    div()
                        .h(px(1.))
                        .ml(dp(ROW_PAD + ROW_AVATAR + ROW_GAP))
                        .bg(color(palette.separator, palette.separator_alpha))
                }))
                .child(row),
        )
    }

    /// What marks the part of a name or number that matched.
    pub(super) fn mark_style(&self) -> HighlightStyle {
        HighlightStyle {
            color: Some(solid(self.look.accent_text)),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        }
    }

    /// The name of an island's section, inside it at the top.
    pub(super) fn island_label(&self, label: impl Into<SharedString>) -> Div {
        let label: SharedString = label.into();
        div()
            .px(dp(ROW_PAD))
            .pt(dp(11.))
            .pb(dp(3.))
            .text_size(dp(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(solid(self.look.accent_text))
            .child(label.to_uppercase())
    }

    /// The press of a row in a segment, its corners matching the island's.
    pub(super) fn segment_row(
        &self,
        id: impl Into<gpui::ElementId>,
        segment: Segment,
    ) -> Stateful<Div> {
        let row = div()
            .id(id)
            .flex()
            .items_center()
            .gap(dp(ROW_GAP))
            .px(dp(ROW_PAD))
            .py(dp(10.))
            .when(segment.last, |row| {
                row.rounded_b(px(self.look.group_radius()))
            });
        self.press(row, Fill::Clear)
    }

    /// A row's two lines of text: `title`, and under it `detail` in the
    /// secondary colour, `title_ink` when given.
    pub(super) fn row_text(
        &self,
        title: impl IntoElement,
        title_ink: Option<u32>,
        detail: impl IntoElement,
    ) -> Div {
        let palette = &self.look.palette;
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(dp(2.))
            .child(
                div()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(solid(title_ink.unwrap_or(palette.foreground)))
                    .child(title),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(dp(5.))
                    .min_w_0()
                    .text_size(dp(14.))
                    .text_color(solid(palette.secondary))
                    .child(detail),
            )
    }

    /// The one island of a state that blocks the whole screen, with its
    /// action.
    pub(super) fn state_island(
        &self,
        glyph: Icon,
        title: &'static str,
        body: &'static str,
        action: Option<Stateful<Div>>,
    ) -> Div {
        island(&self.look)
            .flex()
            .flex_col()
            .items_start()
            .gap(dp(9.))
            .px(dp(21.))
            .pt(dp(24.))
            .pb(dp(20.))
            .child(
                div()
                    .size(dp(46.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(control_radius(&self.look))
                    .bg(control_fill(&self.look))
                    .child(icon(glyph, dp(ICON), solid(self.look.palette.foreground))),
            )
            .child(
                div()
                    .text_size(dp(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_height(gpui::relative(1.2))
                    .child(title),
            )
            .child(
                div()
                    .text_color(solid(self.look.palette.secondary))
                    .child(body),
            )
            .children(action.map(|action| div().pt(dp(6.)).child(action)))
    }

    /// The dialog that asks before something that cannot be undone, in the
    /// bottom zone.
    pub(super) fn dialog(
        &self,
        confirm: &Confirm,
        lift: denial_look::Tween,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (title, consequence, action, glyph) = match confirm {
            Confirm::DeleteCall(_) => (
                tr("Delete this call?"),
                tr("It is removed from Recents on this phone."),
                tr("Delete"),
                Icon::Delete,
            ),
        };
        let shown = confirm.clone();
        let appear = denial_look::decelerate(lift.segment(0, motion::DIM).progress(now));
        island(&self.look)
            .id("dialog")
            .relative()
            .top(px(motion::BOTTOM_TRAVEL * (1.0 - appear)))
            .opacity(appear)
            .flex()
            .flex_col()
            .gap(dp(7.))
            .px(dp(20.))
            .pt(dp(21.))
            .pb(dp(16.))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .text_size(dp(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .text_color(solid(self.look.palette.secondary))
                    .child(consequence),
            )
            .child(
                div()
                    .pt(dp(9.))
                    .flex()
                    .justify_end()
                    .gap(px(INSET))
                    .child(self.button(
                        "cancel",
                        Face::Label(tr("Cancel")),
                        Fill::Control,
                        true,
                        cx,
                        |this, _, cx| this.dismiss(cx),
                    ))
                    .child(self.button(
                        "confirm",
                        Face::Both(glyph, action),
                        Fill::Destructive,
                        true,
                        cx,
                        move |this, _, cx| this.confirmed(shown.clone(), cx),
                    )),
            )
    }

    /// The row inside a top group.
    pub(super) fn top_row(&self) -> Div {
        div()
            .size_full()
            .flex()
            .items_center()
            .gap(dp(8.))
            .pl(dp(4.))
            .pr(dp(4.))
    }

    pub(super) fn title(&self, title: impl Into<SharedString>) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(dp(20.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(title.into())
    }

    /// A top group of a screen under another: Back, then its title.
    pub(super) fn titled_top(
        &self,
        title: impl Into<SharedString>,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Div {
        self.top_row()
            .child(self.back_button(layer, cx))
            .child(self.title(title))
    }

    /// A round glass group holding one button, such as the tabs' menu.
    pub(super) fn round_group(&self, button: Stateful<Div>, diameter: f32) -> Div {
        glass_group(&self.look)
            .flex_none()
            .size(dp(diameter))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .child(button.size(dp(diameter - 8.)).rounded_full())
    }

    pub(super) fn back_button(&self, layer: Layer, cx: &mut Context<Self>) -> Stateful<Div> {
        self.button(
            "back",
            Face::Icon(Icon::Back),
            Fill::Clear,
            layer.live(),
            cx,
            |this, window, cx| this.back(&Back, window, cx),
        )
    }
}

/// Places a content island as `motion` says. It scrolls under the groups,
/// the gesture zone and the keyboard as it is, never faded.
pub(super) fn place(element: impl IntoElement, motion: Motion) -> Div {
    div()
        .relative()
        .top(px(motion.offset))
        .opacity(motion.opacity)
        .child(element)
}

/// Text on `fill`: the accent's own, else readable on it.
pub(super) fn on_fill(look: &denial_look::Look, fill: u32) -> u32 {
    if fill == look.accent {
        look.on_accent
    } else {
        on(fill)
    }
}
