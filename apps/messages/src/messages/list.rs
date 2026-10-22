//! The conversation lists, search, New conversation and the list's menu.
//! Conversations sit as rows in section islands: Pinned, Unread, then the
//! rest, each with its label inside.

use denial_look::words::{fill, tr};
use std::ops::Range;
use std::time::Instant;

use denial_contacts::dialable;
use denial_look::mobile::{AVATAR, GROUP_HEIGHT, TOUCH, content_top};
use denial_look::{CONTROL_FILL, INSET, MIN_RADIUS, floating};
use denial_look_gpui::{
    Icon, chip, color, control_radius, dp, glass_group, icon, island_edge, island_fill, island_run,
    lifted, solid,
};
use gpui::{
    AnyElement, Context, Div, Entity, Focusable, FontWeight, HighlightStyle, SharedString,
    StyledText, Window, div, list, prelude::*, px,
};

use super::parts::{CHIP_ICON, Face, Fill, ICON, place};
use super::{Back, Candidate, Found, Held, Layer, Link, Messages, Overlay, Screen};
use crate::data;
use denial_look_gpui::input::TextInput;

/// A row's padding from its island's edge, in dp.
const ROW_PAD: f32 = 16.0;
/// A row's avatar, in dp.
const ROW_AVATAR: f32 = 42.0;
/// Between a row's avatar and its text, in dp.
const ROW_GAP: f32 = 12.0;

/// The section island a conversation lists in, in list order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Section {
    Pinned,
    Unread,
    Earlier,
}

/// Where a row sits in its section island: the first carries the island's
/// top corners and label, the last its bottom corners.
#[derive(Clone, Copy)]
struct Segment {
    first: bool,
    last: bool,
}

impl Segment {
    /// Item `ix` of a list whose items fall into islands by `key`.
    fn of<K: PartialEq>(ix: usize, count: usize, key: impl Fn(usize) -> K) -> Self {
        Self {
            first: ix == 0 || key(ix - 1) != key(ix),
            last: ix + 1 == count || key(ix + 1) != key(ix),
        }
    }
}

impl Messages {
    /// What a conversation list shows, as indices into `conversations`:
    /// pinned ones, unread ones, then the rest, newest first in each.
    pub(super) fn rows_for(&self, archived: bool) -> Vec<usize> {
        let mut rows: Vec<usize> = (0..self.conversations.len())
            .filter(|&i| self.conversations[i].archived == archived)
            .collect();
        // Stable: dmail lists them newest first.
        rows.sort_by_key(|&i| self.section_of(i));
        rows
    }

    /// The section island conversation `index` lists in.
    pub(super) fn section_of(&self, index: usize) -> Section {
        let conversation = &self.conversations[index];
        if self.pins.contains(&conversation.peer) {
            Section::Pinned
        } else if self.unread(conversation) {
            Section::Unread
        } else {
            Section::Earlier
        }
    }

    /// The label of `section`'s island among `shown`.
    fn section_label(&self, section: Section, shown: &[usize]) -> String {
        match section {
            Section::Pinned => tr("Pinned").into(),
            Section::Unread => {
                let count = shown
                    .iter()
                    .filter(|&&i| self.section_of(i) == Section::Unread)
                    .count();
                fill("Unread · {count}", &[("count", &count.to_string())])
            }
            Section::Earlier
                if shown
                    .first()
                    .is_some_and(|&i| self.section_of(i) != section) =>
            {
                tr("Earlier").into()
            }
            Section::Earlier => tr("Conversations").into(),
        }
    }

    /// The conversations, or the archived ones, with a state in their place
    /// when there are none to show.
    pub(super) fn conversation_list(
        &self,
        archived: bool,
        layer: Layer,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let shown = if archived {
            &self.archived_shown
        } else {
            &self.shown
        };
        if shown.is_empty() {
            return self.empty_list(archived, layer, now, cx);
        }
        let state = if archived { &self.archive } else { &self.rows };
        let first = state.logical_scroll_top().item_ix;
        list(
            state.clone(),
            cx.processor(move |this, ix: usize, _, cx| {
                let shown = if archived {
                    &this.archived_shown
                } else {
                    &this.shown
                };
                let Some(&index) = shown.get(ix) else {
                    return div().into_any_element();
                };
                let segment = Segment::of(ix, shown.len(), |ix| this.section_of(shown[ix]));
                let label = segment
                    .first
                    .then(|| this.section_label(this.section_of(index), shown));
                let held = Held::Conversation(this.conversations[index].peer.clone());
                let lift = this.lift(&held, now);
                let motion = this.item_motion(layer, ix.saturating_sub(first), lift.is_some(), now);
                place(
                    div()
                        .px(px(INSET))
                        .when(ix == 0, |item| item.pt(px(content_top() - INSET)))
                        .when(ix + 1 == shown.len(), |item| {
                            item.pb(px(this.list_bottom(Instant::now())))
                        })
                        .child(
                            this.conversation_row(
                                index, None, segment, label, layer, lift, now, cx,
                            ),
                        ),
                    motion,
                )
                .into_any_element()
            }),
        )
        .size_full()
        .into_any_element()
    }

