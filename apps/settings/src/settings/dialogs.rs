//! Dialogs on the gesture zone, rising in over the dimmed screen and riding
//! on the keyboard while it is up: picking one option, confirming what
//! cannot be undone, and the passwords and names Wi-Fi asks for.

use denial_look::words::fill;
use std::time::Instant;

use denial_look::mobile::TOUCH;
use denial_look::{Tween, decelerate, floating, motion};
use denial_look_gpui::input::TextInput;
use denial_look_gpui::{color, control_radius, dp, island, say, solid};
use gpui::{
    Context, Div, Entity, Focusable, FontWeight, SharedString, Stateful, Window, div, prelude::*,
    px,
};

use super::bound::PinRequest;
use super::panel::hertz;
use super::parts::{Fill, ROW_GAP};
use super::{Dialog, Settings};
use crate::bluetooth::Ask;
use crate::denial;
use crate::lock;
use crate::wifi::{self, Security};

/// A SIM PIN: 4 to 8 digits.
fn pin(typed: &str) -> bool {
    (4..=8).contains(&typed.len()) && typed.bytes().all(|b| b.is_ascii_digit())
}

/// The shortest password each kind of network takes.
fn shortest(security: Security) -> usize {
    match security {
        Security::Wep => 5,
        _ => 8,
    }
}

