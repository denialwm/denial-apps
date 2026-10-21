//! Pieces every screen is built from: avatars, buttons, long presses, the
//! placing of content islands, section chips, blocking states, the action
//! group under a lifted island and the delete or block dialog.

use std::time::Instant;

use denial_look::mobile::{AVATAR, TOUCH};
use denial_look::{INSET, PRESSED_FILL, lerp_rgb, motion, on, palette};
use denial_look_gpui::{
    Icon, color, control_fill, control_radius, dp, glass_group, icon, island, solid,
};
use gpui::{
    Context, Div, FontWeight, HitboxBehavior, LongPressEvent, MouseButton, ObjectFit, Stateful,
    TouchPhase, Window, canvas, div, img, prelude::*, px,
};

use super::{Confirm, Held, Messages, Motion};
use crate::data;

/// Size of an icon in a button, in dp.
pub(super) const ICON: f32 = 22.0;
/// Size of an icon in a chip, in dp.
pub(super) const CHIP_ICON: f32 = 14.0;

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
}

/// What a button shows.
#[derive(Clone, Copy)]
pub(super) enum Face {
    Icon(Icon),
    Label(&'static str),
    Both(Icon, &'static str),
}

impl Messages {
    /// The contact's photo, else initials on the peer's colour, bare on the
    /// frost.
    pub(super) fn avatar(&self, peer: &str, diameter: f32) -> Div {
        let circle = div().flex_none().size(dp(diameter));
        if let Some(photo) = self.people.photo(peer) {
            return circle.child(
                img(photo)
                    .size_full()
                    .rounded_full()
                    .object_fit(ObjectFit::Cover),
            );
        }
        let fill = palette::avatar(peer);
        circle
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(solid(fill))
            .text_size(dp(diameter * 0.36))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(solid(on(fill)))
            .child(data::initials(self.people.name(peer)))
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
    fn fill(&self, fill: Fill) -> Option<u32> {
        match fill {
            Fill::Clear | Fill::Danger | Fill::Control => None,
            Fill::Accent => Some(self.look.accent),
            Fill::Destructive => Some(palette::DESTRUCTIVE),
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

    /// Lifts `held` on a long press, or on a right click with a mouse.
    pub(super) fn holdable(
        &self,
        element: Stateful<Div>,
        held: Held,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let view = cx.weak_entity();
        let clicked = held.clone();
        element
            .relative()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| this.hold(clicked.clone(), cx)),
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
                                // Claims the touch: it ends in no tap and
                                // scrolls nothing.
                                window.prevent_default();
                                cx.stop_propagation();
                                let held = held.clone();
                                let _ = view.update(cx, |this, cx| this.hold(held, cx));
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

    /// A section chip over a list of avatars and islands, in line with the
    /// islands.
    pub(super) fn section(&self, label: impl Into<gpui::SharedString>) -> Div {
        div()
            .pt(dp(10.))
            .pl(dp(AVATAR) + px(INSET))
            .flex()
            .child(denial_look_gpui::chip(&self.look).child(label.into()))
    }

    /// A chip that says what blocks something, with its icon; an error reads
    /// in the error colour.
    pub(super) fn reason_chip(
        &self,
        glyph: Icon,
        text: impl Into<gpui::SharedString>,
        error: bool,
    ) -> Div {
        let ink = if error {
            self.look.palette.error
        } else {
            self.look.palette.foreground
        };
        denial_look_gpui::chip(&self.look)
            .text_color(solid(ink))
            .child(icon(glyph, dp(CHIP_ICON), solid(ink)))
            .child(text.into())
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
            Confirm::DeleteMessage(id) => {
                let peer = self.message_peer(*id).unwrap_or_default();
                (
                    "Delete this message?".to_string(),
                    format!(
                        "It is removed from this phone. {} keeps their copy.",
                        self.people.name(&peer)
                    ),
                    "Delete",
                    Icon::Delete,
                )
            }
            Confirm::DeleteConversation(_) => (
                "Delete this conversation?".to_string(),
                "Its messages are removed from this phone.".to_string(),
                "Delete",
                Icon::Delete,
            ),
            Confirm::Block(peer) => (
                format!("Block {}?", self.people.name(peer)),
                "Their messages go to Blocked messages in settings, without a notification."
                    .to_string(),
                "Block",
                Icon::Block,
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
                        Face::Label("Cancel"),
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

    /// The peer a message is with, or from, in the blocked folder.
    pub(super) fn message_peer(&self, id: u64) -> Option<String> {
        self.conversations
            .iter()
            .find(|c| c.messages.iter().any(|m| m.id == id))
            .map(|c| c.peer.clone())
            .or_else(|| {
                self.blocked_messages
                    .iter()
                    .find(|row| row.0 == id)
                    .map(|row| row.1.clone())
            })
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