    /// The one island in place of a list with nothing to show.
    pub(super) fn empty_list(
        &self,
        archived: bool,
        layer: Layer,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let live = layer.live();
        let island = match &self.link {
            Link::Connecting => return div().into_any_element(),
            Link::Unavailable(reason) => self.state_island(
                Icon::Alert,
                reason,
                "dmail keeps your messages and sends them. Until Messages reaches it, nothing \
                 can be shown or sent.",
                Some(self.button(
                    "reconnect",
                    Face::Both(Icon::Retry, tr("Try again")),
                    Fill::Accent,
                    live,
                    cx,
                    |this, _, cx| this.reconnect(cx),
                )),
            ),
            Link::Ready(_) if archived => self.state_island(
                Icon::Archive,
                tr("No archived conversations"),
                tr("An archived conversation comes back here when a new message arrives in it."),
                None,
            ),
            Link::Ready(_) => self.state_island(
                Icon::Message,
                tr("No messages yet"),
                tr("Texts you send and receive with this SIM appear here."),
                Some(self.button(
                    "first",
                    Face::Both(Icon::New, tr("New message")),
                    Fill::Accent,
                    live,
                    cx,
                    |this, window, cx| this.open(Screen::Compose, window, cx),
                )),
            ),
        };
        div()
            .px(px(INSET))
            .pt(px(content_top()))
            .child(place(island, self.item_motion(layer, 0, false, now)))
            .into_any_element()
    }

