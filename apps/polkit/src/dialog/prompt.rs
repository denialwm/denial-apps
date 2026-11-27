//! The authentication prompt: schema 1 of the agent's dialog socket.
//!
//! Deliberately thin. Polkit and the agent own the conversation, the
//! attempts and the outcome; this shows what they say and relays the
//! user's choice of account and replies.

use std::time::Instant;

use denial_look::Appearance;
use denial_look::words::fill;
use denial_look_gpui::input::{Submitted, TextInput};
use denial_look_gpui::motion::{self, Blink, Presence, Pulse, Shake};
use denial_look_gpui::{color, say, solid};
use gpui::{
    Bounds, ClickEvent, ContentMask, Context, Div, Entity, FocusHandle, Focusable, FontWeight,
    ObjectFit, SharedString, Stateful, Task, Window, canvas, div, fill as paint, img, point,
    prelude::*, px, size,
};
use serde_json::{Value, json};

use super::link::Link;
use super::{Cancel, Confirm, Frame, Receive};
use crate::identity::{self, Profile};

/// The Flutter dialog's measures: the account's column on the leading
/// side, its picture, and the gap to the description and reply beside it.
const AVATAR: f32 = 72.0;
const ACCOUNT_WIDTH: f32 = 96.0;
const GAP: f32 = 16.0;
const SMALL_AVATAR: f32 = 26.0;
const IDENTITY_ROW: f32 = 46.0;
const FIELD_HEIGHT: f32 = 48.0;
/// A secret reply's dots: their radius and the distance between them.
const DOT_RADIUS: f32 = 4.2;
const DOT_SPACING: f32 = 14.0;
/// Time limits and Caps Lock.
const WARNING: u32 = 0xF0A030;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Waiting for the agent's request.
    Connecting,
    /// Several accounts may authorize; the user picks one, once.
    Choosing,
    /// An account is picked; Polkit has not prompted yet.
    Preparing,
    /// A prompt waits for its reply.
    Prompting,
    /// A reply was sent; the next prompt, a message or the end follows.
    Verifying,
    /// Nothing more can happen here: the attempts are used up, or the agent
    /// sent nothing usable.
    Unavailable,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NoticeKind {
    Info,
    Error,
}

struct Identity {
    uid: u32,
    name: String,
    profile: Profile,
}

pub struct Prompt {
    frame: Frame,
    link: Link,
    focus: FocusHandle,
    field: Entity<TextInput>,
    phase: Phase,
    message: String,
    command: Option<String>,
    run_as: Option<String>,
    identities: Vec<Identity>,
    selected: Option<u32>,
    prompt: Option<u64>,
    label: String,
    echo: bool,
    notice: Option<(NoticeKind, String)>,
    /// A reply went out and no verdict on it arrived yet.
    awaiting_verdict: bool,
    closing: bool,
    /// What the field says while empty, as last given to it.
    shown_placeholder: SharedString,
    /// A secret reply's dots as they come and go, and how many the sent
    /// reply had: they stay, waving, while it is checked.
    dots: Presence,
    sent_dots: usize,
    pulse: Pulse,
    /// The card shakes when a reply is refused.
    shake: Shake,
    blink: Blink,
    _blinking: Task<()>,
    /// The reply field has the keyboard: it is focused and the compositor
    /// gave the dialog the keyboard. The dialog asks for it on demand only,
    /// so it opens without it.
    typing: bool,
}

impl Prompt {
    pub fn new(frame: Frame, link: Link, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = cx.new(|cx| {
            let mut field = TextInput::new(SharedString::default(), &frame.look, 1, cx);
            field.set_text_size(15.0, cx);
            field.set_secret(true, cx);
            // The dialog draws a secret reply's dots and its cursor itself.
            field.set_caret(false, cx);
            field
        });
        cx.subscribe(&field, |this, _, _: &Submitted, cx| this.confirm(cx))
            .detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let blinking =
            motion::keep_blinking(cx, |this: &Self| this.caret_blinks().then_some(this.blink));
        Self {
            frame,
            link,
            focus,
            field,
            phase: Phase::Connecting,
            message: String::new(),
            command: None,
            run_as: None,
            identities: Vec::new(),
            selected: None,
            prompt: None,
            label: String::new(),
            echo: false,
            notice: None,
            awaiting_verdict: false,
            closing: false,
            shown_placeholder: SharedString::default(),
            dots: Presence::default(),
            sent_dots: 0,
            pulse: Pulse::default(),
            shake: Shake::default(),
            blink: Blink::default(),
            _blinking: blinking,
            typing: false,
        }
    }

