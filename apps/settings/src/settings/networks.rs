//! Wi-Fi's screens: the page with the switch, the network the phone is on
//! and the networks in range; the saved networks; and one saved network,
//! with how it is joined and what the connection is. What the user does
//! goes to the manager through `wifi::Link`; what changes comes back as a
//! fresh `wifi::State`.

use std::future::Future;
use std::rc::Rc;

use denial_look_gpui::{Icon, dp, icon, island, solid};
use gpui::{Context, Div, FontWeight, SharedString, Window, div, prelude::*};

use super::parts::{Fill, ROW_GAP, ROW_PAD, TILE};
use super::{Dialog, Layer, Screen, Settings};
use crate::wifi::{self, Address, Metered, Phase, Security};

/// Where a row's separator starts: after its icon tile.
const SEPARATOR: f32 = ROW_PAD + TILE + ROW_GAP;

const METERED: [(Metered, &str); 3] = [
    (Metered::Auto, "Detect automatically"),
    (Metered::Yes, "Metered"),
    (Metered::No, "Not metered"),
];

const ADDRESSES: [(Address, &str); 3] = [
    (Address::Default, "Phone's default"),
    (Address::Random, "Private address"),
    (Address::Device, "Device address"),
];

/// Signal bars for a strength in percent.
fn signal(strength: u8) -> Icon {
    match wifi::bars(strength) {
        4 => Icon::Wifi,
        3 => Icon::WifiGood,
        2 => Icon::WifiFair,
        _ => Icon::WifiWeak,
    }
}

/// Why the manager said no, for a note.
fn refused(error: &zbus::Error) -> &'static str {
    match error {
        zbus::Error::MethodError(name, _, _)
            if name.contains("PermissionDenied") || name.contains("NotAuthorized") =>
        {
            "not allowed"
        }
        _ => "it did not work",
    }
}

/// The address alone, without its prefix length.
fn bare(address: &str) -> &str {
    address.split('/').next().unwrap_or(address)
}

impl Settings {
    /// The Wi-Fi page's islands, top to bottom: the switch and the network
    /// the phone is on, the networks in range, and the way to the saved
    /// ones and to a hidden one.
    pub(super) fn wifi_page(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let wifi = &self.wifi;
        let state = &wifi.state;
        if !wifi.read {
            return Vec::new();
        }
        let live = layer.live();
        let Some(manager) = state.manager.filter(|_| wifi.link.is_some()) else {
            return vec![self.missing(
                Icon::Wifi,
                "Wi-Fi is not available",
                "Neither NetworkManager nor iwd runs on this phone",
            )];
        };
        if state.device.is_none() {
            return vec![self.missing(
                Icon::Wifi,
                "No Wi-Fi hardware",
                format!("{} finds no Wi-Fi device", manager.label()),
            )];
        }

        let switch = self.row(
            "wifi-on",
            Some(Icon::Wifi),
            "Use Wi-Fi",
            state
                .blocked
                .then(|| "Off by airplane mode or a hardware switch".into()),
            (true, state.current.is_none() || !state.enabled),
        );
        let enabled = state.enabled;
        let switch = self.live(
            switch.child(self.toggle(enabled)),
            live,
            cx,
            move |this, _, cx| this.set_wifi(!enabled, cx),
        );
        let current = state.current.as_ref().filter(|_| enabled).map(|current| {
            let detail = current.phase.unwrap_or(Phase::Connecting).label();
            let glyph = current.strength.map_or(Icon::Wifi, signal);
            let row = self.row(
                "current",
                Some(glyph),
                current.name.clone(),
                Some(detail.into()),
                (false, true),
            );
            match current.saved.clone() {
                Some(id) => self.live(
                    row.child(self.chevron()),
                    live,
                    cx,
                    move |this, window, cx| {
                        let id = id.clone().into();
                        this.open(
                            Screen::Network {
                                id,
                                from_saved: false,
                            },
                            window,
                            cx,
                        )
                    },
                ),
                None => row,
            }
        });
        let mut parts = vec![island(&self.look).child(switch).children(
            current.map(|current| div().child(self.separator(SEPARATOR)).child(current)),
        )];

        if enabled {
            if state.networks.is_empty() {
                parts.push(self.chip_row(if state.scanning || wifi.joining.is_some() {
                    "Looking for networks…"
                } else {
                    "No networks in range"
                }));
            } else {
                parts.push(self.chip_row("Networks"));
                parts.push(
                    self.rows_island(state.networks.iter(), SEPARATOR, |network, ends| {
                        self.network_row(network, ends, live, cx)
                    }),
                );
            }
        }

        let count = state.saved.len();
        let saved = self
            .row(
                "saved",
                Some(Icon::Lock),
                "Saved networks",
                Some(match count {
                    0 => "None yet".into(),
                    1 => "1 network".into(),
                    count => format!("{count} networks").into(),
                }),
                (true, !enabled),
            )
            .child(self.chevron());
        let saved = self.live(saved, live, cx, |this, window, cx| {
            this.open(Screen::SavedNetworks, window, cx)
        });
        let hidden = enabled.then(|| {
            let row = self.row(
                "hidden",
                Some(Icon::New),
                "Add a hidden network",
                Some("One that does not show its name".into()),
                (false, true),
            );
            self.live(row, live, cx, |this, window, cx| {
                this.ask_hidden(None, window, cx)
            })
        });
        parts.push(
            island(&self.look).child(saved).children(
                hidden.map(|hidden| div().child(self.separator(SEPARATOR)).child(hidden)),
            ),
        );
        parts
    }

