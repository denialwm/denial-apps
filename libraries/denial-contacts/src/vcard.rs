//! vCard 3.0 (RFC 2426) and 4.0 (RFC 6350), as far as names, numbers and
//! photos go. Everything else in a card is skipped.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use base64::Engine;
use base64::alphabet::STANDARD;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

use crate::{Contact, Kind, Phone};

/// Cards in the wild pad base64 or leave it unpadded.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// The cards in a file's text that have a name.
pub fn parse(text: &str) -> Vec<Contact> {
    let mut contacts = Vec::new();
    let mut card: Option<Card> = None;
    for line in unfold(text) {
        let Some(property) = Property::parse(&line) else {
            continue;
        };
        let vcard = property.value.eq_ignore_ascii_case("VCARD");
        match property.name.as_str() {
            "BEGIN" if vcard => card = Some(Card::default()),
            "END" if vcard => contacts.extend(card.take().and_then(Card::finish)),
            _ => {
                if let Some(card) = &mut card {
                    card.take(property);
                }
            }
        }
    }
    contacts
}

/// Logical lines: a line break followed by a space or a tab continues the
/// line before it.
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        match (raw.strip_prefix([' ', '\t']), lines.last_mut()) {
            (Some(rest), Some(line)) => line.push_str(rest),
            _ => lines.push(raw.to_owned()),
        }
    }
    lines
}

/// One content line: `group.NAME;PARAM=a,b;PARAM="c":value`.
struct Property<'a> {
    /// Upper case, without its group.
    name: String,
    /// Each parameter value with its upper-case name. A bare value, as in
    /// `TEL;CELL:`, is a TYPE.
    params: Vec<(String, String)>,
    value: &'a str,
}

impl<'a> Property<'a> {
    fn parse(line: &'a str) -> Option<Self> {
        let colon = split_unquoted(line, ':').next()?.len();
        let value = line.get(colon + 1..)?;
        let mut head = split_unquoted(&line[..colon], ';');
        let name = head.next()?;
        let name = name.rsplit('.').next().unwrap_or(name).to_ascii_uppercase();
        let mut params = Vec::new();
        for param in head {
            let (key, values) = param.split_once('=').unwrap_or(("TYPE", param));
            // 4.0 quotes lists: TYPE="voice,cell".
            for value in values.trim_matches('"').split(',') {
                params.push((key.to_ascii_uppercase(), value.to_owned()));
            }
        }
        Some(Self {
            name,
            params,
            value,
        })
    }

    fn param(&self, key: &str) -> impl Iterator<Item = &str> {
        self.params
            .iter()
            .filter(move |(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Splits `text` at each `separator` outside double quotes.
fn split_unquoted(text: &str, separator: char) -> impl Iterator<Item = &str> {
    let mut quoted = false;
    text.split(move |c| {
        if c == '"' {
            quoted = !quoted;
        }
        c == separator && !quoted
    })
}

/// Splits a text value at each unescaped `separator`, keeping the escapes.
fn split_escaped(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut escaped) = (0, false);
    for (at, c) in text.char_indices() {
        if c == separator && !escaped {
            parts.push(&text[start..at]);
            start = at + c.len_utf8();
        }
        escaped = c == '\\' && !escaped;
    }
    parts.push(&text[start..]);
    parts
}

/// A text value with `\n`, `\,`, `\;` and `\\` turned back into what they
/// stand for.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(c) => out.push(c),
            None => {}
        }
    }
    out
}

#[derive(Default)]
struct Card {
    uid: Option<String>,
    full_name: String,
    /// N's five parts: family, given, additional, prefixes, suffixes.
    name: Vec<String>,
    phones: Vec<Phone>,
    photo: Option<Vec<u8>>,
}

