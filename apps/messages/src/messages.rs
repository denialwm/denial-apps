use std::time::{Duration, Instant};

use denial_look::{
    INSET, Look, MIN_RADIUS, Tween, ease_out_cubic, floating, lerp_rgb, nested_radius, palette,
};
use gpui::{
    AnyElement, App, BoxShadow, Context, Div, Entity, FocusHandle, Focusable, FontWeight, Hsla,
    Pixels, ScrollHandle, Stateful, Subscription, Window, actions, div, point, prelude::*, px,
    relative, rgba,
};

use crate::data::{self, Conversation, Message};
use crate::input::{Submitted, TextInput};

actions!(messages, [Back]);

/// Sizes in this module are Android dp unless they come from Denial's look.
/// Denial lays handheld panels out at 225 logical pixels per inch (deniald's
/// HANDHELD_LOGICAL_PPI) and a dp is 1/160 inch, so one dp is a fixed number
/// of logical pixels on every Denial phone.
pub(crate) fn dp(value: f32) -> Pixels {
    px(value * 225.0 / 160.0)
}

const GROUP_HEIGHT: f32 = 56.0;
/// Smallest comfortable touch target.
const TOUCH: f32 = 48.0;
/// Compact layout: the desktop shows through this gap below the top group.
const REVEAL: f32 = 32.0;
const SWITCH_MS: u64 = 160;
const AVATAR: f32 = 46.0;
/// Denial's mobile shell takes touches in the bottom 52 logical pixels (its
/// gesture handle zone, ReferenceShellMetrics.gestureRect) and draws its pill
/// over the app, so controls stay above it while the canvas runs underneath.
const GESTURE_INSET: f32 = 52.0;

fn color(rgb: u32, alpha: f32) -> Hsla {
    rgba((rgb << 8) | (alpha.clamp(0.0, 1.0) * 255.0).round() as u32).into()
}

fn solid(rgb: u32) -> Hsla {
    color(rgb, 1.0)
}

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    List,
    Thread(usize),
}

/// The sample contact "types" and then answers.
struct PendingReply {
    conversation: usize,
    typing_at: Instant,
    reply_at: Instant,
}