    /// A network in range: its bars, its name, and how it stands.
    fn network_row(
        &self,
        network: &wifi::Network,
        ends: (bool, bool),
        live: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let wifi = &self.wifi;
        let detail = if wifi.joining.as_ref() == Some(&network.ssid) {
            Phase::Connecting.label()
        } else if let Some(failure) = wifi.failed.get(&network.ssid) {
            failure
        } else if network.saved.is_some() {
            "Saved"
        } else {
            network.security.label()
        };
        let locked = network.saved.is_none() && network.security != Security::Open;
        let id = SharedString::from(format!("network-{}", network.name));
        let row = self
            .row(
                id,
                Some(signal(network.strength)),
                network.name.clone(),
                Some(detail.into()),
                ends,
            )
            .when(locked, |row| {
                row.child(icon(
                    Icon::Lock,
                    dp(18.),
                    solid(self.look.palette.secondary),
                ))
            });
        let network = network.clone();
        self.live(row, live, cx, move |this, window, cx| {
            this.choose_network(network.clone(), window, cx)
        })
    }

    /// An island that says why there is nothing to set.
    pub(super) fn missing(&self, glyph: Icon, title: &str, why: impl Into<SharedString>) -> Div {
        island(&self.look).child(self.row(
            "missing",
            Some(glyph),
            title.to_owned(),
            Some(why.into()),
            (true, true),
        ))
    }

    /// The networks the phone remembers, those in range first.
    pub(super) fn saved_networks(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let state = &self.wifi.state;
        if state.saved.is_empty() {
            return vec![self.chip_row("No saved networks")];
        }
        let live = layer.live();
        let mut saved: Vec<_> = state.saved.iter().collect();
        saved.sort_by_key(|saved| {
            let current = self.on_network(&saved.id).is_some();
            let near = state.in_range(&saved.id).is_some();
            (!current, !near, saved.name.to_lowercase())
        });
        vec![
            self.rows_island(saved.into_iter(), SEPARATOR, |saved, ends| {
                let (glyph, status) = self.standing(&saved.id);
                let mut detail = format!("{status} · {}", saved.security.label());
                if !saved.autoconnect {
                    detail.push_str(" · Joined by hand");
                }
                let row = self
                    .row(
                        SharedString::from(format!("saved-{}", saved.id)),
                        Some(glyph),
                        saved.name.clone(),
                        Some(detail.into()),
                        ends,
                    )
                    .child(self.chevron());
                let id: SharedString = saved.id.clone().into();
                self.live(row, live, cx, move |this, window, cx| {
                    let id = id.clone();
                    this.open(
                        Screen::Network {
                            id,
                            from_saved: true,
                        },
                        window,
                        cx,
                    )
                })
            }),
        ]
    }

