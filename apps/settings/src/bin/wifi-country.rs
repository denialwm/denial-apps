//! Sets the country whose rules the Wi-Fi radios follow: Settings runs it
//! through pkexec. `wifi-country CODE`, an ISO 3166-1 code such as IT.
//!
//! The country goes into wireless-regdb's `/etc/conf.d/wireless-regdom`,
//! which its udev rule applies whenever the Wi-Fi stack loads, and `iw reg
//! set` applies it now. Other settings lines in the file are commented out,
//! so the file says one country.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, ExitCode};

const FILE: &str = "/etc/conf.d/wireless-regdom";

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
    let [code] = args.as_slice() else {
        return Err("usage: wifi-country CODE".into());
    };
    if code.len() != 2 || !code.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(format!("{code} is not a country code"));
    }
    let old = fs::read_to_string(FILE).unwrap_or_default();
    let mut new = String::new();
    for line in old.lines() {
        if line.trim_start().starts_with("WIRELESS_REGDOM=") {
            new.push('#');
        }
        new.push_str(line);
        new.push('\n');
    }
    new.push_str(&format!("WIRELESS_REGDOM=\"{code}\"\n"));
    let temporary = format!("{FILE}.new");
    let write = || -> std::io::Result<()> {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(new.as_bytes())?;
        file.set_permissions(fs::Permissions::from_mode(0o644))?;
        file.sync_all()?;
        fs::rename(&temporary, FILE)
    };
    write().map_err(|error| format!("{FILE}: {error}"))?;
    let status = Command::new("/usr/bin/iw")
        .args(["reg", "set", code])
        .status()
        .map_err(|error| format!("iw: {error}"))?;
    if !status.success() {
        return Err(format!("iw reg set {code} failed"));
    }
    Ok(())
}
