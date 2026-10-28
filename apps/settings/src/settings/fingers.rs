//! The fingerprint screen: the switch that lets fingerprints unlock the
//! phone, then the fingerprints on the reader by the names people gave
//! them, a tap renaming or removing one, and a row to add another, named
//! first. While one is added the screen holds only its progress, at the
//! top, leaving the sensor near the bottom clear.
//!
//! Adding and removing go through Denial's fingerprint session, which the
//! PIN opened; once it ends, the next change asks for the PIN again.

use denial_look::words::fill;
use denial_look_gpui::say;
use std::rc::Rc;

use denial_look_gpui::{Icon, dp, island, solid};
use gpui::{
    AppContext, Context, Div, Focusable, FontWeight, SharedString, Window, div, prelude::*, px,
};

use super::parts::{Fill, ROW_PAD};
use super::{Adding, Dialog, Layer, Settings};
use crate::catalog::FINGERPRINT_UNLOCK;
use crate::fingerprint::{self, Event, Progress, SLOTS, Session};

impl Settings {
    pub(super) fn fingers_screen(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        if let Some(adding) = &self.fingers.adding {
            return vec![self.adding_island(adding, layer, cx)];
        }
        let mut parts = vec![
            island(&self.look).child(self.setting(&FINGERPRINT_UNLOCK, true, true, layer, cx)),
            self.chip_row("Fingerprints"),
        ];
        let Some(enrolled) = &self.fingers.enrolled else {
            let said = if self.fingers.read {
                "No fingerprint reader"
            } else {
                "Reading the reader…"
            };
            parts.push(island(&self.look).child(self.row(
                "none",
                Some(Icon::Info),
                said,
                None,
                (true, true),
            )));
            return parts;
        };
        let full = SLOTS
            .iter()
            .all(|slot| enrolled.iter().any(|each| each == slot));
        let rows = enrolled
            .iter()
            .map(|finger| Some(finger.clone()))
            .chain((!full).then_some(None));
        let rows: Vec<Option<String>> = rows.collect();
        let live = layer.live() && self.fingers.removing.is_none();
        parts.push(
            self.rows_island(rows.into_iter(), 0., |finger, ends| match finger {
                Some(finger) => {
                    let removing = self.fingers.removing.as_deref() == Some(finger.as_str());
                    let row = self
                        .row(
                            SharedString::from(finger.clone()),
                            Some(Icon::Fingerprint),
                            self.finger_name(&finger),
                            None,
                            ends,
                        )
                        .when(removing, |row| row.child(self.value("Removing…")))
                        .when(!removing, |row| row.child(self.chevron()));
                    self.live(row, live, cx, move |this, window, cx| {
                        this.ask_finger(Some(finger.clone()), window, cx)
                    })
                }
                None => {
                    let row = self
                        .row("add", Some(Icon::New), "Add a fingerprint", None, ends)
                        .text_color(solid(self.look.accent_text));
                    self.live(row, live, cx, |this, window, cx| {
                        this.ask_finger(None, window, cx)
                    })
                }
            }),
        );
        parts
    }

    /// What a fingerprint is called: the name given to it, or its place on
    /// the list.
    pub(super) fn finger_name(&self, finger: &str) -> String {
        if let Some(name) = self.fingers.names.get(finger) {
            return name.clone();
        }
        let place = self
            .fingers
            .enrolled
            .as_ref()
            .and_then(|enrolled| enrolled.iter().position(|each| each == finger))
            .map_or(1, |ix| ix + 1);
        fill("Fingerprint {number}", &[("number", &place.to_string())])
    }

