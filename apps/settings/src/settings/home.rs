//! The home screen: the pages in one island per group, each group's name
//! inside its island, and search over the pages and the settings on them.

use denial_look::words::fill;
use denial_look_gpui::say;
use denial_look_gpui::{Icon, dp, icon, solid};
use gpui::{Context, Div, SharedString, prelude::*};

use super::parts::{ICON, ROW_GAP, ROW_PAD};
use super::{Layer, Screen, Settings};
use crate::catalog::{self, Content, GROUPS, Page};

/// Where a row's separator starts: after its 40 dp tile.
const SEPARATOR: f32 = ROW_PAD + 40.0 + ROW_GAP;

impl Settings {
    /// The home screen's islands, top to bottom: the groups, or what search
    /// finds.
    pub(super) fn home(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let query = self.search.read(cx).text().trim().to_lowercase();
        if self.searching && !query.is_empty() {
            return self.found(&query, layer, cx);
        }
        GROUPS
            .iter()
            .map(|group| {
                let pages: Vec<&'static Page> = group
                    .pages
                    .iter()
                    .filter(|page| !matches!(page.content, Content::Hidden(_)))
                    .collect();
                let count = pages.len();
                let rows = pages.into_iter().enumerate().map(|(ix, page)| {
                    let summary = match page.content {
                        Content::Wifi => self.wifi_summary(),
                        Content::Mobile => self.mobile_summary(),
                        Content::Bluetooth => self.bluetooth_summary(),
                        Content::Battery => self.battery_summary(),
                        Content::About => self.about_summary(),
                        Content::Storage => self.storage_summary(),
                        Content::Android(_) => Some(self.android_summary()),
                        Content::Settings(_)
                        | Content::Apps(_)
                        | Content::Display
                        | Content::Hidden(_)
                        | Content::Unavailable => None,
                    };
                    let summary = summary.unwrap_or(page.summary.into());
                    self.page_row(page, summary, ix + 1 == count, layer, cx)
                });
                self.rows(self.labelled(group.title), rows)
            })
            .collect()
    }

    /// `rows` in `island`, a hairline between each two.
    fn rows(&self, island: Div, rows: impl Iterator<Item = impl IntoElement>) -> Div {
        rows.enumerate().fold(island, |island, (ix, row)| {
            island
                .when(ix > 0, |island| island.child(self.separator(SEPARATOR)))
                .child(row)
        })
    }

    /// A row that opens `page`, saying `detail` under its title; faint and
    /// still when the page is not on the phone yet.
    fn page_row(
        &self,
        page: &'static Page,
        detail: SharedString,
        last: bool,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let row = self.nav_row(page.id, page.icon, page.title, detail, last);
        if matches!(page.content, Content::Unavailable) {
            row.opacity(0.5)
        } else if layer.live() {
            self.tappable(row, cx, move |this, window, cx| {
                this.open(Screen::Page(page), window, cx)
            })
        } else {
            row
        }
    }

    /// What `query` finds: the pages whose name or summary has it, then the
    /// settings whose name or detail has it, each opening its page.
    fn found(&self, query: &str, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        // In English, and in the language shown.
        let has = |text: &str| {
            text.to_lowercase().contains(query)
                || denial_look::words::translate(text).is_some_and(|shown| shown.contains(query))
        };
        let pages: Vec<&'static Page> = catalog::listed()
            .filter(|page| has(page.title) || has(page.summary))
            .collect();
        let settings: Vec<_> = catalog::items()
            .filter(|(_, _, item)| has(item.title) || item.detail.is_some_and(has))
            .collect();
        if pages.is_empty() && settings.is_empty() {
            return vec![self.chip_row("Nothing found")];
        }
        let mut islands = Vec::new();
        if !pages.is_empty() {
            let count = pages.len();
            let rows = pages.iter().enumerate().map(|(ix, page)| {
                self.page_row(page, page.summary.into(), ix + 1 == count, layer, cx)
            });
            islands.push(self.rows(
                self.labelled(fill("Pages · {count}", &[("count", &count.to_string())])),
                rows,
            ));
        }
        if !settings.is_empty() {
            let count = settings.len();
            let rows = settings
                .iter()
                .enumerate()
                .map(|(ix, (page, section, item))| {
                    let place = match section.title {
                        Some(section) => format!("{} · {}", say(page.title), say(section)),
                        None => say(page.title).to_string(),
                    };
                    let row = self.nav_row(item.key, page.icon, item.title, place, ix + 1 == count);
                    let page = *page;
                    if layer.live() {
                        self.tappable(row, cx, move |this, window, cx| {
                            this.open(Screen::Page(page), window, cx)
                        })
                    } else {
                        row
                    }
                });
            islands.push(self.rows(
                self.labelled(fill("Settings · {count}", &[("count", &count.to_string())])),
                rows,
            ));
        }
        islands
    }

    /// The home screen's top group: the title and Search; while searching,
    /// the search field and a button that ends it.
    pub(super) fn home_top(&self, layer: Layer, cx: &mut Context<Self>) -> Div {
        let live = layer.live();
        if self.searching {
            return self
                .top_row()
                .child(self.field(
                    icon(Icon::Search, dp(ICON), solid(self.look.palette.secondary)),
                    &self.search,
                ))
                .child(self.icon_button(
                    "end-search",
                    Icon::Close,
                    live,
                    cx,
                    |this, window, cx| this.end_search(window, cx),
                ));
        }
        self.top_row()
            .pl(dp(17.))
            .child(self.title("Settings"))
            .child(
                self.icon_button("search", Icon::Search, live, cx, |this, window, cx| {
                    this.start_search(window, cx)
                }),
            )
    }
}
