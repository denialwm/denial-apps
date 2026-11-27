//! denial-portal's Access request: schema 1 of its dialog socket. The
//! caller decides what the answer means and keeps it; this shows the
//! question and relays the answer.

use denial_look::{CONTROL_FILL, floating};
use denial_look_gpui::{color, say, solid};
use gpui::{
    ClickEvent, Context, Div, FocusHandle, Focusable, FontWeight, ObjectFit, SharedString, Window,
    div, img, prelude::*, px,
};
use serde_json::{Map, Value, json};

use super::link::Link;
use super::{Cancel, Confirm, Frame, REVEAL_DEADLINE, Receive, monogram};
use crate::apps;

/// The choice that makes an answer "always" or "never".
const REMEMBER: &str = "remember";
const ICON: f32 = 56.0;

/// One of the caller's choices. Without options it is a boolean, "true" or
/// "false".
struct Choice {
    id: String,
    label: String,
    options: Vec<(String, String)>,
    value: String,
}

impl Choice {
    fn boolean(&self) -> bool {
        self.options.is_empty()
    }
}

pub struct Access {
    frame: Frame,
    link: Link,
    focus: FocusHandle,
    asking: bool,
    app_id: String,
    app: Option<apps::App>,
    title: String,
    subtitle: String,
    body: String,
    grant_label: String,
    deny_label: String,
    choices: Vec<Choice>,
    answered: bool,
    revealed: bool,
}

impl Access {
    pub fn new(frame: Frame, link: Link, window: &mut Window, cx: &mut Context<Self>) -> Self {
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
            asking: false,
            app_id: String::new(),
            app: None,
            title: String::new(),
            subtitle: String::new(),
            body: String::new(),
            grant_label: String::new(),
            deny_label: String::new(),
            choices: Vec::new(),
            answered: false,
            revealed: false,
        }
    }

    /// Once-or-always buttons for both answers, in place of a switch.
    fn remembers(&self) -> bool {
        self.choices
            .iter()
            .any(|choice| choice.id == REMEMBER && choice.boolean())
    }

    fn answer(&mut self, grant: bool, always: bool, cx: &mut Context<Self>) {
        if self.answered {
            return;
        }
        // Declining needs no request: without one there is nothing to tell.
        if !self.asking && !grant {
            return cx.quit();
        }
        if !self.asking {
            return;
        }
        self.answered = true;
        let remembers = self.remembers();
        let choices: Map<String, Value> = self
            .choices
            .iter()
            .map(|choice| {
                let value = if choice.id == REMEMBER && remembers {
                    always.to_string()
                } else {
                    choice.value.clone()
                };
                (choice.id.clone(), Value::String(value))
            })
            .collect();
        let message = json!({"type": "answer", "grant": grant, "choices": choices});
        // The portal ends this process once it has the answer.
        if self.link.send(&message).is_err() {
            cx.quit();
        }
        cx.notify();
    }

    fn choose(&mut self, index: usize, value: String, cx: &mut Context<Self>) {
        if let Some(choice) = self.choices.get_mut(index) {
            choice.value = value;
            cx.notify();
        }
    }

    fn app_identity(&self) -> Div {
        let name = self
            .app
            .as_ref()
            .map(|app| app.name.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| {
                if self.app_id.is_empty() {
                    say("An app").to_string()
                } else {
                    self.app_id.clone()
                }
            });
        let icon = match self.app.as_ref().and_then(|app| app.icon.clone()) {
            Some(path) => {
                let fallback = name.clone();
                img(path)
                    .size(px(ICON))
                    .flex_none()
                    .object_fit(ObjectFit::Contain)
                    .with_fallback(move || monogram(&fallback, ICON).into_any_element())
                    .into_any_element()
            }
            None => monogram(&name, ICON).into_any_element(),
        };
        div().flex().items_center().gap(px(14.)).child(icon).child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .min_w_0()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(solid(self.frame.look.palette.secondary))
                        .truncate()
                        .child(name),
                )
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(gpui::relative(1.32))
                        .font_weight(FontWeight::SEMIBOLD)
                        .line_clamp(3)
                        .child(self.title.clone()),
                ),
        )
    }

    fn choice(&self, index: usize, choice: &Choice, cx: &mut Context<Self>) -> Div {
        let look = &self.frame.look;
        let label = div()
            .flex_1()
            .min_w_0()
            .text_size(px(14.))
            .child(choice.label.clone());
        if choice.boolean() {
            let on = choice.value == "true";
            let track = div()
                .id(("switch", index))
                .flex_none()
                .w(px(46.))
                .h(px(28.))
                .p(px(3.))
                .rounded_full()
                .cursor_pointer()
                .flex()
                .when(on, |track| track.justify_end())
                .bg(if on {
                    solid(look.accent)
                } else {
                    color(look.palette.foreground, CONTROL_FILL * 2.)
                })
                .border(px(floating::EDGE_WIDTH))
                .border_color(color(look.palette.foreground, look.palette.edge_alpha))
                .child(div().size(px(22.)).rounded_full().bg(solid(if on {
                    look.on_accent
                } else {
                    look.palette.foreground
                })))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.choose(index, (!on).to_string(), cx)
                }));
            return div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(label)
                .child(track);
        }
        let options = choice
            .options
            .iter()
            .enumerate()
            .map(|(option, (id, text))| {
                let picked = *id == choice.value;
                let id = id.clone();
                self.frame
                    .button(("option", index * 64 + option), text.clone(), picked, true)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.choose(index, id.clone(), cx)
                    }))
            });
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(label)
            .child(div().flex().flex_wrap().gap(px(8.)).children(options))
    }
}