    fn send(&mut self, message: Value, cx: &mut Context<Self>) {
        if self.link.send(&message).is_err() {
            self.unavailable("Authentication is unavailable.", cx);
        }
    }

    fn unavailable(&mut self, text: &str, cx: &mut Context<Self>) {
        self.phase = Phase::Unavailable;
        self.notice = Some((NoticeKind::Error, say(text.to_owned()).to_string()));
        cx.notify();
    }

    fn choose(&mut self, uid: u32, cx: &mut Context<Self>) {
        if self.phase == Phase::Choosing {
            self.selected = Some(uid);
            cx.notify();
        }
    }

    /// Polkit takes one choice of account before the conversation starts.
    fn start(&mut self, cx: &mut Context<Self>) {
        let Some(uid) = self.selected.filter(|_| self.phase == Phase::Choosing) else {
            return;
        };
        self.phase = Phase::Preparing;
        self.send(json!({"type": "select", "uid": uid}), cx);
        cx.notify();
    }

    fn respond(&mut self, cx: &mut Context<Self>) {
        let Some(prompt) = self.prompt.filter(|_| self.phase == Phase::Prompting) else {
            return;
        };
        let text = self.field.read(cx).text();
        self.sent_dots = if self.echo { 0 } else { text.chars().count() };
        let sent = self.link.respond(prompt, text);
        self.field.update(cx, |field, cx| field.clear(cx));
        if sent.is_err() {
            return self.unavailable("Authentication is unavailable.", cx);
        }
        self.prompt = None;
        self.awaiting_verdict = true;
        self.phase = Phase::Verifying;
        self.notice = None;
        cx.notify();
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        match self.phase {
            Phase::Choosing => self.start(cx),
            Phase::Prompting if !self.locked() => self.respond(cx),
            _ => {}
        }
    }

    /// The agent ends this process once it has the cancellation. Without an
    /// agent to tell, nothing would, so the dialog leaves by itself.
    fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        self.closing = true;
        if self.phase == Phase::Unavailable || self.link.send(&json!({"type": "cancel"})).is_err() {
            cx.quit();
        }
    }

    /// PAM says a lockout's countdown as a message in parentheses: no reply
    /// can succeed meanwhile.
    fn locked(&self) -> bool {
        self.notice
            .as_ref()
            .is_some_and(|(_, text)| parenthesized(text).is_some())
    }

    fn selected_identity(&self) -> Option<&Identity> {
        self.identities
            .iter()
            .find(|identity| Some(identity.uid) == self.selected)
    }
}

impl Receive for Prompt {
    fn receive(&mut self, message: Value, window: &mut Window, cx: &mut Context<Self>) {
        let text = |key: &str| message[key].as_str().unwrap_or_default().to_owned();
        match message["type"].as_str() {
            Some("request") => {
                self.message = text("message");
                self.command = message["command"].as_str().map(str::to_owned);
                self.run_as = message["runAs"].as_str().map(str::to_owned);
                self.identities = message["identities"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|identity| {
                        let uid = u32::try_from(identity["uid"].as_u64()?).ok()?;
                        let name = identity["name"].as_str()?.to_owned();
                        Some(Identity {
                            uid,
                            profile: identity::profile(uid, &name),
                            name,
                        })
                    })
                    .collect();
                let Some(first) = self.identities.first() else {
                    return self.unavailable("No account can authorize this action.", cx);
                };
                self.selected = Some(first.uid);
                self.phase = Phase::Choosing;
                if self.identities.len() == 1 {
                    self.start(cx);
                }
            }
            Some("prompt") => {
                let Some(prompt) = message["prompt"].as_u64() else {
                    return;
                };
                self.prompt = Some(prompt);
                self.sent_dots = 0;
                self.label = label(&text("text"));
                self.echo = message["echo"].as_bool().unwrap_or(false);
                self.awaiting_verdict = false;
                self.phase = Phase::Prompting;
                let echo = self.echo;
                self.field.update(cx, |field, cx| {
                    field.clear(cx);
                    field.set_secret(!echo, cx);
                    field.set_caret(echo, cx);
                });
                if !self.locked() {
                    window.focus(&self.field.focus_handle(cx), cx);
                }
            }
            Some("info") => self.notice = Some((NoticeKind::Info, text("text"))),
            Some("error") => {
                self.notice = Some((NoticeKind::Error, text("text")));
                // PAM may say several things about one rejected reply.
                if std::mem::take(&mut self.awaiting_verdict) {
                    self.shake.start(&self.frame.look);
                }
            }
            Some("failed") => {
                // The attempts are used up; the agent closes the dialog soon.
                if std::mem::take(&mut self.awaiting_verdict) {
                    self.shake.start(&self.frame.look);
                }
                let said = text("text");
                return self.unavailable(
                    if said.is_empty() {
                        "Authentication failed."
                    } else {
                        &said
                    },
                    cx,
                );
            }
            _ => return,
        }
        cx.notify();
    }
}

