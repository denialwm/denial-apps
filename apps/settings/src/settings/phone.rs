//! About Denial: the wordmark, the ways to support the project, and its
//! credits.
//!
//! The About phone page: the phone's model and Denial's version at the top,
//! then the phone's name and addresses, its storage, its hardware and its
//! software. The name is the one nearby devices and Android see, renamed
//! here or on the Bluetooth page; everything else is read, not set.

use denial_look::INSET;
use denial_look::words::{Language, fill, language, tr};
use denial_look_gpui::{Icon, control_fill, icon, island, say, solid};
use gpui::{Context, Div, FontWeight, SharedString, div, prelude::*, px, svg};

use super::parts::{ROW_GAP, ROW_PAD, TILE};
use super::{Layer, Settings};
use crate::about;

const WORDMARK_LETTERS: &[u8] = include_bytes!("../../assets/denial-wordmark-letters.svg");
const WORDMARK_ACCENT: &[u8] = include_bytes!("../../assets/denial-wordmark-accent.svg");

/// Where Denial is supported, as its website's support page has them.
pub(super) const SPONSOR_PAGE: &str = "https://sponsor.denialwm.org/en";
pub(super) const SPONSOR_PAGE_CHINESE: &str = "https://sponsor.denialwm.org/cn";
pub(super) const GITHUB_SPONSORS: &str = "https://github.com/sponsors/doctorlogix";
const KO_FI: &str = "https://ko-fi.com/E5P523D5FN";
pub(super) const REPOSITORY: &str = "https://github.com/denialwm/denial";
const ISSUES: &str = "https://github.com/denialwm/denial/issues";

/// A row that opens a web page: its icon, what it is, what it does, and
/// where it leads.
type Link = (&'static str, Icon, &'static str, &'static str, &'static str);

const SUPPORT: &[Link] = &[
    (
        "about-sponsor",
        Icon::Heart,
        "Support Denial",
        "One-off support",
        SPONSOR_PAGE,
    ),
    (
        "about-github-sponsors",
        Icon::Person,
        "GitHub Sponsors",
        "Monthly or one-time",
        GITHUB_SPONSORS,
    ),
    (
        "about-ko-fi",
        Icon::Cup,
        "Ko-fi",
        "Monthly or one-time",
        KO_FI,
    ),
];

const HELP: &[Link] = &[
    (
        "about-star",
        Icon::Star,
        "Star Denial on GitHub",
        "Most people find Denial that way",
        REPOSITORY,
    ),
    (
        "about-report",
        Icon::Alert,
        "Report what breaks",
        "Test on your hardware and say what went wrong",
        ISSUES,
    ),
];

/// What Denial stands on: the work, and what it does here.
const BUILT_ON: &[(&str, &str)] = &[
    ("Flutter", "The shell and everything it draws"),
    ("Smithay", "The compositor's Wayland building blocks"),
    ("GPUI", "The native apps, this one among them"),
    ("Yaru", "App icons, under CC BY-SA 4.0"),
];

impl Settings {
    /// About Denial: the wordmark, how to support the project, and who
    /// and what it comes from.
    pub(super) fn about_denial_page(
        &self,
        width: f32,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let live = layer.live();
        let wordmark_width = (width - 2.0 * (INSET + ROW_PAD)).clamp(1.0, 420.0);
        let secondary = solid(self.look.palette.secondary);
        let hero = island(&self.look)
            .px(px(ROW_PAD))
            .py(px(40.))
            .flex()
            .justify_center()
            .child(self.wordmark(wordmark_width));

        let free = div()
            .p(px(ROW_PAD))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(say("Denial and Droidloom are free for everyone.")),
            )
            .child(div().text_size(px(14.)).text_color(secondary).child(say(
                "Nothing is locked behind a sponsorship: no paywalls, no sponsor-only \
                         builds, no waiting period. Sponsoring pays for the development time, \
                         test devices and hosting behind them.",
            )));
        let support = SUPPORT.iter().enumerate().fold(
            island(&self.look).child(free),
            |island, (ix, link)| {
                island.child(self.separator(0.)).child(self.link_row(
                    link,
                    (false, ix + 1 == SUPPORT.len()),
                    live,
                    cx,
                ))
            },
        );
        let help = self.rows_island(HELP.iter(), ROW_PAD + TILE + ROW_GAP, |link, ends| {
            self.link_row(link, ends, live, cx)
        });

        let author = island(&self.look)
            .px(px(ROW_PAD))
            .py(px(24.))
            .flex()
            .flex_col()
            .items_center()
            .text_center()
            .gap(px(6.))
            .child(self.caption("Conceived, directed and tested by"))
            .child(
                div()
                    .text_size(px(26.))
                    .font_weight(FontWeight::BOLD)
                    .child("Doctor Logix"),
            )
            .child(
                div()
                    .max_w(px(440.))
                    .text_size(px(14.))
                    .text_color(secondary)
                    .child(say(
                        "Built in continuous collaboration with AI models, mostly Sol 6.1 and \
                         Opus 5.5.",
                    )),
            );
        let built_on = self.rows_island(BUILT_ON.iter(), 0., |(name, does), ends| {
            self.row(*name, None, *name, None, ends)
                .child(self.value(*does))
        });

        vec![
            hero,
            self.chip_row("Support"),
            support,
            self.chip_row("No money? It still helps"),
            help,
            self.chip_row("Credits"),
            author,
            self.chip_row("Built on"),
            built_on,
        ]
    }

    /// Denial's wordmark, `width` wide: its letters in the foreground, its
    /// mark in Denial's own purple.
    pub(super) fn wordmark(&self, width: f32) -> gpui::Stateful<Div> {
        div()
            .id("denial-wordmark")
            .role(gpui::Role::Image)
            .aria_label("Denial")
            .relative()
            .flex_none()
            .w(px(width))
            .h(px(width * 150.0 / 490.0))
            .child(
                svg()
                    .data(WORDMARK_LETTERS)
                    .absolute()
                    .inset_0()
                    .text_color(solid(self.look.palette.foreground)),
            )
            .child(
                svg()
                    .data(WORDMARK_ACCENT)
                    .absolute()
                    .inset_0()
                    .text_color(solid(0x7150B8)),
            )
    }

    /// A row that opens `link`'s page in the browser.
    fn link_row(
        &self,
        &(key, glyph, title, detail, url): &Link,
        ends: (bool, bool),
        live: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let row = self
            .row(key, Some(glyph), title, Some(say(detail)), ends)
            .child(icon(
                Icon::Outgoing,
                px(20.),
                solid(self.look.palette.secondary),
            ));
        // The sponsor page has one address for each language.
        self.live(row, live, cx, move |_, _, cx| {
            cx.open_url(match (url, language()) {
                (SPONSOR_PAGE, Language::Chinese) => SPONSOR_PAGE_CHINESE,
                _ => url,
            })
        })
    }

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
            .gap(px(6.))
            .p(px(ROW_PAD))
            .pt(px(22.))
            .pb(px(20.))
            .child(self.tile(Icon::Phone, 56.))
            .child(
                div()
                    .pt(px(6.))
                    .text_size(px(20.))
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
            .gap(px(10.))
            .px(px(ROW_PAD))
            .py(px(14.))
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
                    .h(px(6.))
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