impl Settings {
    pub(super) fn dialog_island(
        &self,
        dialog: &Dialog,
        shown: Tween,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let appear = decelerate(shown.segment(0, motion::DIM).progress(now));
        // Over the blur, as glass groups are: the page shows through
        // smoke islands, and would read through a dialog.
        let island = island(&self.look)
            .backdrop_blur(px(floating::BACKDROP_BLUR))
            .id("dialog")
            .relative()
            .top(px(motion::BOTTOM_TRAVEL * (1.0 - appear)))
            .opacity(appear)
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation());
        match dialog {
            Dialog::Pick {
                title,
                options,
                picked,
                ..
            } => {
                let rows = options.iter().enumerate().map(|(ix, option)| {
                    let row = div()
                        .id(ix)
                        .h(dp(TOUCH))
                        .flex()
                        .items_center()
                        .gap(dp(ROW_GAP))
                        .px(dp(12.))
                        .rounded(control_radius(&self.look))
                        .child(self.radio(ix == *picked))
                        .child(say(option.clone()));
                    self.pressable(row, cx, move |this, window, cx| this.pick(ix, window, cx))
                });
                island
                    .p(dp(6.))
                    .child(
                        self.dialog_title(title.clone())
                            .px(dp(12.))
                            .pt(dp(12.))
                            .pb(dp(8.)),
                    )
                    .children(rows)
            }
            Dialog::Confirm {
                title,
                consequence,
                action,
                ..
            } => self
                .form(island)
                .child(self.dialog_title(title.clone()))
                .child(self.said(consequence.clone(), false))
                .child(self.answers(action, Fill::Destructive, true, cx)),
            Dialog::Password { network, error } => {
                let typed = self.password.read(cx).text().chars().count();
                let said: SharedString = match error {
                    Some(error) => (*error).into(),
                    None => format!(
                        "{} · {}",
                        say(network.security.label()),
                        say("Enter the password")
                    )
                    .into(),
                };
                self.form(island)
                    .child(self.dialog_title(network.name.clone()))
                    .child(self.said(said, error.is_some()))
                    .child(self.dialog_field(&self.password, true, cx))
                    .child(self.answers(
                        "Join",
                        Fill::Accent,
                        typed >= shortest(network.security),
                        cx,
                    ))
            }
            Dialog::Pin { error } => {
                let tries = super::cellular::tries(self.mobile.state.pin_retries);
                let said: SharedString = match error {
                    Some(error) => format!("{} · {tries}", say(*error)).into(),
                    None => tries.into(),
                };
                let ready = pin(self.pin.read(cx).text());
                self.form(island)
                    .child(self.dialog_title("Unlock the SIM"))
                    .child(self.said(said, error.is_some()))
                    .child(self.dialog_field(&self.pin, true, cx))
                    .child(self.answers("Unlock", Fill::Accent, ready, cx))
            }
            Dialog::Pairing { name, request } => {
                let (said, code, ready, action) = self.pairing_dialog(name, request, cx);
                let typed = matches!(request.ask, Ask::PinCode | Ask::Passkey);
                let answers = match action {
                    Some(action) => self.answers(action, Fill::Accent, ready, cx),
                    None => self
                        .buttons([self.button(
                            "cancel",
                            "Cancel",
                            Fill::Control,
                            true,
                            cx,
                            |this, window, cx| this.dismiss(window, cx),
                        )])
                        .pt(dp(9.)),
                };
                self.form(island)
                    .child(self.dialog_title(fill("Pair with {name}", &[("name", name)])))
                    .child(self.said(said, false))
                    .children(code)
                    .when(typed, |form| {
                        form.child(self.dialog_field(&self.code_field, false, cx))
                    })
                    .child(answers)
            }
            Dialog::Keep { rate, back, .. } => self
                .form(island)
                .child(self.dialog_title(fill("Keep {rate}?", &[("rate", &hertz(*rate))])))
                .child(self.said(
                    fill(
                        "Goes back to {rate} in {count} seconds",
                        &[
                            ("rate", &hertz(*back)),
                            ("count", &denial::KEEP.as_secs().to_string()),
                        ],
                    ),
                    false,
                ))
                .child(self.answers("Keep", Fill::Accent, true, cx)),
            Dialog::LockPin { error, fingers } => {
                let ready = !self.password.read(cx).text().is_empty() && self.new_pin_typed(cx);
                let said = error.unwrap_or(if *fingers {
                    "Fingerprints need a PIN. Your account password, then 6 digits twice"
                } else {
                    "Your account password, then 6 digits twice"
                });
                self.form(island)
                    .child(self.dialog_title("Set a PIN"))
                    .child(self.said(said, error.is_some()))
                    .child(self.dialog_field(&self.password, true, cx))
                    .child(self.dialog_field(&self.new_pin, false, cx))
                    .child(self.dialog_field(&self.pin_again, false, cx))
                    .child(self.answers("Set", Fill::Accent, ready && !self.lock.busy, cx))
            }
            Dialog::ChangePin { error } => {
                let ready =
                    lock::may_be_pin(self.current_pin.read(cx).text()) && self.new_pin_typed(cx);
                let said = error.unwrap_or("The PIN set now, then 6 new digits twice");
                self.form(island)
                    .child(self.dialog_title("Change the PIN"))
                    .child(self.said(said, error.is_some()))
                    .child(self.dialog_field(&self.current_pin, true, cx))
                    .child(self.dialog_field(&self.new_pin, false, cx))
                    .child(self.dialog_field(&self.pin_again, false, cx))
                    .child(self.answers("Change", Fill::Accent, ready && !self.lock.busy, cx))
            }
            Dialog::CheckPin { error } => {
                let ready = lock::may_be_pin(self.current_pin.read(cx).text());
                let said = error.unwrap_or("Fingerprints change only after the PIN");
                self.form(island)
                    .child(self.dialog_title("Enter the PIN"))
                    .child(self.said(said, error.is_some()))
                    .child(self.dialog_field(&self.current_pin, true, cx))
                    .child(self.answers("Next", Fill::Accent, ready && !self.lock.busy, cx))
            }
            Dialog::ClearPin { error } => {
                let ready = !self.password.read(cx).text().is_empty();
                let fingers = self
                    .fingers
                    .enrolled
                    .as_ref()
                    .is_some_and(|fingers| !fingers.is_empty());
                let said = error.unwrap_or(if fingers {
                    "Your account password. Fingerprints stop unlocking until a PIN is set"
                } else {
                    "Your account password; the lock screen then asks for it"
                });
                self.form(island)
                    .child(self.dialog_title("Use the password"))
                    .child(self.said(said, error.is_some()))
                    .child(self.dialog_field(&self.password, true, cx))
                    .child(self.answers("Remove PIN", Fill::Accent, ready && !self.lock.busy, cx))
            }
            Dialog::Rename => {
                let named = !self.name_field.read(cx).text().trim().is_empty();
                self.form(island)
                    .child(self.dialog_title("Phone name"))
                    .child(self.said("What nearby devices see", false))
                    .child(self.dialog_field(&self.name_field, false, cx))
                    .child(self.answers("Rename", Fill::Accent, named, cx))
            }
            Dialog::Clock { date, error } => {
                let typed = !self.clock_field.read(cx).text().trim().is_empty();
                let said = match (error, date) {
                    (Some(error), _) => *error,
                    (None, true) => "Year, month and day: 2026-10-22",
                    (None, false) => "Hours and minutes: 17:45",
                };
                self.form(island)
                    .child(self.dialog_title(if *date { "Date" } else { "Time" }))
                    .child(self.said(said, error.is_some()))
                    .child(self.dialog_field(&self.clock_field, false, cx))
                    .child(self.answers("Set", Fill::Accent, typed, cx))
            }
            Dialog::Finger { finger } => {
                let named = !self.finger_field.read(cx).text().trim().is_empty();
                let form = self
                    .form(island)
                    .child(self.dialog_title(if finger.is_some() {
                        "Fingerprint"
                    } else {
                        "Add a fingerprint"
                    }))
                    .child(self.said("A name to tell it from the others", false))
                    .child(self.dialog_field(&self.finger_field, false, cx));
                match finger.clone() {
                    None => form.child(self.answers("Next", Fill::Accent, named, cx)),
                    Some(finger) => form.child(
                        self.buttons([
                            self.button(
                                "remove",
                                "Remove",
                                Fill::Danger,
                                true,
                                cx,
                                move |this, window, cx| {
                                    this.dismiss(window, cx);
                                    this.ask_remove_finger(finger.clone(), cx);
                                },
                            ),
                            self.button(
                                "answer",
                                "Rename",
                                Fill::Accent,
                                named,
                                cx,
                                |this, window, cx| this.submit_dialog(window, cx),
                            ),
                        ])
                        .pt(dp(9.)),
                    ),
                }
            }
            Dialog::Hidden { error } => {
                let named = !self.hidden_name.read(cx).text().trim().is_empty();
                let typed = self.password.read(cx).text().chars().count();
                let said = error.unwrap_or("Leave the password empty for an open network");
                self.form(island)
                    .child(self.dialog_title("Add a hidden network"))
                    .child(self.said(said, error.is_some()))
                    .child(self.dialog_field(&self.hidden_name, false, cx))
                    .child(self.dialog_field(&self.password, true, cx))
                    .child(self.answers(
                        "Join",
                        Fill::Accent,
                        named && (typed == 0 || typed >= shortest(Security::Personal)),
                        cx,
                    ))
            }
        }
    }

    /// A dialog of text, fields and answers.
    fn form(&self, island: Stateful<Div>) -> Stateful<Div> {
        island.gap(dp(7.)).px(dp(20.)).pt(dp(21.)).pb(dp(16.))
    }

    fn dialog_title(&self, title: impl Into<SharedString>) -> Div {
        div()
            .text_size(dp(20.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(say(title))
    }

    /// What a dialog says under its title; in the error colour when it says
    /// what went wrong.
    fn said(&self, said: impl Into<SharedString>, wrong: bool) -> Div {
        let ink = if wrong {
            self.look.palette.error
        } else {
            self.look.palette.secondary
        };
        div().text_color(solid(ink)).child(say(said))
    }

    /// A text field in a dialog. A secret one ends in a button that shows
    /// what is typed, or hides it again.
    fn dialog_field(&self, input: &Entity<TextInput>, secret: bool, cx: &mut Context<Self>) -> Div {
        let reveal = secret.then(|| {
            let label = if self.revealed { "Hide" } else { "Show" };
            let button = div()
                .id("reveal")
                .flex_none()
                .h(dp(TOUCH))
                .px(dp(12.))
                .flex()
                .items_center()
                .rounded(control_radius(&self.look))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(solid(self.look.accent_text))
                .child(say(label));
            let input = input.clone();
            self.pressable(button, cx, move |this, _, cx| {
                this.revealed = !this.revealed;
                let revealed = this.revealed;
                input.update(cx, |input, cx| input.set_secret(!revealed, cx));
                cx.notify();
            })
        });
        div()
            .mt(dp(9.))
            .h(dp(TOUCH))
            .flex()
            .items_center()
            .pl(dp(14.))
            .pr(dp(if secret { 0. } else { 14. }))
            .rounded(control_radius(&self.look))
            .bg(color(
                self.look.palette.foreground,
                denial_look::CONTROL_FILL,
            ))
            .child(div().flex_1().min_w_0().child(input.clone()))
            .children(reveal)
    }

    /// Cancel, and the button that does what the dialog is for.
    fn answers(
        &self,
        action: &'static str,
        fill: Fill,
        ready: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        self.buttons([
            self.button(
                "cancel",
                "Cancel",
                Fill::Control,
                true,
                cx,
                |this, window, cx| this.dismiss(window, cx),
            ),
            self.button("answer", action, fill, ready, cx, |this, window, cx| {
                this.submit_dialog(window, cx)
            }),
        ])
        .pt(dp(9.))
    }

    /// A radio mark: a ring, filled at its centre when `on`.
    pub(super) fn radio(&self, on: bool) -> Div {
        let ink = if on {
            self.look.accent
        } else {
            self.look.palette.secondary
        };
        div()
            .flex_none()
            .size(dp(22.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border(dp(2.))
            .border_color(solid(ink))
            .when(on, |radio| {
                radio.child(div().size(dp(10.)).rounded_full().bg(solid(ink)))
            })
    }

    /// Picks option `option` of the pick dialog, which closes.
    fn pick(&mut self, option: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((Dialog::Pick { pick, .. }, _)) = self.dialog.take() {
            window.focus(&self.focus_handle, cx);
            pick(self, option, window, cx);
            cx.notify();
        }
    }

    /// The dialog's own button, or Enter in its field: does what the dialog
    /// is for, once what it asks for is there.
    pub(super) fn submit_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((dialog, _)) = self.dialog.clone() else {
            return;
        };
        let password = self.password.read(cx).text().to_owned();
        match dialog {
            Dialog::Pick { .. } => {}
            Dialog::Confirm { confirm, .. } => {
                self.dismiss(window, cx);
                confirm(self, window, cx);
            }
            Dialog::Password { network, .. } => {
                if password.chars().count() >= shortest(network.security) {
                    self.join(network, Some(password), window, cx);
                }
            }
            Dialog::Pin { .. } => {
                let typed = self.pin.read(cx).text().to_owned();
                if pin(&typed) {
                    self.unlock(typed, window, cx);
                }
            }
            Dialog::Pairing { .. } => {
                let typed = self.code_field.read(cx).text().trim().to_owned();
                self.answer_pairing(Some(typed), window, cx);
            }
            Dialog::Keep { token, .. } => self.answer_keep(token, true, window, cx),
            Dialog::LockPin { fingers, .. } => {
                if password.is_empty() {
                    return;
                }
                match self.new_pin(cx) {
                    Ok(Some(pin)) => self.send_pin(
                        PinRequest::Set {
                            password,
                            pin,
                            fingers,
                        },
                        window,
                        cx,
                    ),
                    Ok(None) => {}
                    Err(differ) => self.ask_lock_pin(Some(differ), fingers, window, cx),
                }
            }
            Dialog::ChangePin { .. } => {
                let current = self.current_pin.read(cx).text().to_owned();
                if !lock::may_be_pin(&current) {
                    return;
                }
                match self.new_pin(cx) {
                    Ok(Some(pin)) => self.send_pin(PinRequest::Change { current, pin }, window, cx),
                    Ok(None) => {}
                    Err(differ) => self.ask_change_pin(Some(differ), window, cx),
                }
            }
            Dialog::CheckPin { .. } => {
                let pin = self.current_pin.read(cx).text().to_owned();
                if lock::may_be_pin(&pin) {
                    self.open_session(pin, window, cx);
                }
            }
            Dialog::ClearPin { .. } => {
                if !password.is_empty() {
                    self.send_pin(PinRequest::Clear { password }, window, cx);
                }
            }
            Dialog::Rename => {
                let name = self.name_field.read(cx).text().trim().to_owned();
                if !name.is_empty() {
                    self.rename(name, window, cx);
                }
            }
            Dialog::Clock { date, .. } => self.set_clock(date, window, cx),
            Dialog::Finger { finger } => {
                let name = self.finger_field.read(cx).text().trim().to_owned();
                if !name.is_empty() {
                    self.dismiss(window, cx);
                    self.name_finger(finger, name, cx);
                }
            }
            Dialog::Hidden { .. } => {
                let name = self.hidden_name.read(cx).text().trim().to_owned();
                let typed = password.chars().count();
                if !name.is_empty() && (typed == 0 || typed >= shortest(Security::Personal)) {
                    let password = (typed > 0).then_some(password);
                    self.join_hidden(name, password, window, cx);
                }
            }
        }
    }

    /// Asks for `network`'s password, saying `error` when asked again.
    pub(super) fn ask_password(
        &mut self,
        network: wifi::Network,
        error: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_secrets(cx);
        self.show(Dialog::Password { network, error }, cx);
        window.focus(&self.password.focus_handle(cx), cx);
    }

    /// Asks for the SIM's PIN, saying `error` when asked again.
    pub(super) fn ask_pin(
        &mut self,
        error: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_secrets(cx);
        self.show(Dialog::Pin { error }, cx);
        window.focus(&self.pin.focus_handle(cx), cx);
    }

    /// Asks for a hidden network's name and password; after a failure,
    /// keeps the name and says `error`.
    pub(super) fn ask_hidden(
        &mut self,
        error: Option<&'static str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_secrets(cx);
        if error.is_none() {
            self.hidden_name.update(cx, |input, cx| input.clear(cx));
        }
        self.show(Dialog::Hidden { error }, cx);
        window.focus(&self.hidden_name.focus_handle(cx), cx);
    }

    /// Both new PIN fields hold 6 digits.
    fn new_pin_typed(&self, cx: &Context<Self>) -> bool {
        lock::valid_pin(self.new_pin.read(cx).text())
            && lock::valid_pin(self.pin_again.read(cx).text())
    }

    /// The new PIN once typed the same twice; None while it is not typed
    /// yet, and why it is asked again when the two differ.
    fn new_pin(&self, cx: &Context<Self>) -> Result<Option<String>, &'static str> {
        let (pin, again) = (self.new_pin.read(cx).text(), self.pin_again.read(cx).text());
        if !self.new_pin_typed(cx) {
            Ok(None)
        } else if pin != again {
            Err("The two new PINs differ")
        } else {
            Ok(Some(pin.to_owned()))
        }
    }

    /// Empties the password and PIN fields and hides what they hold.
    pub(super) fn clear_secrets(&mut self, cx: &mut Context<Self>) {
        self.revealed = false;
        for field in [
            &self.password,
            &self.pin,
            &self.current_pin,
            &self.new_pin,
            &self.pin_again,
        ] {
            field.update(cx, |input, cx| {
                input.clear(cx);
                input.set_secret(true, cx);
            });
        }
    }
}
