//! The home screen: the pages in one island per group, each group's name
//! inside its island, and search over the pages and the settings on them.

use denial_look::INSET;
use denial_look::words::fill;
use denial_look_gpui::say;
use denial_look_gpui::{Icon, control_fill, icon, island, solid};
use gpui::{Context, Div, SharedString, div, prelude::*, px};

use super::parts::{ICON, ROW_GAP, TILE};
use super::{Layer, Screen, Settings};
use crate::catalog::{self, Content, GROUPS, Page};

/// A row's tile where the list is the whole window; beside a page it is
/// `TILE`, which leaves the names more of the list's width.
const HOME_TILE: f32 = 40.0;

impl Settings {
    /// The home screen's islands, top to bottom: the groups, or what search
    /// finds.
    pub(super) fn home(
        &self,
        selected: Option<&str>,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let query = self.search.read(cx).text().trim().to_lowercase();
        // Beside a page the search field is always there.
        if (self.searching || self.wide) && !query.is_empty() {
            return self.found(&query, selected, layer, cx);
        }
        GROUPS
            .iter()
            .filter_map(|group| {
                let pages: Vec<&'static Page> = group
                    .pages
                    .iter()
                    .filter(|page| catalog::shown(page))
                    .collect();
                // A group this machine has nothing of is not listed.
                if pages.is_empty() {
                    return None;
                }
                let count = pages.len();
                let rows = pages.into_iter().enumerate().map(|(ix, page)| {
                    let summary = match page.content {
                        Content::Wifi => self.wifi_summary(),
                        Content::Mobile => self.mobile_summary(),
                        Content::Location => self.location_summary(),
                        Content::Bluetooth => self.bluetooth_summary(),
                        Content::Battery => self.battery_summary(),
                        Content::About => self.about_summary(),
                        Content::Storage => self.storage_summary(),
                        Content::Plugins => self.plugins_summary(),
                        Content::Developer => self.developer_summary(),
                        Content::Android(_) => Some(self.android_summary()),
                        Content::Monitor(_) => self.monitor_summary(),
                        Content::Settings(_)
                        | Content::Apps(_)
                        | Content::AboutDenial
                        | Content::Lights
                        | Content::Display
                        | Content::OutputConfiguration
                        | Content::Input
                        | Content::Permissions
                        | Content::Profile
                        | Content::Environment
                        | Content::Shortcuts
                        | Content::Hidden(_)
                        | Content::Unavailable => None,
                    };
                    // Beside a page a row says only what it found: the
                    // page's own description does not fit the list there.
                    let summary = if self.wide {
                        summary
                    } else {
                        Some(summary.unwrap_or(page.summary.into()))
                    };
                    self.page_row(
                        page,
                        summary,
                        (self.wide && ix == 0, ix + 1 == count),
                        selected == Some(page.id),
                        layer,
                        cx,
                    )
                });
                Some(self.group(group.title, rows))
            })
            .collect()
    }

    /// `rows` in an island named `label`. Where the list is the whole
    /// window the name is inside its island; beside a page it is a chip
    /// over the island, as the page names its sections.
    fn group(
        &self,
        label: impl Into<SharedString>,
        rows: impl Iterator<Item = impl IntoElement>,
    ) -> Div {
        if self.wide {
            div()
                .flex()
                .flex_col()
                .gap(px(INSET))
                .child(self.chip_row(label).pl(px(self.nav_side())))
                .child(self.rows(island(&self.look), rows))
        } else {
            self.rows(self.labelled(label), rows)
        }
    }

    /// `rows` in `island`, a hairline between each two.
    fn rows(&self, island: Div, rows: impl Iterator<Item = impl IntoElement>) -> Div {
        rows.enumerate().fold(island, |island, (ix, row)| {
            island
                .when(ix > 0, |island| {
                    island.child(self.separator(self.nav_side() + self.home_tile() + ROW_GAP))
                })
                .child(row)
        })
    }

    /// How large a row's tile is here; its separator starts after it.
    pub(super) fn home_tile(&self) -> f32 {
        if self.wide { TILE } else { HOME_TILE }
    }

