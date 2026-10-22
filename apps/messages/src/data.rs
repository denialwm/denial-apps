//! Conversations as dmail keeps them, how their peers and times read.

use denial_look::words::{fill, tr};
use std::ops::Range;

use ::messages::dmail::{ConversationRow, MessageRow};
use denial_look::Tween;

use crate::detect::{self, Span};

pub struct Message {
    pub id: u64,
    pub text: String,
    pub outgoing: bool,
    /// Unix time.
    pub time: i64,
    /// received; or sending, sent, failed, delivered, undelivered or expired.
    pub state: String,
    /// What in the text can be tapped.
    pub spans: Vec<Span>,
    /// Set for messages added while the app runs, so they animate in.
    pub appear: Option<Tween>,
}

/// How far a message's state has come: a later state never gives way to an
/// earlier one.
fn rank(state: &str) -> u8 {
    match state {
        "sending" => 0,
        "sent" => 1,
        _ => 2,
    }
}

impl Message {
    pub fn from_row((id, outgoing, text, time, state): MessageRow) -> Self {
        Self {
            id,
            spans: detect::spans(&text),
            text,
            outgoing,
            time,
            state,
            appear: None,
        }
    }

    /// Takes dmail's state for the message. States only move on, sending to
    /// sent to delivered, so a reread that raced MessageChanged never takes
    /// one back.
    pub fn settle(&mut self, state: String) {
        if rank(&state) >= rank(&self.state) {
            self.state = state;
        }
    }

    pub fn failed(&self) -> bool {
        self.outgoing && self.state == "failed"
    }

    /// Sent, but the network reported it did not arrive.
    pub fn undelivered(&self) -> bool {
        self.outgoing && matches!(self.state.as_str(), "undelivered" | "expired")
    }

    /// The label under the last bubble of a run: what happened to a message
    /// going out, else its time. Errors are marked so they read as errors.
    pub fn label(&self) -> (String, bool) {
        let time = clock(self.time);
        match self.state.as_str() {
            "sending" if self.outgoing => (tr("Sending…").into(), false),
            "failed" if self.outgoing => (tr("Not sent. Tap to try again").into(), true),
            "delivered" if self.outgoing => (fill("{time} · Delivered", &[("time", &time)]), false),
            _ if self.undelivered() => (tr("Not delivered").into(), true),
            _ => (time, false),
        }
    }

    /// The full time and state, shown while the message is held.
    pub fn details(&self, now: i64) -> String {
        let state = match self.state.as_str() {
            _ if !self.outgoing => tr("Received"),
            "sending" => tr("Sending"),
            "failed" => tr("Not sent"),
            "delivered" => tr("Delivered"),
            "undelivered" => tr("Not delivered"),
            "expired" => tr("Expired"),
            _ => tr("Sent"),
        };
        format!("{} · {state}", full_time(self.time, now))
    }
}

pub struct Conversation {
    /// The number or sender name dmail files the conversation under.
    pub peer: String,
    pub last_text: String,
    pub last_time: i64,
    pub last_outgoing: bool,
    /// Unread messages, as dmail counts them.
    pub unread: u32,
    pub archived: bool,
    /// Fades the count that was unread after the conversation is opened.
    pub read_fade: Option<(Tween, u32)>,
    /// Oldest first; read from dmail a page at a time, newest page first.
    pub messages: Vec<Message>,
    /// `messages` reaches back to the first message.
    pub complete: bool,
    /// An older page is being read.
    pub loading: bool,
    /// dmail has no message with this peer: a conversation being written, or
    /// one left with only a draft.
    pub local: bool,
    /// One of the made-up conversations of `samples`, which dmail never sees.
    pub sample: bool,
}

impl Conversation {
    pub fn from_row(
        (peer, last_text, last_time, last_outgoing, unread): ConversationRow,
        archived: bool,
    ) -> Self {
        Self {
            peer,
            last_text,
            last_time,
            last_outgoing,
            unread,
            archived,
            read_fade: None,
            messages: Vec::new(),
            complete: false,
            loading: false,
            local: false,
            sample: false,
        }
    }

    pub fn new(peer: String) -> Self {
        Self {
            local: true,
            ..Self::from_row((peer, String::new(), now(), true, 0), false)
        }
    }
}

/// A sender name's initials; a number has none.
pub fn initials(peer: &str) -> String {
    if !peer.chars().any(char::is_alphabetic) {
        return "#".into();
    }
    peer.split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .collect()
}

/// Whether dmail can send to `peer`: a number as dmail takes it. Sender
/// names such as a carrier's take no replies.
pub fn replyable(peer: &str) -> bool {
    denial_contacts::dialable(peer).is_some_and(|number| number == peer)
}

