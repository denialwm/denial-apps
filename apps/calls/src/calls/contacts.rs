//! Contacts: the favourites island, then one island per initial letter with
//! the letter inside it; the search bar at the top of Recents and Contacts;
//! and the detail of a contact, or of a number no one has.

use denial_look::words::{fill, tr};
use std::ops::Range;
use std::time::Instant;

use denial_contacts::{dialable, same_number};
use denial_look::mobile::{AVATAR, GROUP_HEIGHT, TOUCH, content_top};
use denial_look::{INSET, floating};
use denial_look_gpui::{
    Icon, chip, color, control_fill, control_radius, dp, glass_group, icon, island, shadow, solid,
};
use gpui::{
    AnyElement, Context, Div, Focusable, FontWeight, SharedString, Stateful, StyledText, Window,
    div, list, prelude::*, px, relative,
};

use super::parts::{Face, Fill, ICON, ROW_AVATAR, ROW_PAD, Segment, place};
use super::{Calls, Entry, Found, HISTORY, Held, Layer, Screen, Who};
use crate::data::{self, Direction};

/// A favourite's avatar, in dp.
const FAVOURITE_AVATAR: f32 = 54.0;
/// The avatar over a name island, in dp.
const LARGE_AVATAR: f32 = 88.0;

impl Calls {
    /// What the contacts list holds: the favourites, when there are any,
    /// then everyone by name.
    pub(super) fn contact_entries(&self) -> Vec<Entry> {
        let count = self.people.all().len();
        let favourites = (0..count).any(|index| self.favourite(index));
        favourites
            .then_some(Entry::Favourites)
            .into_iter()
            .chain((0..count).map(Entry::Contact))
            .collect()
    }

    fn favourite(&self, index: usize) -> bool {
        self.favourites.contains(self.people.key(index))
    }

    /// The contacts, with a state in their place when there are none.
    pub(super) fn contacts_list(
        &self,
        layer: Layer,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.entries.is_empty() {
            let action = self.samples_hidden.on().then(|| {
                self.button(
                    "show-samples",
                    Face::Both(Icon::Person, tr("Show sample contacts")),
                    Fill::Accent,
                    layer.live(),
                    cx,
                    |this, _, cx| this.show_samples(true, cx),
                )
            });
            let island = self.state_island(
                Icon::Person,
                tr("No contacts"),
                tr("Contacts are read from the vCard files in ~/.local/share/contacts."),
                action,
            );
            return div()
                .px(px(INSET))
                .pt(px(content_top()))
                .child(place(island, self.item_motion(layer, 0, false, now)))
                .into_any_element();
        }
        let first = self.contacts.logical_scroll_top().item_ix;
        let bottom = self.bottom_padding(&Screen::Contacts, now);
        list(
            self.contacts.clone(),
            cx.processor(move |this, ix: usize, _, cx| {
                let entries = &this.entries;
                let count = entries.len();
                let Some(&entry) = entries.get(ix) else {
                    return div().into_any_element();
                };
                let motion =
                    |lifted| this.item_motion(layer, ix.saturating_sub(first), lifted, now);
                let (item, motion) = match entry {
                    Entry::Favourites => (this.favourites_island(layer, cx), motion(false)),
                    Entry::Contact(index) => {
                        let letter = |ix: usize| match entries[ix] {
                            Entry::Favourites => String::new(),
                            Entry::Contact(index) => data::letter(&this.people.all()[index].name),
                        };
                        let segment = Segment::of(ix, count, letter);
                        let label = segment.first.then(|| letter(ix));
                        let held = Held::Contact(this.people.key(index).to_owned());
                        let lift = this.lift(&held, now);
                        let row = this.contact_row(index, segment, label, layer, lift, now, cx);
                        (row, motion(lift.is_some()))
                    }
                };
                place(
                    div()
                        .px(px(INSET))
                        .when(ix == 0, |item| item.pt(px(content_top() - INSET)))
                        .when(ix + 1 == count, |item| item.pb(px(bottom)))
                        .child(item),
                    motion,
                )
                .into_any_element()
            }),
        )
        .size_full()
        .into_any_element()
    }

