//! The apps' words in the user's language. Apps are written in English;
//! each registers its translations, from the English, and passes what it
//! shows through `tr`. Denial's `localization.locale` picks the language,
//! as it does the shell's: English, Simplified Chinese, or the system's,
//! from `LC_ALL`, `LC_MESSAGES` or `LANG`. Words with no translation stay
//! English.
//!
//! Words that take a value name it in braces, `"{count} fingers"`, the
//! same in both languages; the app replaces it after `tr`.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    English,
    Chinese,
}

static LANGUAGE: AtomicU8 = AtomicU8::new(0);
static CHINESE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();

/// The app's words in Simplified Chinese, by their English; once, at start.
pub fn register_chinese(words: &'static [(&'static str, &'static str)]) {
    let _ = CHINESE.set(words.iter().copied().collect());
}

/// The language the app shows, as Denial's settings last said.
pub fn language() -> Language {
    match LANGUAGE.load(Ordering::Relaxed) {
        1 => Language::Chinese,
        _ => Language::English,
    }
}

/// Takes the language from Denial's settings, for a program that shows
/// words without a look; `Look::load` does this too.
pub fn load() {
    follow(&crate::settings());
}

pub(crate) fn follow(settings: &Value) {
    let language = match settings["localization"]["locale"].as_str() {
        Some("english") => Language::English,
        Some("simplifiedChinese") => Language::Chinese,
        _ => system(),
    };
    LANGUAGE.store(language as u8, Ordering::Relaxed);
}

/// The system's language, as the C library would pick it.
fn system() -> Language {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(std::env::var_os)
        .find(|value| !value.is_empty())
        .unwrap_or_default();
    if locale.to_string_lossy().starts_with("zh") {
        Language::Chinese
    } else {
        Language::English
    }
}

/// `english` in the app's language.
pub fn tr(english: &'static str) -> &'static str {
    match language() {
        Language::English => english,
        Language::Chinese => CHINESE
            .get()
            .and_then(|words| words.get(english))
            .copied()
            .unwrap_or(english),
    }
}

/// `english` in the app's language, its named values filled in:
/// `fill("{count} fingers", &[("count", &count.to_string())])`.
pub fn fill(english: &'static str, values: &[(&str, &str)]) -> String {
    values
        .iter()
        .fold(tr(english).to_owned(), |text, (name, value)| {
            text.replace(&format!("{{{name}}}"), value)
        })
}

/// `text` in the app's language, when it is a word the app translates:
/// for text that may also be a name, as a network's, which stays as it is.
pub fn translate(text: &str) -> Option<&'static str> {
    match language() {
        Language::English => None,
        Language::Chinese => CHINESE.get()?.get(text).copied(),
    }
}

/// What is wrong with a table of words, for the app's tests: an English
/// given twice, or a translation that names other values than its English.
pub fn faults(words: &[(&str, &str)]) -> Vec<String> {
    let names = |text: &str| {
        let mut names: Vec<String> = text
            .split('{')
            .skip(1)
            .filter_map(|rest| rest.split_once('}').map(|(name, _)| name.to_owned()))
            .collect();
        names.sort();
        names
    };
    let mut seen = std::collections::HashSet::new();
    let mut faults = Vec::new();
    for (english, translated) in words {
        if !seen.insert(*english) {
            faults.push(format!("{english:?} twice"));
        }
        if names(english) != names(translated) {
            faults.push(format!("{english:?} and {translated:?} name other values"));
        }
    }
    faults
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_faults() {
        assert!(faults(&[("{count} fingers", "{count} 个指纹")]).is_empty());
        assert_eq!(faults(&[("A", "甲"), ("A", "乙")]).len(), 1);
        assert_eq!(faults(&[("{count} fingers", "{n} 个指纹")]).len(), 1);
    }
}