impl Card {
    fn take(&mut self, property: Property) {
        match property.name.as_str() {
            "UID" => {
                let uid = unescape(property.value).trim().to_owned();
                self.uid = (!uid.is_empty()).then_some(uid);
            }
            "FN" => self.full_name = unescape(property.value).trim().to_owned(),
            "N" => {
                self.name = split_escaped(property.value, ';')
                    .into_iter()
                    .map(|part| {
                        let values = split_escaped(part, ',').into_iter().map(unescape);
                        values.collect::<Vec<_>>().join(" ").trim().to_owned()
                    })
                    .collect()
            }
            "TEL" => {
                // 4.0 writes a tel: URI, maybe with ;ext= after the number.
                let value = property.value.trim();
                let number = value
                    .get(..4)
                    .filter(|scheme| scheme.eq_ignore_ascii_case("tel:"))
                    .map_or(value, |_| &value[4..]);
                let number = number.split(';').next().unwrap_or(number).trim();
                if !number.is_empty() {
                    self.phones.push(Phone {
                        number: number.to_owned(),
                        kind: kind(property.param("TYPE")),
                    });
                }
            }
            "PHOTO" if self.photo.is_none() => self.photo = photo(&property),
            _ => {}
        }
    }

    fn finish(self) -> Option<Contact> {
        let name = if self.full_name.is_empty() {
            // Prefixes, given, additional, family, suffixes.
            [3, 1, 2, 0, 4]
                .iter()
                .filter_map(|&i| self.name.get(i).filter(|part| !part.is_empty()))
                .cloned()
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            self.full_name
        };
        (!name.is_empty()).then_some(Contact {
            uid: self.uid,
            name,
            phones: self.phones,
            photo: self.photo,
        })
    }
}

/// A number's kind from its TYPE values; voice, text, pref and the like say
/// nothing about it.
fn kind<'a>(types: impl Iterator<Item = &'a str>) -> Kind {
    let types: Vec<String> = types.map(str::to_ascii_lowercase).collect();
    let has = |names: &[&str]| types.iter().any(|t| names.contains(&t.as_str()));
    if has(&["fax"]) {
        Kind::Fax
    } else if has(&["pager"]) {
        Kind::Pager
    } else if has(&["cell", "mobile", "iphone"]) {
        Kind::Mobile
    } else if has(&["work"]) {
        Kind::Work
    } else if has(&["home"]) {
        Kind::Home
    } else if has(&["main"]) {
        Kind::Main
    } else {
        Kind::Other
    }
}

/// A PHOTO's bytes: base64 in the card (3.0's ENCODING=b, 4.0's data: URI),
/// or the file a file: URI names. Web URIs are not fetched.
fn photo(property: &Property) -> Option<Vec<u8>> {
    let value = property.value.trim();
    let encoded = if let Some(data) = value.strip_prefix("data:") {
        Some(data.split_once(',')?.1)
    } else if property
        .param("ENCODING")
        .any(|e| e.eq_ignore_ascii_case("b") || e.eq_ignore_ascii_case("base64"))
    {
        Some(value)
    } else {
        None
    };
    if let Some(encoded) = encoded {
        let encoded: String = encoded.split_ascii_whitespace().collect();
        return BASE64.decode(encoded).ok();
    }
    let path = value.strip_prefix("file://")?;
    let path = path.strip_prefix("localhost").unwrap_or(path);
    std::fs::read(PathBuf::from(OsString::from_vec(percent_decode(path)))).ok()
}

