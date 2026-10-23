//! Bluetooth's screens: the page with the switch, the phone's name, the
//! paired devices and the devices nearby; and one paired device. What the
//! user does goes to BlueZ through `bluetooth::Link`; what changes comes
//! back as a fresh `bluetooth::State`. Pairing questions from Settings'
//! agent arrive as `bluetooth::Request`s and are answered in a dialog.

use denial_look::words::fill;
use denial_look_gpui::say;
use std::future::Future;
use std::rc::Rc;

use denial_look_gpui::{Icon, dp, island, solid};
use gpui::{Context, Div, Focusable, FontWeight, SharedString, Stateful, Window, div, prelude::*};

use super::parts::{Fill, ROW_GAP, ROW_PAD, TILE};
use super::{Dialog, Layer, Screen, Settings};
use crate::bluetooth::{self, Ask, Device, Kind, Request};

/// Where a row's separator starts: after its icon tile.
const SEPARATOR: f32 = ROW_PAD + TILE + ROW_GAP;

fn glyph(kind: Kind) -> Icon {
    match kind {
        Kind::Headphones => Icon::Headphones,
        Kind::Speaker => Icon::Speaker,
        Kind::Keyboard => Icon::Keyboard,
        Kind::Mouse => Icon::Mouse,
        Kind::Gamepad => Icon::Gamepad,
        Kind::Phone => Icon::Phone,
        Kind::Computer => Icon::Computer,
        Kind::Other => Icon::Bluetooth,
    }
}

/// A passkey as people read it: two groups of three.
fn grouped(code: &str) -> String {
    if code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()) {
        format!("{} {}", &code[..3], &code[3..])
    } else {
        code.to_owned()
    }
}

impl Settings {
    /// The Bluetooth page's islands, top to bottom: the switch and the
    /// phone's name, the paired devices, and the devices nearby.
    pub(super) fn bluetooth_page(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let bt = &self.bluetooth;
        if !bt.read {
            return Vec::new();
        }
        if bt.link.is_none() {
            return vec![self.missing(
                Icon::Bluetooth,
                "Bluetooth is not available",
                "BlueZ does not run on this phone",
            )];
        }
        let Some(adapter) = &bt.state.adapter else {
            return vec![self.missing(
                Icon::Bluetooth,
                "No Bluetooth hardware",
                "BlueZ finds no Bluetooth controller",
            )];
        };
        let live = layer.live();
        let powered = adapter.powered;
        let switch = self.row(
            "bluetooth-on",
            Some(Icon::Bluetooth),
            "Use Bluetooth",
            if adapter.blocked {
                Some("Off by airplane mode or a hardware switch".into())
            } else {
                None
            },
            (true, false),
        );
        let switch = self.live(
            switch.child(self.toggle(powered)),
            live,
            cx,
            move |this, _, cx| this.set_bluetooth(!powered, cx),
        );
        let name = self
            .row(
                "phone-name",
                Some(Icon::Phone),
                "Phone name",
                None,
                (false, true),
            )
            .child(self.value(adapter.name.clone()))
            .child(self.chevron());
        let name = self.live(name, live, cx, |this, window, cx| this.ask_name(window, cx));
        let mut parts = vec![
            island(&self.look)
                .child(switch)
                .child(self.separator(SEPARATOR))
                .child(name),
        ];
        if !powered {
            return parts;
        }

        let (paired, nearby): (Vec<&Device>, Vec<&Device>) =
            bt.state.devices.iter().partition(|device| device.paired);
        if !paired.is_empty() {
            parts.push(self.chip_row("Paired devices"));
            parts.push(
                self.rows_island(paired.into_iter(), SEPARATOR, |device, ends| {
                    self.paired_row(device, ends, live, cx)
                }),
            );
        }
        if nearby.is_empty() {
            parts.push(self.chip_row(if adapter.discovering {
                "Looking for devices…"
            } else {
                "No devices nearby"
            }));
        } else {
            parts.push(self.chip_row("Devices nearby"));
            parts.push(
                self.rows_island(nearby.into_iter(), SEPARATOR, |device, ends| {
                    self.nearby_row(device, ends, live, cx)
                }),
            );
        }
        parts
    }

    /// How a device stands, under its name.
    fn device_status(&self, device: &Device) -> SharedString {
        let bt = &self.bluetooth;
        if let Some((_, doing)) = bt.busy.as_ref().filter(|(path, _)| *path == device.path) {
            return (*doing).into();
        }
        if let Some(failure) = bt.failed.get(&device.path) {
            return (*failure).into();
        }
        match (device.paired, device.connected, device.battery) {
            (true, true, Some(battery)) => fill(
                "Connected · Battery {level}%",
                &[("level", &battery.to_string())],
            )
            .into(),
            (true, true, None) => "Connected".into(),
            (true, false, _) => "Not connected".into(),
            (false, _, _) => device.kind.label().into(),
        }
    }

