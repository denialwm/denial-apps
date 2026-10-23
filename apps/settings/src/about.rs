//! What the phone is: read from the kernel's and systemd's files when About
//! opens. The phone's number and Bluetooth address come from dmail and
//! BlueZ, which Settings follows anyway; Denial's version from deniald.

use denial_look::words::fill;
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct Facts {
    /// The name the owner gave the phone.
    pub name: Option<String>,
    /// As the device tree or the firmware names it: "OnePlus Ace 3".
    pub model: Option<String>,
    /// The chip: "Snapdragon SM8550".
    pub chip: Option<String>,
    pub cores: usize,
    /// The fastest core's top speed, in GHz.
    pub top_ghz: Option<f64>,
    /// Bytes the kernel manages; a little under what the phone has.
    pub memory: Option<u64>,
    /// Bytes used and in all, on the system's file system.
    pub storage: Option<(u64, u64)>,
    /// The lit panel's pixels, width by height.
    pub panel: Option<(u32, u32)>,
    pub system: Option<String>,
    /// The kernel's release, and the day it was built.
    pub kernel: Option<String>,
    pub kernel_built: Option<String>,
    /// The Wi-Fi hardware address.
    pub wifi: Option<String>,
    pub up: Option<Duration>,
}

/// Reads what the kernel's files say; never blocks for long.
pub fn read() -> Facts {
    let (cores, top_ghz) = cores();
    let kernel_version = text("/proc/sys/kernel/version");
    Facts {
        name: crate::name::read(),
        model: text("/sys/firmware/devicetree/base/model")
            .or_else(|| text("/sys/class/dmi/id/product_name")),
        chip: chip(),
        cores,
        top_ghz,
        memory: meminfo_kib("MemTotal").map(|kib| kib * 1024),
        storage: storage("/"),
        panel: panel(),
        system: os_release("PRETTY_NAME").or_else(|| os_release("NAME")),
        kernel: text("/proc/sys/kernel/osrelease"),
        kernel_built: kernel_version.as_deref().and_then(built_day),
        wifi: wifi(),
        up: text("/proc/uptime")
            .and_then(|up| up.split_whitespace().next()?.parse::<f64>().ok())
            .map(Duration::from_secs_f64),
    }
}

/// A file's text, without the device tree's NUL or trailing space; None
/// when missing or empty.
fn text(path: impl AsRef<Path>) -> Option<String> {
    let read = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&read)
        .trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_owned();
    (!text.is_empty()).then_some(text)
}

/// The SoC as the kernel names it, else the CPU's model name.
fn chip() -> Option<String> {
    let soc = "/sys/devices/soc0";
    match (
        text(format!("{soc}/family")),
        text(format!("{soc}/machine")),
    ) {
        (Some(family), Some(machine)) => return Some(format!("{family} {machine}")),
        (None, Some(machine)) => return Some(machine),
        _ => {}
    }
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    cpuinfo.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == "model name").then(|| value.trim().to_owned())
    })
}

/// How many cores, and the top speed of the fastest.
fn cores() -> (usize, Option<f64>) {
    let Ok(entries) = std::fs::read_dir("/sys/devices/system/cpu") else {
        return (0, None);
    };
    let mut count = 0;
    let mut top_khz: Option<u64> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(number) = name.to_str().and_then(|name| name.strip_prefix("cpu")) else {
            continue;
        };
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        count += 1;
        let khz = text(entry.path().join("cpufreq/cpuinfo_max_freq"))
            .and_then(|khz| khz.parse::<u64>().ok());
        top_khz = top_khz.max(khz);
    }
    (count, top_khz.map(|khz| khz as f64 / 1e6))
}

fn meminfo_kib(key: &str) -> Option<u64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    meminfo.lines().find_map(|line| {
        let rest = line.strip_prefix(key)?.strip_prefix(':')?;
        rest.split_whitespace().next()?.parse().ok()
    })
}

/// Bytes used and in all on the file system that holds `path`.
pub fn storage(path: &str) -> Option<(u64, u64)> {
    let path = std::ffi::CString::new(path).ok()?;
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is NUL-terminated and `stats` is a writable statvfs.
    if unsafe { libc::statvfs(path.as_ptr(), &mut stats) } != 0 {
        return None;
    }
    let block = stats.f_frsize as u64;
    let all = stats.f_blocks as u64 * block;
    let free = stats.f_bavail as u64 * block;
    (all > 0).then(|| (all.saturating_sub(free), all))
}

/// Bytes the files in the home folder take on its file system, as `du`
/// counts them: links are not followed, other file systems mounted inside
/// are left out. Takes a while on a full phone.
pub fn home_used() -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    let home = std::path::PathBuf::from(std::env::var_os("HOME")?);
    let device = std::fs::symlink_metadata(&home).ok()?.dev();
    let mut used = 0;
    let mut folders = vec![home];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.dev() != device {
                continue;
            }
            used += meta.blocks() * 512;
            if meta.is_dir() {
                folders.push(entry.path());
            }
        }
    }
    Some(used)
}