impl Prompt {
    fn placeholder(&self) -> SharedString {
        let label = split_time(&self.label).0;
        if label.is_empty() || label.eq_ignore_ascii_case("password") {
            say("Password")
        } else {
            label.to_owned().into()
        }
    }

    /// What is asked: in the user's language when a command is to run,
    /// else the requester's own words.
    fn question(&self) -> SharedString {
        if self.command.is_some() {
            return match self.run_as.as_deref() {
                None | Some("root") => say("Administrator authentication required."),
                Some(user) => fill(
                    "Authentication is required to run this command as {user}.",
                    &[("user", user)],
                )
                .into(),
            };
        }
        let described = split_time(self.message.trim()).0;
        if !described.is_empty() {
            described.to_owned().into()
        } else if self.phase == Phase::Unavailable {
            say("Authentication is unavailable.")
        } else {
            say("Authentication is required.")
        }
    }

    /// A time limit Polkit or PAM gave, shown as a badge.
    fn time_limit(&self) -> Option<String> {
        if let Some((_, text)) = &self.notice
            && let Some(inner) = parenthesized(text)
        {
            return Some(inner.to_owned());
        }
        split_time(&self.label)
            .1
            .or_else(|| split_time(self.message.trim()).1)
            .map(str::to_owned)
    }