    /// The finger being added: what to do, the touches taken, and Cancel.
    fn adding_island(&self, adding: &Adding, layer: Layer, cx: &mut Context<Self>) -> Div {
        let (done, of) = match adding.progress {
            Progress::Touches { done, of } => (done, of),
            _ => (0, 0),
        };
        // Lifting the finger before the reader has the touch stops it all.
        let said = adding.again.unwrap_or(if done == 0 {
            "Hold your finger on the circle until the bar moves"
        } else {
            "Lift it, then hold it on the circle again, a little moved"
        });
        let filled = if of > 0 {
            (done as f32 / of as f32).min(1.0)
        } else {
            0.0
        };
        let bar = div()
            .w_full()
            .mt(dp(8.))
            .h(dp(6.))
            .rounded_full()
            .bg(denial_look_gpui::control_fill(&self.look))
            .child(
                div()
                    .h_full()
                    .w(gpui::relative(filled))
                    .rounded_full()
                    .bg(solid(self.look.accent)),
            );
        let cancel = self.button(
            "cancel-adding",
            "Cancel",
            Fill::Control,
            layer.live(),
            cx,
            |this, _, cx| {
                this.fingers.adding = None;
                if let Some((session, _)) = this.fingers.session.as_mut() {
                    session.cancel();
                }
                cx.notify();
            },
        );
        island(&self.look)
            .flex()
            .flex_col()
            .items_center()
            .gap(dp(6.))
            .p(dp(ROW_PAD))
            .pt(dp(22.))
            .child(self.tile(Icon::Fingerprint, 56.))
            .child(
                div()
                    .pt(dp(6.))
                    .text_size(dp(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(fill("Adding {name}", &[("name", &adding.name)])),
            )
            .child(
                div()
                    .text_center()
                    .text_color(solid(if adding.again.is_some() {
                        self.look.palette.error
                    } else {
                        self.look.palette.secondary
                    }))
                    .child(say(said)),
            )
            .child(bar)
            .child(self.buttons([cancel]).w_full().pt(dp(12.)))
    }

    /// The ring around the sensor under the screen, while a finger is added
    /// and the reader is there; `size` is the window's, which fills the
    /// panel.
    pub(super) fn sensor_ring(&self, size: gpui::Size<gpui::Pixels>) -> Option<Div> {
        let [left, top, width, height] = self.fingers.adding.as_ref()?.sensor?;
        let (window_width, window_height) = (f32::from(size.width), f32::from(size.height));
        Some(
            div()
                .absolute()
                .left(px(left * window_width))
                .top(px(top * window_height))
                .w(px(width * window_width))
                .h(px(height * window_height))
                .rounded_full()
                .border(dp(3.))
                .border_color(solid(self.look.accent)),
        )
    }

    /// Reads the fingers on the reader.
    pub(super) fn read_fingers(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let enrolled = cx.background_spawn(fingerprint::enrolled()).await;
            let names = cx.background_spawn(async { fingerprint::names() }).await;
            let _ = this.update(cx, |view, cx| {
                view.fingers.enrolled = enrolled;
                view.fingers.names = names;
                view.fingers.read = true;
                cx.notify();
            });
        })
        .detach();
    }

