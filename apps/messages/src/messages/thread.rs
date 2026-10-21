//! A conversation: its bubbles, anchored to the bottom like a chat, the
//! actions of a held message, the composer and the chips over it.

use std::time::Instant;

use denial_look::mobile::{AVATAR_SMALL, COMPOSER_HEIGHT, TOUCH, dp as dp_lp};
use denial_look::{INSET, MIN_RADIUS, ease_out_cubic, motion};
use denial_look_gpui::{Icon, chip, dp, icon, island_run, lifted, solid};
use gpui::{
    AnyElement, ClickEvent, Context, Div, Focusable, FontWeight, HighlightStyle, MouseButton,
    StyledText, UnderlineStyle, div, list, prelude::*, px, relative,
};

use super::list::PADDING;
use super::parts::{Face, Fill, ICON, place};
use super::{Confirm, Held, Layer, Link, Messages, Motion, Overlay};
use crate::data::{self, Message};
use crate::input;
use crate::segments;

/// Space beside the bubbles.
const SIDE: f32 = 16.0;
/// The composer shows how much of the last SMS is left from this many
/// characters, and always on a long message.
const COUNTER_FROM: usize = 20;

impl Messages {
    /// The thread's top group: Back, then who it is with.
    pub(super) fn thread_top(&self, index: usize, layer: Layer, cx: &mut Context<Self>) -> Div {
        let peer = &self.conversations[index].peer;
        let name = self.people.name(peer);
        let detail = if name == peer {
            "SMS".to_owned()
        } else {
            peer.clone()
        };
        self.top_row()
            .child(self.back_button(layer, cx))
            .child(self.avatar(peer, AVATAR_SMALL))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_size(dp(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(relative(1.2))
                            .child(name.to_owned()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(dp(12.))
                            .text_color(solid(self.look.palette.secondary))
                            .child(detail),
                    ),
            )
    }

    /// The open thread, newest message at the bottom.
    pub(super) fn thread(
        &self,
        index: usize,
        layer: Layer,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let conversation = &self.conversations[index];
        if conversation.messages.is_empty() && matches!(self.link, Link::Unavailable(_)) {
            return self.empty_list(false, layer, now, cx);
        }
        list(
            self.thread_list.clone(),
            cx.processor(move |this, ix: usize, _, cx| {
                let now = Instant::now();
                let count = this.conversations[index].messages.len();
                let item = this.message_item(index, ix, layer, now, cx);
                place(
                    div()
                        .px(px(SIDE))
                        .when(ix == 0, |item| item.pt(px(PADDING)))
                        .when(ix + 1 == count, |item| item.pb(px(PADDING)))
                        .child(item),
                    Motion::STILL,
                )
                .into_any_element()
            }),
        )
        .size_full()
        .into_any_element()
    }

    /// The thread's message `ix`, under its day when the day changes.
    fn message_item(
        &self,
        index: usize,
        ix: usize,
        layer: Layer,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> Div {
        let messages = &self.conversations[index].messages;
        let Some(message) = messages.get(ix) else {
            return div();
        };
        let previous = ix.checked_sub(1).and_then(|i| messages.get(i));
        let new_day = previous.is_none_or(|p| !data::same_day(p.time, message.time));
        let first = new_day || previous.is_some_and(|p| p.outgoing != message.outgoing);
        // A run of bubbles from one side within one minute shares a time.
        let last = messages.get(ix + 1).is_none_or(|next| {
            next.outgoing != message.outgoing
                || next.time / 60 != message.time / 60
                || !data::same_day(next.time, message.time)
        });
        let held = Held::Message(message.id);
        let lift = self.lift(&held, now);
        let motion = self.item_motion(layer, messages.len() - 1 - ix, lift.is_some(), now);
        let appear = message
            .appear
            .map_or(1.0, |appear| ease_out_cubic(appear.progress(now)));
        let day = new_day.then(|| {
            div()
                .pt(dp(12.))
                .flex()
                .justify_center()
                .child(chip(&self.look).child(data::day(message.time, data::now())))
        });
        let bubble = self.bubble(message, last, layer, lift, cx);
        let side = |column: Div| {
            if message.outgoing {
                column.items_end()
            } else {
                column.items_start()
            }
        };
        let details = lift.map(|_| chip(&self.look).child(message.details(data::now())));
        let label = match (lift, last) {
            (Some(_), _) => Some(self.message_actions(message, now, cx)),
            (None, true) => Some(div().pt(dp(4.)).child(self.label_chip(message))),
            (None, false) => None,
        };
        div()
            .relative()
            .top(px(
                motion.offset + dp_lp(motion::NEW_MESSAGE_RISE) * (1.0 - appear)
            ))
            .opacity(motion.opacity * appear)
            .children(day)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .map(side)
                    .pt(dp(if first { 12. } else { 3. }))
                    .children(details.map(|details| div().pb(dp(6.)).child(details)))
                    .child(bubble)
                    .children(label),
            )
    }

    /// What happened to a message, or its time, on the chip under its run.
    fn label_chip(&self, message: &Message) -> Div {
        let (label, error) = message.label();
        if error {
            self.reason_chip(Icon::Alert, label, true)
        } else {
            chip(&self.look).child(label)
        }
    }

    fn bubble(
        &self,
        message: &Message,
        last: bool,
        layer: Layer,
        lift: Option<f32>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let ink = if message.outgoing {
            self.look.on_accent
        } else {
            self.look.palette.foreground
        };
        let underline = HighlightStyle {
            underline: Some(UnderlineStyle {
                thickness: px(1.),
                ..Default::default()
            }),
            ..Default::default()
        };
        let text = StyledText::new(message.text.clone()).with_highlights(
            message
                .spans
                .iter()
                .map(|span| (span.range.clone(), underline)),
        );
        let layout = text.layout().clone();
        let radius = px(self.look.bubble_radius());
        let fill = if message.outgoing {
            div().bg(solid(self.look.accent)).rounded(radius)
        } else {
            island_run(&self.look).rounded(radius)
        };
        let mut bubble = fill
            .id(("message", message.id as usize))
            .max_w(relative(0.78))
            .px(dp(14.))
            .py(dp(9.))
            .text_color(solid(ink))
            .line_height(relative(1.4))
            .child(text);
        if last {
            bubble = if message.outgoing {
                bubble.rounded_br(px(MIN_RADIUS))
            } else {
                bubble.rounded_bl(px(MIN_RADIUS))
            };
        }
        if let Some(t) = lift {
            bubble = bubble.shadow(lifted(t));
        }
        if layer.live() {
            let id = message.id;
            bubble = bubble.on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                cx.stop_propagation();
                let at = layout.index_for_position(event.position()).ok();
                this.tap_message(id, at, cx);
            }));
            bubble = self.holdable(bubble, Held::Message(id), cx);
        }
        bubble
    }

    /// The actions under a held message: Copy, Delete, and Retry when it did
    /// not go out or arrive.
    pub(super) fn message_actions(
        &self,
        message: &Message,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = message.id;
        let mut buttons = Vec::new();
        if message.failed() || message.undelivered() {
            buttons.push(self.button(
                "retry",
                Face::Both(Icon::Retry, "Retry"),
                Fill::Clear,
                true,
                cx,
                move |this, _, cx| {
                    this.dismiss(cx);
                    this.retry(id, cx);
                },
            ));
        }
        buttons.extend(self.copy_delete(id, cx));
        self.action_group(now, buttons)
    }

    /// Copy and Delete for message `id`, in a thread or the blocked folder.
    pub(super) fn copy_delete(&self, id: u64, cx: &mut Context<Self>) -> [gpui::Stateful<Div>; 2] {
        [
            self.button(
                "copy",
                Face::Both(Icon::Copy, "Copy"),
                Fill::Clear,
                true,
                cx,
                move |this, _, cx| {
                    this.dismiss(cx);
                    if let Some(text) = this.message_text(id) {
                        this.copy(text, "Copied", cx);
                    }
                },
            ),
            self.button(
                "delete",
                Face::Both(Icon::Delete, "Delete"),
                Fill::Danger,
                true,
                cx,
                move |this, _, cx| this.show(Overlay::Confirm(Confirm::DeleteMessage(id)), cx),
            ),
        ]
    }

    /// The thread's bottom zone: a chip that blocks sending, and the
    /// composer, or a chip in its place for a sender that takes no replies.
    pub(super) fn composer_zone(
        &self,
        index: usize,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let peer = &self.conversations[index].peer;
        if !data::replyable(peer) {
            return div()
                .flex()
                .justify_center()
                .pb(dp(12.))
                .child(chip(&self.look).child("This sender takes no replies"))
                .into_any_element();
        }
        let text = self.composer.read(cx).text().trim();
        let blocked = self.cannot_send_to(peer);
        let ready = !text.is_empty() && blocked.is_none();
        let counter = (!text.is_empty())
            .then(|| segments::count(text))
            .filter(|count| count.parts > 1 || count.left <= COUNTER_FROM)
            .map(|count| match count.parts {
                1 => count.left.to_string(),
                parts => format!("{} · {parts}", count.left),
            });
        let reason = blocked.map(|(glyph, reason)| {
            div()
                .flex()
                .justify_center()
                .child(self.reason_chip(glyph, reason, false))
        });
        let palette = &self.look.palette;
        let composer = self.composer.clone();
        let mut field = island_run(&self.look)
            .id("composer")
            .flex_1()
            .min_w_0()
            .min_h(dp(COMPOSER_HEIGHT))
            .px(dp(14.))
            .py(dp((COMPOSER_HEIGHT - input::LINE) / 2.0))
            .flex()
            .items_end()
            .gap(dp(8.))
            .child(self.composer.clone())
            .children(counter.map(|counter| {
                div()
                    .flex_none()
                    .line_height(dp(input::LINE))
                    .text_size(dp(12.))
                    .text_color(solid(palette.secondary))
                    .child(counter)
            }));
        if layer.live() {
            field = field.on_click(move |_, window, cx| {
                cx.stop_propagation();
                window.focus(&composer.focus_handle(cx), cx);
            });
        }
        let send = if ready {
            self.button(
                "send",
                Face::Icon(Icon::Send),
                Fill::Accent,
                layer.live(),
                cx,
                |this, _, cx| this.send(cx),
            )
        } else {
            island_run(&self.look)
                .id("send")
                .text_color(solid(palette.secondary))
                .child(icon(Icon::Send, dp(ICON), solid(palette.secondary)))
        };
        div()
            .flex()
            .flex_col()
            .gap(px(INSET))
            .children(reason)
            // The field grows upwards as the text wraps; Send stays at its
            // foot.
            .child(
                div().flex().items_end().gap(px(INSET)).child(field).child(
                    send.flex_none()
                        .size(dp(TOUCH))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        // A press on Send leaves the focus in the composer, so
                        // the keyboard stays up for the next message.
                        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default()),
                ),
            )
            .into_any_element()
    }
}
