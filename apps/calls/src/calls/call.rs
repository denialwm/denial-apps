//! The call screen: whom it is with on an island at the top, the six
//! controls in one glass group, and End as a round island in the destructive
//! fill; Decline and Answer while a call rings here. The backing stays the
//! neutral tint: a call never takes the caller's colour. Elsewhere a call
//! going on shows as a bar on the gesture zone, over the tab bar on the tabs.

use denial_look::words::{fill, tr};
use std::time::Instant;

use denial_look::INSET;
use denial_look::mobile::{GROUP_HEIGHT, TOUCH, content_top};
use denial_look::palette;
use denial_look_gpui::{Icon, control_fill, control_radius, dp, glass_group, icon, solid};
use gpui::{AnyElement, Context, Div, FontWeight, Stateful, Window, div, prelude::*, px};

use super::keypad::CALL;
use super::parts::{Face, Fill, ICON, place};
use super::{Calls, Layer, Live, Phase, Screen};
use crate::data;

/// Tones the call screen shows at most, the latest.
const TONES: usize = 24;
/// The round control of a glass group, in dp.
const CONTROL: f32 = 56.0;

impl Calls {
    /// Where the call is, as the screen and the bar say it.
    fn status(&self, live: &Live, now: Instant) -> String {
        let clock = data::timer(live.talked(now));
        match live.phase {
            Phase::Ringing(_) if live.incoming => tr("Incoming call").into(),
            Phase::Ringing(_) => tr("Calling…").into(),
            Phase::Connected(_) if live.held => fill("On hold · {clock}", &[("clock", &clock)]),
            Phase::Connected(_) if live.muted => fill("{clock} · Muted", &[("clock", &clock)]),
            Phase::Connected(_) => clock,
            Phase::Ended(_, 0) => tr("Call ended").into(),
            Phase::Ended(..) => fill("Call ended · {clock}", &[("clock", &clock)]),
        }
    }