    /// The network the phone is on or joining, when it is saved one `id`.
    fn on_network(&self, id: &str) -> Option<&wifi::Current> {
        let state = &self.wifi.state;
        state
            .current
            .as_ref()
            .filter(|current| state.enabled && current.saved.as_deref() == Some(id))
    }

    /// Saved network `id`'s bars and how it stands.
    fn standing(&self, id: &str) -> (Icon, &'static str) {
        let wifi = &self.wifi;
        let near = wifi.state.in_range(id);
        if let Some(current) = self.on_network(id) {
            let glyph = current.strength.map_or(Icon::Wifi, signal);
            return (glyph, current.phase.unwrap_or(Phase::Connecting).label());
        }
        match near {
            Some(network) if wifi.joining.as_ref() == Some(&network.ssid) => {
                (signal(network.strength), Phase::Connecting.label())
            }
            Some(network) => match wifi.failed.get(&network.ssid) {
                Some(failure) => (signal(network.strength), failure),
                None => (signal(network.strength), "In range"),
            },
            None if wifi.state.saved(id).is_some_and(|saved| saved.hidden) => {
                (Icon::Wifi, "Hidden network")
            }
            None => (Icon::Wifi, "Not in range"),
        }
    }

    /// One saved network: how it stands with Connect or Disconnect and
    /// Forget, the connection while the phone is on it, and how it is
    /// joined.
    pub(super) fn network(
        &self,
        id: &SharedString,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let state = &self.wifi.state;
        let Some(saved) = state.saved(id) else {
            return vec![self.chip_row("This network is no longer saved")];
        };
        let live = layer.live();
        let current = self.on_network(id);
        let near = state.in_range(id).cloned();
        let (glyph, status) = self.standing(id);

        let leave = current.is_some();
        let join = near.clone().filter(|_| !leave && state.enabled);
        let connect = if leave {
            Some(self.button(
                "disconnect",
                "Disconnect",
                Fill::Control,
                live,
                cx,
                |this, _, cx| this.disconnect(cx),
            ))
        } else {
            join.clone().map(|network| {
                self.button(
                    "connect",
                    "Connect",
                    Fill::Accent,
                    live,
                    cx,
                    move |this, window, cx| this.join(network.clone(), None, window, cx),
                )
            })
        };
        let forget = {
            let saved = saved.clone();
            self.button(
                "forget",
                "Forget",
                Fill::Danger,
                live,
                cx,
                move |this, _, cx| this.ask_forget(saved.clone(), cx),
            )
        };
        let hero = island(&self.look)
            .flex()
            .flex_col()
            .items_center()
            .gap(dp(6.))
            .p(dp(ROW_PAD))
            .pt(dp(22.))
            .child(self.tile(glyph, 56.))
            .child(
                div()
                    .pt(dp(6.))
                    .text_size(dp(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(saved.name.clone()),
            )
            .child(
                div()
                    .text_color(solid(self.look.palette.secondary))
                    .child(status),
            )
            .child(
                self.buttons(connect.into_iter().chain([forget]))
                    .w_full()
                    .pt(dp(12.)),
            );
        let mut parts = vec![hero];

        if let Some(current) = current.filter(|current| current.phase == Some(Phase::Connected)) {
            let mut info: Vec<(&str, String)> = Vec::new();
            if let Some(strength) = current.strength {
                info.push((
                    "Signal",
                    format!("{} · {strength}%", wifi::quality(strength)),
                ));
            }
            if let Some(frequency) = current.frequency {
                info.push(("Band", wifi::band(frequency).to_owned()));
            }
            if let Some(bitrate) = current.bitrate {
                info.push(("Link speed", format!("{bitrate} Mb/s")));
            }
            let security = current.security.unwrap_or(saved.security);
            info.push(("Security", security.label().to_owned()));
            if let Some(ipv4) = &current.ipv4 {
                info.push(("IP address", bare(ipv4).to_owned()));
            }
            if let Some(gateway) = &current.gateway {
                info.push(("Gateway", gateway.clone()));
            }
            if !current.dns.is_empty() {
                info.push(("DNS", current.dns.join(", ")));
            }
            if let Some(ipv6) = current
                .ipv6
                .iter()
                .find(|address| !address.starts_with("fe80"))
            {
                info.push(("IPv6 address", bare(ipv6).to_owned()));
            }
            if let Some(device) = &state.device {
                info.push(("MAC address", device.address.clone()));
            }
            parts.push(self.chip_row("Connection"));
            parts.push(
                self.rows_island(info.into_iter(), 0., |(title, value), ends| {
                    self.row(title, None, title, None, ends)
                        .child(self.value(value))
                }),
            );
        }

        parts.push(self.chip_row("Joining"));
        let mut rows: Vec<gpui::Stateful<Div>> = Vec::new();
        let autoconnect = saved.autoconnect;
        let row = self.row(
            "autoconnect",
            None,
            "Join by itself",
            Some("Whenever the network is in range".into()),
            (true, false),
        );
        let saved_id = saved.id.clone();
        rows.push(self.live(
            row.child(self.toggle(autoconnect)),
            live,
            cx,
            move |this, _, cx| this.set_autoconnect(&saved_id, !autoconnect, cx),
        ));
        if let Some(metered) = saved.metered {
            let picked = METERED.iter().position(|(m, _)| *m == metered).unwrap_or(0);
            let row = self
                .row(
                    "metered",
                    None,
                    "Metered",
                    Some("Billed by the amount of data".into()),
                    (false, false),
                )
                .child(self.value(METERED[picked].1))
                .child(self.chevron());
            let id = saved.id.clone();
            rows.push(self.live(row, live, cx, move |this, _, cx| {
                let id = id.clone();
                this.show(
                    Dialog::Pick {
                        title: "Metered".into(),
                        options: METERED.iter().map(|(_, label)| (*label).into()).collect(),
                        picked,
                        pick: Rc::new(move |this, option, _, cx| {
                            this.set_metered(&id, METERED[option].0, cx)
                        }),
                    },
                    cx,
                )
            }));
        }
        if let Some(address) = saved.address {
            let picked = ADDRESSES
                .iter()
                .position(|(a, _)| *a == address)
                .unwrap_or(0);
            let row = self
                .row(
                    "address",
                    None,
                    "Address",
                    Some("The hardware address the network sees".into()),
                    (false, false),
                )
                .child(self.value(ADDRESSES[picked].1))
                .child(self.chevron());
            let id = saved.id.clone();
            rows.push(self.live(row, live, cx, move |this, _, cx| {
                let id = id.clone();
                this.show(
                    Dialog::Pick {
                        title: "Address".into(),
                        options: ADDRESSES.iter().map(|(_, label)| (*label).into()).collect(),
                        picked,
                        pick: Rc::new(move |this, option, _, cx| {
                            this.set_address(&id, ADDRESSES[option].0, cx)
                        }),
                    },
                    cx,
                )
            }));
        }
        if let Some(network) = join.filter(|network| network.security.password()) {
            let row = self
                .row(
                    "new-password",
                    None,
                    "Join with a new password",
                    None,
                    (false, false),
                )
                .text_color(solid(self.look.accent_text))
                .font_weight(FontWeight::SEMIBOLD);
            rows.push(self.live(row, live, cx, move |this, window, cx| {
                this.ask_password(network.clone(), None, window, cx)
            }));
        }
        // The last row is rounded as the island is.
        let count = rows.len();
        let radius = gpui::px(self.look.group_radius());
        let rows = rows
            .into_iter()
            .enumerate()
            .map(|(ix, row)| row.when(ix + 1 == count, |row| row.rounded_b(radius)));
        parts.push(self.rows_island(rows, 0., |row, _| row));
        parts
    }

    /// Wi-Fi's line under its title on the home screen.
    pub(super) fn wifi_summary(&self) -> Option<SharedString> {
        let wifi = &self.wifi;
        let state = &wifi.state;
        if !wifi.read {
            return None;
        }
        Some(if wifi.link.is_none() {
            "Not available".into()
        } else if state.device.is_none() {
            "No Wi-Fi hardware".into()
        } else if !state.enabled {
            "Off".into()
        } else if let Some(current) = &state.current {
            match current.phase {
                Some(Phase::Connected) => current.name.clone().into(),
                _ => format!("Connecting to {}", current.name).into(),
            }
        } else {
            "Not connected".into()
        })
    }

    /// The manager and the device, while there are both.
    fn wifi_link(&self) -> Option<(wifi::Link, wifi::Device)> {
        let wifi = &self.wifi;
        wifi.link.clone().zip(wifi.state.device.clone())
    }

    /// Turns Wi-Fi on or off; the switch moves at once, and back if the
    /// manager says no.
    fn set_wifi(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some(link) = self.wifi.link.clone() else {
            return;
        };
        self.wifi.state.enabled = on;
        cx.notify();
        cx.spawn(async move |this, cx| {
            if let Err(error) = link.set_enabled(on).await {
                let _ = this.update(cx, |view, cx| {
                    view.wifi.state.enabled = !on;
                    let turn = if on { "on" } else { "off" };
                    view.note(
                        format!("Could not turn Wi-Fi {turn}: {}", refused(&error)),
                        cx,
                    );
                });
            }
        })
        .detach();
    }

    /// A tap on a network in range: joins it, asking for its password first
    /// when it takes one the phone does not have.
    fn choose_network(
        &mut self,
        network: wifi::Network,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.wifi.joining.as_ref() == Some(&network.ssid) {
            return;
        }
        if network.security == Security::Enterprise {
            return self.note(wifi::Failure::Unsupported.label(), cx);
        }
        if network.saved.is_none() && network.security.password() {
            self.ask_password(network, None, window, cx);
        } else {
            self.join(network, None, window, cx);
        }
    }

    /// Joins `network`, with `password` in place of a saved one. A wrong
    /// password asks again; other failures stay on its row.
    pub(super) fn join(
        &mut self,
        network: wifi::Network,
        password: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((link, device)) = self.wifi_link() else {
            return;
        };
        self.dismiss(window, cx);
        self.wifi.failed.remove(&network.ssid);
        self.wifi.joining = Some(network.ssid.clone());
        self._joining = cx.spawn_in(window, async move |this, cx| {
            let joined = link.connect(&device, &network, password).await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.wifi.joining = None;
                match joined {
                    Ok(()) => {}
                    Err(wifi::Failure::WrongPassword) => {
                        view.ask_password(network, Some("Wrong password, try again"), window, cx)
                    }
                    Err(failure) => {
                        view.note(format!("{}: {}", network.name, failure.label()), cx);
                        view.wifi.failed.insert(network.ssid, failure.label());
                    }
                }
                cx.notify();
            });
        });
        cx.notify();
    }