/// A URI path with its `%XX` escapes decoded.
fn percent_decode(path: &str) -> Vec<u8> {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_three_card_with_folded_lines_and_escapes() {
        let cards = parse(
            "BEGIN:VCARD\r\n\
             VERSION:3.0\r\n\
             FN:Mario Rossi\\, jr.\r\n\
             N:Rossi;Mario;;;jr.\r\n\
             item1.TEL;TYPE=CELL,VOICE:+39 351\r\n \
             851 3652\r\n\
             TEL;TYPE=HOME;TYPE=VOICE:02 1234 5678\r\n\
             TEL;WORK:02 8765 4321\r\n\
             TEL;TYPE=WORK,FAX:02 0000 0000\r\n\
             NOTE:first\\nsecond\\; third\r\n\
             END:VCARD\r\n",
        );
        assert_eq!(cards.len(), 1);
        let card = &cards[0];
        assert_eq!(card.name, "Mario Rossi, jr.");
        let phones: Vec<(&str, Kind)> = card
            .phones
            .iter()
            .map(|p| (p.number.as_str(), p.kind))
            .collect();
        assert_eq!(
            phones,
            [
                ("+39 351851 3652", Kind::Mobile),
                ("02 1234 5678", Kind::Home),
                ("02 8765 4321", Kind::Work),
                ("02 0000 0000", Kind::Fax),
            ]
        );
        assert_eq!(card.photo, None);
    }

    #[test]
    fn reads_a_four_card_with_tel_uris() {
        let cards = parse(
            "BEGIN:VCARD\n\
             VERSION:4.0\n\
             FN:Ada Lovelace\n\
             TEL;VALUE=uri;PREF=1;TYPE=\"voice,cell\":tel:+44-20-7946-0958\n\
             TEL;VALUE=uri;TYPE=work:tel:+44-20-7946-0000;ext=12\n\
             TEL:tel:4040\n\
             END:VCARD\n",
        );
        let numbers: Vec<(&str, Kind)> = cards[0]
            .phones
            .iter()
            .map(|p| (p.number.as_str(), p.kind))
            .collect();
        assert_eq!(
            numbers,
            [
                ("+44-20-7946-0958", Kind::Mobile),
                ("+44-20-7946-0000", Kind::Work),
                ("4040", Kind::Other),
            ]
        );
    }

    #[test]
    fn names_from_n_when_fn_is_missing_and_skips_nameless_cards() {
        let cards = parse(
            "BEGIN:VCARD\nVERSION:3.0\nN:Hopper;Grace;Brewster\\, M.;Dr.;\nEND:VCARD\n\
             BEGIN:VCARD\nVERSION:3.0\nTEL:123\nEND:VCARD\n\
             BEGIN:VCARD\nVERSION:3.0\nFN:  \nN:;;;;\nEND:VCARD\n",
        );
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].name, "Dr. Grace Brewster, M. Hopper");
    }

    #[test]
    fn reads_photos_inline_and_from_files() {
        // "PNG!" in base64, folded the way a writer wraps at 75 characters.
        let cards = parse(
            "BEGIN:VCARD\nVERSION:3.0\nFN:Three\nPHOTO;ENCODING=b;TYPE=PNG:UE5\n HIQ==\nEND:VCARD\n\
             BEGIN:VCARD\nVERSION:4.0\nFN:Four\nPHOTO:data:image/png;base64,UE5HIQ==\nEND:VCARD\n\
             BEGIN:VCARD\nVERSION:4.0\nFN:Web\nPHOTO:https://example.org/a.jpg\nEND:VCARD\n",
        );
        assert_eq!(cards[0].photo.as_deref(), Some(&b"PNG!"[..]));
        assert_eq!(cards[1].photo.as_deref(), Some(&b"PNG!"[..]));
        assert_eq!(cards[2].photo, None);

        let dir = std::env::temp_dir().join(format!("denial-contacts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a photo.jpg");
        std::fs::write(&file, b"JPEG").unwrap();
        let uri = format!("file://{}", dir.display()).replace(' ', "%20") + "/a%20photo.jpg";
        let cards = parse(&format!(
            "BEGIN:VCARD\nVERSION:3.0\nFN:File\nPHOTO;VALUE=uri:{uri}\nEND:VCARD\n"
        ));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(cards[0].photo.as_deref(), Some(&b"JPEG"[..]));
    }

    #[test]
    fn takes_every_card_in_a_file() {
        let text = "BEGIN:VCARD\nFN:A\nEND:VCARD\nBEGIN:vcard\nFN:B\nEND:vcard\n";
        let names: Vec<String> = parse(text).into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["A", "B"]);
    }
}