    /// One row's share of a section island, smoke or the opaque `fill`: the
    /// label over the first row, a separator from the text column between
    /// rows. `lift` raises it off the frost.
    fn segment(
        &self,
        segment: Segment,
        label: Option<String>,
        fill: Option<u32>,
        lift: Option<f32>,
        row: impl IntoElement,
    ) -> Div {
        let look = &self.look;
        let palette = &look.palette;
        let radius = px(look.group_radius());
        let (fill, edge) = match fill {
            Some(fill) => (solid(fill), None),
            None => (island_fill(look), island_edge(look)),
        };
        let mut island = div().bg(fill);
        if segment.first {
            island = island.rounded_t(radius);
        }
        if segment.last {
            island = island.rounded_b(radius);
        }
        if let Some(color) = edge {
            let edge = px(floating::EDGE_WIDTH);
            island = island
                .border_color(color)
                .border_l(edge)
                .border_r(edge)
                .when(segment.first, |island| island.border_t(edge))
                .when(segment.last, |island| island.border_b(edge));
        }
        if let Some(t) = lift {
            island = island.shadow(lifted(t));
        }
        div().when(segment.first, |item| item.pt(px(INSET))).child(
            island
                .children(label.map(|label| {
                    div()
                        .px(dp(ROW_PAD))
                        .pt(dp(11.))
                        .pb(dp(3.))
                        .text_size(dp(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(solid(look.accent_text))
                        .child(label.to_uppercase())
                }))
                .children((!segment.first).then(|| {
                    div()
                        .h(px(1.))
                        .ml(dp(ROW_PAD + ROW_AVATAR + ROW_GAP))
                        .bg(color(palette.separator, palette.separator_alpha))
                }))
                .child(row),
        )
    }

    /// The press of a row in a segment, its corners matching the island's.
    fn segment_row(&self, id: impl Into<gpui::ElementId>, segment: Segment) -> gpui::Stateful<Div> {
        let row = div()
            .id(id)
            .flex()
            .items_center()
            .gap(dp(ROW_GAP))
            .px(dp(ROW_PAD))
            .py(dp(10.))
            .when(segment.last, |row| {
                row.rounded_b(px(self.look.group_radius()))
            });
        self.press(row, Fill::Clear)
    }

    /// Conversation `index` as a row of its section island, the parts of
    /// its name that match `query` marked. Held, it lifts and its actions
    /// show under it.
    #[allow(clippy::too_many_arguments)]
    fn conversation_row(
        &self,
        index: usize,
        query: Option<&str>,
        segment: Segment,
        label: Option<String>,
        layer: Layer,
        lift: Option<f32>,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> Div {
        let conversation = &self.conversations[index];
        let peer = conversation.peer.clone();
        let unread = self.unread(conversation);
        let palette = &self.look.palette;
        let name = self.people.name(&peer).to_owned();
        let marks = query.map_or_else(Vec::new, |query| data::matches(&name, query));
        let preview = match (&conversation.last_text, conversation.local) {
            (_, true) => match self.drafts.get(&peer) {
                Some(draft) => fill("Draft: {draft}", &[("draft", draft)]),
                None => tr("New conversation").into(),
            },
            (text, false) if conversation.last_outgoing => fill("You: {text}", &[("text", text)]),
            (text, false) => text.clone(),
        };
        let (preview, time) = (
            preview.replace('\n', " "),
            data::list_time(conversation.last_time, data::now()),
        );
        let ink = if unread {
            palette.foreground
        } else {
            palette.secondary
        };
        let dot = if conversation.unread > 0 || self.marked.contains(&peer) {
            Some(1.0)
        } else {
            conversation
                .read_fade
                .filter(|(fade, _)| fade.running(now))
                .map(|(fade, _)| 1.0 - fade.progress(now))
        };
        let mut row =
            self.segment_row(("conversation", index), segment)
                .child(self.avatar(&peer, ROW_AVATAR))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(dp(2.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(dp(7.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .font_weight(if unread {
                                            FontWeight::BOLD
                                        } else {
                                            FontWeight::SEMIBOLD
                                        })
                                        .child(self.marked_text(name, marks)),
                                )
                                .children(self.pins.contains(&peer).then(|| {
                                    icon(Icon::Pin, dp(CHIP_ICON), solid(palette.secondary))
                                }))
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(dp(12.))
                                        .text_color(solid(ink))
                                        .child(time),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(dp(7.))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(dp(14.))
                                        .text_color(solid(ink))
                                        .child(preview),
                                )
                                .children(dot.map(|opacity| {
                                    div()
                                        .flex_none()
                                        .size(dp(10.))
                                        .rounded_full()
                                        .bg(solid(self.look.accent))
                                        .opacity(opacity)
                                })),
                        ),
                );
        if layer.live() {
            let open = peer.clone();
            row = row.on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                if this.overlay.is_some() {
                    return this.dismiss(cx);
                }
                this.open(Screen::Thread(open.clone()), window, cx);
            }));
            row = self.holdable(row, Held::Conversation(peer.clone()), cx);
        }
        div()
            .child(self.segment(segment, label, None, lift, row))
            .children(lift.map(|_| self.conversation_actions(&peer, now, cx)))
    }

    /// Text with the `marks` drawn bold in the accent text colour.
    fn marked_text(&self, text: String, marks: Vec<Range<usize>>) -> StyledText {
        let mark = HighlightStyle {
            color: Some(solid(self.look.accent_text)),
            font_weight: Some(FontWeight::BOLD),
            ..Default::default()
        };
        StyledText::new(text).with_highlights(marks.into_iter().map(|range| (range, mark)))
    }

