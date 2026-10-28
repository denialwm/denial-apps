//! The Mobile network page: where the modem is and the network it is on,
//! the mobile data switch, which SIM is used, and the PIN dialog's work;
//! then what dmail knows about the session and the network's refusals, and
//! the steps that bring a lost network back. What the user does goes to
//! dmail through `mobile::Link`; what changes comes back as a fresh
//! `mobile::State`.

use denial_look::words::{fill, tr};
use denial_look_gpui::say;
use std::rc::Rc;
use std::time::Duration;

use denial_look_gpui::{Icon, dp, island};
use gpui::{Context, Div, SharedString, Window, prelude::*};

use super::parts::{Fill, ROW_GAP, ROW_PAD, TILE};
use super::{Dialog, Layer, Settings};
use crate::mobile::{self, Data, Link, Modem, Refusal};
use crate::{about, catalog, clock, wifi};

/// How often the page reads the data link's traffic while it shows.
const TRAFFIC_EVERY: Duration = Duration::from_secs(2);

/// The ways back to a working network, smallest first: id, title, what it
/// does, what its row says while it runs, and what a failure note says.
struct Step {
    id: &'static str,
    title: &'static str,
    does: &'static str,
    doing: &'static str,
    failed: &'static str,
    run: fn(Link) -> futures_lite::future::Boxed<Result<(), Refusal>>,
}

