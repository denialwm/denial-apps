//! The tab bar, the tabs' menu, and Recents: the call log in day islands,
//! Today, Yesterday and Earlier, each with its label inside.

use denial_look::words::tr;
use std::time::Instant;

use denial_look::mobile::{GESTURE_INSET, GROUP_HEIGHT, TAB_BAR, content_top};
use denial_look::{INSET, floating};
use denial_look_gpui::{Icon, color, dp, glass_group, icon, island_fill, solid};
use gpui::{AnyElement, Context, Div, FontWeight, Window, div, list, prelude::*, px, relative};

use super::parts::{Face, Fill, ROW_AVATAR, SMALL_ICON, Segment, place};
use super::{Calls, Confirm, Held, Layer, Overlay, Screen};
use crate::data::{self, Direction};

/// The accent pill under the shown tab's icon, in dp.
const PILL_WIDTH: f32 = 56.0;
const PILL_HEIGHT: f32 = 28.0;
const PILL_TOP: f32 = 6.0;

/// The tabs, left to right.
const TABS: [(Screen, Icon, &str); 3] = [
    (Screen::Recents, Icon::Recent, "Recents"),
    (Screen::Contacts, Icon::Person, "Contacts"),
    (Screen::Keypad, Icon::Keypad, "Keypad"),
];

/// The day island a call lists in, in list order.
#[derive(Clone, Copy, PartialEq)]
enum Day {
    Today,
    Yesterday,
    Earlier,
}

impl Day {
    fn of(time: i64, now: i64) -> Self {
        match data::days_ago(time, now) {
            ..=0 => Day::Today,
            1 => Day::Yesterday,
            _ => Day::Earlier,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Day::Today => tr("Today"),
            Day::Yesterday => tr("Yesterday"),
            Day::Earlier => tr("Earlier"),
        }
    }
}

impl Calls {
    /// The call log, newest first, with a state in its place when it is
    /// empty.
    pub(super) fn recents_list(
        &self,
        layer: Layer,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.log.is_empty() {
            let island = self.state_island(
                Icon::Phone,
                tr("No calls yet"),
                "Calls you make and get show here. Calls are mocked for now: nothing is \
                 dialled, and no one is reached.",
                Some(self.button(
                    "open-keypad",
                    Face::Both(Icon::Keypad, tr("Open the keypad")),
                    Fill::Accent,
                    layer.live(),
                    cx,
                    |this, window, cx| this.open(Screen::Keypad, window, cx),
                )),
            );
            return div()
                .px(px(INSET))
                .pt(px(content_top()))
                .child(place(island, self.item_motion(layer, 0, false, now)))
                .into_any_element();
        }
        let first = self.recents.logical_scroll_top().item_ix;
        let bottom = self.bottom_padding(&Screen::Recents, now);
        list(
            self.recents.clone(),
            cx.processor(move |this, ix: usize, _, cx| {
                let count = this.log.len();
                let Some(call) = this.log.get(ix) else {
                    return div().into_any_element();
                };
                let today = data::now();
                let day = |ix: usize| Day::of(this.log[ix].time, today);
                let segment = Segment::of(ix, count, day);
                let label = segment.first.then(|| day(ix).label().to_owned());
                let lift = this.lift(&Held::Call(call.id), now);
                let motion = this.item_motion(layer, ix.saturating_sub(first), lift.is_some(), now);
                place(
                    div()
                        .px(px(INSET))
                        .when(ix == 0, |item| item.pt(px(content_top() - INSET)))
                        .when(ix + 1 == count, |item| item.pb(px(bottom)))
                        .child(this.recent_row(ix, segment, label, layer, lift, now, cx)),
                    motion,
                )
                .into_any_element()
            }),
        )
        .size_full()
        .into_any_element()
    }

