//! Messages' settings: delivery reports, the SIM dmail uses, the blocked
//! numbers, the sample conversations, and the folder of messages from
//! blocked senders.

use std::time::Instant;

use denial_look::mobile::{AVATAR, content_top};
use denial_look::{INSET, MIN_RADIUS};
use denial_look_gpui::{
    Icon, chip, color, control_fill, control_radius, dp, icon, island, island_run, lifted, solid,
};
use gpui::{AnyElement, Context, Div, FontWeight, SharedString, Stateful, div, prelude::*, px};

use super::parts::{Face, Fill, ICON, place};
use super::{Held, Layer, Messages, Screen};
use crate::data;

impl Messages {
    pub(super) fn settings(
        &self,
        layer: Layer,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let live = layer.live();
        let reports = self.delivery_reports;
        let mut report_toggle = self.toggle(reports).id("delivery-reports");
        if live {
            report_toggle = report_toggle.on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.set_delivery_reports(!reports, cx);
            }));
        }
        let sending = island(&self.look)
            .child(self.row(
                Icon::Delivered,
                "Delivery reports",
                Some("Ask the network to confirm each message arrived".into()),
                report_toggle,
            ))
            .child(self.separator())
            .child(self.row(
                Icon::Sim,
                "SIM for messages",
                None,
                self.sim_choice(live, cx),
            ));
        let mut blocked = island(&self.look);
        if self.blocked.is_empty() {
            blocked = blocked.child(self.row(Icon::Block, "No blocked numbers", None, div()));
        }
        for (ix, number) in self.blocked.iter().enumerate() {
            let number = number.clone();
            let name = self.people.name(&number).to_owned();
            let detail = (name != number).then(|| number.clone().into());
            let unblock = self.button(
                ("unblock", ix),
                Face::Label("Unblock"),
                Fill::Control,
                live,
                cx,
                move |this, _, cx| this.block(number.clone(), false, cx),
            );
            blocked = blocked
                .when(ix > 0, |island| island.child(self.separator()))
                .child(self.row(Icon::Block, name, detail, unblock));
        }
        let count = self.blocked_messages.len();
        let mut folder = self
            .row(
                Icon::Message,
                "Blocked messages",
                None,
                div()
                    .flex()
                    .items_center()
                    .gap(dp(6.))
                    .text_color(solid(self.look.palette.secondary))
                    .children((count > 0).then(|| count.to_string()))
                    .child(icon(
                        Icon::Chevron,
                        dp(ICON),
                        solid(self.look.palette.secondary),
                    )),
            )
            .id("blocked-messages");
        if live {
            folder =
                self.press(folder, Fill::Clear)
                    .on_click(cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        this.open(Screen::Blocked, window, cx)
                    }));
        }
        let blocked = blocked
            .child(self.separator())
            .child(folder.rounded_b(px(self.look.group_radius())));
        let shown = self.samples.on();
        let mut sample_toggle = self.toggle(shown).id("samples");
        if live {
            sample_toggle = sample_toggle.on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.show_samples(!shown, cx);
            }));
        }
        let samples = island(&self.look).child(self.row(
            Icon::Message,
            "Sample conversations",
            Some("Made-up chats to try the app. Nothing in them is sent".into()),
            sample_toggle,
        ));
        let parts: [Div; 6] = [
            self.chip_row("Sending"),
            sending,
            self.chip_row("Blocked numbers"),
            blocked,
            self.chip_row("Try it"),
            samples,
        ];
        div()
            .id("settings")
            .size_full()
            .overflow_y_scroll()
            .px(px(INSET))
            .pt(px(content_top()))
            .pb(px(self.list_bottom(Instant::now())))
            .flex()
            .flex_col()
            .gap(px(INSET))
            .children(
                parts
                    .into_iter()
                    .enumerate()
                    .map(|(ix, part)| place(part, self.item_motion(layer, ix, false, now))),
            )
            .into_any_element()
    }

    /// Messages from blocked senders, newest first, each with its sender and
    /// time on a chip.
    pub(super) fn blocked_folder(
        &self,
        layer: Layer,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.blocked_messages.is_empty() {
            let island = self.state_island(
                Icon::Block,
                "No blocked messages",
                "Messages from blocked numbers are kept here, without a notification.",
                None,
            );
            return div()
                .px(px(INSET))
                .pt(px(content_top()))
                .child(place(island, self.item_motion(layer, 0, false, now)))
                .into_any_element();
        }
        let items: Vec<_> = self
            .blocked_messages
            .iter()
            .enumerate()
            .map(|(ix, (id, sender, text, time))| {
                let held = Held::Message(*id);
                let lift = self.lift(&held, now);
                let mut bubble = island_run(&self.look)
                    .id(("blocked", *id as usize))
                    .max_w_full()
                    .rounded(px(self.look.bubble_radius()))
                    .rounded_bl(px(MIN_RADIUS))
                    .px(dp(14.))
                    .py(dp(9.))
                    .line_height(gpui::relative(1.4))
                    .child(text.clone());
                if let Some(t) = lift {
                    bubble = bubble.shadow(lifted(t));
                }
                if layer.live() {
                    bubble = bubble.on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.dismiss(cx);
                    }));
                    bubble = self.holdable(bubble, held, cx);
                }
                let under = match lift {
                    Some(_) => self.action_group(now, self.copy_delete(*id, cx).into()),
                    None => chip(&self.look).child(format!(
                        "{} · {}",
                        self.people.name(sender),
                        data::full_time(*time, data::now())
                    )),
                };
                let item = div()
                    .pt(dp(9.))
                    .flex()
                    .items_end()
                    .gap(px(INSET))
                    .child(self.avatar(sender, AVATAR))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap(dp(4.))
                            .child(bubble)
                            .child(under),
                    );
                place(item, self.item_motion(layer, ix, lift.is_some(), now))
            })
            .collect();
        div()
            .id("blocked-folder")
            .size_full()
            .overflow_y_scroll()
            .px(px(INSET))
            .pt(px(content_top() - INSET))
            .pb(px(self.list_bottom(Instant::now())))
            .children(items)
            .into_any_element()
    }

    /// A section chip over an island of rows.
    fn chip_row(&self, label: &'static str) -> Div {
        div().pt(dp(6.)).flex().child(chip(&self.look).child(label))
    }

    /// A setting: its icon tile, its label and what it says, and its control.
    fn row(
        &self,
        glyph: Icon,
        label: impl Into<SharedString>,
        detail: Option<SharedString>,
        control: impl IntoElement,
    ) -> Div {
        div()
            .flex()
            .items_center()
            .gap(dp(11.))
            .px(dp(14.))
            .py(dp(11.))
            .child(
                div()
                    .flex_none()
                    .size(dp(33.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(control_radius(&self.look))
                    .bg(control_fill(&self.look))
                    .child(icon(glyph, dp(20.), solid(self.look.palette.foreground))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(dp(1.))
                    .child(div().truncate().child(label.into()))
                    .children(detail.map(|detail| {
                        div()
                            .text_size(dp(12.))
                            .text_color(solid(self.look.palette.secondary))
                            .child(detail)
                    })),
            )
            .child(div().flex_none().child(control))
    }

    fn separator(&self) -> Div {
        div().h(px(1.)).bg(color(
            self.look.palette.separator,
            self.look.palette.separator_alpha,
        ))
    }

    fn toggle(&self, on: bool) -> Div {
        let palette = &self.look.palette;
        let (track, knob) = if on {
            (solid(self.look.accent), self.look.on_accent)
        } else {
            (control_fill(&self.look), palette.secondary)
        };
        div()
            .w(dp(45.))
            .h(dp(26.))
            .p(dp(3.))
            .flex()
            .when(on, |toggle| toggle.justify_end())
            .rounded_full()
            .bg(track)
            .cursor_pointer()
            .child(div().size(dp(20.)).rounded_full().bg(solid(knob)))
    }

    /// SIM 1 or SIM 2, as dmail has it; none until dmail says.
    fn sim_choice(&self, live: bool, cx: &mut Context<Self>) -> Div {
        let segment = |slot: u8, cx: &mut Context<Self>| -> Stateful<Div> {
            let chosen = self.sim_slot == slot;
            let mut segment = div()
                .id(("sim", slot as usize))
                .min_w(dp(44.))
                .h(dp(34.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(denial_look::nested_radius(
                    control_radius(&self.look).as_f32(),
                    3.0,
                )))
                .font_weight(FontWeight::SEMIBOLD)
                .child(slot.to_string());
            if chosen {
                segment = segment
                    .bg(solid(self.look.accent))
                    .text_color(solid(self.look.on_accent));
            } else if live {
                segment = segment
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.set_sim_slot(slot, cx);
                    }));
            }
            segment
        };
        div()
            .flex()
            .p(dp(3.))
            .gap(dp(3.))
            .rounded(control_radius(&self.look))
            .bg(control_fill(&self.look))
            .child(segment(1, cx))
            .child(segment(2, cx))
    }
}