pub struct Messages {
    look: Look,
    font: String,
    focus_handle: FocusHandle,
    conversations: Vec<Conversation>,
    search: Entity<TextInput>,
    composer: Entity<TextInput>,
    shown: Screen,
    target: Screen,
    switch: Option<Tween>,
    launched: Instant,
    list_scroll: ScrollHandle,
    thread_scroll: ScrollHandle,
    pending: Option<PendingReply>,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for Messages {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Messages {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let look = Look::load();
        let search = cx.new(|cx| TextInput::new("Search", look.accent, cx));
        let composer = cx.new(|cx| TextInput::new("Message", look.accent, cx));
        let subscriptions = vec![
            cx.observe(&search, |_, _, cx| cx.notify()),
            cx.observe(&composer, |_, _, cx| cx.notify()),
            cx.subscribe(&composer, |this, _, _: &Submitted, cx| this.send(cx)),
        ];
        let font = look.font_family(&cx.text_system().all_font_names());
        Self {
            look,
            font,
            focus_handle: cx.focus_handle(),
            conversations: data::sample(),
            search,
            composer,
            shown: Screen::List,
            target: Screen::List,
            switch: None,
            launched: Instant::now(),
            list_scroll: ScrollHandle::new(),
            thread_scroll: ScrollHandle::new(),
            pending: None,
            _subscriptions: subscriptions,
        }
    }

    /// Latest request wins; returning to the outgoing screen cancels the switch.
    fn open(&mut self, screen: Screen, window: &mut Window, cx: &mut Context<Self>) {
        if screen == self.target {
            return;
        }
        self.target = screen;
        if screen == self.shown {
            self.switch = None;
        } else if self.switch.is_none() {
            self.switch = Some(self.look.tween(SWITCH_MS));
        }
        match screen {
            Screen::Thread(index) => {
                let conversation = &mut self.conversations[index];
                if conversation.unread {
                    conversation.unread = false;
                    conversation.read_fade = Some(self.look.tween(420));
                }
                self.thread_scroll.scroll_to_bottom();
            }
            // Leaving the thread closes the on-screen keyboard.
            Screen::List => window.focus(&self.focus_handle, cx),
        }
        cx.notify();
    }

    fn back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.target, Screen::Thread(_)) {
            self.open(Screen::List, window, cx);
        }
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let Screen::Thread(index) = self.target else {
            return;
        };
        let text = self.composer.read(cx).text().trim().to_owned();
        if text.is_empty() {
            return;
        }
        self.composer.update(cx, |composer, cx| composer.clear(cx));
        self.conversations[index].messages.push(Message {
            text,
            outgoing: true,
            time: "Now",
            appear: Some(self.look.tween(300)),
        });
        self.thread_scroll.scroll_to_bottom();
        let typing_at = Instant::now() + Duration::from_millis(700);
        self.pending = Some(PendingReply {
            conversation: index,
            typing_at,
            reply_at: typing_at + Duration::from_millis(1600),
        });
        cx.notify();
    }

    fn settle(&mut self, now: Instant) {
        if self.switch.is_some_and(|switch| !switch.running(now)) {
            self.switch = None;
            self.shown = self.target;
        }
        if let Some(pending) = self.pending.take_if(|pending| now >= pending.reply_at) {
            let appear = self.look.tween(300);
            let conversation = &mut self.conversations[pending.conversation];
            let text = conversation.next_reply().to_owned();
            conversation.messages.push(Message {
                text,
                outgoing: false,
                time: "Now",
                appear: Some(appear),
            });
            if self.target == Screen::Thread(pending.conversation) {
                self.thread_scroll.scroll_to_bottom();
            } else {
                conversation.unread = true;
            }
        }
    }

    fn animating(&self, now: Instant) -> bool {
        self.switch.is_some()
            || self.pending.is_some()
            || self.row_entrance(now, self.conversations.len()) < 1.0
            || self.conversations.iter().any(|conversation| {
                conversation.read_fade.is_some_and(|fade| fade.running(now))
                    || conversation
                        .messages
                        .iter()
                        .rev()
                        .take(2)
                        .any(|message| message.appear.is_some_and(|appear| appear.running(now)))
            })
    }

    /// Rows rise in one after another at launch.
    fn row_entrance(&self, now: Instant, position: usize) -> f32 {
        if self.look.reduced_motion() {
            return 1.0;
        }
        let elapsed =
            now.duration_since(self.launched).as_secs_f32() * 1000.0 / self.look.duration_scale;
        ease_out_cubic(((elapsed - 120.0 - position as f32 * 45.0) / 380.0).clamp(0.0, 1.0))
    }

    fn glass_group(&self) -> Div {
        div()
            .bg(color(palette::GLASS, self.look.panel_alpha))
            .rounded(px(self.look.group_radius()))
            .border(px(floating::EDGE_WIDTH))
            .border_color(color(palette::FOREGROUND, floating::EDGE_ALPHA))
            .shadow(vec![BoxShadow {
                color: color(0x000000, floating::SHADOW_ALPHA),
                offset: point(px(0.), px(floating::SHADOW_OFFSET_Y)),
                blur_radius: px(floating::SHADOW_BLUR),
                spread_radius: px(0.),
                inset: false,
            }])
    }

    fn avatar(&self, conversation: &Conversation, diameter: f32) -> Div {
        div()
            .flex_none()
            .size(dp(diameter))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(solid(conversation.avatar))
            .text_size(dp(diameter * 0.36))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(solid(palette::FOREGROUND))
            .child(conversation.initials())
    }

    fn press(&self, element: Stateful<Div>, fill: u32) -> Stateful<Div> {
        element
            .cursor_pointer()
            .hover(move |s| s.bg(solid(lerp_rgb(fill, 0xFFFFFF, 0.10))))
            .active(move |s| s.bg(solid(lerp_rgb(fill, 0x000000, 0.18))))
    }

    fn search_bar(&self) -> Div {
        div()
            .size_full()
            .px(dp(14.))
            .flex()
            .items_center()
            .child(self.search.clone())
    }

    fn thread_bar(&self, index: usize, live: bool, cx: &mut Context<Self>) -> Div {
        let conversation = &self.conversations[index];
        let mut back = div()
            .id("back")
            .flex_none()
            .size(dp(TOUCH))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(nested_radius(self.look.group_radius(), 4.0)))
            .text_size(dp(30.))
            .text_color(solid(palette::FOREGROUND))
            .hover(|s| s.bg(color(palette::FOREGROUND, 0.10)))
            .active(|s| s.bg(color(palette::FOREGROUND, 0.16)))
            .cursor_pointer()
            .child("‹");
        if live {
            back = back.on_click(cx.listener(|this, _, window, cx| {
                this.open(Screen::List, window, cx);
            }));
        }
        div()
            .size_full()
            .pl(dp(4.))
            .pr(dp(14.))
            .flex()
            .items_center()
            .gap_2()
            .child(back)
            .child(self.avatar(conversation, 36.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(dp(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(conversation.name),
                    )
                    .child(
                        div()
                            .text_size(dp(12.))
                            .text_color(solid(palette::SECONDARY))
                            .child("Mobile"),
                    ),
            )
    }

    fn list(&self, live: bool, now: Instant, cx: &mut Context<Self>) -> AnyElement {
        let query = self.search.read(cx).text().trim().to_lowercase();
        let visible: Vec<usize> = (0..self.conversations.len())
            .filter(|&index| {
                let conversation = &self.conversations[index];
                query.is_empty()
                    || conversation.name.to_lowercase().contains(&query)
                    || conversation.last().text.to_lowercase().contains(&query)
            })
            .collect();
        let unread = self.conversations.iter().filter(|c| c.unread).count();
        let summary = match unread {
            0 => "All caught up".to_string(),
            1 => "1 unread conversation".to_string(),
            n => format!("{n} unread conversations"),
        };
        let header = div()
            .px(dp(24.))
            .pt(dp(28.))
            .pb(dp(12.))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_size(dp(26.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Messages"),
            )
            .child(div().text_color(solid(palette::SECONDARY)).child(summary));
        let empty = visible.is_empty().then(|| {
            div()
                .px(dp(24.))
                .py(dp(48.))
                .flex()
                .justify_center()
                .text_color(solid(palette::SECONDARY))
                .child(format!("No conversations match “{query}”"))
        });
        let rows: Vec<AnyElement> = visible
            .iter()
            .enumerate()
            .map(|(position, &index)| self.row(index, position, live, now, cx))
            .collect();
        div()
            .id("list")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.list_scroll)
            .child(header)
            .children(rows)
            .children(empty)
            .child(div().h(px(GESTURE_INSET)))
            .into_any_element()
    }

    fn row(
        &self,
        index: usize,
        position: usize,
        live: bool,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let conversation = &self.conversations[index];
        let last = conversation.last();
        let enter = self.row_entrance(now, position);
        let dot = if conversation.unread {
            1.0
        } else {
            conversation
                .read_fade
                .map_or(0.0, |fade| 1.0 - fade.progress(now))
        };
        let strong = conversation.unread;
        let preview = if last.outgoing {
            format!("You: {}", last.text)
        } else {
            last.text.clone()
        };

        let mut row = div()
            .id(("conversation", index))
            .flex()
            .items_center()
            .gap_3()
            .px(dp(24.))
            .py(dp(12.))
            .cursor_pointer()
            .hover(|s| s.bg(color(palette::FOREGROUND, 0.04)))
            .active(|s| s.bg(color(palette::FOREGROUND, 0.08)))
            .child(self.avatar(conversation, AVATAR))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(dp(3.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(if strong {
                                        FontWeight::BOLD
                                    } else {
                                        FontWeight::SEMIBOLD
                                    })
                                    .child(conversation.name),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(dp(12.))
                                    .text_color(solid(if strong {
                                        palette::FOREGROUND
                                    } else {
                                        palette::SECONDARY
                                    }))
                                    .child(last.time),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(dp(14.))
                                    .text_color(solid(if strong {
                                        palette::FOREGROUND
                                    } else {
                                        palette::SECONDARY
                                    }))
                                    .child(preview),
                            )
                            .when(dot > 0.0, |line| {
                                line.child(
                                    div()
                                        .flex_none()
                                        .size(dp(10.))
                                        .rounded_full()
                                        .bg(color(self.look.accent, dot)),
                                )
                            }),
                    ),
            );
        if live {
            row = row.on_click(cx.listener(move |this, _, window, cx| {
                this.open(Screen::Thread(index), window, cx);
            }));
        }
        div()
            .relative()
            .top(dp((1.0 - enter) * 18.0))
            .opacity(enter)
            .child(row)
            .child(
                div()
                    .h(px(1.))
                    .ml(dp(24. + AVATAR + 12.))
                    .bg(color(palette::SEPARATOR, palette::SEPARATOR_ALPHA)),
            )
            .into_any_element()
    }

    fn bubble_radius(&self) -> f32 {
        self.look.window_radius.max(MIN_RADIUS)
    }

    fn bubble(&self, message: &Message, first: bool, last: bool, now: Instant) -> Div {
        let appear = message
            .appear
            .map_or(1.0, |appear| ease_out_cubic(appear.progress(now)));
        let (fill, ink) = if message.outgoing {
            (self.look.accent, self.look.on_accent)
        } else {
            (palette::RAISED, palette::FOREGROUND)
        };
        let mut bubble = div()
            .max_w(relative(0.78))
            .px(dp(14.))
            .py(dp(9.))
            .rounded(px(self.bubble_radius()))
            .bg(solid(fill))
            .text_color(solid(ink))
            .line_height(relative(1.4))
            .child(message.text.clone());
        if last {
            bubble = if message.outgoing {
                bubble.rounded_br(px(MIN_RADIUS))
            } else {
                bubble.rounded_bl(px(MIN_RADIUS))
            };
        }
        div()
            .w_full()
            .flex()
            .flex_col()
            .map(|column| {
                if message.outgoing {
                    column.items_end()
                } else {
                    column.items_start()
                }
            })
            .mt(dp(if first { 12. } else { 3. }))
            .relative()
            .top(dp((1.0 - appear) * 16.0))
            .opacity(appear)
            .child(bubble)
            .when(last, |column| {
                column.child(
                    div()
                        .mt(dp(4.))
                        .px(dp(6.))
                        .text_size(dp(11.))
                        .text_color(solid(palette::SECONDARY))
                        .child(message.time),
                )
            })
    }

    fn typing(&self, now: Instant) -> Div {
        let elapsed = now.duration_since(self.launched).as_secs_f32();
        let dots = (0..3).map(|k| {
            let wave = if self.look.reduced_motion() {
                0.0
            } else {
                (elapsed * 7.0 - k as f32 * 0.9).sin().max(0.0)
            };
            div()
                .size(dp(7.))
                .rounded_full()
                .relative()
                .top(dp(-3.0 * wave))
                .bg(color(palette::SECONDARY, 0.45 + 0.55 * wave))
        });
        div().w_full().flex().mt(dp(12.)).child(
            div()
                .h(dp(38.))
                .px(dp(16.))
                .flex()
                .items_center()
                .gap(dp(5.))
                .rounded(px(self.bubble_radius()))
                .rounded_bl(px(MIN_RADIUS))
                .bg(solid(palette::RAISED))
                .children(dots),
        )
    }

    fn composer_bar(&self, live: bool, cx: &mut Context<Self>) -> Div {
        let empty = self.composer.read(cx).text().trim().is_empty();
        let radius = self.look.group_radius();
        let fill = if empty {
            palette::CONTROL
        } else {
            self.look.accent
        };
        let mut send = div()
            .id("send")
            .flex_none()
            .h(dp(TOUCH))
            .px_4()
            .flex()
            .items_center()
            .rounded(px(radius))
            .bg(solid(fill))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(solid(if empty {
                palette::SECONDARY
            } else {
                self.look.on_accent
            }))
            .child("Send");
        if live && !empty {
            send = self
                .press(send, fill)
                .on_click(cx.listener(|this, _, _, cx| this.send(cx)));
        }
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(INSET))
            .px(px(INSET))
            .pt(px(INSET))
            .pb(px(GESTURE_INSET))
            .bg(solid(palette::CANVAS))
            .border_t(px(1.))
            .border_color(color(palette::SEPARATOR, palette::SEPARATOR_ALPHA))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(dp(TOUCH))
                    .px(dp(14.))
                    .flex()
                    .items_center()
                    .rounded(px(radius))
                    .bg(solid(palette::CONTROL))
                    .child(self.composer.clone()),
            )
            .child(send)
    }

    fn thread(&self, index: usize, live: bool, now: Instant, cx: &mut Context<Self>) -> AnyElement {
        let conversation = &self.conversations[index];
        let messages = &conversation.messages;
        let bubbles: Vec<Div> = messages
            .iter()
            .enumerate()
            .map(|(i, message)| {
                let first = i == 0 || messages[i - 1].outgoing != message.outgoing;
                let last = messages.get(i + 1).is_none_or(|next| {
                    next.outgoing != message.outgoing || next.time != message.time
                });
                self.bubble(message, first, last, now)
            })
            .collect();
        let typing = self
            .pending
            .as_ref()
            .filter(|pending| pending.conversation == index && now >= pending.typing_at)
            .map(|_| self.typing(now));
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id(("thread", index))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.thread_scroll)
                    .px(dp(16.))
                    .pt(dp(16.))
                    .pb(dp(16.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .justify_center()
                            .text_size(dp(12.))
                            .text_color(solid(palette::SECONDARY))
                            .child("Today"),
                    )
                    .children(bubbles)
                    .children(typing),
            )
            .child(self.composer_bar(live, cx))
            .into_any_element()
    }
}

