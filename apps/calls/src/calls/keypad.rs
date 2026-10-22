//! The keypad, on the tab bar: the contacts and recent numbers the typed
//! digits could be, on an island; the number on its own island; each key an
//! island in a run, its letters under its digit; and under the keys the
//! round green Call, with backspace beside it. The digits find contacts by
//! number and by name, spelled on the letters of the keys.

use denial_look::words::tr;
use std::ops::Range;

use denial_contacts::same_number;
use denial_look::INSET;
use denial_look_gpui::{Icon, dp, island, island_run, solid};
use gpui::{
    Context, Div, FontWeight, SharedString, StyledText, Window, div, prelude::*, px, relative,
};

use super::parts::{Face, Fill, ROW_AVATAR, Segment};
use super::{Calls, Layer, Screen};
use crate::data;

/// The keys, each with the letters under it.
const KEYS: [(char, &str); 12] = [
    ('1', ""),
    ('2', "ABC"),
    ('3', "DEF"),
    ('4', "GHI"),
    ('5', "JKL"),
    ('6', "MNO"),
    ('7', "PQRS"),
    ('8', "TUV"),
    ('9', "WXYZ"),
    ('*', ""),
    ('0', "+"),
    ('#', ""),
];
/// Characters the keypad holds at most.
const LONGEST: usize = 32;
/// Height of a key, in dp; of an in-call key.
const KEY: f32 = 60.0;
const TONE_KEY: f32 = 52.0;
/// The round Call, in dp; backspace beside it.
pub(super) const CALL: f32 = 72.0;
const BACKSPACE: f32 = 56.0;
/// Height of the number's island, in dp.
const NUMBER: f32 = 92.0;
/// Suggestions shown at most.
const SUGGESTIONS: usize = 3;
/// Digits typed before names are spelled with them, and before numbers
/// that only hold them are suggested.
const SPELLED: usize = 2;
const WITHIN: usize = 3;

/// The key `text` names, if it is one of the keypad's or the plus.
pub(super) fn key(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let key = chars.next()?;
    (chars.next().is_none() && (key.is_ascii_digit() || matches!(key, '*' | '#' | '+')))
        .then_some(key)
}

/// `number` as the keypad shows it: only what its keys type.
pub(super) fn keyable(number: &str) -> String {
    number
        .chars()
        .filter(|&c| key(c.encode_utf8(&mut [0; 4])).is_some())
        .take(LONGEST)
        .collect()
}

/// The key a letter or digit is on: a, b and c on 2, and so on, accents
/// ignored.
fn letter_key(c: char) -> Option<char> {
    let c = match c.to_lowercase().next()? {
        'à'..='å' => 'a',
        'ç' => 'c',
        'è'..='ë' => 'e',
        'ì'..='ï' => 'i',
        'ñ' => 'n',
        'ò'..='ö' | 'ø' => 'o',
        'ù'..='ü' => 'u',
        'ý' | 'ÿ' => 'y',
        c => c,
    };
    Some(match c {
        'a'..='c' => '2',
        'd'..='f' => '3',
        'g'..='i' => '4',
        'j'..='l' => '5',
        'm'..='o' => '6',
        'p'..='s' => '7',
        't'..='v' => '8',
        'w'..='z' => '9',
        '0'..='9' => c,
        _ => return None,
    })
}

/// Where `digits`, typed on the keys, spell `name` from the start of one of
/// its words on, spaces skipped: the part of `name` they cover, and whether
/// it starts at the first word.
fn spelled(name: &str, digits: &str) -> Option<(Range<usize>, bool)> {
    let starts = name.char_indices().filter(|&(at, c)| {
        c.is_alphanumeric()
            && name[..at]
                .chars()
                .next_back()
                .is_none_or(|before| !before.is_alphanumeric())
    });
    for (word, (start, _)) in starts.enumerate() {
        let mut wanted = digits.chars().peekable();
        for (at, c) in name[start..].char_indices() {
            let Some(key) = letter_key(c) else {
                continue;
            };
            if wanted.next() != Some(key) {
                break;
            }
            if wanted.peek().is_none() {
                return Some((start..start + at + c.len_utf8(), word == 0));
            }
        }
    }
    None
}

