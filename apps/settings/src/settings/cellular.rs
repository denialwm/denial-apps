//! The Mobile network page: where the modem is and the network it is on,
//! the mobile data switch, which SIM is used, and the PIN dialog's work.
//! What the user does goes to dmail through `mobile::Link`; what changes
//! comes back as a fresh `mobile::State`.

use std::rc::Rc;

use denial_look_gpui::{Icon, dp, island};
use gpui::{Context, Div, SharedString, Window, prelude::*};

use super::parts::{Fill, ROW_GAP, ROW_PAD, TILE};
use super::{Dialog, Layer, Settings};
use crate::mobile::{self, Data, Modem, Refusal};
use crate::wifi;

/// Where a row's separator starts: after its icon tile.
const SEPARATOR: f32 = ROW_PAD + TILE + ROW_GAP;

/// What dmail's SIM slots are called.
const SLOTS: [&str; 2] = ["SIM 1", "SIM 2"];

impl Settings {
    /// The Mobile network page's islands, top to bottom: the network and
    /// the data switch, the SIM, and the network's details.
    pub(super) fn mobile_page(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let mobile = &self.mobile;
        if !mobile.read {
            return Vec::new();
        }
        if mobile.link.is_none() {
            return vec![self.missing(
                Icon::Network,
                "Mobile network is not available",
                "dmail, the modem service, does not run on this phone",
            )];
        }
        let live = layer.live();
        let state = &mobile.state;
        let (title, detail): (SharedString, Option<SharedString>) = match state.modem {
            Modem::Off => ("The modem is off".into(), None),
            Modem::Starting => ("Starting the modem…".into(), None),
            Modem::Failed => (
                "The modem stopped".into(),
                Some("dmail could not keep it running".into()),
            ),
            Modem::NoSim => (
                "No SIM card".into(),
                Some(format!("Nothing in {}", slot_name(state.sim_slot)).into()),
            ),
            Modem::Locked if state.pin_retries == 0 => (
                "The SIM is blocked".into(),
                Some("It needs its PUK, which the phone cannot take yet".into()),
            ),
            Modem::Locked => (
                "The SIM is locked".into(),
                Some(tries(state.pin_retries).into()),
            ),
            Modem::Searching => ("Looking for a network…".into(), None),
            Modem::Registered => (
                state.operator.clone().into(),
                Some(network_line(state).into()),
            ),
        };
        let unlock = (state.modem == Modem::Locked && state.pin_retries > 0).then(|| {
            let unlock = self.button(
                "unlock",
                "Unlock",
                Fill::Accent,
                live,
                cx,
                |this, window, cx| this.ask_pin(None, window, cx),
            );
            self.buttons([unlock]).px(dp(ROW_PAD)).pb(dp(ROW_PAD))
        });
        let status = self.row("network", Some(Icon::Network), title, detail, (true, false));

        let enabled = state.data_enabled;
        let data_detail = match (enabled, state.data) {
            (false, _) => None,
            (true, Data::Connected) => Some("Connected"),
            (true, Data::Connecting) => Some("Connecting…"),
            (true, Data::Failed) => Some("The network turned data down"),
            (true, Data::Off) => Some("When the phone is on a network"),
        };
        let data = self.row(
            "data",
            Some(Icon::Sim),
            "Mobile data",
            data_detail.map(Into::into),
            (false, true),
        );
        let data = self.live(
            data.child(self.toggle(enabled)),
            live,
            cx,
            move |this, _, cx| this.set_mobile_data(!enabled, cx),
        );
        let mut parts = vec![
            island(&self.look)
                .child(status)
                .children(unlock)
                .child(self.separator(SEPARATOR))
                .child(data),
        ];

        parts.push(self.chip_row("SIM"));
        let slot = state.sim_slot;
        let row = self
            .row(
                "sim-slot",
                Some(Icon::Sim),
                "SIM used",
                Some("For messages, calls and data".into()),
                (true, true),
            )
            .child(self.value(slot_name(slot)))
            .child(self.chevron());
        parts.push(
            island(&self.look).child(self.live(row, live, cx, move |this, _, cx| {
                this.show(
                    Dialog::Pick {
                        title: "SIM used".into(),
                        options: SLOTS.iter().map(|&slot| slot.into()).collect(),
                        picked: usize::from(slot.clamp(1, 2) - 1),
                        pick: Rc::new(|this, option, _, cx| {
                            this.set_sim_slot(option as u8 + 1, cx)
                        }),
                    },
                    cx,
                )
            })),
        );

        if state.modem == Modem::Registered {
            let mut info: Vec<(&str, String)> = Vec::new();
            if let Some(technology) = state.technology() {
                info.push(("Network type", technology));
            }
            info.push((
                "Signal",
                format!("{} · {}%", wifi::quality(state.signal), state.signal),
            ));
            info.push((
                "Roaming",
                if state.roaming { "Yes" } else { "No" }.to_owned(),
            ));
            if state.data == Data::Connected && !state.interface.is_empty() {
                info.push(("Data link", state.interface.clone()));
            }
            parts.push(self.chip_row("Network"));
            parts.push(
                self.rows_island(info.into_iter(), 0., |(title, value), ends| {
                    self.row(title, None, title, None, ends)
                        .child(self.value(value))
                }),
            );
        }
        parts
    }