    /// The favourites, four to a line, in one island.
    fn favourites_island(&self, layer: Layer, cx: &mut Context<Self>) -> Div {
        let mut tiles = div().flex().flex_wrap().px(dp(6.)).pb(dp(8.));
        for index in (0..self.people.all().len()).filter(|&index| self.favourite(index)) {
            let contact = &self.people.all()[index];
            let first_name = contact.name.split_whitespace().next().unwrap_or_default();
            let mut tile = div()
                .id(("favourite", index))
                .w(relative(0.25))
                .flex()
                .flex_col()
                .items_center()
                .gap(dp(6.))
                .px(dp(4.))
                .py(dp(8.))
                .rounded(control_radius(&self.look))
                .child(self.avatar(Some(index), "", FAVOURITE_AVATAR))
                .child(
                    div()
                        .max_w_full()
                        .truncate()
                        .text_size(dp(13.))
                        .child(first_name.to_owned()),
                );
            if layer.live() {
                let who = Who::Contact(self.people.key(index).to_owned());
                tile = self.press(tile, Fill::Clear).on_click(cx.listener(
                    move |this, _, window, cx| {
                        cx.stop_propagation();
                        if this.overlay.is_some() {
                            return this.dismiss(cx);
                        }
                        this.open(Screen::Person(who.clone()), window, cx)
                    },
                ));
            }
            tiles = tiles.child(tile);
        }
        self.segment(
            Segment::ALONE,
            Some(tr("Favourites").into()),
            None,
            None,
            tiles,
        )
    }