    /// Whom the call is with, on the island at the top, and the tones sent.
    pub(super) fn call_screen(&self, layer: Layer, now: Instant) -> AnyElement {
        let Some(live) = &self.live else {
            return div().into_any_element();
        };
        let contact = self.people.find(&live.number);
        let detail = if live.tones.is_empty() {
            match self.people.kind(&live.number) {
                Some(kind) => format!("{} · {}", tr(kind.label()), live.number),
                None => live.number.clone(),
            }
        } else {
            live.tones.clone()
        };
        let island = self.name_island(
            contact,
            &live.number,
            self.people.name(&live.number).to_owned(),
            vec![
                div()
                    .text_size(dp(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.status(live, now)),
                self.secondary(detail),
            ],
        );
        div()
            .px(px(INSET))
            .pt(px(content_top()))
            .child(place(island, self.item_motion(layer, 0, false, now)))
            .into_any_element()
    }

    /// The call screen's bottom zone: Decline and Answer while it rings here,
    /// else the controls, or the in-call keypad, over End.
    pub(super) fn call_controls(&self, layer: Layer, cx: &mut Context<Self>) -> Option<AnyElement> {
        let live = self.live.as_ref()?;
        let active = layer.live() && !live.ended();
        if live.ringing_here() {
            return Some(
                div()
                    .flex()
                    .justify_around()
                    .pb(dp(16.))
                    .child(self.round(
                        "decline",
                        Icon::HangUp,
                        Fill::Destructive,
                        CALL,
                        active,
                        cx,
                        |this, window, cx| this.hang_up(window, cx),
                    ))
                    .child(self.round(
                        "answer",
                        Icon::Phone,
                        Fill::Call,
                        CALL,
                        active,
                        cx,
                        |this, window, cx| this.answer(window, cx),
                    ))
                    .into_any_element(),
            );
        }
        let upper = if live.keypad {
            self.tone_keys(active, cx)
        } else {
            self.controls(live, active, cx)
        };
        let hide = live.keypad.then(|| {
            glass_group(&self.look).rounded_full().child(
                self.button(
                    "hide-keypad",
                    Face::Icon(Icon::Keypad),
                    Fill::Clear,
                    active,
                    cx,
                    |this, _, cx| {
                        if let Some(live) = this.live.as_mut() {
                            live.keypad = false;
                        }
                        cx.notify();
                    },
                )
                .rounded_full(),
            )
        });
        let end = self.round(
            "end",
            Icon::HangUp,
            Fill::Destructive,
            CALL,
            active,
            cx,
            |this, window, cx| this.hang_up(window, cx),
        );
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(INSET))
                .when(live.ended(), |zone| zone.opacity(0.5))
                .child(upper)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .pt(px(INSET))
                        .child(div().flex_1())
                        .child(end)
                        .child(div().flex_1().flex().justify_center().children(hide)),
                )
                .into_any_element(),
        )
    }

    /// Mute, Keypad, Speaker, Hold, Message and Details, in one glass group.
    fn controls(&self, live: &Live, active: bool, cx: &mut Context<Self>) -> Div {
        let connected = matches!(live.phase, Phase::Connected(_));
        let (number, who) = (live.number.clone(), self.who(&live.number));
        let row = |controls: [Stateful<Div>; 3]| div().flex().children(controls);
        glass_group(&self.look)
            .flex()
            .flex_col()
            .gap(dp(4.))
            .p(dp(8.))
            .child(row([
                self.control(
                    "mute",
                    Icon::Mute,
                    tr("Mute"),
                    live.muted,
                    active,
                    cx,
                    |live| live.muted = !live.muted,
                ),
                self.control(
                    "keypad",
                    Icon::Keypad,
                    tr("Keypad"),
                    false,
                    active,
                    cx,
                    |live| live.keypad = true,
                ),
                self.control(
                    "speaker",
                    Icon::Speaker,
                    tr("Speaker"),
                    live.speaker,
                    active,
                    cx,
                    |live| live.speaker = !live.speaker,
                ),
            ]))
            .child(row([
                self.control(
                    "hold",
                    Icon::Hold,
                    tr("Hold"),
                    live.held,
                    active && connected,
                    cx,
                    |live| live.held = !live.held,
                ),
                self.action_control(
                    "message",
                    Icon::Message,
                    tr("Message"),
                    active,
                    cx,
                    move |this, _, cx| this.message(&number, cx),
                ),
                self.action_control(
                    "details",
                    Icon::Person,
                    tr("Details"),
                    active,
                    cx,
                    move |this, window, cx| this.open(Screen::Person(who.clone()), window, cx),
                ),
            ]))
    }

    /// A control that turns something of the call on or off.
    #[allow(clippy::too_many_arguments)]
    fn control(
        &self,
        id: &'static str,
        glyph: Icon,
        label: &'static str,
        on: bool,
        active: bool,
        cx: &mut Context<Self>,
        flip: fn(&mut Live),
    ) -> Stateful<Div> {
        self.control_face(id, glyph, label, on, active, cx, move |this, _, cx| {
            if let Some(live) = this.live.as_mut() {
                flip(live);
            }
            cx.notify();
        })
    }

    fn action_control(
        &self,
        id: &'static str,
        glyph: Icon,
        label: &'static str,
        active: bool,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        self.control_face(id, glyph, label, false, active, cx, action)
    }

    /// A control of the call's group: a round tile, the foreground when on,
    /// over its name.
    #[allow(clippy::too_many_arguments)]
    fn control_face(
        &self,
        id: &'static str,
        glyph: Icon,
        label: &'static str,
        on: bool,
        active: bool,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let palette = &self.look.palette;
        let (tile, ink) = if on {
            (solid(palette.foreground), palette.canvas)
        } else {
            (control_fill(&self.look), palette.foreground)
        };
        let control = div()
            .id(id)
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .gap(dp(6.))
            .py(dp(6.))
            .rounded(control_radius(&self.look))
            .when(!active, |control| control.opacity(0.5))
            .child(
                div()
                    .size(dp(CONTROL))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(tile)
                    .child(icon(glyph, dp(ICON), solid(ink))),
            )
            .child(div().text_size(dp(12.)).child(label));
        if !active {
            return control;
        }
        self.press(control, Fill::Clear)
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                action(this, window, cx)
            }))
    }

    /// Sends `key` as a tone, while the call is connected.
    pub(super) fn tone(&mut self, key: char, cx: &mut Context<Self>) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        if !matches!(live.phase, Phase::Connected(_)) {
            return;
        }
        live.tones.push(key);
        if live.tones.len() > TONES {
            live.tones.remove(0);
        }
        cx.notify();
    }

    /// The bar of a call going on behind another screen: whom it is with and
    /// where it is, and End. A tap goes back to the call.
    pub(super) fn call_bar(
        &self,
        layer: Layer,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let live = self.live.as_ref().filter(|live| !live.ended())?;
        let active = layer.live();
        let mut bar = glass_group(&self.look)
            .id("call-bar")
            .h(dp(GROUP_HEIGHT))
            .flex()
            .items_center()
            .gap(dp(12.))
            .pl(dp(18.))
            .pr(dp(4.))
            .child(
                div()
                    .flex_none()
                    .size(dp(10.))
                    .rounded_full()
                    .bg(solid(palette::CALL)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.people.name(&live.number).to_owned()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(dp(12.))
                            .child(self.status(live, now)),
                    ),
            )
            .child(
                self.button(
                    "bar-end",
                    Face::Icon(Icon::HangUp),
                    Fill::Destructive,
                    active,
                    cx,
                    |this, window, cx| this.hang_up(window, cx),
                )
                .size(dp(TOUCH))
                .rounded_full(),
            );
        if active {
            bar = bar
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.open(Screen::Call, window, cx)
                }));
        }
        Some(bar.into_any_element())
    }
}
