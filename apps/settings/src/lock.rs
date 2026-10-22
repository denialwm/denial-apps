//! How the lock screen opens: the lock PIN, kept by Denial and changed
//! through `deniald --lock-pin`.
//!
//! The PIN helper runs as the session's user and checks the account password
//! itself before it writes or removes the PIN. Secrets go one per line on
//! its input, never in its arguments, environment or logs; its last output
//! line is its answer, as JSON.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// Digits a lock PIN may have; deniald holds the same bounds.
pub const PIN_DIGITS: std::ops::RangeInclusive<usize> = 4..=12;

pub fn valid_pin(pin: &str) -> bool {
    PIN_DIGITS.contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
}

/// What the helper made of a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Saved,
    Cleared,
    WrongPassword,
    InvalidPin,
    /// The helper did not run, or said nothing it should.
    Failed,
}

/// The PIN's length, 0 when the lock screen asks for the password; None
/// when the helper does not answer.
pub fn pin_length() -> Option<u32> {
    let answer = run(&["status"])?;
    (answer["event"] == "status").then(|| answer["length"].as_u64().unwrap_or(0) as u32)
}

/// Sets the PIN, once `password` is the account's.
pub fn set_pin(password: &str, pin: &str) -> Outcome {
    if !valid_pin(pin) {
        return Outcome::InvalidPin;
    }
    outcome(run(&["set", password, pin]))
}

/// Removes the PIN, so the lock screen asks for the password.
pub fn clear_pin(password: &str) -> Outcome {
    outcome(run(&["clear", password]))
}

fn outcome(answer: Option<serde_json::Value>) -> Outcome {
    match answer.as_ref().and_then(|answer| answer["event"].as_str()) {
        Some("saved") => Outcome::Saved,
        Some("cleared") => Outcome::Cleared,
        Some("authentication-failed") => Outcome::WrongPassword,
        Some("invalid-pin") => Outcome::InvalidPin,
        _ => Outcome::Failed,
    }
}

/// Runs the helper with `lines` on its input. The compositor that started
/// Settings names itself in `DENIAL_COMPOSITOR_BINARY`, so the helper
/// speaks its protocol; else the first `deniald` on PATH.
fn run(lines: &[&str]) -> Option<serde_json::Value> {
    if lines
        .iter()
        .any(|line| line.contains(['\n', '\r', '\0']) || line.is_empty())
    {
        return None;
    }
    let binary = std::env::var_os("DENIAL_COMPOSITOR_BINARY")
        .filter(|binary| !binary.is_empty())
        .unwrap_or_else(|| "deniald".into());
    let mut helper = Command::new(binary)
        .arg("--lock-pin")
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| eprintln!("settings: starting the lock PIN helper: {error}"))
        .ok()?;
    let mut input = helper.stdin.take()?;
    for line in lines {
        writeln!(input, "{line}").ok()?;
    }
    drop(input);
    let last = BufReader::new(helper.stdout.take()?)
        .lines()
        .map_while(Result::ok)
        .last();
    let _ = helper.wait();
    serde_json::from_str(&last?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_are_four_to_twelve_digits() {
        assert!(valid_pin("1234"));
        assert!(valid_pin("123456789012"));
        assert!(!valid_pin("123"));
        assert!(!valid_pin("1234567890123"));
        assert!(!valid_pin("12a4"));
    }

    /// Reads the PIN length on a phone: `cargo test -- --ignored
    /// --nocapture reads_the_lock`, as the session's user.
    #[test]
    #[ignore]
    fn reads_the_lock() {
        println!("PIN length {:?}", pin_length());
    }
}
