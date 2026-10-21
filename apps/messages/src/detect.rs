//! What in a message can be tapped: links, e-mail addresses, phone numbers and
//! one-time codes.

use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Opens in the browser; the URL carries its scheme.
    Link(String),
    /// A `mailto:` URL.
    Mail(String),
    /// Copied; there is no dialer yet.
    Phone(String),
    /// A one-time code, copied.
    Code(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    /// Bytes of the message text.
    pub range: Range<usize>,
    pub kind: Kind,
}

/// Top-level domains a bare host like `iliad.it/offerte` may end in. Hosts with
/// a scheme or `www.` need none of these.
const DOMAINS: [&str; 24] = [
    "com", "net", "org", "info", "app", "dev", "io", "me", "co", "eu", "it", "uk", "de", "fr",
    "es", "ch", "at", "be", "nl", "pt", "ly", "gl", "to", "tv",
];

/// Words a message with a one-time code carries, as prefixes.
const CODE_WORDS: [&str; 10] = [
    "code", "codic", "codig", "códig", "otp", "pin", "password", "passcode", "verif", "token",
];

const CODE_DIGITS: Range<usize> = 4..9;
const PHONE_DIGITS: Range<usize> = 9..16;
/// Fewer digits do with a leading plus.
const PLUS_PHONE_DIGITS: Range<usize> = 7..16;

/// Tappable parts of `text`, in order and never overlapping.
pub fn spans(text: &str) -> Vec<Span> {
    let mut spans = phones(text);
    let codes = text.split(|c: char| !c.is_alphabetic()).any(|word| {
        let word = word.to_lowercase();
        CODE_WORDS.iter().any(|prefix| word.starts_with(prefix))
    });
    for (start, word) in words(text) {
        let range = start..start + word.len();
        if spans
            .iter()
            .any(|s| s.range.start < range.end && range.start < s.range.end)
        {
            continue;
        }
        if let Some(kind) = classify(word, codes) {
            spans.push(Span { range, kind });
        }
    }
    spans.sort_by_key(|s| s.range.start);
    spans
}

/// Whitespace-separated words with their offsets, without the punctuation
/// around them.
fn words(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.split_whitespace().filter_map(move |word| {
        let start = word.as_ptr() as usize - text.as_ptr() as usize;
        let lead = word.len()
            - word
                .trim_start_matches(['(', '[', '<', '"', '\'', '«'])
                .len();
        let core = word[lead..]
            .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '>', '"', '\'', '»']);
        (!core.is_empty()).then_some((start + lead, core))
    })
}

fn classify(word: &str, codes: bool) -> Option<Kind> {
    let lower = word.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return Some(Kind::Link(word.to_owned()));
    }
    let host = lower.split(['/', '?', '#']).next().unwrap_or_default();
    if lower.starts_with("www.") && is_host(host, |_| true) {
        return Some(Kind::Link(format!("https://{word}")));
    }
    if let Some((local, domain)) = word.split_once('@') {
        let local_ok = !local.is_empty()
            && local
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._%+-".contains(c));
        let domain_ok = is_host(&domain.to_ascii_lowercase(), |tld| {
            tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic())
        });
        return (local_ok && domain_ok).then(|| Kind::Mail(format!("mailto:{word}")));
    }
    if is_host(host, |tld| DOMAINS.contains(&tld)) {
        return Some(Kind::Link(format!("https://{word}")));
    }
    let digits = word.bytes().all(|b| b.is_ascii_digit());
    (codes && digits && CODE_DIGITS.contains(&word.len())).then(|| Kind::Code(word.to_owned()))
}

/// Dot-separated labels of letters, digits and hyphens, at least two, the last
/// one accepted by `tld`.
fn is_host(host: &str, tld: impl Fn(&str) -> bool) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        && labels.last().is_some_and(|last| tld(last))
}

