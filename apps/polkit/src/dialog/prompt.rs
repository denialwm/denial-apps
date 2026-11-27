//! The authentication prompt: schema 1 of the agent's dialog socket.
//!
//! Deliberately thin. Polkit and the agent own the conversation, the
//! attempts and the outcome; this shows what they say and relays the
//! user's choice of account and replies.

use std::path::PathBuf;

use denial_look::words::fill;
use denial_look_gpui::input::{Submitted, TextInput};
use denial_look_gpui::{color, control_radius, say, solid};
use gpui::{
    ClickEvent, Context, Div, Entity, FocusHandle, Focusable, FontWeight, ObjectFit, SharedString,
    Stateful, Window, div, img, prelude::*, px,
};
use serde_json::{Value, json};

use super::link::Link;
use super::{Cancel, Confirm, Frame, REVEAL_DEADLINE, Receive, monogram};
use crate::identity::{self, Profile};

const AVATAR: f32 = 52.0;
const SMALL_AVATAR: f32 = 32.0;

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
    revealed: bool,
    closing: bool,
}

impl Prompt {
    pub fn new(frame: Frame, link: Link, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = cx.new(|cx| {
            let mut field = TextInput::new(say("Password"), &frame.look, 1, cx);
            field.set_secret(true, cx);
            field
        });
        cx.subscribe(&field, |this, _, _: &Submitted, cx| this.confirm(cx))
            .detach();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REVEAL_DEADLINE).await;
            let _ = this.update(cx, |this, cx| {
                this.revealed = true;
                cx.notify();
            });
        })
        .detach();
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
            revealed: false,
            closing: false,
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
        let sent = self.link.respond(prompt, self.field.read(cx).text());
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
                self.revealed = true;
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
                self.label = label(&text("text"));
                self.echo = message["echo"].as_bool().unwrap_or(false);
                self.awaiting_verdict = false;
                self.phase = Phase::Prompting;
                let (echo, placeholder) = (self.echo, self.placeholder());
                self.field.update(cx, |field, cx| {
                    field.clear(cx);
                    field.set_placeholder(placeholder, cx);
                    field.set_secret(!echo, cx);
                });
                if !self.locked() {
                    window.focus(&self.field.focus_handle(cx), cx);
                }
            }
            Some("info") => self.notice = Some((NoticeKind::Info, text("text"))),
            Some("error") => {
                self.notice = Some((NoticeKind::Error, text("text")));
                // PAM may say several things about one rejected reply.
                self.awaiting_verdict = false;
            }
            Some("failed") => {
                // The attempts are used up; the agent closes the dialog soon.
                self.awaiting_verdict = false;
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

    fn account(&self, identity: &Identity) -> Div {
        let look = &self.frame.look;
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(self.picture(identity, AVATAR))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(identity.profile.display_name.clone()),
                    )
                    .when(identity.profile.display_name != identity.name, |column| {
                        column.child(
                            div()
                                .text_size(px(13.))
                                .text_color(solid(look.palette.secondary))
                                .truncate()
                                .child(identity.name.clone()),
                        )
                    }),
            )
    }

    fn picture(&self, identity: &Identity, diameter: f32) -> gpui::AnyElement {
        let name = identity.profile.display_name.clone();
        match &identity.profile.picture {
            Some(path) => img(PathBuf::from(path))
                .size(px(diameter))
                .flex_none()
                .rounded_full()
                .object_fit(ObjectFit::Cover)
                .with_fallback(move || monogram(&name, diameter).into_any_element())
                .into_any_element(),
            None => monogram(&name, diameter).into_any_element(),
        }
    }

    fn identity_list(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let look = &self.frame.look;
        let rows = self.identities.iter().map(|identity| {
            let uid = identity.uid;
            let selected = self.selected == Some(uid);
            div()
                .id(("identity", uid as usize))
                .h(px(self.frame.control_height().max(SMALL_AVATAR + 12.)))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(12.))
                .rounded(control_radius(look))
                .cursor_pointer()
                .when(selected, |row| {
                    row.bg(color(look.palette.foreground, denial_look::CONTROL_FILL))
                })
                .active(|style| style.bg(color(look.palette.foreground, denial_look::PRESSED_FILL)))
                .child(self.picture(identity, SMALL_AVATAR))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(identity.profile.display_name.clone()),
                )
                .when(selected, |row| {
                    row.child(div().size(px(10.)).rounded_full().bg(solid(look.accent)))
                })
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    this.choose(uid, cx);
                    // A second tap on the picked account goes on with it.
                    if event.click_count() > 1 {
                        this.start(cx);
                    }
                }))
        });
        div()
            .id("identities")
            .flex()
            .flex_col()
            .gap(px(2.))
            .max_h(px(220.))
            .overflow_y_scroll()
            .children(rows)
    }

    fn reply(&self, window: &Window) -> Div {
        let look = &self.frame.look;
        let locked = self.locked();
        let error = self
            .notice
            .as_ref()
            .is_some_and(|(kind, _)| *kind == NoticeKind::Error);
        let caps = !self.echo && window.capslock().on;
        div()
            .h(px(self.frame.control_height().max(44.)))
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(14.))
            .rounded(control_radius(look))
            .bg(color(look.palette.foreground, denial_look::CONTROL_FILL))
            .when(error, |field| {
                field.border(px(1.)).border_color(solid(look.palette.error))
            })
            .when(locked || self.phase != Phase::Prompting, |field| {
                field.opacity(0.6)
            })
            .child(div().flex_1().min_w_0().child(self.field.clone()))
            .when(caps, |field| {
                field.child(
                    div()
                        .flex_none()
                        .px(px(8.))
                        .py(px(2.))
                        .rounded_full()
                        .bg(color(look.palette.foreground, denial_look::CONTROL_FILL))
                        .text_size(px(12.))
                        .text_color(solid(look.palette.secondary))
                        .child(say("Caps Lock")),
                )
            })
    }
}