    /// A paired device: a tap connects or disconnects it, its gear opens it.
    fn paired_row(
        &self,
        device: &Device,
        ends: (bool, bool),
        live: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let path: SharedString = device.path.clone().into();
        let details = self.icon_button(
            "details",
            Icon::Settings,
            live,
            cx,
            move |this, window, cx| {
                let path = path.clone();
                this.open(Screen::Device { path }, window, cx)
            },
        );
        let row = self
            .row(
                SharedString::from(format!("paired-{}", device.path)),
                Some(glyph(device.kind)),
                device.name.clone(),
                Some(self.device_status(device)),
                ends,
            )
            .pr(dp(4.))
            .child(details);
        let device = device.clone();
        self.live(row, live, cx, move |this, _, cx| {
            if device.connected {
                this.disconnect_device(device.clone(), cx)
            } else {
                this.connect_device(device.clone(), cx)
            }
        })
    }

    /// A device nearby: a tap pairs with it.
    fn nearby_row(
        &self,
        device: &Device,
        ends: (bool, bool),
        live: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let row = self.row(
            SharedString::from(format!("nearby-{}", device.path)),
            Some(glyph(device.kind)),
            device.name.clone(),
            Some(self.device_status(device)),
            ends,
        );
        let device = device.clone();
        self.live(row, live, cx, move |this, _, cx| {
            this.pair(device.clone(), cx)
        })
    }