    /// Joins the hidden network `name`, open without a password. A failure
    /// asks again, keeping the name.
    pub(super) fn join_hidden(
        &mut self,
        name: String,
        password: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((link, device)) = self.wifi_link() else {
            return;
        };
        self.dismiss(window, cx);
        self.wifi.joining = Some(name.as_bytes().to_vec());
        self._joining = cx.spawn_in(window, async move |this, cx| {
            let joined = link.connect_hidden(&device, &name, password).await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.wifi.joining = None;
                if let Err(failure) = joined {
                    let error = match failure {
                        wifi::Failure::WrongPassword => "Wrong password, try again",
                        failure => failure.label(),
                    };
                    view.hidden_name
                        .update(cx, |input, cx| input.set_text(name, cx));
                    view.ask_hidden(Some(error), window, cx);
                }
                cx.notify();
            });
        });
        cx.notify();
    }

    fn disconnect(&mut self, cx: &mut Context<Self>) {
        let Some((link, device)) = self.wifi_link() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            if let Err(error) = link.disconnect(&device).await {
                let _ = this.update(cx, |view, cx| {
                    view.note(format!("Could not disconnect: {}", refused(&error)), cx)
                });
            }
        })
        .detach();
    }

    fn ask_forget(&mut self, saved: wifi::Saved, cx: &mut Context<Self>) {
        let consequence = if saved.security.password() {
            "The phone stops joining it, and its password is deleted."
        } else {
            "The phone stops joining it."
        };
        self.show(
            Dialog::Confirm {
                title: format!("Forget {}?", saved.name).into(),
                consequence: consequence.into(),
                action: "Forget",
                confirm: Rc::new(move |this, window, cx| this.forget(saved.clone(), window, cx)),
            },
            cx,
        );
    }

    /// Forgets `saved`, leaving its screen at once.
    fn forget(&mut self, saved: wifi::Saved, window: &mut Window, cx: &mut Context<Self>) {
        let Some(link) = self.wifi.link.clone() else {
            return;
        };
        if matches!(&self.screen, Screen::Network { id, .. } if *id == saved.id) {
            let parent = Self::parent(&self.screen);
            self.open_at(parent, true, window, cx);
        }
        cx.spawn(async move |this, cx| {
            if let Err(error) = link.forget(&saved).await {
                let _ = this.update(cx, |view, cx| {
                    view.note(
                        format!("Could not forget {}: {}", saved.name, refused(&error)),
                        cx,
                    )
                });
            }
        })
        .detach();
    }

    fn set_autoconnect(&mut self, id: &str, on: bool, cx: &mut Context<Self>) {
        self.change_saved(
            id,
            |saved| saved.autoconnect = on,
            move |link, saved| async move { link.set_autoconnect(&saved, on).await },
            cx,
        );
    }

    fn set_metered(&mut self, id: &str, metered: Metered, cx: &mut Context<Self>) {
        self.change_saved(
            id,
            |saved| saved.metered = Some(metered),
            move |link, saved| async move { link.set_metered(&saved, metered).await },
            cx,
        );
    }

    fn set_address(&mut self, id: &str, address: Address, cx: &mut Context<Self>) {
        self.change_saved(
            id,
            |saved| saved.address = Some(address),
            move |link, saved| async move { link.set_address(&saved, address).await },
            cx,
        );
    }

    /// Changes saved network `id` as `edit` does, at once, and has the
    /// manager keep it by `call`; puts it back if the manager says no.
    fn change_saved<F>(
        &mut self,
        id: &str,
        edit: impl FnOnce(&mut wifi::Saved),
        call: impl FnOnce(wifi::Link, wifi::Saved) -> F + 'static,
        cx: &mut Context<Self>,
    ) where
        F: Future<Output = zbus::Result<()>> + 'static,
    {
        let Some(link) = self.wifi.link.clone() else {
            return;
        };
        let Some(saved) = self
            .wifi
            .state
            .saved
            .iter_mut()
            .find(|saved| saved.id == id)
        else {
            return;
        };
        let before = saved.clone();
        edit(saved);
        let after = saved.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            if let Err(error) = call(link, after).await {
                let _ = this.update(cx, |view, cx| {
                    let saved = view.wifi.state.saved.iter_mut();
                    if let Some(saved) = saved.into_iter().find(|saved| saved.id == before.id) {
                        *saved = before;
                    }
                    view.note(format!("Could not change it: {}", refused(&error)), cx);
                });
            }
        })
        .detach();
    }
}