    /// Call `ix` of the log as a row of its day island: who, what happened
    /// and when, and a button that calls back. Tapped, it opens whom it was
    /// with; held, it lifts and its actions show under it.
    #[allow(clippy::too_many_arguments)]
    fn recent_row(
        &self,
        ix: usize,
        segment: Segment,
        label: Option<String>,
        layer: Layer,
        lift: Option<f32>,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> Div {
        let call = &self.log[ix];
        let palette = &self.look.palette;
        let missed = call.direction == Direction::Missed;
        let ink = if missed {
            palette.error
        } else {
            palette.secondary
        };
        let glyph = match call.direction {
            Direction::Outgoing => Icon::Outgoing,
            Direction::Incoming => Icon::Incoming,
            Direction::Missed | Direction::Declined => Icon::Missed,
        };
        let mut detail = vec![call.direction.label().to_owned()];
        detail.extend(
            self.people
                .kind(&call.number)
                .map(|kind| tr(kind.label()).to_owned()),
        );
        if call.talked > 0 {
            detail.push(data::length(call.talked));
        }
        let contact = self.people.find(&call.number);
        let (id, number) = (call.id, call.number.clone());
        let live = layer.live();
        let dial = number.clone();
        let mut row = self
            .segment_row(("call", id as usize), segment)
            .child(self.avatar(contact, &number, ROW_AVATAR))
            .child(
                self.row_text(
                    self.people.name(&number).to_owned(),
                    missed.then_some(palette.error),
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(dp(5.))
                        .child(icon(glyph, dp(SMALL_ICON), solid(ink)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(detail.join(" · ")),
                        ),
                ),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(dp(12.))
                    .text_color(solid(palette.secondary))
                    .child(data::list_time(call.time, data::now())),
            )
            .child(self.button(
                ("dial", id as usize),
                Face::Icon(Icon::Phone),
                Fill::Clear,
                live,
                cx,
                move |this, window, cx| this.dial(dial.clone(), window, cx),
            ));
        if live {
            let who = self.who(&number);
            row = row.on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                if this.overlay.is_some() {
                    return this.dismiss(cx);
                }
                this.open(Screen::Person(who.clone()), window, cx);
            }));
            row = self.holdable(row, Held::Call(id), cx);
        }
        div()
            .child(self.segment(segment, label, None, lift, row))
            .children(lift.map(|_| self.call_actions(id, &number, now, cx)))
    }

    /// The actions under a held call: Call, Message, Copy the number, and
    /// Delete it from the log.
    fn call_actions(&self, id: u64, number: &str, now: Instant, cx: &mut Context<Self>) -> Div {
        let (dial, write, copy) = (number.to_owned(), number.to_owned(), number.to_owned());
        let buttons = vec![
            self.button(
                "call",
                Face::Both(Icon::Phone, tr("Call")),
                Fill::Clear,
                true,
                cx,
                move |this, window, cx| this.dial(dial.clone(), window, cx),
            ),
            self.button(
                "message",
                Face::Both(Icon::Message, tr("Message")),
                Fill::Clear,
                true,
                cx,
                move |this, _, cx| this.message(&write, cx),
            ),
            self.button(
                "copy",
                Face::Both(Icon::Copy, tr("Copy")),
                Fill::Clear,
                true,
                cx,
                move |this, _, cx| this.copy(copy.clone(), tr("Number copied"), cx),
            ),
            self.button(
                "delete",
                Face::Both(Icon::Delete, tr("Delete")),
                Fill::Danger,
                true,
                cx,
                move |this, _, cx| this.show(Overlay::Confirm(Confirm::DeleteCall(id)), cx),
            ),
        ];
        self.action_group(now, buttons)
    }

    /// The tab bar, anchored at the bottom: opaque, the window's full width,
    /// down to its edge with the gesture zone under the tabs. Each tab is its
    /// icon over its name; the shown one's icon sits on an accent pill that
    /// slides along as the tabs do.
    pub(super) fn tab_bar(
        &self,
        shown: &Screen,
        layer: Layer,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> Div {
        let live = layer.live();
        let palette = &self.look.palette;
        let to = shown.tab_index().unwrap_or(0) as f32;
        let at = match (&self.arrival.leaving, self.arrival.gone(now)) {
            (Some(from), Some(slid)) if self.arrival.slide.is_some() => {
                let from = from.tab_index().unwrap_or(0) as f32;
                denial_look::lerp(from, to, slid)
            }
            _ => to,
        };
        let count = TABS.len() as f32;
        let mut tabs = div().relative().h(dp(TAB_BAR)).flex().child(
            div()
                .absolute()
                .top(dp(PILL_TOP))
                .left(relative((at + 0.5) / count))
                .ml(-dp(PILL_WIDTH / 2.))
                .w(dp(PILL_WIDTH))
                .h(dp(PILL_HEIGHT))
                .rounded_full()
                .bg(solid(self.look.accent)),
        );
        for (ix, (screen, glyph, label)) in TABS.into_iter().enumerate() {
            let selected = (at - ix as f32).abs() < 0.5;
            let (glyph_ink, label_ink) = if selected {
                (self.look.on_accent, palette.foreground)
            } else {
                (palette.secondary, palette.secondary)
            };
            let mut tab = div()
                .id(label)
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .items_center()
                .pt(dp(PILL_TOP))
                .gap(dp(2.))
                .text_size(dp(11.))
                .font_weight(if selected {
                    FontWeight::BOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(solid(label_ink))
                .child(
                    div()
                        .w(dp(PILL_WIDTH))
                        .h(dp(PILL_HEIGHT))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(glyph, dp(20.), solid(glyph_ink))),
                )
                .child(div().max_w_full().truncate().child(tr(label)));
            if live && *shown != screen {
                tab = tab
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open(screen.clone(), window, cx)
                    }));
            }
            tabs = tabs.child(tab);
        }
        div()
            .occlude()
            .w_full()
            .backdrop_blur(px(floating::BACKDROP_BLUR))
            .bg(island_fill(&self.look))
            .border_t(px(floating::EDGE_WIDTH))
            .border_color(color(palette.foreground, palette.edge_alpha))
            .pb(px(GESTURE_INSET))
            .child(tabs)
    }

    /// The tabs' menu button, round, at the top on the right.
    pub(super) fn menu_button(&self, live: bool, cx: &mut Context<Self>) -> Div {
        let button = self.button(
            "menu",
            Face::Icon(Icon::More),
            Fill::Clear,
            live,
            cx,
            |this, _, cx| this.show(Overlay::Menu, cx),
        );
        self.round_group(button, GROUP_HEIGHT)
    }

    /// The menu under the top group: a made-up incoming call, and the sample
    /// contacts and calls shown or hidden.
    pub(super) fn menu(&self, now: Instant, cx: &mut Context<Self>) -> Option<Div> {
        if !matches!(self.overlay, Some((Overlay::Menu, _))) {
            return None;
        }
        let shown = !self.samples_hidden.on();
        let item = |id: &'static str,
                    face: Face,
                    cx: &mut Context<Self>,
                    action: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            self.button(id, face, Fill::Clear, true, cx, action)
                .justify_start()
        };
        Some(
            div()
                .absolute()
                .top(px(INSET * 2. - self.top_away.away()) + dp(GROUP_HEIGHT))
                .right(px(INSET))
                .opacity(self.dim(now))
                .child(
                    glass_group(&self.look)
                        .flex()
                        .flex_col()
                        .items_stretch()
                        .p(dp(4.))
                        .child(item(
                            "incoming",
                            Face::Both(Icon::Incoming, tr("Simulate an incoming call")),
                            cx,
                            |this, window, cx| this.simulate_incoming(window, cx),
                        ))
                        .child(item(
                            "samples",
                            Face::Both(
                                Icon::Person,
                                if shown {
                                    tr("Hide sample contacts")
                                } else {
                                    tr("Show sample contacts")
                                },
                            ),
                            cx,
                            |this, _, cx| {
                                this.overlay = None;
                                let shown = this.samples_hidden.on();
                                this.show_samples(shown, cx);
                            },
                        )),
                ),
        )
    }
}