    /// The actions under a held conversation.
    fn conversation_actions(&self, peer: &str, now: Instant, cx: &mut Context<Self>) -> Div {
        let Some(index) = self.conversation(peer) else {
            return div();
        };
        let conversation = &self.conversations[index];
        let pinned = self.pins.contains(peer);
        let unread = self.unread(conversation);
        let archived = conversation.archived;
        let blocked = self.is_blocked(peer);
        // Archive, block and delete are dmail's; a sample has none of them.
        let sample = conversation.sample;
        let kept = conversation.local || sample;
        let mut buttons = vec![
            self.button(
                "pin",
                Face::Both(Icon::Pin, if pinned { tr("Unpin") } else { tr("Pin") }),
                Fill::Clear,
                true,
                cx,
                action(peer, |this, peer, _| {
                    let pinned = this.pins.contains(&peer);
                    this.pins.set(&peer, !pinned);
                }),
            ),
            self.button(
                "unread",
                if unread {
                    Face::Both(Icon::Read, tr("Mark read"))
                } else {
                    Face::Both(Icon::Unread, tr("Mark unread"))
                },
                Fill::Clear,
                true,
                cx,
                action(peer, Self::toggle_unread),
            ),
        ];
        if !kept {
            buttons.push(self.button(
                "archive",
                if archived {
                    Face::Both(Icon::Unarchive, tr("Unarchive"))
                } else {
                    Face::Both(Icon::Archive, tr("Archive"))
                },
                Fill::Clear,
                true,
                cx,
                action(peer, move |this, peer, cx| {
                    this.set_archived(peer, !archived, cx)
                }),
            ));
            buttons.push(self.button(
                "block",
                Face::Both(
                    Icon::Block,
                    if blocked { tr("Unblock") } else { tr("Block") },
                ),
                Fill::Clear,
                true,
                cx,
                action(peer, move |this, peer, cx| {
                    if blocked {
                        this.block(peer, false, cx)
                    } else {
                        this.show(Overlay::Confirm(super::Confirm::Block(peer)), cx)
                    }
                }),
            ));
        }
        if !sample {
            buttons.push(self.button(
                "delete",
                Face::Both(Icon::Delete, tr("Delete")),
                Fill::Danger,
                true,
                cx,
                action(peer, |this, peer, cx| {
                    this.show(
                        Overlay::Confirm(super::Confirm::DeleteConversation(peer)),
                        cx,
                    )
                }),
            ));
        }
        self.action_group(now, buttons)
    }

    /// What the search field's text finds, top to bottom: dmail's hits under
    /// a chip, then the conversations whose name or number match.
    pub(super) fn find_items(&self, cx: &gpui::App) -> Vec<Found> {
        let query = self.search.read(cx).text().trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let mut items = Vec::new();
        let hits = &self.hits.1;
        if self.hits.0.to_lowercase() == query && !hits.is_empty() {
            items.push(Found::Chip(fill(
                "Messages · {count}",
                &[("count", &hits.len().to_string())],
            )));
            items.extend((0..hits.len()).map(Found::Hit));
        }
        items.extend(
            (0..self.conversations.len())
                .filter(|&i| {
                    let peer = &self.conversations[i].peer;
                    self.people.name(peer).to_lowercase().contains(&query)
                        || peer.to_lowercase().contains(&query)
                })
                .map(Found::Conversation),
        );
        if items.is_empty() {
            items.push(Found::Chip(tr("Nothing found").into()));
        }
        items
    }

    /// The search results under the field.
    pub(super) fn found_list(&self, layer: Layer, cx: &mut Context<Self>) -> AnyElement {
        list(
            self.found.clone(),
            cx.processor(move |this, ix: usize, _, cx| {
                let now = Instant::now();
                let items = &this.found_items;
                let count = items.len();
                let motion = this.item_motion(layer, ix, false, now);
                let query = this.search.read(cx).text().trim().to_owned();
                let item = match items.get(ix) {
                    Some(Found::Chip(label)) => this.section(label.clone()),
                    Some(Found::Hit(hit)) => this.hit(*hit, layer, cx),
                    Some(&Found::Conversation(index)) => {
                        let segment = Segment::of(ix, count, |ix| {
                            matches!(items[ix], Found::Conversation(_))
                        });
                        let label = segment.first.then(|| {
                            let matched = items
                                .iter()
                                .filter(|item| matches!(item, Found::Conversation(_)))
                                .count();
                            fill(
                                "Conversations · {count}",
                                &[("count", &matched.to_string())],
                            )
                        });
                        this.conversation_row(
                            index,
                            Some(&query),
                            segment,
                            label,
                            layer,
                            None,
                            now,
                            cx,
                        )
                    }
                    None => div(),
                };
                place(
                    div()
                        .px(px(INSET))
                        .when(ix == 0, |item| item.pt(px(content_top() - INSET)))
                        .when(ix + 1 == count, |item| {
                            item.pb(px(this.list_bottom(Instant::now())))
                        })
                        .child(item),
                    motion,
                )
                .into_any_element()
            }),
        )
        .size_full()
        .into_any_element()
    }