    /// Follows the session the PIN opened, which found `enrolled`.
    pub(super) fn follow_session(
        &mut self,
        session: Session,
        enrolled: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let events = session.events.clone();
        let task = cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                if this
                    .update(cx, |view, cx| view.session_said(event, cx))
                    .is_err()
                {
                    return;
                }
            }
        });
        self.fingers.session = Some((session, task));
        self.fingers.enrolled = Some(enrolled);
        self.fingers.read = true;
        cx.notify();
    }

    fn session_said(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Fingers(enrolled) => {
                if let Some(finger) = self.fingers.removing.take()
                    && !enrolled.contains(&finger)
                {
                    self.note(
                        fill("{name} removed", &[("name", &self.finger_name(&finger))]),
                        cx,
                    );
                    self.fingers.names.remove(&finger);
                    self.keep_names(cx);
                }
                self.fingers.enrolled = Some(enrolled);
            }
            Event::Progress(step) => self.took(step, cx),
            Event::Refused(why) => self.note(why, cx),
            Event::Ended => {
                self.fingers.session = None;
                self.fingers.removing = None;
                if self.fingers.adding.take().is_some() {
                    self.note("Enter the PIN again to add it", cx);
                }
            }
        }
        cx.notify();
    }

    /// The open session, or, without one, the PIN asked for again.
    fn session(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<&mut Session> {
        if self.fingers.session.is_none() {
            self.open_fingers(window, cx);
        }
        self.fingers.session.as_mut().map(|(session, _)| session)
    }

    /// Asks the name of a fingerprint to add, or, for `finger`, its new
    /// name, with the choice to remove it.
    fn ask_finger(&mut self, finger: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if finger.is_none() && self.session(window, cx).is_none() {
            return;
        }
        let name = match &finger {
            Some(finger) => self.finger_name(finger),
            None => String::new(),
        };
        self.finger_field
            .update(cx, |input, cx| input.set_text(name, cx));
        self.show(Dialog::Finger { finger }, cx);
        window.focus(&self.finger_field.focus_handle(cx), cx);
    }

    /// What the name dialog answers: adds a fingerprint called `name` in the
    /// first free slot, or renames `finger`.
    pub(super) fn name_finger(
        &mut self,
        finger: Option<String>,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let name: String = name.trim().chars().take(fingerprint::NAME_LENGTH).collect();
        if name.is_empty() {
            return;
        }
        match finger {
            Some(finger) => {
                self.fingers.names.insert(finger, name);
                self.keep_names(cx);
            }
            None => {
                let enrolled = self.fingers.enrolled.clone().unwrap_or_default();
                if let Some(slot) = SLOTS
                    .iter()
                    .find(|slot| !enrolled.iter().any(|each| each == *slot))
                {
                    self.add_finger(slot, name, cx);
                }
            }
        }
        cx.notify();
    }

    fn keep_names(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = fingerprint::save_names(&self.fingers.names) {
            self.note(
                fill(
                    "Could not keep the names: {why}",
                    &[("why", &error.to_string())],
                ),
                cx,
            );
        }
    }

    /// Adds `finger`, the screen following each touch, until it is added,
    /// fails or is cancelled.
    fn add_finger(&mut self, finger: &'static str, name: String, cx: &mut Context<Self>) {
        let Some((session, _)) = self.fingers.session.as_mut() else {
            self.note("Enter the PIN again to add it", cx);
            return;
        };
        if !session.add(finger) {
            self.note("Fingerprints cannot change now", cx);
            return;
        }
        self.fingers.adding = Some(Adding {
            finger,
            name,
            sensor: fingerprint::sensor(),
            progress: Progress::Touches { done: 0, of: 0 },
            again: None,
        });
        cx.notify();
    }

    fn took(&mut self, step: Progress, cx: &mut Context<Self>) {
        let Some(adding) = self.fingers.adding.as_mut() else {
            return;
        };
        match step {
            Progress::Touches { .. } => {
                adding.progress = step;
                adding.again = None;
            }
            Progress::Again(said) => adding.again = Some(said),
            Progress::Added => {
                let (finger, name) = (adding.finger.to_owned(), adding.name.clone());
                self.fingers.adding = None;
                self.note(fill("{name} added", &[("name", &name)]), cx);
                self.fingers.names.insert(finger, name);
                self.keep_names(cx);
            }
            Progress::Failed(why) => {
                self.fingers.adding = None;
                self.note(why, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn ask_remove_finger(&mut self, finger: String, cx: &mut Context<Self>) {
        self.show(
            Dialog::Confirm {
                title: fill("Remove {name}?", &[("name", &self.finger_name(&finger))]).into(),
                consequence: "It no longer unlocks the phone.".into(),
                action: "Remove",
                confirm: Rc::new(move |this, window, cx| {
                    this.remove_finger(finger.clone(), window, cx)
                }),
            },
            cx,
        );
    }

    fn remove_finger(&mut self, finger: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.session(window, cx) else {
            return;
        };
        if !session.remove(&finger) {
            self.note("Fingerprints cannot change now", cx);
            return;
        }
        self.fingers.removing = Some(finger);
        cx.notify();
    }
}