/// Where each word of `query` starts a word of `text`, case ignored, as
/// dmail's search matches: the parts to mark in a hit.
pub fn matches(text: &str, query: &str) -> Vec<Range<usize>> {
    let words: Vec<Vec<char>> = query
        .split_whitespace()
        .map(|word| word.chars().flat_map(char::to_lowercase).collect())
        .collect();
    let mut found = Vec::new();
    let mut previous = None;
    for (start, c) in text.char_indices() {
        let starts_word = c.is_alphanumeric() && !previous.is_some_and(char::is_alphanumeric);
        previous = Some(c);
        if !starts_word {
            continue;
        }
        let rest = &text[start..];
        let longest = words
            .iter()
            .filter_map(|word| prefix_length(rest, word))
            .max();
        if let Some(length) = longest {
            found.push(start..start + length);
        }
    }
    found
}

/// The bytes of `text` that `word`, lower case, starts it with, if it does.
fn prefix_length(text: &str, word: &[char]) -> Option<usize> {
    let mut wanted = word.iter();
    let mut length = 0;
    for c in text.chars() {
        let mut lower = c.to_lowercase();
        let all = lower.all(|l| wanted.next() == Some(&l));
        if !all {
            return None;
        }
        length += c.len_utf8();
        if wanted.as_slice().is_empty() {
            return Some(length);
        }
    }
    None
}

/// A Unix time in the phone's time zone.
struct Local {
    /// Days since 1970-01-01 in local time.
    day: i64,
    weekday: usize,
    month: usize,
    month_day: i32,
    year: i32,
    hour: i32,
    minute: i32,
}

fn local(time: i64) -> Local {
    // SAFETY: localtime_r only writes the tm it is given.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&(time as libc::time_t), &mut tm);
        tm
    };
    Local {
        day: (time + tm.tm_gmtoff).div_euclid(86_400),
        weekday: tm.tm_wday.clamp(0, 6) as usize,
        month: tm.tm_mon.clamp(0, 11) as usize,
        month_day: tm.tm_mday,
        year: tm.tm_year + 1900,
        hour: tm.tm_hour,
        minute: tm.tm_min,
    }
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// "18:02".
pub fn clock(time: i64) -> String {
    let t = local(time);
    format!("{:02}:{:02}", t.hour, t.minute)
}

/// Today, Yesterday, a weekday within the week, else the date.
pub fn day(time: i64, now: i64) -> String {
    let (t, today) = (local(time), local(now));
    match today.day - t.day {
        0 => tr("Today").into(),
        1 => tr("Yesterday").into(),
        2..=6 => tr(WEEKDAYS[t.weekday]).into(),
        _ if t.year == today.year => fill(
            "{day} {month}",
            &[
                ("day", &t.month_day.to_string()),
                ("month", tr(MONTHS[t.month])),
            ],
        ),
        _ => fill(
            "{day} {month} {year}",
            &[
                ("day", &t.month_day.to_string()),
                ("month", tr(MONTHS[t.month])),
                ("year", &t.year.to_string()),
            ],
        ),
    }
}

pub fn same_day(a: i64, b: i64) -> bool {
    local(a).day == local(b).day
}

/// "Today 18:02", or "21 Oct 2026, 18:02" beyond the week.
pub fn full_time(time: i64, now: i64) -> String {
    let t = local(time);
    if local(now).day - t.day <= 6 {
        return format!("{} {:02}:{:02}", day(time, now), t.hour, t.minute);
    }
    fill(
        "{day} {month} {year}, {time}",
        &[
            ("day", &t.month_day.to_string()),
            ("month", tr(MONTHS[t.month])),
            ("year", &t.year.to_string()),
            ("time", &format!("{:02}:{:02}", t.hour, t.minute)),
        ],
    )
}

/// The clock for today, the day otherwise.
pub fn list_time(time: i64, now: i64) -> String {
    if same_day(time, now) {
        clock(time)
    } else {
        day(time, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_words_the_query_starts() {
        let text = "I'm bringing the Tiramisù, tirami-sù";
        let marked: Vec<&str> = matches(text, "tirami BRING")
            .into_iter()
            .map(|range| &text[range])
            .collect();
        assert_eq!(marked, ["bring", "Tirami", "tirami"]);
        assert!(matches("untiramisù", "tirami").is_empty());
        assert!(matches("anything", "").is_empty());
    }

    #[test]
    fn states_only_move_on() {
        let mut message = Message::from_row((1, true, "hi".into(), 0, "sending".into()));
        message.settle("delivered".into());
        message.settle("sent".into());
        assert_eq!(message.state, "delivered");
        message.settle("undelivered".into());
        assert!(message.undelivered());
        assert_eq!(message.label(), ("Not delivered".into(), true));
    }
}
