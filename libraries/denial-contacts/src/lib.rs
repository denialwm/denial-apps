//! The phone's contacts: a folder of vCard files, `~/.local/share/contacts`,
//! read-only here. Messages names its peers with them; a Contacts app will
//! write them, and vdirsyncer can sync the folder with a CardDAV server.
//!
//! Numbers match as dmail files conversations, so a contact names the
//! conversation its number lands in.

mod vcard;
mod watch;

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub use watch::Watch;

/// Numbers name the same phone when this many last digits match, as on
/// Android and in dmail: the network sends +39 333…, the SIM's own number can
/// lack the plus, and people type 333….
const MATCH_DIGITS: usize = 9;
/// Numbers shorter than this must match whole.
const MIN_DIGITS: usize = 7;

pub struct Contact {
    /// The card's UID: the same contact in two collections has the same one.
    pub uid: Option<String>,
    /// FN, or the parts of N when a card has no FN. Never empty.
    pub name: String,
    pub phones: Vec<Phone>,
    /// The picture's file as stored: a JPEG or PNG, usually. Read from the
    /// card, or from the file its URI names.
    pub photo: Option<Vec<u8>>,
}

pub struct Phone {
    /// As written in the card.
    pub number: String,
    pub kind: Kind,
}

/// What a number is for, from the card's TYPE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Mobile,
    Home,
    Work,
    Main,
    Fax,
    Pager,
    Other,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Mobile => "Mobile",
            Kind::Home => "Home",
            Kind::Work => "Work",
            Kind::Main => "Main",
            Kind::Fax => "Fax",
            Kind::Pager => "Pager",
            Kind::Other => "Other",
        }
    }
}

/// Every contact in the folder, by name, with their numbers indexed.
#[derive(Default)]
pub struct Contacts {
    list: Vec<Contact>,
    /// Indices into `list`, by `key` of each of their numbers.
    numbers: HashMap<String, Vec<usize>>,
}

impl Contacts {
    /// `$XDG_DATA_HOME/contacts`, else `~/.local/share/contacts`.
    pub fn folder() -> Option<PathBuf> {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .map(|dir| dir.join("contacts"))
    }

    /// Reads every `.vcf` file in `folder` and in its subfolders, one level
    /// down: vdirsyncer keeps a collection per subfolder. A contact in two
    /// collections, by UID, is taken once. A missing folder holds no one; a
    /// file that cannot be read is skipped.
    pub fn load(folder: &Path) -> Self {
        let mut list = Vec::new();
        let mut uids = HashSet::new();
        for path in cards(folder) {
            match std::fs::read(&path) {
                Ok(bytes) => list.extend(
                    vcard::parse(&String::from_utf8_lossy(&bytes))
                        .into_iter()
                        .filter(|contact| {
                            contact
                                .uid
                                .as_ref()
                                .is_none_or(|uid| uids.insert(uid.clone()))
                        }),
                ),
                Err(error) => eprintln!("denial-contacts: {}: {error}", path.display()),
            }
        }
        Self::new(list)
    }

    fn new(mut list: Vec<Contact>) -> Self {
        list.sort_by_cached_key(|contact| contact.name.to_lowercase());
        let mut numbers: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, contact) in list.iter().enumerate() {
            for phone in &contact.phones {
                if let Some(key) = key(&phone.number) {
                    numbers.entry(key).or_default().push(index);
                }
            }
        }
        Self { list, numbers }
    }

    pub fn all(&self) -> &[Contact] {
        &self.list
    }

    /// Adds `more` to the contacts, such as made-up ones to try an app with,
    /// in name order with the rest.
    pub fn extend(&mut self, more: impl IntoIterator<Item = Contact>) {
        let mut list = std::mem::take(&mut self.list);
        list.extend(more);
        *self = Self::new(list);
    }

    /// The contact with `number`, as an index into `all()`. A sender name
    /// finds no one.
    pub fn find(&self, number: &str) -> Option<usize> {
        self.numbers
            .get(&key(number)?)?
            .iter()
            .copied()
            .find(|&index| {
                self.list[index]
                    .phones
                    .iter()
                    .any(|phone| same_number(&phone.number, number))
            })
    }
}

/// The vCard files in `folder` and in its subfolders one level down, hidden
/// ones left out. Sorted, so a contact in two collections is taken from the
/// same one every time.
fn cards(folder: &Path) -> Vec<PathBuf> {
    let entries = |dir: &Path| {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| !hidden(&entry.file_name()))
            .map(|entry| entry.path())
            .collect::<Vec<_>>()
    };
    let mut cards: Vec<PathBuf> = entries(folder)
        .into_iter()
        .flat_map(|path| {
            if path.is_dir() {
                entries(&path)
            } else {
                vec![path]
            }
        })
        .filter(|path| is_card(path))
        .collect();
    cards.sort();
    cards
}

/// Whether a file or folder is hidden, as a dot starts its name.
fn hidden(name: &OsStr) -> bool {
    name.as_encoded_bytes().starts_with(b".")
}

/// Whether `path` names a vCard file.
fn is_card(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("vcf"))
}