    /// The account's picture, in a thin rim.
    fn picture(&self, identity: Option<&Identity>, diameter: f32) -> gpui::AnyElement {
        let look = &self.frame.look;
        let name = identity.map_or_else(String::new, |identity| {
            identity.profile.display_name.clone()
        });
        let rim = if look.appearance == Appearance::Dark {
            color(0xFFFFFF, 0.26)
        } else {
            color(0xFFFFFF, 0.95)
        };
        let (accent, on_accent) = (look.accent, look.on_accent);
        let monogram = move || {
            let letter: String = name
                .chars()
                .find(|c| c.is_alphanumeric())
                .map(|c| c.to_uppercase().collect())
                .unwrap_or_default();
            div()
                .size_full()
                .rounded_full()
                .bg(solid(accent))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(diameter * 0.42))
                .line_height(px(diameter * 0.42))
                .font_weight(FontWeight::BOLD)
                .text_color(solid(on_accent))
                .child(letter)
                .into_any_element()
        };
        let inner = match identity.and_then(|identity| identity.profile.picture.clone()) {
            Some(path) => img(path)
                .size_full()
                .rounded_full()
                .object_fit(ObjectFit::Cover)
                .with_fallback(monogram)
                .into_any_element(),
            None => monogram(),
        };
        div()
            .relative()
            .size(px(diameter))
            .flex_none()
            .child(inner)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .border(px(1.))
                    .border_color(rim),
            )
            .into_any_element()
    }

    /// The account's name, level with the reply; its account name under it
    /// when the two differ.
    fn account_name(&self, identity: &Identity) -> Div {
        let look = &self.frame.look;
        let name = identity.profile.display_name.clone();
        div()
            .w(px(ACCOUNT_WIDTH))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .text_center()
            .child(
                div()
                    .w_full()
                    .text_size(px(13.))
                    .line_height(gpui::relative(1.25))
                    .font_weight(FontWeight::SEMIBOLD)
                    .line_clamp(2)
                    .child(name.clone()),
            )
            .when(name != identity.name, |column| {
                column.child(
                    div()
                        .w_full()
                        .text_size(px(11.5))
                        .text_color(color(look.palette.secondary, 0.7))
                        .truncate()
                        .child(identity.name.clone()),
                )
            })
    }

    /// The accounts Polkit offers, in the reply's place; picking one goes
    /// on with it.
    fn identity_list(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let look = &self.frame.look;
        let foreground = look.palette.foreground;
        let rows = self.identities.iter().map(|identity| {
            let uid = identity.uid;
            let selected = self.selected == Some(uid);
            let name = identity.profile.display_name.clone();
            div()
                .id(("identity", uid as usize))
                .h(px(IDENTITY_ROW))
                .flex_none()
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(12.))
                .cursor_pointer()
                .hover(move |style| style.bg(color(foreground, 0.06)))
                .active(move |style| style.bg(color(foreground, 0.1)))
                .child(self.picture(Some(identity), SMALL_AVATAR))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .gap(px(8.))
                        .text_size(px(14.))
                        .child(
                            div()
                                .flex_none()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(solid(if selected {
                                    look.accent_text
                                } else {
                                    foreground
                                }))
                                .child(name.clone()),
                        )
                        .when(name != identity.name, |row| {
                            row.child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(solid(look.palette.secondary))
                                    .child(identity.name.clone()),
                            )
                        }),
                )
                .when(selected, |row| {
                    row.child(
                        div()
                            .flex_none()
                            .text_size(px(16.))
                            .text_color(solid(look.accent_text))
                            .child("✓"),
                    )
                })
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    this.choose(uid, cx);
                    // A second tap on the picked account goes on with it.
                    if event.click_count() > 1 {
                        this.start(cx);
                    }
                }))
        });
        self.frame
            .toolbar(div().id("identities"))
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .max_h(px(IDENTITY_ROW * 4.))
            .overflow_y_scroll()
            .children(rows)
    }

    /// The reply field on the toolbar material. A secret reply shows as
    /// dots this draws, with its own cursor.
    fn reply(&self, window: &Window, cx: &Context<Self>) -> Div {
        let look = &self.frame.look;
        let locked = self.locked();
        let field = self.field.read(cx);
        let empty = field.text().is_empty();
        let error = self
            .notice
            .as_ref()
            .is_some_and(|(kind, _)| *kind == NoticeKind::Error);
        let secret = !self.echo;
        let caps = secret && self.phase == Phase::Prompting && !locked && window.capslock().on;
        let (ink, accent) = (look.palette.foreground, look.accent);
        let (dots, pulse) = (self.dots.clone(), self.pulse);
        let now = Instant::now();
        let caret = self.caret_blinks() && self.blink.visible(now);
        let dots = canvas(
            |_, _, _| {},
            move |bounds: Bounds<gpui::Pixels>, _, window, _| {
                let dot = |window: &mut Window, center, size_of: f32| {
                    let radius = DOT_RADIUS * size_of;
                    window.paint_quad(
                        paint(
                            Bounds::centered_at(center, size(px(radius * 2.), px(radius * 2.))),
                            solid(ink),
                        )
                        .corner_radii(px(radius)),
                    );
                };
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    let y = bounds.center().y;
                    let count = dots.len();
                    // Long replies scroll; the oldest dots leave past the
                    // leading edge.
                    let room = f32::from(bounds.size.width) - 14.0;
                    let shift = (room - count as f32 * DOT_SPACING).min(0.0);
                    let x = |index: usize| {
                        bounds.origin.x + px(DOT_SPACING / 2. + index as f32 * DOT_SPACING + shift)
                    };
                    for (index, size_of) in dots.leaving(now) {
                        dot(window, point(x(index), y), size_of);
                    }
                    for index in 0..count {
                        let size_of = dots.size(index, now) * pulse.size(index, now);
                        dot(window, point(x(index), y), size_of);
                    }
                    if caret {
                        let caret_x = if count == 0 {
                            bounds.origin.x + px(1.)
                        } else {
                            x(count - 1) + px(DOT_SPACING / 2. + 3.)
                        };
                        window.paint_quad(
                            paint(
                                Bounds::centered_at(
                                    point(caret_x + px(1.), y),
                                    size(px(2.), px(20.)),
                                ),
                                solid(accent),
                            )
                            .corner_radii(px(1.)),
                        );
                    }
                });
            },
        )
        .absolute()
        .inset_0();
        let field = div()
            .relative()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    // A secret's own characters stay unseen: the dots stand
                    // in for them.
                    .when(secret, |input| input.text_color(color(ink, 0.0)))
                    .child(self.field.clone()),
            )
            .when(secret, |field| field.child(dots));
        self.frame
            .toolbar(div())
            .flex_1()
            .min_w_0()
            .h(px(FIELD_HEIGHT))
            .flex()
            .items_center()
            .pl(px(16.))
            .pr(px(14.))
            .when(error && empty, |field| {
                field
                    .border(px(1.5))
                    .border_color(solid(look.palette.error))
            })
            .child(field)
            .when(caps, |field| {
                field.child(
                    div()
                        .flex_none()
                        .pl(px(8.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(solid(WARNING))
                        .child(format!("⇪ {}", say("Caps Lock"))),
                )
            })
    }

    /// The dialog draws a blinking cursor: a secret reply is awaited and
    /// typing reaches it.
    fn caret_blinks(&self) -> bool {
        self.typing && !self.echo && self.phase == Phase::Prompting && !self.locked()
    }

    /// Follows whether typing reaches the reply field. GPUI redraws when the
    /// window gains or loses the keyboard.
    fn follow_keyboard(&mut self, window: &Window, cx: &Context<Self>) {
        let typing = window.is_window_active() && self.field.focus_handle(cx).is_focused(window);
        if typing && !self.typing {
            // The cursor shows as soon as typing reaches it.
            self.blink.restart();
        }
        self.typing = typing;
    }

    /// Follows the reply's dots, the wave while it is checked and the
    /// cursor's blink to this frame.
    fn follow_dots(&mut self, cx: &Context<Self>) {
        let verifying = self.phase == Phase::Verifying;
        let count = if self.echo {
            0
        } else if verifying {
            self.sent_dots
        } else {
            self.field.read(cx).text().chars().count()
        };
        if count != self.dots.len() {
            // Typing keeps the cursor lit.
            self.blink.restart();
        }
        self.dots.sync(count, &self.frame.look);
        self.pulse.set(verifying && count > 0, &self.frame.look);
    }

    /// What the empty field says: nothing while it waits for typing, else
    /// why it does not.
    fn wanted_placeholder(&self) -> SharedString {
        if self.phase == Phase::Verifying && self.sent_dots > 0 {
            // The sent reply's dots stand in the field.
            SharedString::default()
        } else if self.locked() {
            say("Temporarily locked")
        } else if self.phase != Phase::Prompting {
            say("Preparing…")
        } else if self.typing {
            SharedString::default()
        } else {
            self.placeholder()
        }
    }
}