    /// One paired device: Connect or Disconnect and Forget, and what it is.
    pub(super) fn device_screen(
        &self,
        path: &SharedString,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let Some(device) = self
            .bluetooth
            .state
            .device(path)
            .filter(|device| device.paired)
        else {
            return vec![self.chip_row("This device is no longer paired")];
        };
        let live = layer.live() && self.bluetooth.busy.is_none();
        let connect = {
            let device = device.clone();
            if device.connected {
                self.button(
                    "disconnect",
                    "Disconnect",
                    Fill::Control,
                    live,
                    cx,
                    move |this, _, cx| this.disconnect_device(device.clone(), cx),
                )
            } else {
                self.button(
                    "connect",
                    "Connect",
                    Fill::Accent,
                    live,
                    cx,
                    move |this, _, cx| this.connect_device(device.clone(), cx),
                )
            }
        };
        let forget = {
            let device = device.clone();
            self.button(
                "forget",
                "Forget",
                Fill::Danger,
                layer.live(),
                cx,
                move |this, _, cx| this.ask_forget_device(device.clone(), cx),
            )
        };
        let hero = island(&self.look)
            .flex()
            .flex_col()
            .items_center()
            .gap(dp(6.))
            .p(dp(ROW_PAD))
            .pt(dp(22.))
            .child(self.tile(glyph(device.kind), 56.))
            .child(
                div()
                    .pt(dp(6.))
                    .text_size(dp(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(device.name.clone()),
            )
            .child(
                div()
                    .text_color(solid(self.look.palette.secondary))
                    .child(self.device_status(device)),
            )
            .child(self.buttons([connect, forget]).w_full().pt(dp(12.)));

        let mut info: Vec<(&str, String)> = vec![("Type", device.kind.label().to_owned())];
        if let Some(battery) = device.battery {
            info.push(("Battery", format!("{battery}%")));
        }
        info.push(("Address", device.address.clone()));
        vec![
            hero,
            self.chip_row("About"),
            self.rows_island(info.into_iter(), 0., |(title, value), ends| {
                self.row(title, None, title, None, ends)
                    .child(self.value(value))
            }),
        ]
    }

    /// Bluetooth's line under its title on the home screen.
    pub(super) fn bluetooth_summary(&self) -> Option<SharedString> {
        let bt = &self.bluetooth;
        if !bt.read {
            return None;
        }
        if bt.link.is_none() {
            return Some("Not available".into());
        }
        let Some(adapter) = &bt.state.adapter else {
            return Some("No Bluetooth hardware".into());
        };
        if !adapter.powered {
            return Some("Off".into());
        }
        let connected: Vec<&Device> = bt
            .state
            .devices
            .iter()
            .filter(|device| device.connected)
            .collect();
        Some(match connected.as_slice() {
            [] => "On".into(),
            [device] => fill("Connected to {name}", &[("name", &device.name)]).into(),
            devices => fill(
                "{count} devices connected",
                &[("count", &devices.len().to_string())],
            )
            .into(),
        })
    }

    /// The manager and the controller, while there are both.
    fn bluetooth_link(&self) -> Option<(bluetooth::Link, bluetooth::Adapter)> {
        let bt = &self.bluetooth;
        bt.link.clone().zip(bt.state.adapter.clone())
    }

    /// Turns Bluetooth on or off; the switch moves at once, and back if
    /// BlueZ says no.
    fn set_bluetooth(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some((link, adapter)) = self.bluetooth_link() else {
            return;
        };
        if let Some(adapter) = &mut self.bluetooth.state.adapter {
            adapter.powered = on;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            if let Err(error) = link.set_powered(&adapter, on).await {
                let _ = this.update(cx, |view, cx| {
                    if let Some(adapter) = &mut view.bluetooth.state.adapter {
                        adapter.powered = !on;
                    }
                    let failed = if on {
                        "Could not turn Bluetooth on: {why}"
                    } else {
                        "Could not turn Bluetooth off: {why}"
                    };
                    let why = say(bluetooth::failure(&error));
                    view.note(fill(failed, &[("why", &why)]), cx);
                });
            }
        })
        .detach();
    }

    /// Runs one action on `device`, saying `doing` on its row meanwhile;
    /// a failure stays on its row and in a note.
    fn device_action<F>(
        &mut self,
        device: Device,
        doing: &'static str,
        action: impl FnOnce(bluetooth::Link) -> F + 'static,
        cx: &mut Context<Self>,
    ) where
        F: Future<Output = zbus::Result<()>> + 'static,
    {
        let Some(link) = self.bluetooth.link.clone() else {
            return;
        };
        if self.bluetooth.busy.is_some() {
            return;
        }
        self.bluetooth.failed.remove(&device.path);
        self.bluetooth.busy = Some((device.path.clone(), doing));
        cx.notify();
        self._device_action = cx.spawn(async move |this, cx| {
            let done = action(link).await;
            let _ = this.update(cx, |view, cx| {
                view.bluetooth.busy = None;
                if let Err(error) = done {
                    let why = bluetooth::failure(&error);
                    view.note(format!("{}: {}", device.name, say(why)), cx);
                    view.bluetooth.failed.insert(device.path, why);
                }
                cx.notify();
            });
        });
    }

    fn connect_device(&mut self, device: Device, cx: &mut Context<Self>) {
        let target = device.clone();
        self.device_action(
            device,
            "Connecting…",
            move |link| async move { link.connect(&target).await },
            cx,
        );
    }

    fn disconnect_device(&mut self, device: Device, cx: &mut Context<Self>) {
        let target = device.clone();
        self.device_action(
            device,
            "Disconnecting…",
            move |link| async move { link.disconnect(&target).await },
            cx,
        );
    }

    /// Pairs with `device` and connects it; the agent asks what pairing
    /// needs meanwhile.
    fn pair(&mut self, device: Device, cx: &mut Context<Self>) {
        let target = device.clone();
        self.device_action(
            device,
            "Pairing…",
            move |link| async move {
                link.pair(&target).await?;
                link.connect(&target).await
            },
            cx,
        );
    }

    fn ask_forget_device(&mut self, device: Device, cx: &mut Context<Self>) {
        self.show(
            Dialog::Confirm {
                title: fill("Forget {name}?", &[("name", &device.name)]).into(),
                consequence: "The phone unpairs it. Pairing again needs it in pairing mode.".into(),
                action: "Forget",
                confirm: Rc::new(move |this, window, cx| {
                    this.forget_device(device.clone(), window, cx)
                }),
            },
            cx,
        );
    }

    /// Forgets `device`, leaving its screen at once.
    fn forget_device(&mut self, device: Device, window: &mut Window, cx: &mut Context<Self>) {
        let Some((link, adapter)) = self.bluetooth_link() else {
            return;
        };
        if matches!(&self.screen, Screen::Device { path } if *path == device.path) {
            let parent = Self::parent(&self.screen);
            self.open_at(parent, true, window, cx);
        }
        cx.spawn(async move |this, cx| {
            if let Err(error) = link.forget(&adapter, &device).await {
                let _ = this.update(cx, |view, cx| {
                    let why = bluetooth::failure(&error);
                    view.note(
                        fill(
                            "Could not forget {name}: {why}",
                            &[("name", &device.name), ("why", &say(why))],
                        ),
                        cx,
                    )
                });
            }
        })
        .detach();
    }

    /// Asks for the phone's new name.
    pub(super) fn ask_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.phone_name().unwrap_or_default();
        self.name_field
            .update(cx, |input, cx| input.set_text(name, cx));
        self.show(Dialog::Rename, cx);
        window.focus(&self.name_field.focus_handle(cx), cx);
    }