    /// A row that opens `page`, saying `detail` under its title; faint and
    /// still when the page is not on the phone yet.
    fn page_row(
        &self,
        page: &'static Page,
        detail: Option<SharedString>,
        (first, last): (bool, bool),
        selected: bool,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        // The shown page's row is filled and marked in the accent; both
        // fade from the row the selection leaves to the one it goes to.
        let (fill, accent) = (control_fill(&self.look), solid(self.look.accent));
        let radius = px(self.look.group_radius());
        let pad = self.nav_side();
        let mark = self.eased("selected", selected, move |at| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .when(first, |mark| mark.rounded_t(radius))
                .when(last, |mark| mark.rounded_b(radius))
                .bg(fill)
                .opacity(at)
                .child(
                    div()
                        .absolute()
                        .left(px(4.))
                        .top(px(pad))
                        .bottom(px(pad))
                        .w(px(3.))
                        .rounded_full()
                        .bg(accent),
                )
        });
        let row = self
            .nav_row(page.id, page.icon, page.title, detail, (first, last))
            .relative()
            .child(mark);
        if matches!(page.content, Content::Unavailable) {
            row.opacity(0.5)
        } else if layer.live() {
            let row = self.tappable(row, cx, move |this, window, cx| {
                this.open(Screen::Page(page), window, cx)
            });
            if selected { row } else { self.hovered(row) }
        } else {
            row
        }
    }

    /// What `query` finds: the pages whose name or summary has it, then the
    /// settings whose name or detail has it, each opening its page.
    fn found(
        &self,
        query: &str,
        selected: Option<&str>,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        // In English, and in the language shown.
        let has = |text: &str| {
            text.to_lowercase().contains(query)
                || denial_look::words::translate(text).is_some_and(|shown| shown.contains(query))
        };
        let pages: Vec<&'static Page> = catalog::listed()
            .filter(|page| has(page.title) || has(page.summary))
            .collect();
        let settings: Vec<_> = catalog::items()
            .filter(|(_, _, item)| {
                self.applies(item) && (has(item.title) || item.detail.is_some_and(has))
            })
            .collect();
        if pages.is_empty() && settings.is_empty() {
            return vec![self.notice_row("Nothing found")];
        }
        let mut islands = Vec::new();
        if !pages.is_empty() {
            let count = pages.len();
            let rows = pages.iter().enumerate().map(|(ix, page)| {
                self.page_row(
                    page,
                    Some(page.summary.into()),
                    (self.wide && ix == 0, ix + 1 == count),
                    selected == Some(page.id),
                    layer,
                    cx,
                )
            });
            islands.push(self.group(
                fill("Pages · {count}", &[("count", &count.to_string())]),
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
                    let row = self.nav_row(
                        item.key,
                        page.icon,
                        item.title,
                        Some(place.into()),
                        (self.wide && ix == 0, ix + 1 == count),
                    );
                    let page = *page;
                    let disclosure = section.disclosure;
                    if layer.live() {
                        let row = self.tappable(row, cx, move |this, window, cx| {
                            if let Some(title) = disclosure
                                && this.expanded_sections.insert(title)
                            {
                                cx.notify();
                            }
                            this.open(Screen::Page(page), window, cx)
                        });
                        self.hovered(row)
                    } else {
                        row
                    }
                });
            islands.push(self.group(
                fill("Settings · {count}", &[("count", &count.to_string())]),
                rows,
            ));
        }
        islands
    }

    /// The home screen's top group: the title and Search; while searching,
    /// the search field and a button that ends it. Beside a page the field
    /// is always there, over the list, and the button shows once it holds
    /// text.
    pub(super) fn home_top(&self, layer: Layer, cx: &mut Context<Self>) -> Div {
        let live = layer.live();
        if self.wide {
            let typed = !self.search.read(cx).text().is_empty();
            let field = self
                .field(
                    icon(Icon::Search, px(ICON), solid(self.look.palette.secondary)),
                    &self.search,
                )
                .h(px(40.));
            return self.top_row().px_0().child(field).children(typed.then(|| {
                self.icon_button("end-search", Icon::Close, live, cx, |this, window, cx| {
                    this.end_search(window, cx)
                })
            }));
        }
        if self.searching {
            return self
                .top_row()
                .child(self.field(
                    icon(Icon::Search, px(ICON), solid(self.look.palette.secondary)),
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
            .pl(px(17.))
            .child(self.title("Settings"))
            .child(
                self.icon_button("search", Icon::Search, live, cx, |this, window, cx| {
                    this.start_search(window, cx)
                }),
            )
    }
}