impl Render for Prompt {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.follow_keyboard(window, cx);
        self.follow_dots(cx);
        let now = Instant::now();
        motion::drive(
            window,
            self.dots.running(now) || self.pulse.running() || self.shake.running(now),
        );
        let placeholder = self.wanted_placeholder();
        if placeholder != self.shown_placeholder {
            self.shown_placeholder = placeholder.clone();
            self.field
                .update(cx, |field, cx| field.set_placeholder(placeholder, cx));
        }
        let look = &self.frame.look;
        let unavailable = self.phase == Phase::Unavailable;
        let choosing = self.phase == Phase::Choosing && self.identities.len() > 1;
        let identity = self.selected_identity();

        // Two columns: the account on the leading side; the description,
        // reply and actions beside it. The name shares a row with the reply.
        let mut description = div().flex_1().min_w_0().flex().flex_col().child(
            div()
                .text_size(px(14.5))
                .line_height(gpui::relative(1.42))
                .line_clamp(5)
                .child(self.question()),
        );
        if let Some(command) = &self.command {
            description = description.child(
                div()
                    .mt(px(10.))
                    .px(px(12.))
                    .py(px(9.))
                    .rounded(px(10.))
                    .bg(color(look.palette.foreground, 0.07))
                    .when_some(self.frame.mono.clone(), |line, family| {
                        line.font_family(family)
                    })
                    .text_size(px(12.5))
                    .line_height(gpui::relative(1.2))
                    .whitespace_nowrap()
                    .truncate()
                    .child(command.clone()),
            );
        }
        let mut content = div().flex().flex_col().child(
            div()
                .flex()
                .items_center()
                .gap(px(GAP))
                .child(
                    div()
                        .w(px(ACCOUNT_WIDTH))
                        .flex_none()
                        .flex()
                        .justify_center()
                        .child(self.picture(identity, AVATAR)),
                )
                .child(description),
        );
        if !unavailable {
            let reply = if choosing {
                self.identity_list(cx).into_any_element()
            } else {
                self.reply(window, cx).into_any_element()
            };
            content = content.child(
                div()
                    .mt(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(GAP))
                    .child(match identity {
                        Some(identity) => self.account_name(identity),
                        None => div().w(px(ACCOUNT_WIDTH)).flex_none(),
                    })
                    .child(reply),
            );
        }
        let notice = self
            .notice
            .as_ref()
            .filter(|(_, text)| parenthesized(text).is_none());
        if let Some((kind, text)) = notice {
            let ink = match kind {
                NoticeKind::Error => look.palette.error,
                NoticeKind::Info => look.palette.secondary,
            };
            content = content.child(
                div()
                    .pl(px(ACCOUNT_WIDTH + GAP + 4.))
                    .pt(px(10.))
                    .text_size(px(12.5))
                    .line_height(gpui::relative(1.35))
                    .line_clamp(3)
                    .text_color(solid(ink))
                    .child(text.clone()),
            );
        }
        // Verification keeps Confirm legible while inert.
        let held = matches!(self.phase, Phase::Verifying | Phase::Preparing);
        let live = choosing || (self.phase == Phase::Prompting && !self.locked());
        let mut buttons = vec![
            self.frame
                .button(
                    "cancel",
                    if unavailable { "Close" } else { "Cancel" },
                    false,
                    true,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.cancel(cx))),
        ];
        if !unavailable {
            let label = if choosing { "Continue" } else { "Confirm" };
            buttons.push(
                self.frame
                    .button("confirm", label, true, live || held)
                    .when(live, |button| {
                        button.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.confirm(cx)))
                    }),
            );
        }
        let limit = self.time_limit().map(|limit| {
            div()
                .flex_none()
                .max_w(px(220.))
                .px(px(9.))
                .py(px(4.))
                .rounded(px(14.))
                .bg(color(WARNING, 0.16))
                .text_size(px(11.5))
                .line_height(gpui::relative(1.25))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(solid(WARNING))
                .line_clamp(3)
                .child(if self.locked() {
                    format!("{} · {limit}", say("Temporarily locked"))
                } else {
                    limit
                })
        });
        content = content.child(
            div()
                .mt(px(14.))
                .flex()
                .items_center()
                .gap(px(20.))
                .child(div().flex_1().min_w_0().children(limit))
                .child(self.frame.buttons(buttons).pt(px(0.))),
        );
        let card = self.frame.canvas(self.shake.offset(now), content, window);
        card.track_focus(&self.focus)
            .key_context("Dialog")
            .on_action(cx.listener(|this, _: &Cancel, _, cx| this.cancel(cx)))
            .on_action(cx.listener(|this, _: &Confirm, _, cx| this.confirm(cx)))
            .on_modifiers_changed(cx.listener(|_, _, _, cx| cx.notify()))
    }
}

