//! Holds a battery's charge while the charger stays connected, or lets it
//! charge again: Settings runs it through pkexec. `charge-hold SUPPLY on`
//! or `charge-hold SUPPLY off`, SUPPLY a battery under
//! `/sys/class/power_supply`.
//!
//! It writes the battery's standard `charge_behaviour`: `inhibit-charge`
//! holds the charge, so the device runs from the charger and the battery
//! neither charges nor drains; `auto` charges as usual. Only a battery
//! whose driver offers `inhibit-charge` is written.

use std::fs;
use std::process::ExitCode;

const SUPPLIES: &str = "/sys/class/power_supply";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("{why}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [supply, hold] = args.as_slice() else {
        return Err("usage: charge-hold SUPPLY on|off".into());
    };
    let behaviour = match hold.as_str() {
        "on" => "inhibit-charge",
        "off" => "auto",
        _ => return Err(format!("{hold} is neither on nor off")),
    };
    if supply.is_empty() || supply.starts_with('.') || supply.contains('/') {
        return Err(format!("{supply} is not a power supply"));
    }
    let folder = format!("{SUPPLIES}/{supply}");
    let kind = fs::read_to_string(format!("{folder}/type"))
        .map_err(|error| format!("{folder}: {error}"))?;
    if kind.trim() != "Battery" {
        return Err(format!("{supply} is not a battery"));
    }
    let file = format!("{folder}/charge_behaviour");
    let offered = fs::read_to_string(&file).map_err(|error| format!("{file}: {error}"))?;
    if !offered
        .split_whitespace()
        .any(|each| each.trim_matches(['[', ']']) == "inhibit-charge")
    {
        return Err(format!("{supply} cannot hold its charge"));
    }
    fs::write(&file, behaviour).map_err(|error| format!("{file}: {error}"))
}