impl Receive for Access {
    fn receive(&mut self, message: Value, _: &mut Window, cx: &mut Context<Self>) {
        if message["type"] != "request" {
            return;
        }
        let text = |key: &str| message[key].as_str().unwrap_or_default().to_owned();
        self.app_id = text("appId");
        self.app = apps::find(&self.app_id);
        self.title = text("title");
        self.subtitle = text("subtitle");
        self.body = text("body");
        self.grant_label = text("grantLabel");
        self.deny_label = text("denyLabel");
        self.choices = message["choices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|choice| {
                Some(Choice {
                    id: choice["id"].as_str()?.to_owned(),
                    label: choice["label"].as_str()?.to_owned(),
                    value: choice["initial"].as_str()?.to_owned(),
                    options: choice["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|option| {
                            Some((
                                option["id"].as_str()?.to_owned(),
                                option["label"].as_str()?.to_owned(),
                            ))
                        })
                        .collect(),
                })
            })
            .collect();
        self.asking = true;
        self.revealed = true;
        cx.notify();
    }
}

impl Render for Access {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = &self.frame.look;
        let asking = self.asking && !self.answered;
        let remembers = self.remembers();
        let mut content = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(self.app_identity());
        if !self.subtitle.is_empty() {
            content = content.child(
                div()
                    .text_size(px(14.))
                    .line_height(gpui::relative(1.42))
                    .line_clamp(4)
                    .child(self.subtitle.clone()),
            );
        }
        if !self.body.is_empty() {
            content = content.child(
                div()
                    .text_size(px(13.))
                    .line_height(gpui::relative(1.4))
                    .line_clamp(6)
                    .text_color(solid(look.palette.secondary))
                    .child(self.body.clone()),
            );
        }
        for (index, choice) in self.choices.iter().enumerate() {
            if remembers && choice.id == REMEMBER {
                continue;
            }
            content = content.child(self.choice(index, choice, cx));
        }
        let button =
            |frame: &Frame, id: &'static str, label: SharedString, primary: bool, live: bool| {
                frame.button(id, label, primary, live)
            };
        if remembers {
            // "Allow once" is the colored answer: allowing for good is there,
            // but takes a deliberate choice.
            let rows = [
                [
                    button(&self.frame, "always", say("Always allow"), false, asking).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.answer(true, true, cx)),
                    ),
                    button(&self.frame, "once", say("Allow once"), true, asking).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.answer(true, false, cx)),
                    ),
                ],
                [
                    button(&self.frame, "never", say("Never allow"), false, asking).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.answer(false, true, cx)),
                    ),
                    button(&self.frame, "decline", say("Decline once"), false, true).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.answer(false, false, cx)),
                    ),
                ],
            ];
            let mut answers = div().flex().flex_col().gap(px(10.)).pt(px(8.));
            for row in rows {
                answers = answers.child(
                    div()
                        .flex()
                        .gap(px(10.))
                        .children(row.into_iter().map(|button| button.flex_1().min_w_0())),
                );
            }
            content = content.child(answers);
        } else {
            let deny = if self.deny_label.is_empty() {
                say("Decline")
            } else {
                self.deny_label.clone().into()
            };
            let grant = if self.grant_label.is_empty() {
                say("Allow")
            } else {
                self.grant_label.clone().into()
            };
            content = content.child(self.frame.buttons([
                button(&self.frame, "deny", deny, false, true).on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| this.answer(false, false, cx)),
                ),
                button(&self.frame, "grant", grant, true, asking).on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| this.answer(true, false, cx)),
                ),
            ]));
        }
        self.frame
            .canvas(self.revealed, 0.0, content, window)
            .track_focus(&self.focus)
            .key_context("Dialog")
            .on_action(cx.listener(|this, _: &Cancel, _, cx| this.answer(false, false, cx)))
            // Enter never grants: allowing takes a deliberate press.
            .on_action(cx.listener(|_, _: &Confirm, _, _| {}))
    }
}

impl Focusable for Access {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}
