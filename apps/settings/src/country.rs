//! The country whose rules the Wi-Fi radios follow: wireless-regdb's
//! `/etc/conf.d/wireless-regdom`, set through the `wifi-country` helper
//! with pkexec. Without one the radios keep the world rules, which find
//! some 5 GHz networks only by listening.

use std::process::Command;

use denial_look::words::{self, Language};

use crate::countries::COUNTRIES;

const FILE: &str = "/etc/conf.d/wireless-regdom";
const HELPER: &str = "/usr/lib/denial-apps/wifi-country";
const ZONES: &str = "/usr/share/zoneinfo/zone.tab";

/// The country set, when it is one Settings knows.
pub fn read() -> Option<&'static str> {
    set_in(&std::fs::read_to_string(FILE).ok()?)
}

/// The country a wireless-regdom file sets: its last settings line.
fn set_in(text: &str) -> Option<&'static str> {
    let set = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("WIRELESS_REGDOM="))
        .last()?
        .trim_matches(['"', '\'']);
    known(set)
}

/// The country of a time zone, from tzdata's zone.tab.
pub fn for_zone(zone: &str) -> Option<&'static str> {
    in_table(&std::fs::read_to_string(ZONES).ok()?, zone)
}

fn in_table(table: &str, zone: &str) -> Option<&'static str> {
    table
        .lines()
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| {
            let mut fields = line.split('\t');
            let code = fields.next()?;
            (fields.nth(1)? == zone).then_some(code)
        })
        .and_then(known)
}

fn known(code: &str) -> Option<&'static str> {
    COUNTRIES
        .iter()
        .find(|(each, _, _)| *each == code)
        .map(|(each, _, _)| *each)
}

/// A country's name in the app's language.
pub fn name(code: &str) -> &'static str {
    match COUNTRIES.iter().find(|(each, _, _)| *each == code) {
        Some((_, _, chinese)) if words::language() == Language::Chinese => chinese,
        Some((_, english, _)) => english,
        None => "",
    }
}

/// Whether a country's name in either language holds what was typed.
pub fn matches(code: &str, typed: &str) -> bool {
    let typed = typed.trim().to_lowercase();
    typed.is_empty()
        || COUNTRIES
            .iter()
            .find(|(each, _, _)| *each == code)
            .is_some_and(|(each, english, chinese)| {
                english.to_lowercase().contains(&typed)
                    || chinese.contains(&typed)
                    || each.eq_ignore_ascii_case(&typed)
            })
}

/// Sets the country; blocks until the helper is done. Denial's polkit agent
/// asks for a password where the phone's rules do not allow it outright.
pub fn set(code: &str) -> Result<(), String> {
    let output = Command::new("pkexec")
        .args([HELPER, code])
        .output()
        .map_err(|error| error.to_string())?;
    match output.status.code() {
        Some(0) => Ok(()),
        // pkexec's own: the password asked for was not given, or the
        // rules do not allow it.
        Some(126 | 127) => Err("Not allowed".to_owned()),
        _ => Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_settings_line_counts() {
        let file = "# WIRELESS_REGDOM=\"DE\"\n#WIRELESS_REGDOM=\"FR\"\nWIRELESS_REGDOM=\"IT\"\n";
        assert_eq!(set_in(file), Some("IT"));
        assert_eq!(set_in("#WIRELESS_REGDOM=\"IT\"\n"), None);
        assert_eq!(set_in("WIRELESS_REGDOM=\"XX\"\n"), None);
    }

    #[test]
    fn a_zone_names_its_country() {
        let table =
            "# comment\nIT\t+4154+01229\tEurope/Rome\nVA\t+415408+0122711\tEurope/Vatican\n";
        assert_eq!(in_table(table, "Europe/Rome"), Some("IT"));
        assert_eq!(in_table(table, "Europe/Paris"), None);
    }

    #[test]
    fn countries_are_searchable_by_name_and_code() {
        assert!(matches("IT", "ital"));
        assert!(matches("IT", "意大利"));
        assert!(matches("IT", "it"));
        assert!(!matches("IT", "france"));
    }

    #[test]
    fn the_table_holds_each_code_once() {
        let mut codes: Vec<_> = COUNTRIES.iter().map(|(code, _, _)| *code).collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), COUNTRIES.len());
    }
}