    /// Mobile network's line under its title on the home screen.
    pub(super) fn mobile_summary(&self) -> Option<SharedString> {
        let mobile = &self.mobile;
        let state = &mobile.state;
        if !mobile.read {
            return None;
        }
        if mobile.link.is_none() {
            return Some("Not available".into());
        }
        Some(match state.modem {
            Modem::Off | Modem::Starting => "Starting…".into(),
            Modem::Failed => "The modem stopped".into(),
            Modem::NoSim => "No SIM card".into(),
            Modem::Locked => "SIM locked".into(),
            Modem::Searching => "Looking for a network…".into(),
            Modem::Registered => match state.technology() {
                Some(technology) => format!("{} · {technology}", state.operator).into(),
                None => state.operator.clone().into(),
            },
        })
    }

    /// Turns mobile data on or off; the switch moves at once, and back if
    /// dmail says no.
    fn set_mobile_data(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some(link) = self.mobile.link.clone() else {
            return;
        };
        self.mobile.state.data_enabled = on;
        cx.notify();
        cx.spawn(async move |this, cx| {
            if let Err(refusal) = link.set_data(on).await {
                let _ = this.update(cx, |view, cx| {
                    view.mobile.state.data_enabled = !on;
                    let turn = if on { "on" } else { "off" };
                    view.note(
                        format!("Could not turn mobile data {turn}: {}", said(&refusal)),
                        cx,
                    );
                });
            }
        })
        .detach();
    }

    fn set_sim_slot(&mut self, slot: u8, cx: &mut Context<Self>) {
        let Some(link) = self.mobile.link.clone() else {
            return;
        };
        if self.mobile.state.sim_slot == slot {
            return;
        }
        let before = self.mobile.state.sim_slot;
        self.mobile.state.sim_slot = slot;
        cx.notify();
        cx.spawn(async move |this, cx| {
            if let Err(refusal) = link.set_sim_slot(slot).await {
                let _ = this.update(cx, |view, cx| {
                    view.mobile.state.sim_slot = before;
                    view.note(format!("Could not change the SIM: {}", said(&refusal)), cx);
                });
            }
        })
        .detach();
    }

    /// Unlocks the SIM with `pin`. A wrong PIN asks again, with the tries
    /// left.
    pub(super) fn unlock(&mut self, pin: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(link) = self.mobile.link.clone() else {
            return;
        };
        self.dismiss(window, cx);
        self._unlocking = cx.spawn_in(window, async move |this, cx| {
            let unlocked = link.unlock(pin).await;
            let _ = this.update_in(cx, |view, window, cx| match unlocked {
                Ok(()) => {}
                Err(Refusal::WrongPin) => view.ask_pin(Some("Wrong PIN"), window, cx),
                Err(refusal) => view.note(format!("Could not unlock: {}", said(&refusal)), cx),
            });
        });
    }
}

/// What a refusal says in a note.
fn said(refusal: &Refusal) -> &str {
    match refusal {
        Refusal::WrongPin => "wrong PIN",
        Refusal::Other(said) => said,
    }
}

fn slot_name(slot: u8) -> &'static str {
    SLOTS[usize::from(slot.clamp(1, 2) - 1)]
}

/// PIN tries left, as said under the lock.
pub(super) fn tries(left: u32) -> String {
    match left {
        1 => "1 try left before the SIM blocks".to_owned(),
        left => format!("{left} tries left"),
    }
}

/// The network's type, signal and roaming, as one line.
fn network_line(state: &mobile::State) -> String {
    let mut line: Vec<String> = Vec::new();
    line.extend(state.technology());
    line.push(wifi::quality(state.signal).to_owned());
    if state.roaming {
        line.push("Roaming".to_owned());
    }
    line.join(" · ")
}