impl Focusable for Prompt {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

/// PAM prompts such as "Password: " read better as a field's placeholder.
fn label(text: &str) -> String {
    let trimmed = text.trim();
    let label = trimmed.strip_suffix(':').unwrap_or(trimmed).trim_end();
    label.to_owned()
}

/// The words inside `text` when all of it is wrapped in parentheses.
fn parenthesized(text: &str) -> Option<&str> {
    let text = text.trim();
    let inner = text.strip_prefix('(')?.strip_suffix(')')?.trim();
    (!inner.is_empty()).then_some(inner)
}

/// Splits a trailing parenthetical about time, such as "(timeout 30s)",
/// from a message or prompt: the text before it, and its words.
fn split_time(text: &str) -> (&str, Option<&str>) {
    let end = text.trim_end();
    let end = end.strip_suffix(':').unwrap_or(end).trim_end();
    let Some(body) = end.strip_suffix(')') else {
        return (text, None);
    };
    let Some(open) = body.rfind('(') else {
        return (text, None);
    };
    let inner = &body[open + 1..];
    let lower = inner.to_lowercase();
    if inner.contains(['(', ')']) || !(lower.contains("time") || lower.contains("expire")) {
        return (text, None);
    }
    (body[..open].trim_end(), Some(inner.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_become_placeholders() {
        assert_eq!(label("Password: "), "Password");
        assert_eq!(label("  PIN:"), "PIN");
        assert_eq!(label(""), "");
    }

    #[test]
    fn time_limits_split_off() {
        assert_eq!(
            split_time("Password (timeout 30s):"),
            ("Password", Some("timeout 30s"))
        );
        assert_eq!(
            split_time("Token (expires in 2 minutes)"),
            ("Token", Some("expires in 2 minutes"))
        );
        assert_eq!(split_time("Password (old)"), ("Password (old)", None));
        assert_eq!(split_time("Password"), ("Password", None));
    }

    #[test]
    fn lockouts_are_parenthesized() {
        assert_eq!(
            parenthesized("(account locked for 10 minutes)"),
            Some("account locked for 10 minutes")
        );
        assert_eq!(parenthesized("Wrong password"), None);
        assert_eq!(parenthesized("()"), None);
    }
}