/// Phone numbers: digit groups joined by single spaces, hyphens or dots, the
/// first group maybe in parentheses, with an optional leading plus.
fn phones(text: &str) -> Vec<Span> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let plus = bytes[i] == b'+';
        let opens = bytes[i] == b'(' || bytes[i].is_ascii_digit() || plus;
        // Inside a word, a date, an amount or a link: not a number.
        let joined = i > 0
            && (bytes[i - 1].is_ascii_alphanumeric() || b"+/.,:-_@#=&".contains(&bytes[i - 1]));
        if !opens || joined {
            i += 1;
            continue;
        }
        let (mut digits, mut last_digit) = (0, i);
        for (end, &b) in bytes.iter().enumerate().skip(i) {
            let next = bytes.get(end + 1).copied();
            let next_digit = next.is_some_and(|n| n.is_ascii_digit());
            let joins = match b {
                b'0'..=b'9' => {
                    digits += 1;
                    last_digit = end + 1;
                    true
                }
                b'+' => end == i,
                b' ' => next_digit || next == Some(b'('),
                b'-' | b'.' | b'(' => next_digit,
                b')' => next_digit || next == Some(b' '),
                _ => false,
            };
            if !joins {
                break;
            }
        }
        // "(333 1234567)": the parenthesis belongs to the sentence.
        let start = if bytes[i] == b'(' && !text[i..last_digit].contains(')') {
            i + 1
        } else {
            i
        };
        let body = &text[start..last_digit.max(start)];
        let wanted = if plus {
            PLUS_PHONE_DIGITS
        } else {
            PHONE_DIGITS
        };
        let alone = bytes
            .get(last_digit)
            .is_none_or(|b| !b.is_ascii_alphanumeric());
        let balanced = body.matches('(').count() == body.matches(')').count();
        if wanted.contains(&digits) && alone && balanced {
            spans.push(Span {
                range: start..last_digit,
                kind: Kind::Phone(
                    body.chars()
                        .filter(|c| c.is_ascii_digit() || *c == '+')
                        .collect(),
                ),
            });
        }
        i = last_digit.max(i + 1);
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(&str, Kind)> {
        spans(text)
            .into_iter()
            .map(|s| (&text[s.range.clone()], s.kind))
            .collect()
    }

    #[test]
    fn finds_links() {
        assert_eq!(
            kinds("See https://example.org/a?b=1, or www.iliad.it."),
            [
                (
                    "https://example.org/a?b=1",
                    Kind::Link("https://example.org/a?b=1".into())
                ),
                ("www.iliad.it", Kind::Link("https://www.iliad.it".into())),
            ]
        );
        assert_eq!(
            kinds("(iliad.it/offerte)"),
            [(
                "iliad.it/offerte",
                Kind::Link("https://iliad.it/offerte".into())
            )]
        );
        assert!(kinds("file.txt and v1.2 and e.g. this").is_empty());
    }

    #[test]
    fn finds_mail() {
        assert_eq!(
            kinds("write to info@example.com!"),
            [(
                "info@example.com",
                Kind::Mail("mailto:info@example.com".into())
            )]
        );
        assert!(kinds("@home now").is_empty());
    }

    #[test]
    fn finds_phone_numbers() {
        assert_eq!(
            kinds("Call +39 351 851 3652 or 02-1234-5678."),
            [
                ("+39 351 851 3652", Kind::Phone("+393518513652".into())),
                ("02-1234-5678", Kind::Phone("0212345678".into())),
            ]
        );
        assert_eq!(
            kinds("(02) 1234 5678"),
            [("(02) 1234 5678", Kind::Phone("0212345678".into()))]
        );
        assert_eq!(
            kinds("me (333 1234567)"),
            [("333 1234567", Kind::Phone("3331234567".into()))]
        );
        assert!(
            kinds("On 21.10.2026 at 10:30, €1.234.567, IT60X0542811101000000123456").is_empty()
        );
    }

    #[test]
    fn finds_codes_only_when_the_message_speaks_of_one() {
        assert_eq!(
            kinds("Il tuo codice di verifica è 482913."),
            [("482913", Kind::Code("482913".into()))]
        );
        assert_eq!(
            kinds("Your code: 1234"),
            [("1234", Kind::Code("1234".into()))]
        );
        assert!(kinds("See you in 2026, room 4821").is_empty());
    }

    #[test]
    fn a_number_is_not_cut_into_codes() {
        assert_eq!(
            kinds("PIN reset? Call 333 123 4567"),
            [("333 123 4567", Kind::Phone("3331234567".into()))]
        );
    }
}