/// The first connected, enabled panel's preferred mode.
fn panel() -> Option<(u32, u32)> {
    let mut connectors: Vec<_> = std::fs::read_dir("/sys/class/drm")
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("modes").exists())
        .collect();
    connectors.sort();
    connectors.iter().find_map(|connector| {
        if text(connector.join("status")).as_deref() != Some("connected")
            || text(connector.join("enabled")).as_deref() == Some("disabled")
        {
            return None;
        }
        let modes = text(connector.join("modes"))?;
        let (width, height) = modes.lines().next()?.split_once('x')?;
        let height = height.trim_end_matches(|c: char| !c.is_ascii_digit());
        Some((width.parse().ok()?, height.parse().ok()?))
    })
}

fn os_release(key: &str) -> Option<String> {
    let release = std::fs::read_to_string("/etc/os-release")
        .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
        .ok()?;
    release.lines().find_map(|line| {
        let value = line.strip_prefix(key)?.strip_prefix('=')?;
        let value = value.trim_matches('"').to_owned();
        (!value.is_empty()).then_some(value)
    })
}

/// The build day in the kernel's version string, as a date:
/// "#1 SMP PREEMPT 2026-10-22 13:59:43 UTC" says 2026-10-22.
fn built_day(version: &str) -> Option<String> {
    version
        .split_whitespace()
        .find(|word| {
            let bytes = word.as_bytes();
            bytes.len() == 10
                && bytes[4] == b'-'
                && bytes[7] == b'-'
                && word.bytes().filter(u8::is_ascii_digit).count() == 8
        })
        .map(str::to_owned)
}

/// The hardware address of the first wireless interface.
fn wifi() -> Option<String> {
    let mut interfaces: Vec<_> = std::fs::read_dir("/sys/class/net")
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("wireless").exists() || path.join("phy80211").exists())
        .collect();
    interfaces.sort();
    interfaces
        .iter()
        .find_map(|interface| text(interface.join("address")))
        .map(|address| address.to_uppercase())
}

/// Memory as phones are sold with it: the kernel keeps some for itself, so
/// 14.6 GiB managed is a 16 GB phone.
pub fn memory_size(bytes: u64) -> String {
    const SIZES: [u64; 15] = [1, 2, 3, 4, 6, 8, 10, 12, 16, 18, 24, 32, 48, 64, 128];
    let gib = bytes as f64 / (1u64 << 30) as f64;
    match SIZES.iter().find(|&&size| size as f64 >= gib) {
        Some(size) => format!("{size} GB"),
        None => format!("{} GB", gib.ceil()),
    }
}

/// Bytes in decimal units, as storage is sold: "205 GB".
pub fn bytes(bytes: u64) -> String {
    let gb = bytes as f64 / 1e9;
    if gb >= 100.0 {
        format!("{gb:.0} GB")
    } else if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", bytes as f64 / 1e6)
    }
}

/// How long the phone has been on: "3 h 12 min", "2 days 4 h".
pub fn up_time(up: Duration) -> String {
    let minutes = up.as_secs() / 60;
    let (days, hours, minutes) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    let text = |count: u64| count.to_string();
    match (days, hours) {
        (0, 0) => fill("{minutes} min", &[("minutes", &text(minutes))]),
        (0, _) => fill(
            "{hours} h {minutes} min",
            &[("hours", &text(hours)), ("minutes", &text(minutes))],
        ),
        (1, _) => fill("1 day {hours} h", &[("hours", &text(hours))]),
        _ => fill(
            "{days} days {hours} h",
            &[("days", &text(days)), ("hours", &text(hours))],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_times() {
        assert_eq!(memory_size(15_334_168 * 1024), "16 GB");
        assert_eq!(memory_size(7_700_000 * 1024), "8 GB");
        assert_eq!(bytes(229_780_000_000), "230 GB");
        assert_eq!(bytes(23_400_000_000), "23.4 GB");
        assert_eq!(
            up_time(Duration::from_secs(3 * 3600 + 12 * 60 + 5)),
            "3 h 12 min"
        );
        assert_eq!(up_time(Duration::from_secs(26 * 3600)), "1 day 2 h");
        assert_eq!(
            built_day("#1 SMP PREEMPT 2026-10-22 13:59:43 UTC").as_deref(),
            Some("2026-10-22")
        );
    }

    /// Prints what a phone says: `cargo test -- --ignored --nocapture
    /// reads_facts`.
    #[test]
    #[ignore]
    fn reads_facts() {
        println!("{:#?}", read());
    }
}