    /// Renames the phone, and Bluetooth with it.
    pub(super) fn rename(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let bluetooth = self.bluetooth_link();
        self.dismiss(window, cx);
        cx.spawn(async move |this, cx| {
            let why = match crate::name::set(&name).await {
                Err(said) => Some(said),
                Ok(()) => {
                    let _ = this.update(cx, |view, cx| {
                        if let Some(facts) = &mut view.about.facts {
                            facts.name = Some(name.clone());
                        }
                        cx.notify();
                    });
                    match bluetooth {
                        Some((link, adapter)) => link
                            .set_name(&adapter, &name)
                            .await
                            .err()
                            .map(|error| say(bluetooth::failure(&error)).to_string()),
                        None => None,
                    }
                }
            };
            if let Some(why) = why {
                let _ = this.update(cx, |view, cx| {
                    view.note(
                        fill("Could not rename the phone: {why}", &[("why", &why)]),
                        cx,
                    )
                });
            }
        })
        .detach();
    }

    /// A question from Settings' pairing agent: shown as a dialog, or, for
    /// a cancel, its dialog closed.
    pub(super) fn pairing_request(
        &mut self,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(request.ask, Ask::Cancel) {
            if matches!(self.dialog, Some((Dialog::Pairing { .. }, _))) {
                self.dialog = None;
                window.focus(&self.focus_handle, cx);
                cx.notify();
            }
            return;
        }
        let name = self
            .bluetooth
            .state
            .device(&request.device)
            .map_or_else(|| "the device".into(), |device| device.name.clone().into());
        let typed = matches!(request.ask, Ask::PinCode | Ask::Passkey);
        self.code_field.update(cx, |input, cx| input.clear(cx));
        self.show(Dialog::Pairing { name, request }, cx);
        if typed {
            window.focus(&self.code_field.focus_handle(cx), cx);
        }
    }

    /// What the pairing dialog says, the code it shows to compare or type,
    /// whether it can be answered yet, and its answer button; none when it
    /// can only be cancelled.
    pub(super) fn pairing_dialog(
        &self,
        name: &SharedString,
        request: &Request,
        cx: &mut Context<Self>,
    ) -> (SharedString, Option<Div>, bool, Option<&'static str>) {
        let code = |code: String| {
            div()
                .py(dp(10.))
                .text_size(dp(32.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(grouped(&code))
        };
        let typed = self.code_field.read(cx).text().trim().to_owned();
        match &request.ask {
            Ask::Confirm(passkey) => (
                fill("Check that {name} shows this code", &[("name", &name)]).into(),
                Some(code(format!("{passkey:06}"))),
                true,
                Some("Pair"),
            ),
            Ask::Authorize => (
                fill("{name} asks to pair", &[("name", &name)]).into(),
                None,
                true,
                Some("Pair"),
            ),
            Ask::PinCode => (
                fill(
                    "Type the PIN of {name}, often 0000 or 1234",
                    &[("name", &name)],
                )
                .into(),
                None,
                (1..=16).contains(&typed.len()),
                Some("Pair"),
            ),
            Ask::Passkey => (
                fill("Type the code {name} shows", &[("name", &name)]).into(),
                None,
                (1..=6).contains(&typed.len()) && typed.bytes().all(|b| b.is_ascii_digit()),
                Some("Pair"),
            ),
            Ask::Show(shown) => (
                fill(
                    "Type this code on {name}, then press Enter there",
                    &[("name", &name)],
                )
                .into(),
                Some(code(shown.clone())),
                false,
                None,
            ),
            Ask::Cancel => (SharedString::default(), None, false, None),
        }
    }

    /// Answers the pairing dialog: `answer` is the code or yes, None is no.
    pub(super) fn answer_pairing(
        &mut self,
        answer: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((Dialog::Pairing { request, .. }, _)) = self.dialog.take() else {
            return;
        };
        self.code_field.update(cx, |input, cx| input.clear(cx));
        match request.reply {
            Some(reply) => {
                let _ = reply.try_send(answer);
            }
            // Only a code on show: no ends the pairing itself.
            None => self.cancel_pairing(&request.device, cx),
        }
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn cancel_pairing(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(link) = self.bluetooth.link.clone() else {
            return;
        };
        let Some(device) = self.bluetooth.state.device(path).cloned() else {
            return;
        };
        cx.spawn(async move |_, _| link.cancel_pairing(&device).await)
            .detach();
    }
}