    /// Contact `index` as a row of its letter's island. Tapped, it opens the
    /// contact; held, it lifts and its actions show under it.
    #[allow(clippy::too_many_arguments)]
    fn contact_row(
        &self,
        index: usize,
        segment: Segment,
        label: Option<String>,
        layer: Layer,
        lift: Option<f32>,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> Div {
        let contact = &self.people.all()[index];
        let key = self.people.key(index).to_owned();
        let mut row = self
            .segment_row(("contact", index), segment)
            .py(dp(8.))
            .child(self.avatar(Some(index), "", ROW_AVATAR))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(contact.name.clone()),
            );
        if layer.live() {
            let who = Who::Contact(key.clone());
            row = row.on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                if this.overlay.is_some() {
                    return this.dismiss(cx);
                }
                this.open(Screen::Person(who.clone()), window, cx);
            }));
            row = self.holdable(row, Held::Contact(key), cx);
        }
        div()
            .child(self.segment(segment, label, None, lift, row))
            .children(lift.map(|_| self.contact_actions(index, now, cx)))
    }

    /// The actions under a held contact: Call and Message on their first
    /// number, and Favourite.
    fn contact_actions(&self, index: usize, now: Instant, cx: &mut Context<Self>) -> Div {
        let key = self.people.key(index).to_owned();
        let favourite = self.favourite(index);
        let mut buttons = Vec::new();
        if let Some(phone) = self.people.all()[index].phones.first() {
            let (dial, write) = (phone.number.clone(), phone.number.clone());
            buttons.push(self.button(
                "call",
                Face::Both(Icon::Phone, tr("Call")),
                Fill::Clear,
                true,
                cx,
                move |this, window, cx| this.dial(dial.clone(), window, cx),
            ));
            buttons.push(self.button(
                "message",
                Face::Both(Icon::Message, tr("Message")),
                Fill::Clear,
                true,
                cx,
                move |this, _, cx| this.message(&write, cx),
            ));
        }
        buttons.push(self.button(
            "favourite",
            if favourite {
                Face::Both(Icon::Starred, tr("Unfavourite"))
            } else {
                Face::Both(Icon::Star, tr("Favourite"))
            },
            Fill::Clear,
            true,
            cx,
            move |this, _, cx| this.toggle_favourite(key.clone(), cx),
        ));
        self.action_group(now, buttons)
    }

    /// The search bar at the top of Recents and Contacts: a glass group
    /// with the field, and Clear while it holds anything. A tap anywhere on
    /// it starts typing; the keyboard rises over the screen and the bar stays
    /// where it is.
    pub(super) fn search_bar(&self, layer: Layer, cx: &mut Context<Self>) -> Stateful<Div> {
        let live = layer.live();
        let ink = solid(self.look.palette.foreground);
        let clear = (live && self.searched(cx)).then(|| {
            self.button(
                "clear-search",
                Face::Icon(Icon::Close),
                Fill::Clear,
                true,
                cx,
                |this, window, cx| this.end_search(window, cx),
            )
        });
        let end_pad = if clear.is_some() { 4. } else { 16. };
        let mut bar = glass_group(&self.look)
            .id(match layer {
                Layer::Shown => "search-bar",
                Layer::Leaving => "leaving-search-bar",
            })
            .flex_1()
            .min_w_0()
            .h(dp(GROUP_HEIGHT))
            .flex()
            .items_center()
            .gap(dp(10.))
            .pl(dp(16.))
            .pr(dp(end_pad))
            .child(icon(Icon::Search, dp(ICON), ink))
            .child(div().flex_1().min_w_0().child(self.search.clone()))
            .children(clear);
        if live {
            bar = bar
                .cursor_text()
                .on_click(cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.dismiss(cx);
                    window.focus(&this.search.focus_handle(cx), cx);
                }));
        }
        bar
    }

    /// What the search field's text finds, best first: the typed number when
    /// no contact has it, then the contacts whose name or number match.
    pub(super) fn find_items(&self, cx: &gpui::App) -> Vec<Found> {
        let query = self.search.read(cx).text().trim().to_owned();
        if query.is_empty() {
            return Vec::new();
        }
        let lower = query.to_lowercase();
        let number = dialable(&query);
        let digits = data::digits(&query);
        let people = self.people.all();
        let matches: Vec<usize> = (0..people.len())
            .filter(|&index| {
                let contact = &people[index];
                contact.name.to_lowercase().contains(&lower)
                    || (number.is_some()
                        && digits.len() >= 2
                        && contact
                            .phones
                            .iter()
                            .any(|phone| data::digits(&phone.number).contains(&digits)))
            })
            .collect();
        let mut items = Vec::new();
        if let Some(number) = number {
            let known = matches.iter().any(|&index| {
                people[index]
                    .phones
                    .iter()
                    .any(|phone| same_number(&phone.number, &number))
            });
            if !known {
                items.push(Found::Dial(query.clone()));
            }
        }
        items.extend(matches.into_iter().map(Found::Contact));
        if items.is_empty() {
            items.push(Found::Chip(tr("No contact matches")));
        }
        items
    }

    /// The search results under the bar, on `screen`.
    pub(super) fn found_list(
        &self,
        screen: &Screen,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let screen = screen.clone();
        list(
            self.found.clone(),
            cx.processor(move |this, ix: usize, _, cx| {
                let now = Instant::now();
                let bottom = this.bottom_padding(&screen, now);
                let items = &this.found_items;
                let count = items.len();
                let motion = this.item_motion(layer, ix, false, now);
                let item = match items.get(ix) {
                    Some(Found::Chip(label)) => div()
                        .pt(dp(10.))
                        .pl(dp(AVATAR) + px(INSET))
                        .flex()
                        .child(chip(&this.look).child(*label)),
                    Some(Found::Dial(number)) => this.dial_row(number.clone(), layer, cx),
                    Some(&Found::Contact(index)) => {
                        let segment =
                            Segment::of(ix, count, |ix| matches!(items[ix], Found::Contact(_)));
                        let label = segment.first.then(|| {
                            let matched = items
                                .iter()
                                .filter(|item| matches!(item, Found::Contact(_)))
                                .count();
                            fill("Contacts · {count}", &[("count", &matched.to_string())])
                        });
                        this.found_contact(index, segment, label, layer, cx)
                    }
                    None => div(),
                };
                place(
                    div()
                        .px(px(INSET))
                        .when(ix == 0, |item| item.pt(px(content_top() - INSET)))
                        .when(ix + 1 == count, |item| item.pb(px(bottom)))
                        .child(item),
                    motion,
                )
                .into_any_element()
            }),
        )
        .size_full()
        .into_any_element()
    }

    /// A typed number no contact has, on an accent island of its own.
    fn dial_row(&self, number: String, layer: Layer, cx: &mut Context<Self>) -> Div {
        let ink = self.look.on_accent;
        let mut row = self
            .segment_row("dial-typed", Segment::ALONE)
            .text_color(solid(ink))
            .child(
                div()
                    .flex_none()
                    .size(dp(ROW_AVATAR))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(color(ink, 0.15))
                    .child(icon(Icon::Phone, dp(ICON), solid(ink))),
            )
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
                            .child(fill("Call {number}", &[("number", &number)])),
                    )
                    .child(
                        div()
                            .text_size(dp(14.))
                            .opacity(0.8)
                            .child(tr("Not in your contacts")),
                    ),
            );
        if layer.live() {
            row = row.on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.dial(number.clone(), window, cx);
            }));
        }
        self.segment(Segment::ALONE, None, Some(self.look.accent), None, row)
    }

    /// A contact the search found, the part of its name that matches marked,
    /// with its matching number and a button that calls it.
    fn found_contact(
        &self,
        index: usize,
        segment: Segment,
        label: Option<String>,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Div {
        let query = self.search.read(cx).text().trim().to_owned();
        let contact = &self.people.all()[index];
        let digits = data::digits(&query);
        let phone = contact
            .phones
            .iter()
            .find(|phone| !digits.is_empty() && data::digits(&phone.number).contains(&digits))
            .or(contact.phones.first());
        let detail = phone.map_or_else(
            || tr("No number").to_owned(),
            |phone| format!("{} · {}", tr(phone.kind.label()), phone.number),
        );
        let name = StyledText::new(contact.name.clone())
            .with_highlights(mark(&contact.name, &query).map(|range| (range, self.mark_style())));
        let live = layer.live();
        let mut row = self
            .segment_row(("found", index), segment)
            .child(self.avatar(Some(index), "", ROW_AVATAR))
            .child(self.row_text(name, None, div().truncate().child(detail)));
        if let Some(phone) = phone {
            let number = phone.number.clone();
            row = row.child(self.button(
                ("found-dial", index),
                Face::Icon(Icon::Phone),
                Fill::Clear,
                live,
                cx,
                move |this, window, cx| this.dial(number.clone(), window, cx),
            ));
        }
        if live {
            let who = Who::Contact(self.people.key(index).to_owned());
            row = row.on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open(Screen::Person(who.clone()), window, cx);
            }));
        }
        self.segment(segment, label, None, None, row)
    }

    /// A detail screen's top group: Back, and Favourite for a contact.
    pub(super) fn person_top(&self, who: &Who, layer: Layer, cx: &mut Context<Self>) -> Div {
        let star = self.contact_of(who).map(|index| {
            let key = self.people.key(index).to_owned();
            let glyph = if self.favourite(index) {
                Icon::Starred
            } else {
                Icon::Star
            };
            self.button(
                "favourite",
                Face::Icon(glyph),
                Fill::Clear,
                layer.live(),
                cx,
                move |this, _, cx| this.toggle_favourite(key.clone(), cx),
            )
        });
        self.titled_top("", layer, cx).children(star)
    }

    /// The detail of a contact, or of a number no one has: who, the commands
    /// for them, their numbers and their recent calls.
    pub(super) fn person(
        &self,
        who: &Who,
        layer: Layer,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let live = layer.live();
        let contact = self.contact_of(who);
        let history: Vec<_> = self.history(who).take(HISTORY).collect();
        let mut parts: Vec<Div> = Vec::new();
        match (who, contact) {
            (Who::Contact(_), None) => parts.push(self.state_island(
                Icon::Person,
                tr("This contact is gone"),
                tr("Its card was removed from the contacts folder."),
                None,
            )),
            (Who::Contact(_), Some(index)) => {
                let detail = match history.first() {
                    Some(call) => fill(
                        "Last call {time}",
                        &[("time", &data::full_time(call.time, data::now()))],
                    ),
                    None => tr("No calls yet").into(),
                };
                let name = self.people.all()[index].name.clone();
                parts.push(self.name_island(Some(index), "", name, vec![self.secondary(detail)]));
                let number = self.number_of(who);
                parts.push(self.commands(number, Some(index), live, cx));
                parts.push(self.numbers(index, live, cx));
            }
            (Who::Number(number), _) => {
                parts.push(self.name_island(
                    None,
                    number,
                    number.clone(),
                    vec![self.secondary(tr("Not in your contacts"))],
                ));
                parts.push(self.commands(Some(number.clone()), None, live, cx));
            }
        }
        if !history.is_empty() {
            let mut calls = island(&self.look).child(self.island_label(tr("Recent calls")));
            for call in history {
                let missed = call.direction == Direction::Missed;
                let ink = if missed {
                    self.look.palette.error
                } else {
                    self.look.palette.foreground
                };
                let glyph = match call.direction {
                    Direction::Outgoing => Icon::Outgoing,
                    Direction::Incoming => Icon::Incoming,
                    Direction::Missed | Direction::Declined => Icon::Missed,
                };
                calls = calls.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(dp(11.))
                        .px(dp(ROW_PAD))
                        .py(dp(9.))
                        .child(
                            div()
                                .flex_none()
                                .size(dp(33.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(control_radius(&self.look))
                                .bg(control_fill(&self.look))
                                .child(icon(glyph, dp(18.), solid(ink))),
                        )
                        .child(self.row_text(
                            call.summary(),
                            missed.then_some(ink),
                            div().child(format!(
                                "{} · {}",
                                data::full_time(call.time, data::now()),
                                call.number
                            )),
                        )),
                );
            }
            parts.push(calls.pb(dp(6.)));
        }
        div()
            .id("person")
            .size_full()
            .overflow_y_scroll()
            .px(px(INSET))
            .pt(px(content_top()))
            .pb(px(self.bottom_padding(&Screen::Person(who.clone()), now)))
            .flex()
            .flex_col()
            .gap(px(INSET))
            .children(
                parts
                    .into_iter()
                    .enumerate()
                    .map(|(ix, part)| place(part, self.item_motion(layer, ix, false, now))),
            )
            .into_any_element()
    }

    /// An island with a name, a large avatar bare over its top edge, and
    /// `lines` under the name: a contact, a number, or whom a call is with.
    pub(super) fn name_island(
        &self,
        contact: Option<usize>,
        number: &str,
        name: String,
        lines: Vec<Div>,
    ) -> Div {
        let half = LARGE_AVATAR / 2.0;
        div().pt(dp(half)).child(
            island(&self.look)
                .relative()
                .flex()
                .flex_col()
                .items_center()
                .gap(dp(3.))
                .px(dp(20.))
                .pt(dp(half + 12.))
                .pb(dp(20.))
                .child(
                    div()
                        .absolute()
                        .top(-dp(half))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(
                            self.avatar(contact, number, LARGE_AVATAR)
                                .rounded_full()
                                .shadow(shadow(&self.look)),
                        ),
                )
                .child(
                    div()
                        .max_w_full()
                        .truncate()
                        .text_size(dp(26.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .line_height(relative(1.25))
                        .child(name),
                )
                .children(lines),
        )
    }

    pub(super) fn secondary(&self, text: impl Into<SharedString>) -> Div {
        div()
            .max_w_full()
            .truncate()
            .text_color(solid(self.look.palette.secondary))
            .child(text.into())
    }

    /// Call, Message, and Share for a contact or Copy for a number, in one
    /// glass group.
    fn commands(
        &self,
        number: Option<String>,
        contact: Option<usize>,
        live: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut group = glass_group(&self.look).flex().p(dp(4.)).gap(dp(4.));
        if let Some(number) = number.clone() {
            group = group.child(self.command("call", Icon::Phone, tr("Call"), live, cx, {
                move |this, window, cx| this.dial(number.clone(), window, cx)
            }));
        }
        if let Some(number) = number.clone() {
            group = group.child(
                self.command("message", Icon::Message, tr("Message"), live, cx, {
                    move |this, _, cx| this.message(&number, cx)
                }),
            );
        }
        group = match (contact, number) {
            (Some(index), _) => group.child(self.command(
                "share",
                Icon::Share,
                tr("Share"),
                live,
                cx,
                move |this, _, cx| this.share(index, cx),
            )),
            (None, Some(number)) => group.child(self.command(
                "copy",
                Icon::Copy,
                tr("Copy"),
                live,
                cx,
                move |this, _, cx| this.copy(number.clone(), tr("Number copied"), cx),
            )),
            (None, None) => group,
        };
        group
    }

    /// A command of a glass group: its icon over its name.
    fn command(
        &self,
        id: &'static str,
        glyph: Icon,
        label: &'static str,
        live: bool,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let ink = solid(self.look.palette.foreground);
        let command = div()
            .id(id)
            .flex_1()
            .min_h(dp(TOUCH))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(dp(4.))
            .py(dp(8.))
            .rounded(control_radius(&self.look))
            .text_size(dp(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(icon(glyph, dp(ICON), ink))
            .child(label);
        if !live {
            return command;
        }
        self.press(command, Fill::Clear)
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                action(this, window, cx)
            }))
    }

    /// A contact's numbers in one island, each with its own Message and Call.
    /// A long press copies one.
    fn numbers(&self, index: usize, live: bool, cx: &mut Context<Self>) -> Div {
        let contact = &self.people.all()[index];
        let palette = &self.look.palette;
        let mut numbers = island(&self.look).child(self.island_label(tr("Phone")));
        if contact.phones.is_empty() {
            numbers = numbers.child(
                div()
                    .px(dp(ROW_PAD))
                    .pt(dp(4.))
                    .pb(dp(14.))
                    .text_color(solid(palette.secondary))
                    .child(tr("No phone number")),
            );
        }
        let radius = px(self.look.group_radius());
        for (ix, phone) in contact.phones.iter().enumerate() {
            let last = ix + 1 == contact.phones.len();
            let (dial, write, copy) = (
                phone.number.clone(),
                phone.number.clone(),
                phone.number.clone(),
            );
            let mut row = div()
                .id(("number", ix))
                .flex()
                .items_center()
                .gap(dp(8.))
                .pl(dp(ROW_PAD))
                .pr(dp(8.))
                .py(dp(6.))
                .when(last, |row| row.rounded_b(radius))
                .when(ix > 0, |row| {
                    row.border_t(px(floating::EDGE_WIDTH))
                        .border_color(color(palette.separator, palette.separator_alpha))
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().truncate().child(phone.number.clone()))
                        .child(
                            div()
                                .text_size(dp(12.))
                                .text_color(solid(palette.secondary))
                                .child(tr(phone.kind.label())),
                        ),
                )
                .child(self.button(
                    ("write", ix),
                    Face::Icon(Icon::Message),
                    Fill::Control,
                    live,
                    cx,
                    move |this, _, cx| this.message(&write, cx),
                ))
                .child(self.button(
                    ("ring", ix),
                    Face::Icon(Icon::Phone),
                    Fill::Control,
                    live,
                    cx,
                    move |this, window, cx| this.dial(dial.clone(), window, cx),
                ));
            if live {
                row = self.long_press(row, cx, move |this, cx| {
                    this.copy(copy.clone(), tr("Number copied"), cx)
                });
            }
            numbers = numbers.child(row);
        }
        numbers.when(!contact.phones.is_empty(), |numbers| numbers.pb(dp(4.)))
    }
}

/// Where `query` is in `name`, case ignored, to mark it. Names whose lower
/// case changes their length are left unmarked.
fn mark(name: &str, query: &str) -> Option<Range<usize>> {
    let lower = name.to_lowercase();
    if query.is_empty() || lower.len() != name.len() {
        return None;
    }
    let query = query.to_lowercase();
    let start = lower.find(&query)?;
    Some(start..start + query.len())
}
