//! The About phone page: the phone's model and Denial's version at the top,
//! then the phone's name and addresses, its storage, its hardware and its
//! software. The name is the one nearby devices and Android see, renamed
//! here or on the Bluetooth page; everything else is read, not set.

use denial_look::words::{fill, tr};
use denial_look_gpui::{Icon, control_fill, dp, island, solid};
use gpui::{Context, Div, FontWeight, SharedString, div, prelude::*};

use super::parts::ROW_PAD;
use super::{Layer, Settings};
use crate::about;

impl Settings {
    /// Reads the phone's facts again, and Denial's version once.
    pub(super) fn read_about(&mut self, cx: &mut Context<Self>) {
        let link = self.denial.link.clone();
        let ask_version = self.about.denial.is_none();
        cx.spawn(async move |this, cx| {
            let facts = cx.background_spawn(async { about::read() }).await;
            let version = if ask_version {
                link.version().await.ok()
            } else {
                None
            };
            let _ = this.update(cx, |view, cx| {
                view.about.facts = Some(facts);
                if version.is_some() {
                    view.about.denial = version;
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn about_page(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let Some(facts) = &self.about.facts else {
            return Vec::new();
        };
        let mut parts = vec![self.about_hero(facts)];

        let adapter = self.bluetooth.state.adapter.as_ref();
        let renames = layer.live();
        let name = self.phone_name().unwrap_or_else(|| "Unknown".to_owned());
        let mut phone: Vec<(&str, String)> = Vec::new();
        let number = &self.mobile.state.number;
        if self.mobile.read && self.mobile.link.is_some() {
            phone.push((
                "Phone number",
                if number.is_empty() {
                    "Not on the SIM".to_owned()
                } else {
                    number.clone()
                },
            ));
        }
        if let Some(wifi) = &facts.wifi {
            phone.push(("Wi-Fi address", wifi.clone()));
        }
        if let Some(address) = adapter
            .map(|adapter| &adapter.address)
            .filter(|a| !a.is_empty())
        {
            phone.push(("Bluetooth address", address.clone()));
        }
        let rows = std::iter::once(None).chain(phone.into_iter().map(Some));
        let rows: Vec<_> = rows.collect();
        parts.push(self.chip_row("This phone"));
        parts.push(self.rows_island(rows.into_iter(), 0., |row, ends| {
            match row {
                None => {
                    let row = self
                        .row("about-name", None, "Device name", None, ends)
                        .child(self.value(name.clone()))
                        .when(renames, |row| row.child(self.chevron()));
                    self.live(row, renames, cx, |this, window, cx| {
                        this.ask_name(window, cx)
                    })
                }
                Some((title, value)) => self
                    .row(title, None, title, None, ends)
                    .child(self.value(value)),
            }
        }));

        if let Some((used, all)) = facts.storage {
            parts.push(self.chip_row("Storage"));
            parts.push(self.storage_island(used, all));
        }

        let mut hardware: Vec<(&str, String, Option<SharedString>)> = Vec::new();
        if let Some(chip) = &facts.chip {
            let mut detail = match facts.cores {
                0 => String::new(),
                1 => tr("1 core").to_owned(),
                cores => fill("{count} cores", &[("count", &cores.to_string())]),
            };
            if let Some(ghz) = facts.top_ghz {
                if !detail.is_empty() {
                    detail.push_str(tr(", "));
                }
                detail.push_str(&fill(
                    "up to {speed} GHz",
                    &[("speed", &format!("{ghz:.2}"))],
                ));
            }
            hardware.push((
                "Processor",
                chip.clone(),
                (!detail.is_empty()).then(|| detail.into()),
            ));
        }
        if let Some(memory) = facts.memory {
            hardware.push(("Memory", about::memory_size(memory), None));
        }
        if let Some((width, height)) = facts.panel {
            hardware.push(("Display", format!("{width} × {height}"), None));
        }
        if !hardware.is_empty() {
            parts.push(self.chip_row("Hardware"));
            parts.push(self.rows_island(
                hardware.into_iter(),
                0.,
                |(title, value, detail), ends| {
                    self.row(title, None, title, detail, ends)
                        .child(self.value(value))
                },
            ));
        }

        let mut software: Vec<(&str, String, Option<SharedString>)> = Vec::new();
        if let Some(denial) = &self.about.denial {
            software.push(("Denial", denial.clone(), None));
        }
        if let Some(system) = &facts.system {
            software.push(("System", system.clone(), None));
        }
        if let Some(kernel) = &facts.kernel {
            let built = facts
                .kernel_built
                .as_ref()
                .map(|day| fill("Built {day}", &[("day", day)]).into());
            software.push(("Kernel", kernel.clone(), built));
        }
        if let Some(up) = facts.up {
            software.push(("Up time", about::up_time(up), None));
        }
        if !software.is_empty() {
            parts.push(self.chip_row("Software"));
            parts.push(self.rows_island(
                software.into_iter(),
                0.,
                |(title, value, detail), ends| {
                    self.row(title, None, title, detail, ends)
                        .child(self.value(value))
                },
            ));
        }
        parts
    }

    /// The phone's model, large, over Denial's version.
    fn about_hero(&self, facts: &about::Facts) -> Div {
        let model = facts
            .model
            .clone()
            .unwrap_or_else(|| "This phone".to_owned());
        let under = match &self.about.denial {
            Some(version) => format!("Denial {version}"),
            None => "Denial".to_owned(),
        };
        island(&self.look)
            .flex()
            .flex_col()
            .items_center()
            .gap(dp(6.))
            .p(dp(ROW_PAD))
            .pt(dp(22.))
            .pb(dp(20.))
            .child(self.tile(Icon::Phone, 56.))
            .child(
                div()
                    .pt(dp(6.))
                    .text_size(dp(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(model),
            )
            .child(
                div()
                    .text_color(solid(self.look.palette.secondary))
                    .child(under),
            )
    }

    /// What is used of the storage, in words and as a bar.
    pub(super) fn storage_island(&self, used: u64, all: u64) -> Div {
        let filled = (used as f32 / all as f32).clamp(0.0, 1.0);
        island(&self.look)
            .flex()
            .flex_col()
            .gap(dp(10.))
            .px(dp(ROW_PAD))
            .py(dp(14.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(fill("{size} used", &[("size", &about::bytes(used))]))
                    .child(self.value(fill("of {size}", &[("size", &about::bytes(all))]))),
            )
            .child(
                div()
                    .w_full()
                    .h(dp(6.))
                    .rounded_full()
                    .bg(control_fill(&self.look))
                    .child(
                        div()
                            .h_full()
                            .w(gpui::relative(filled))
                            .rounded_full()
                            .bg(solid(self.look.accent)),
                    ),
            )
    }

    /// The owner's name for the phone, else Bluetooth's, else the system's.
    pub(super) fn phone_name(&self) -> Option<String> {
        let adapter = self.bluetooth.state.adapter.as_ref();
        (self.about.facts.as_ref())
            .and_then(|facts| facts.name.clone())
            .or_else(|| adapter.map(|adapter| adapter.name.clone()))
            .filter(|name| !name.is_empty())
            .or_else(about_hostname)
    }

    /// About's line under its title on the home screen: the model.
    pub(super) fn about_summary(&self) -> Option<SharedString> {
        self.about
            .facts
            .as_ref()?
            .model
            .clone()
            .map(SharedString::from)
    }
}

/// The system's name, when BlueZ cannot say the phone's.
fn about_hostname() -> Option<String> {
    let name = std::fs::read_to_string("/proc/sys/kernel/hostname").ok()?;
    let name = name.trim().to_owned();
    (!name.is_empty()).then_some(name)
}