/// Where `digits` are in `number`, spaces and signs skipped: the part of
/// `number` they cover.
fn within(number: &str, digits: &str) -> Option<Range<usize>> {
    if digits.is_empty() {
        return None;
    }
    let found: Vec<(usize, char)> = number
        .char_indices()
        .filter(|(_, c)| c.is_ascii_digit())
        .collect();
    let plain: String = found.iter().map(|&(_, c)| c).collect();
    // The digits are ASCII: a byte of `plain` is a digit of `found`.
    let at = plain.find(digits)?;
    let (last, c) = found[at + digits.len() - 1];
    Some(found[at].0..last + c.len_utf8())
}

/// A contact, or a recent number no one has, that the typed digits could be.
struct Suggestion {
    contact: Option<usize>,
    number: String,
    /// Where the digits are in the name, spelled, or in the number.
    name: Option<Range<usize>>,
    digits: Option<Range<usize>>,
    /// Lower first: the very number, a name spelled from its first word or
    /// from a later one, a number holding the digits.
    rank: u8,
}

impl Calls {
    pub(super) fn press_key(&mut self, key: char, cx: &mut Context<Self>) {
        if self.typed.len() < LONGEST {
            self.typed.push(key);
        }
        cx.notify();
    }

    /// Takes the last character off the number, or all of it.
    pub(super) fn erase(&mut self, all: bool, cx: &mut Context<Self>) {
        if all {
            self.typed.clear();
        } else {
            self.typed.pop();
        }
        cx.notify();
    }

