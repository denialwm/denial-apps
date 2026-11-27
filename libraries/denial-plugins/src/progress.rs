//! The stages a request reports. Their labels are a protocol value that
//! Settings words itself: never reword one.

use serde_json::Value;

use crate::object;
use crate::util::Map;

/// Reads the Flutter build's verbose output for real target starts; other
/// lines are never progress.
#[derive(Default)]
pub struct CompilerProgress {
    last: Option<&'static str>,
}

fn without_escapes(line: &str) -> String {
    let mut clean = String::with_capacity(line.len());
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' && characters.peek() == Some(&'[') {
            characters.next();
            while let Some(&next) = characters.peek() {
                characters.next();
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
            continue;
        }
        clean.push(character);
    }
    clean
}

/// The target a line says has started, if it says so.
fn started_target(line: &str) -> Option<String> {
    let mut rest = line.trim_start();
    if let Some(after) = rest.strip_prefix('[') {
        // `[  +12 ms]`
        let end = after.find(']')?;
        let inside = after[..end].trim();
        let number = inside.strip_prefix('+').unwrap_or(inside);
        if !inside.is_empty() {
            let digits = number.strip_suffix("ms")?.trim_end();
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
        }
        rest = after[end + 1..].trim_start();
    }
    let (target, tail) = rest.split_once(": Starting")?;
    if !(tail.is_empty() || tail.starts_with(char::is_whitespace)) {
        return None;
    }
    let known = matches!(
        target,
        "kernel_snapshot"
            | "kernel_snapshot_program"
            | "aot_elf_release"
            | "release_bundle_linux-x64_assets"
            | "release_bundle_linux-arm64_assets"
    );
    known.then(|| target.to_owned())
}

impl CompilerProgress {
    pub fn observe(&mut self, line: &str) -> Option<&'static str> {
        let target = started_target(&without_escapes(line))?;
        let phase = match target.as_str() {
            "kernel_snapshot" | "kernel_snapshot_program" => "Compiling Dart sources",
            "aot_elf_release" => "Optimizing and generating native code",
            _ => "Preparing assets",
        };
        if self.last == Some(phase) {
            return None;
        }
        self.last = Some(phase);
        Some(phase)
    }
}

/// The completed stages of `operation` that `message` reports, not an
/// estimate of time. A line it does not know never replaces the last stage.
pub fn job_progress(operation: &str, message: &str) -> Option<Map> {
    // A rebuild after a Denial update also waits for a pause to switch.
    let rebuilding = operation == "rebuild";
    let applying = rebuilding || operation == "apply" || operation == "update";
    let building = |applying_step: i64, other: i64| if applying { applying_step } else { other };
    let (step, label): (i64, &str) = if message.starts_with("Snapshotting")
        || message.starts_with("Resolving")
    {
        (0, "Preparing your plugins")
    } else if message.starts_with("Discovering")
        || message == "Reusing checked plugin contributions"
        || message == "Preparing plugin discovery"
    {
        (1, "Checking plugin compatibility")
    } else if message == "Preparing the release compiler" {
        (building(2, 0), "Preparing your desktop build")
    } else if [
        "Compiling Dart sources",
        "Optimizing and generating native code",
        "Preparing assets",
    ]
    .contains(&message)
    {
        (building(2, 0), message)
    } else if message.starts_with("Compiling") || message == "Reusing a verified desktop bundle" {
        let label = if message.starts_with("Reusing") {
            "Loading your saved desktop"
        } else {
            "Compiling your desktop"
        };
        (building(2, 0), label)
    } else if message.starts_with("Validating and sealing") {
        (building(3, 1), "Verifying your desktop")
    } else if rebuilding && message == "Waiting for a pause to switch" {
        (4, "Switching when you pause")
    } else if message == "Applying your desktop" {
        let step = if rebuilding {
            5
        } else if applying {
            4
        } else {
            0
        };
        (step, "Applying and checking your desktop")
    } else if message == "Verifying the installed release build tools" {
        (0, "Verifying plugin tools")
    } else if message == "Preparing your compiler cache" {
        (building(0, 1), "Preparing plugin tools")
    } else {
        return None;
    };
    let total = if rebuilding {
        6
    } else if applying {
        5
    } else if ["activate", "restore", "revert"].contains(&operation) {
        1
    } else {
        2
    };
    Some(object! {
        "completed" => step,
        "total" => total,
        "label" => Value::from(label),
    })
}