impl Render for Messages {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        self.settle(now);
        if self.animating(now) {
            window.request_animation_frame();
        }

        // During a switch the outgoing screen fades and slides out, then the
        // incoming one fades and slides in. Material backings stay put.
        let forward = matches!(self.target, Screen::Thread(_));
        let direction = if forward { 1.0 } else { -1.0 };
        let (screen, alpha, live, shift) = match self.switch {
            Some(switch) => {
                let t = switch.progress(now);
                if t < 0.5 {
                    let out = 2.0 * t;
                    (self.shown, 1.0 - out, false, -direction * 20.0 * out)
                } else {
                    let incoming = ease_out_cubic(2.0 * t - 1.0);
                    (
                        self.target,
                        incoming,
                        true,
                        direction * 20.0 * (1.0 - incoming),
                    )
                }
            }
            None => (self.shown, 1.0, true, 0.0),
        };

        let top = match screen {
            Screen::List => self.search_bar(),
            Screen::Thread(index) => self.thread_bar(index, live, cx),
        };
        let body = match screen {
            Screen::List => self.list(live, now, cx),
            Screen::Thread(index) => self.thread(index, live, now, cx),
        };
        let backing = if self.look.window_alpha >= 1.0 {
            solid(palette::CANVAS)
        } else {
            color(palette::GLASS, self.look.window_alpha)
        };

        div()
            .size_full()
            .relative()
            .key_context("Messages")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::back))
            .bg(backing)
            .font_family(self.font.clone())
            .text_color(solid(palette::FOREGROUND))
            .child(
                div()
                    .absolute()
                    .top(px(INSET))
                    .left(px(INSET))
                    .right(px(INSET))
                    .h(dp(GROUP_HEIGHT))
                    .child(
                        self.glass_group()
                            .size_full()
                            .child(div().size_full().opacity(alpha).child(top)),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(px(INSET) + dp(GROUP_HEIGHT + REVEAL))
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .overflow_hidden()
                    .bg(solid(palette::CANVAS))
                    .child(
                        div()
                            .size_full()
                            .relative()
                            .left(dp(shift))
                            .opacity(alpha)
                            .child(body),
                    ),
            )
    }
}