    /// Calls the number on the keypad; on an empty keypad, brings back the
    /// number called last.
    pub(super) fn dial_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.typed.is_empty() {
            if let Some(last) = &self.last_dialled {
                self.typed = keyable(last);
                cx.notify();
            }
            return;
        }
        let number = std::mem::take(&mut self.typed);
        self.dial(number, window, cx);
    }

    /// Whom the typed digits could be, best first: contacts by number or by
    /// their name spelled on the keys, then recent numbers no one has.
    fn suggestions(&self) -> Vec<Suggestion> {
        let digits = data::digits(&self.typed);
        if digits.is_empty() {
            return Vec::new();
        }
        // A star, hash or plus past the start spells no name.
        let spells = digits.len() >= SPELLED
            && !self.typed.trim_start_matches('+').contains(['*', '#', '+']);
        let parts = digits.len() >= WITHIN;
        let mut found = Vec::new();
        for (index, contact) in self.people.all().iter().enumerate() {
            let Some(first) = contact.phones.first() else {
                continue;
            };
            let exact = contact
                .phones
                .iter()
                .find(|phone| same_number(&phone.number, &self.typed));
            let name = spells.then(|| spelled(&contact.name, &digits)).flatten();
            let part = parts
                .then(|| {
                    contact.phones.iter().find_map(|phone| {
                        within(&phone.number, &digits).map(|range| (phone, range))
                    })
                })
                .flatten();
            let (rank, phone, name, part) = match (exact, name, part) {
                (Some(phone), name, _) => (0, phone, name.map(|(range, _)| range), None),
                (None, Some((range, at_first)), _) => {
                    (if at_first { 1 } else { 2 }, first, Some(range), None)
                }
                (None, None, Some((phone, range))) => (3, phone, None, Some(range)),
                (None, None, None) => continue,
            };
            let digits = if rank == 0 {
                within(&phone.number, &digits)
            } else {
                part
            };
            found.push(Suggestion {
                contact: Some(index),
                number: phone.number.clone(),
                name,
                digits,
                rank,
            });
        }
        let mut seen: Vec<&str> = Vec::new();
        for call in &self.log {
            let known = self.people.find(&call.number).is_some()
                || seen.iter().any(|number| same_number(number, &call.number));
            if known {
                continue;
            }
            seen.push(&call.number);
            let rank = if same_number(&call.number, &self.typed) {
                0
            } else if parts && within(&call.number, &digits).is_some() {
                3
            } else {
                continue;
            };
            found.push(Suggestion {
                contact: None,
                number: call.number.clone(),
                name: None,
                digits: within(&call.number, &digits),
                rank,
            });
        }
        // Stable: contacts keep their order by name, ahead of numbers.
        found.sort_by_key(|suggestion| suggestion.rank);
        found.truncate(SUGGESTIONS);
        found
    }

    /// The keypad's bottom zone, on the tab bar.
    pub(super) fn keypad(&self, layer: Layer, cx: &mut Context<Self>) -> Div {
        let live = layer.live();
        let found = self.suggestions();
        let exact = found
            .first()
            .filter(|suggestion| suggestion.rank == 0)
            .and_then(|suggestion| suggestion.contact.map(|index| (index, &suggestion.number)));
        let mut backspace = self.button(
            "backspace",
            Face::Icon(Icon::Backspace),
            Fill::Clear,
            live,
            cx,
            |this, _, cx| this.erase(false, cx),
        );
        if live {
            backspace = self.long_press(backspace, cx, |this, cx| this.erase(true, cx));
        }
        let backspace = (!self.typed.is_empty()).then(|| self.round_group(backspace, BACKSPACE));
        let call = div()
            .flex()
            .items_center()
            .child(div().flex_1())
            .child(self.round(
                "dial",
                Icon::Phone,
                Fill::Call,
                CALL,
                live,
                cx,
                |this, window, cx| this.dial_typed(window, cx),
            ))
            .child(div().flex_1().flex().justify_center().children(backspace));
        div()
            .flex()
            .flex_col()
            .gap(px(INSET))
            .children(self.suggested(&found, live, cx))
            .child(self.number_island(exact, live, cx))
            .child(self.keys("key", KEY, live, cx, |this, key, cx| {
                this.press_key(key, cx)
            }))
            .child(call)
    }

    /// The typed number, large, on its island, and under it whose it is
    /// when it is a contact's; a tap there opens them.
    fn number_island(
        &self,
        exact: Option<(usize, &String)>,
        live: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let palette = &self.look.palette;
        let shown = if self.typed.is_empty() {
            div()
                .text_size(dp(18.))
                .text_color(solid(palette.secondary))
                .child(tr("Enter a number"))
        } else {
            let size = match self.typed.chars().count() {
                0..=13 => 34.,
                14..=17 => 28.,
                _ => 22.,
            };
            div()
                .max_w_full()
                .truncate()
                .text_size(dp(size))
                .line_height(relative(1.15))
                .child(self.typed.clone())
        };
        let owner = exact.map(|(index, number)| {
            let contact = &self.people.all()[index];
            let kind = self
                .people
                .kind(number)
                .map_or(String::new(), |kind| format!(" · {}", tr(kind.label())));
            let mut owner = div()
                .id("owner")
                .max_w_full()
                .truncate()
                .px(dp(8.))
                .rounded(denial_look_gpui::control_radius(&self.look))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(solid(self.look.accent_text))
                .child(format!("{}{kind}", contact.name));
            if live {
                let who = super::Who::Contact(self.people.key(index).to_owned());
                owner = self.press(owner, Fill::Clear).on_click(cx.listener(
                    move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open(Screen::Person(who.clone()), window, cx)
                    },
                ));
            }
            owner
        });
        island(&self.look)
            .h(dp(NUMBER))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(dp(2.))
            .px(dp(16.))
            .child(shown)
            .children(owner)
    }

    /// Whom the digits could be, on an island above the number: a tap puts
    /// their number on the keypad, their button calls it.
    fn suggested(&self, found: &[Suggestion], live: bool, cx: &mut Context<Self>) -> Option<Div> {
        if found.is_empty() {
            return None;
        }
        let marks = self.mark_style();
        let mut island = div();
        for (ix, suggestion) in found.iter().enumerate() {
            let segment = Segment::of(ix, found.len(), |_| ());
            let label = segment.first.then(|| tr("Suggestions").to_owned());
            let number = suggestion.number.clone();
            let marked = |text: String, range: Option<Range<usize>>| {
                StyledText::new(SharedString::from(text))
                    .with_highlights(range.map(|range| (range, marks)))
            };
            let (title, detail) = match suggestion.contact {
                Some(index) => {
                    let prefix = self
                        .people
                        .kind(&number)
                        .map_or(String::new(), |kind| format!("{} · ", tr(kind.label())));
                    let digits = suggestion
                        .digits
                        .clone()
                        .map(|range| prefix.len() + range.start..prefix.len() + range.end);
                    (
                        marked(
                            self.people.all()[index].name.clone(),
                            suggestion.name.clone(),
                        ),
                        marked(format!("{prefix}{number}"), digits),
                    )
                }
                None => (
                    marked(number.clone(), suggestion.digits.clone()),
                    marked(tr("Not in your contacts").into(), None),
                ),
            };
            let dial = number.clone();
            let mut row = self
                .segment_row(("suggestion", ix), segment)
                .py(dp(8.))
                .child(self.avatar(suggestion.contact, &number, ROW_AVATAR))
                .child(self.row_text(title, None, div().truncate().child(detail)))
                .child(self.button(
                    ("suggestion-dial", ix),
                    Face::Icon(Icon::Phone),
                    Fill::Clear,
                    live,
                    cx,
                    move |this, window, cx| this.dial(dial.clone(), window, cx),
                ));
            if live {
                let typed = keyable(&number);
                row = row.on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.typed.clone_from(&typed);
                    cx.notify();
                }));
            }
            island = island.child(self.segment(segment, label, None, None, row));
        }
        Some(island)
    }

    /// The in-call keypad, which sends what it types as tones.
    pub(super) fn tone_keys(&self, live: bool, cx: &mut Context<Self>) -> Div {
        self.keys("tone", TONE_KEY, live, cx, |this, key, cx| {
            this.tone(key, cx)
        })
    }

    /// The twelve keys, each an island of a run, `height` dp tall: the
    /// digit large, its letters small under it. A long press on 0 types a
    /// plus, but on the in-call keypad.
    fn keys(
        &self,
        id: &'static str,
        height: f32,
        live: bool,
        cx: &mut Context<Self>,
        typed: impl Fn(&mut Self, char, &mut Context<Self>) + Clone + 'static,
    ) -> Div {
        let palette = &self.look.palette;
        let radius = px(self.look.group_radius());
        let mut grid = div().flex().flex_col().gap(px(INSET));
        for row in KEYS.chunks(3) {
            let mut keys = div().flex().gap(px(INSET));
            for &(digit, letters) in row {
                let mut key = div()
                    .id((id, digit as usize))
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .rounded(radius)
                    .child(
                        div()
                            .text_size(dp(30.))
                            .line_height(relative(1.05))
                            .child(digit.to_string()),
                    )
                    .child(
                        div()
                            .h(dp(13.))
                            .text_size(dp(10.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(solid(palette.secondary))
                            .child(letters),
                    );
                if live {
                    let tap = typed.clone();
                    key = self.press(key, Fill::Clear).on_click(cx.listener(
                        move |this, _, _, cx| {
                            cx.stop_propagation();
                            tap(this, digit, cx)
                        },
                    ));
                    if digit == '0' && id == "key" {
                        let plus = typed.clone();
                        key = self.long_press(key, cx, move |this, cx| plus(this, '+', cx));
                    }
                }
                keys = keys.child(island_run(&self.look).flex_1().h(dp(height)).child(key));
            }
            grid = grid.child(keys);
        }
        grid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_spell_names_from_a_word_on() {
        // 4-4-8 is "giu", 7-4-6 "rin" in Rinaldi, 6-2-6 "mam".
        assert_eq!(spelled("Giulia Rinaldi", "448"), Some((0..3, true)));
        assert_eq!(spelled("Giulia Rinaldi", "746"), Some((7..10, false)));
        assert_eq!(spelled("Mamma", "626"), Some((0..3, true)));
        // Across the space, and with an accent.
        assert_eq!(spelled("Ana Lu", "26258"), Some((0..6, true)));
        assert_eq!(spelled("Élodie", "356"), Some((0..4, true)));
        assert_eq!(spelled("Marco", "99"), None);
    }

    #[test]
    fn digits_are_found_in_a_spaced_number() {
        assert_eq!(within("+44 7700 900123", "7700"), Some(4..8));
        assert_eq!(within("+44 7700 900123", "0090"), Some(6..11));
        assert_eq!(within("+44 7700 900123", "555"), None);
    }
}