/// The digits of a phone number, without its plus and the spaces, dashes,
/// dots, parentheses and slashes people write in it; None for anything else,
/// such as a sender name.
fn digits(text: &str) -> Option<String> {
    let number = text.trim();
    let digits: String = number
        .strip_prefix('+')
        .unwrap_or(number)
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '.' | '(' | ')' | '/'))
        .collect();
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then_some(digits)
}

/// What every number `same_number` matches with `text` has in common: its
/// last `MIN_DIGITS` digits, or all of them when it is shorter.
fn key(text: &str) -> Option<String> {
    let digits = digits(text)?;
    Some(digits[digits.len().saturating_sub(MIN_DIGITS)..].to_owned())
}

/// A written number as dmail takes it, an optional plus and up to 20
/// digits; None when it is not a number.
pub fn dialable(text: &str) -> Option<String> {
    let digits = digits(text).filter(|digits| digits.len() <= 20)?;
    Some(if text.trim().starts_with('+') {
        format!("+{digits}")
    } else {
        digits
    })
}

/// Whether two numbers reach the same phone, as dmail files conversations:
/// their last 9 digits match, or the whole numbers when either has fewer than
/// 7. Sender names match only themselves.
pub fn same_number(a: &str, b: &str) -> bool {
    match (digits(a), digits(b)) {
        (Some(a), Some(b)) if a.len().min(b.len()) >= MIN_DIGITS => {
            let n = a.len().min(b.len()).min(MATCH_DIGITS);
            a[a.len() - n..] == b[b.len() - n..]
        }
        (Some(a), Some(b)) => a == b,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_numbers_become_dialable() {
        assert_eq!(
            dialable("+39 351 851-3652").as_deref(),
            Some("+393518513652")
        );
        assert_eq!(dialable("(02) 1234.5678").as_deref(), Some("0212345678"));
        assert_eq!(dialable("Iliad"), None);
        assert_eq!(dialable("+"), None);
        assert_eq!(dialable("123456789012345678901"), None);
    }

    #[test]
    fn numbers_match_on_their_last_nine_digits() {
        assert!(same_number("+393518513652", "3518513652"));
        assert!(same_number("393518513652", "+39 351 851 3652"));
        assert!(same_number("0039 351 851 3652", "+393518513652"));
        assert!(!same_number("+393518513652", "+393518513653"));
        // Seven or eight digits match on all of the shorter number.
        assert!(same_number("8513652", "+393518513652"));
        assert!(!same_number("8513652", "+393518513662"));
        // Below seven, only whole numbers match.
        assert!(!same_number("12345", "912345"));
        assert!(same_number("4040", "4040"));
        assert!(same_number("Iliad", "Iliad"));
        assert!(!same_number("Iliad", "iliad"));
    }

    #[test]
    fn finds_contacts_by_any_of_their_numbers() {
        let contacts = Contacts::new(vcard::parse(
            "BEGIN:VCARD\nVERSION:3.0\nFN:Zoe\nTEL;TYPE=CELL:+39 351 851 3652\n\
             TEL;TYPE=WORK:02 1234 5678\nEND:VCARD\n\
             BEGIN:VCARD\nVERSION:4.0\nFN:Ada\nTEL:4040\nEND:VCARD\n",
        ));
        let name = |number| contacts.find(number).map(|i| &contacts.all()[i].name);
        assert_eq!(name("3518513652").map(String::as_str), Some("Zoe"));
        assert_eq!(name("+390212345678").map(String::as_str), Some("Zoe"));
        assert_eq!(name("4040").map(String::as_str), Some("Ada"));
        assert_eq!(name("14040"), None);
        assert_eq!(name("Iliad"), None);
        // Sorted by name.
        assert_eq!(contacts.all()[0].name, "Ada");
    }

    #[test]
    fn reads_collections_one_level_down_once_per_uid() {
        let folder =
            std::env::temp_dir().join(format!("denial-contacts-load-{}", std::process::id()));
        let card = |uid: &str, name: &str| {
            format!("BEGIN:VCARD\nVERSION:3.0\nUID:{uid}\nFN:{name}\nEND:VCARD\n")
        };
        for collection in ["", "default", "work", "work/deeper", ".hidden"] {
            std::fs::create_dir_all(folder.join(collection)).unwrap();
        }
        std::fs::write(folder.join("flat.vcf"), card("1", "Flat")).unwrap();
        std::fs::write(folder.join("default/ada.vcf"), card("2", "Ada")).unwrap();
        std::fs::write(folder.join("work/ada.vcf"), card("2", "Ada")).unwrap();
        std::fs::write(folder.join("work/zoe.vcf"), card("3", "Zoe")).unwrap();
        std::fs::write(folder.join("work/deeper/no.vcf"), card("4", "Deep")).unwrap();
        std::fs::write(folder.join(".hidden/no.vcf"), card("5", "Hidden")).unwrap();
        let contacts = Contacts::load(&folder);
        let names: Vec<&str> = contacts.all().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Ada", "Flat", "Zoe"]);
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