impl Render for Prompt {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = &self.frame.look;
        let unavailable = self.phase == Phase::Unavailable;
        let choosing = self.phase == Phase::Choosing && self.identities.len() > 1;
        let mut content = div().flex().flex_col().gap(px(12.));
        if let Some(identity) = self.selected_identity().filter(|_| !choosing) {
            content = content.child(self.account(identity));
        }
        content = content.child(
            div()
                .text_size(px(15.))
                .line_height(gpui::relative(1.42))
                .line_clamp(5)
                .child(self.question()),
        );
        if let Some(command) = &self.command {
            content = content.child(
                div()
                    .px(px(12.))
                    .py(px(9.))
                    .rounded(control_radius(look))
                    .bg(color(look.palette.foreground, 0.07))
                    .text_size(px(12.5))
                    .truncate()
                    .child(command.clone()),
            );
        }
        if choosing {
            content = content.child(self.identity_list(cx));
        } else if !unavailable {
            content = content.child(self.reply(window));
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
                    .text_size(px(13.))
                    .line_height(gpui::relative(1.35))
                    .line_clamp(3)
                    .text_color(solid(ink))
                    .child(text.clone()),
            );
        }
        let busy = match self.phase {
            Phase::Preparing => Some(say("Preparing…")),
            Phase::Verifying => Some(say("Verifying…")),
            _ => None,
        };
        let confirm_label = busy.unwrap_or_else(|| {
            if choosing {
                say("Continue")
            } else {
                say("Confirm")
            }
        });
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
            buttons.push(
                self.frame
                    .button("confirm", confirm_label, true, live)
                    .when(live, |button| {
                        button.on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.confirm(cx)))
                    }),
            );
        }
        let limit = self.time_limit().map(|limit| {
            let locked = self.locked();
            div()
                .flex_none()
                .max_w(px(220.))
                .px(px(9.))
                .py(px(4.))
                .rounded_full()
                .bg(color(look.palette.error, 0.16))
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(solid(look.palette.error))
                .line_clamp(2)
                .child(if locked {
                    format!("{} · {limit}", say("Temporarily locked"))
                } else {
                    limit
                })
        });
        content = content.child(match limit {
            Some(limit) if !self.frame.phone => div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(div().flex_1().min_w_0().child(limit))
                .child(self.frame.buttons(buttons)),
            Some(limit) => div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(limit)
                .child(self.frame.buttons(buttons)),
            None => self.frame.buttons(buttons),
        });
        let card = self.frame.canvas(self.revealed, content, window);
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