const STEPS: [Step; 3] = [
    Step {
        id: "reconnect",
        title: "Reconnect mobile data",
        does: "Ends the data session and starts a new one",
        doing: "Reconnecting…",
        failed: "Could not reconnect: {why}",
        run: |link| Box::pin(async move { link.reconnect().await }),
    },
    Step {
        id: "restart-radio",
        title: "Restart the radio",
        does: "Calls and data stop for a few seconds while the phone registers again",
        doing: "Restarting the radio…",
        failed: "Could not restart the radio: {why}",
        run: |link| Box::pin(async move { link.restart_radio().await }),
    },
    Step {
        id: "restart-modem",
        title: "Start the modem over",
        does: "dmail sets the modem up again from the beginning",
        doing: "Starting the modem over…",
        failed: "Could not start the modem over: {why}",
        run: |link| Box::pin(async move { link.restart().await }),
    },
];

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
                Some(
                    fill(
                        "Nothing in {slot}",
                        &[("slot", &say(slot_name(state.sim_slot)))],
                    )
                    .into(),
                ),
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
        let waiting = state.retry_at - clock::seconds_now();
        let data_detail: Option<SharedString> = match (enabled, state.data) {
            (false, _) => None,
            (true, Data::Connected) => Some(tr("Connected").into()),
            (true, Data::Connecting) if state.retry_at > 0 => Some(
                fill(
                    "Trying again in {seconds} s",
                    &[("seconds", &waiting.max(0).to_string())],
                )
                .into(),
            ),
            (true, Data::Connecting) if state.modem == Modem::Registered && !state.attached => {
                Some(tr("Waiting for the network to allow data").into())
            }
            (true, Data::Connecting) => Some(tr("Connecting…").into()),
            (true, Data::Failed) if state.modem == Modem::Failed => {
                Some(tr("The modem stopped").into())
            }
            (true, Data::Failed) => {
                Some(tr("The network turned data down. Restart the radio to ask again").into())
            }
            (true, Data::Off) => Some(tr("When the phone is on a network").into()),
        };
        let data = self.row(
            "data",
            Some(Icon::Sim),
            "Mobile data",
            data_detail,
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

        let mut network: Vec<Info> = Vec::new();
        if state.modem == Modem::Registered {
            if let Some(technology) = state.technology() {
                network.push(Info::Value("Network type", technology));
            }
            network.push(Info::Value(
                "Signal",
                format!("{} · {}%", say(wifi::quality(state.signal)), state.signal),
            ));
            network.push(Info::Value(
                "Roaming",
                tr(if state.roaming { "Yes" } else { "No" }).to_owned(),
            ));
            network.push(Info::Value(
                "Packet service",
                tr(if state.attached {
                    "Attached"
                } else {
                    "Not attached"
                })
                .to_owned(),
            ));
        }
        if state.last_reject.1 > 0 {
            network.push(Info::Detail(
                "Last refusal",
                format!(
                    "{} · {}",
                    self.when(state.last_reject.1),
                    state.last_reject.0
                ),
            ));
        }
        if !network.is_empty() {
            parts.push(self.chip_row("Network"));
            parts.push(self.info_island(network));
        }

        let mut session: Vec<Info> = Vec::new();
        if state.data == Data::Connected {
            session.push(Info::Value("APN", state.apn.clone()));
            session.push(Info::Value("Address", state.address.clone()));
            if !state.dns.is_empty() {
                session.push(Info::Value("DNS", state.dns.join(", ")));
            }
            if state.mtu > 0 {
                session.push(Info::Value("MTU", state.mtu.to_string()));
            }
            session.push(Info::Value("Data link", state.interface.clone()));
            if state.since > 0 {
                let up = (clock::seconds_now() - state.since).max(0) as u64;
                session.push(Info::Value(
                    "Connected for",
                    about::up_time(Duration::from_secs(up)),
                ));
            }
            if let Some((received, sent)) = mobile.traffic {
                session.push(Info::Value(
                    "Received and sent",
                    format!("{} · {}", about::bytes(received), about::bytes(sent)),
                ));
            }
        } else if enabled && !state.apn.is_empty() {
            session.push(Info::Value("APN", state.apn.clone()));
        }
        if enabled {
            session.push(Info::Value(
                "Drops since dmail started",
                state.drops.to_string(),
            ));
        }
        if state.last_failure.1 > 0 {
            session.push(Info::Detail(
                "Last drop",
                format!(
                    "{} · {}",
                    self.when(state.last_failure.1),
                    state.last_failure.0
                ),
            ));
        }
        if !session.is_empty() {
            parts.push(self.chip_row("Mobile data"));
            parts.push(self.info_island(session));
        }

        parts.push(self.chip_row("If the network goes away"));
        parts.push(self.recovery_island(live, cx));
        parts
    }

    /// The recovery steps, one row each; while one runs, none can start.
    fn recovery_island(&self, live: bool, cx: &mut Context<Self>) -> Div {
        let state = &self.mobile.state;
        let busy = self.mobile.busy;
        self.rows_island(STEPS.iter(), 0., |step, ends| {
            let possible = match step.id {
                "reconnect" => state.data_enabled && state.modem != Modem::Failed,
                "restart-radio" => !matches!(state.modem, Modem::Failed | Modem::Starting),
                _ => true,
            };
            let detail = match busy {
                Some(doing) if doing == step.doing => tr(doing),
                _ => tr(step.does),
            };
            let row = self.row(step.id, None, step.title, Some(detail.into()), ends);
            if !possible || busy.is_some() {
                return row.opacity(0.5);
            }
            self.live(row, live, cx, move |this, _, cx| this.recover(step, cx))
        })
    }

    /// Runs one recovery step; dmail's answer ends it, a refusal says why.
    fn recover(&mut self, step: &'static Step, cx: &mut Context<Self>) {
        let Some(link) = self.mobile.link.clone() else {
            return;
        };
        self.mobile.busy = Some(step.doing);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let done = (step.run)(link).await;
            let _ = this.update(cx, |view, cx| {
                view.mobile.busy = None;
                if let Err(refusal) = done {
                    view.note(fill(step.failed, &[("why", &say(said(&refusal)))]), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Rows of what dmail reports: a value at the end, or a longer line
    /// under the title.
    fn info_island(&self, rows: Vec<Info>) -> Div {
        self.rows_island(rows.into_iter(), 0., |info, ends| match info {
            Info::Value(title, value) => self
                .row(title, None, title, None, ends)
                .child(self.value(value)),
            Info::Detail(title, detail) => self.row(title, None, title, Some(detail.into()), ends),
        })
    }

    /// When something happened: the time today, the date and time before.
    fn when(&self, unix: i64) -> String {
        let twenty_four = catalog::find("time.24").is_some_and(|item| self.on(item));
        let then = clock::local(unix as libc::time_t);
        let time = clock::time_text(&then, twenty_four);
        if clock::date_text(&then) == clock::date_text(&clock::now()) {
            time
        } else {
            format!("{} {time}", clock::date_text(&then))
        }
    }

    /// Reads the data link's traffic every two seconds while the page
    /// shows, which also moves its times on.
    pub(super) fn follow_traffic(&mut self, cx: &mut Context<Self>) {
        if !self.screen.shows_mobile() {
            self.reading_mobile = None;
            return;
        }
        if self.reading_mobile.is_some() {
            return;
        }
        self.reading_mobile = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TRAFFIC_EVERY).await;
                let taken = this.update(cx, |view, cx| {
                    let mobile = &mut view.mobile;
                    mobile.traffic = mobile::traffic(&mobile.state.interface);
                    cx.notify();
                });
                if taken.is_err() {
                    return;
                }
            }
        }));
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
                Some(technology) => format!("{} · {}", state.operator, say(technology)).into(),
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
                    let failed = if on {
                        "Could not turn mobile data on: {why}"
                    } else {
                        "Could not turn mobile data off: {why}"
                    };
                    view.note(fill(failed, &[("why", &say(said(&refusal)))]), cx);
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
                    view.note(
                        fill(
                            "Could not change the SIM: {why}",
                            &[("why", &say(said(&refusal)))],
                        ),
                        cx,
                    );
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
                Err(refusal) => view.note(
                    fill("Could not unlock: {why}", &[("why", &say(said(&refusal)))]),
                    cx,
                ),
            });
        });
    }
}

/// One row of what dmail reports.
enum Info {
    Value(&'static str, String),
    Detail(&'static str, String),
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
        1 => tr("1 try left before the SIM blocks").to_owned(),
        left => fill("{count} tries left", &[("count", &left.to_string())]),
    }
}

/// The network's type, signal and roaming, as one line.
fn network_line(state: &mobile::State) -> String {
    let mut line: Vec<String> = Vec::new();
    line.extend(state.technology());
    line.push(say(wifi::quality(state.signal)).to_string());
    if state.roaming {
        line.push(tr("Roaming").to_owned());
    }
    line.join(" · ")
}