    /// dmail's hit `hit`: the message as an incoming bubble, its match in
    /// accent text, with who and when on a chip under it.
    fn hit(&self, hit: usize, layer: Layer, cx: &mut Context<Self>) -> Div {
        let Some((peer, id, outgoing, text, time, _)) = self.hits.1.get(hit).cloned() else {
            return div();
        };
        let marks = data::matches(&text, &self.hits.0);
        let who = if outgoing {
            tr("You").to_owned()
        } else {
            self.people.name(&peer).to_owned()
        };
        let mut bubble = island_run(&self.look)
            .id(("hit", hit))
            .max_w_full()
            .rounded(px(self.look.bubble_radius()))
            .rounded_bl(px(MIN_RADIUS))
            .px(dp(14.))
            .py(dp(9.))
            .line_height(gpui::relative(1.4))
            .child(self.marked_text(text, marks));
        if layer.live() {
            bubble = bubble.on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_at(Screen::Thread(peer.clone()), Some(id), false, window, cx);
            }));
        }
        div()
            .pt(dp(9.))
            .flex()
            .items_end()
            .gap(px(INSET))
            .child(self.avatar(&self.hits.1[hit].0, AVATAR))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(dp(4.))
                    .child(bubble)
                    .child(
                        chip(&self.look)
                            .child(format!("{who} · {}", data::full_time(time, data::now()))),
                    ),
            )
    }

    /// Who a new conversation can go to, best first: the typed number on an
    /// accent island of its own, then contacts and recent conversations in
    /// one island; a chip says why when there is no one.
    pub(super) fn candidate_list(&self, layer: Layer, cx: &mut Context<Self>) -> AnyElement {
        list(
            self.candidates.clone(),
            cx.processor(move |this, ix: usize, _, cx| {
                let now = Instant::now();
                let items = &this.candidate_items;
                let count = items.len();
                let motion = this.item_motion(layer, ix, false, now);
                let item = if count == 0 {
                    this.section(this.candidates_chip(cx))
                } else {
                    this.candidate(ix, Segment::of(ix, count, |ix| items[ix].typed), layer, cx)
                };
                place(
                    div()
                        .px(px(INSET))
                        .when(ix == 0, |item| item.pt(px(content_top() - INSET)))
                        .when(ix + 1 >= count, |item| {
                            item.pb(px(this.list_bottom(Instant::now())))
                        })
                        .child(item),
                    motion,
                )
                .into_any_element()
            }),
        )
        .size_full()
        .into_any_element()
    }

    fn candidates_chip(&self, cx: &gpui::App) -> &'static str {
        let typed = self.recipient.read(cx).text().trim();
        if typed.chars().any(|c| c.is_ascii_digit()) && dialable(typed).is_none() {
            tr("Not a phone number")
        } else {
            tr("No contact matches")
        }
    }

    fn candidate(
        &self,
        index: usize,
        segment: Segment,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(candidate) = self.candidate_items.get(index) else {
            return div();
        };
        let Candidate {
            peer,
            title,
            detail,
            typed,
        } = candidate;
        let (fill, ink, secondary) = if *typed {
            (
                Some(self.look.accent),
                self.look.on_accent,
                self.look.on_accent,
            )
        } else {
            let palette = &self.look.palette;
            (None, palette.foreground, palette.secondary)
        };
        let avatar = if *typed {
            div()
                .flex_none()
                .size(dp(ROW_AVATAR))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(color(ink, 0.15))
                .child(icon(Icon::Send, dp(ICON), solid(ink)))
        } else {
            self.avatar(peer, ROW_AVATAR)
        };
        let mut row = self
            .segment_row(("candidate", index), segment)
            .text_color(solid(ink))
            .child(avatar)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(dp(2.))
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(dp(14.))
                            .text_color(solid(secondary))
                            .when(*typed, |detail| detail.opacity(0.8))
                            .child(detail.replace('\n', " ")),
                    ),
            );
        if layer.live() {
            let peer = peer.clone();
            row = row.on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.start(peer.clone(), window, cx);
            }));
        }
        let label = (segment.first && !*typed).then(|| tr("Contacts and recent").to_owned());
        self.segment(segment, label, fill, None, row)
    }

    /// A text field in the top group: `lead`, a glyph or a label, before it.
    fn field(&self, lead: impl IntoElement, input: &Entity<TextInput>) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .h(dp(TOUCH))
            .flex()
            .items_center()
            .gap(dp(8.))
            .px(dp(11.))
            .rounded(control_radius(&self.look))
            .bg(color(self.look.palette.foreground, CONTROL_FILL))
            .text_color(solid(self.look.palette.secondary))
            .child(div().flex_none().child(lead))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(solid(self.look.palette.foreground))
                    .child(input.clone()),
            )
    }

    /// The list's top group: its title, search, the menu and New; while
    /// searching, the search field and a button that ends it.
    pub(super) fn list_top(&self, layer: Layer, cx: &mut Context<Self>) -> Div {
        let live = layer.live();
        if self.searching {
            return self
                .top_row()
                .child(self.field(
                    icon(Icon::Search, dp(ICON), solid(self.look.palette.secondary)),
                    &self.search,
                ))
                .child(self.button(
                    "end-search",
                    Face::Icon(Icon::Close),
                    Fill::Clear,
                    live,
                    cx,
                    |this, window, cx| this.back(&Back, window, cx),
                ));
        }
        self.top_row()
            .pl(dp(17.))
            .child(self.title("Messages"))
            .child(self.button(
                "search",
                Face::Icon(Icon::Search),
                Fill::Clear,
                live,
                cx,
                |this, window, cx| {
                    this.searching = true;
                    window.focus(&this.search.focus_handle(cx), cx);
                    cx.notify();
                },
            ))
            .child(self.button(
                "menu",
                Face::Icon(Icon::More),
                Fill::Clear,
                live,
                cx,
                |this, _, cx| this.show(Overlay::Menu, cx),
            ))
            .child(
                self.button(
                    "new",
                    Face::Icon(Icon::New),
                    Fill::Accent,
                    live,
                    cx,
                    |this, window, cx| this.open(Screen::Compose, window, cx),
                )
                .rounded_full(),
            )
    }

    /// New conversation's top group: Back and the To field.
    pub(super) fn compose_top(&self, layer: Layer, cx: &mut Context<Self>) -> Div {
        self.top_row()
            .child(self.back_button(layer, cx))
            .child(self.field(tr("To"), &self.recipient))
    }

    /// The row inside a top group.
    pub(super) fn top_row(&self) -> Div {
        div()
            .size_full()
            .flex()
            .items_center()
            .gap(dp(8.))
            .pl(dp(4.))
            .pr(dp(4.))
    }

    pub(super) fn title(&self, title: impl Into<SharedString>) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(dp(20.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(title.into())
    }

    /// A top group of a screen under another: Back, then its title.
    pub(super) fn titled_top(
        &self,
        title: impl Into<SharedString>,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Div {
        self.top_row()
            .child(self.back_button(layer, cx))
            .child(self.title(title))
    }

    pub(super) fn back_button(&self, layer: Layer, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        self.button(
            "back",
            Face::Icon(Icon::Back),
            Fill::Clear,
            layer.live(),
            cx,
            |this, window, cx| this.back(&Back, window, cx),
        )
    }

    /// The list's menu under the top group: the archive and settings.
    pub(super) fn menu(&self, now: Instant, cx: &mut Context<Self>) -> Option<Div> {
        if !matches!(self.overlay, Some((Overlay::Menu, _))) {
            return None;
        }
        let archived = self.conversations.iter().filter(|c| c.archived).count();
        Some(
            div()
                .absolute()
                .top(px(INSET * 2. - self.top_away.away()) + dp(GROUP_HEIGHT))
                .right(px(INSET))
                .opacity(self.dim(now))
                .child(
                    glass_group(&self.look)
                        .flex()
                        .flex_col()
                        .items_stretch()
                        .p(dp(4.))
                        .child(
                            self.button(
                                "archived",
                                Face::Both(Icon::Archive, tr("Archived")),
                                Fill::Clear,
                                true,
                                cx,
                                |this, window, cx| this.open(Screen::Archived, window, cx),
                            )
                            .justify_start()
                            .children((archived > 0).then(|| {
                                div()
                                    .text_color(solid(self.look.palette.secondary))
                                    .child(archived.to_string())
                            })),
                        )
                        .child(
                            self.button(
                                "settings",
                                Face::Both(Icon::Settings, tr("Settings")),
                                Fill::Clear,
                                true,
                                cx,
                                |this, window, cx| this.open(Screen::Settings, window, cx),
                            )
                            .justify_start(),
                        ),
                ),
        )
    }
}

/// A held conversation's action: the overlay closes and `run` gets its peer.
fn action(
    peer: &str,
    run: impl Fn(&mut Messages, String, &mut Context<Messages>) + 'static,
) -> impl Fn(&mut Messages, &mut Window, &mut Context<Messages>) + 'static {
    let peer = peer.to_owned();
    move |this, _, cx| {
        this.overlay = None;
        run(this, peer.clone(), cx);
        cx.notify();
    }
}
