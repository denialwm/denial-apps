//! The phone's clock, through systemd's timedated
//! (`org.freedesktop.timedate1` on the system bus): its time zone, whether
//! it keeps the time from the network, and the time itself when it does
//! not. The 24-hour clock is Denial's, in its settings document.

use denial_look::words::{fill, tr};
use std::time::{SystemTime, UNIX_EPOCH};

use zbus::Connection;
use zbus::zvariant::OwnedValue;

const NAME: &str = "org.freedesktop.timedate1";
const PATH: &str = "/org/freedesktop/timedate1";

/// The time zones to pick from, by a city in each, west to east.
pub const ZONES: [(&str, &str); 31] = [
    ("Honolulu", "Pacific/Honolulu"),
    ("Anchorage", "America/Anchorage"),
    ("Los Angeles", "America/Los_Angeles"),
    ("Denver", "America/Denver"),
    ("Chicago", "America/Chicago"),
    ("New York", "America/New_York"),
    ("São Paulo", "America/Sao_Paulo"),
    ("UTC", "UTC"),
    ("London", "Europe/London"),
    ("Lisbon", "Europe/Lisbon"),
    ("Paris", "Europe/Paris"),
    ("Berlin", "Europe/Berlin"),
    ("Rome", "Europe/Rome"),
    ("Madrid", "Europe/Madrid"),
    ("Athens", "Europe/Athens"),
    ("Istanbul", "Europe/Istanbul"),
    ("Moscow", "Europe/Moscow"),
    ("Cairo", "Africa/Cairo"),
    ("Johannesburg", "Africa/Johannesburg"),
    ("Dubai", "Asia/Dubai"),
    ("Karachi", "Asia/Karachi"),
    ("Kolkata", "Asia/Kolkata"),
    ("Bangkok", "Asia/Bangkok"),
    ("Singapore", "Asia/Singapore"),
    ("Shanghai", "Asia/Shanghai"),
    ("Hong Kong", "Asia/Hong_Kong"),
    ("Taipei", "Asia/Taipei"),
    ("Seoul", "Asia/Seoul"),
    ("Tokyo", "Asia/Tokyo"),
    ("Sydney", "Australia/Sydney"),
    ("Auckland", "Pacific/Auckland"),
];

#[derive(Clone, Debug, Default)]
pub struct State {
    /// As the tz database names it: "Europe/Rome".
    pub zone: String,
    /// The time comes from the network.
    pub network: bool,
    /// Whether there is a service that would keep it.
    pub can_network: bool,
}

/// A zone's city, when it is one to pick from; else its last part.
pub fn city(zone: &str) -> String {
    match ZONES.iter().find(|(_, each)| *each == zone) {
        Some((city, _)) => (*city).to_owned(),
        None => zone.rsplit('/').next().unwrap_or(zone).replace('_', " "),
    }
}

async fn property(bus: &Connection, name: &str) -> zbus::Result<OwnedValue> {
    bus.call_method(
        Some(NAME),
        PATH,
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &(NAME, name),
    )
    .await?
    .body()
    .deserialize()
}

/// What timedated says; None when it does not answer.
pub async fn read() -> Option<State> {
    let bus = Connection::system().await.ok()?;
    let zone = String::try_from(property(&bus, "Timezone").await.ok()?).ok()?;
    let flag = async |name| {
        property(&bus, name)
            .await
            .ok()
            .and_then(|value| bool::try_from(value).ok())
            .unwrap_or(false)
    };
    Some(State {
        zone,
        network: flag("NTP").await,
        can_network: flag("CanNTP").await,
    })
}

/// Calls one of timedated's methods. Denial's polkit agent asks for a
/// password where the phone's rules do not allow the change outright.
async fn call<B>(method: &str, body: &B) -> Result<(), String>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    let bus = Connection::system()
        .await
        .map_err(|error| error.to_string())?;
    bus.call_method(Some(NAME), PATH, Some(NAME), method, body)
        .await
        .map(drop)
        .map_err(|error| match error {
            zbus::Error::MethodError(name, _, _)
                if name.as_str()
                    == "org.freedesktop.DBus.Error.InteractiveAuthorizationRequired"
                    || name.as_str().ends_with("AccessDenied") =>
            {
                "Not allowed".to_owned()
            }
            zbus::Error::MethodError(_, Some(said), _) => said,
            error => error.to_string(),
        })
}

pub async fn set_zone(zone: &str) -> Result<(), String> {
    call("SetTimezone", &(zone, true)).await
}

pub async fn set_network(on: bool) -> Result<(), String> {
    call("SetNTP", &(on, true)).await
}

/// Sets the clock to `seconds` since the epoch, UTC.
pub async fn set_time(seconds: i64) -> Result<(), String> {
    call("SetTime", &(seconds * 1_000_000, false, true)).await
}

/// The local time now, as broken down by the C library: the time zone
/// timedated set is the one `localtime_r` reads.
pub fn now() -> libc::tm {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as libc::time_t);
    local(seconds)
}

/// Seconds since the epoch, now.
pub fn seconds_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}

unsafe extern "C" {
    /// The C library's; the libc crate does not bind it.
    fn tzset();
}

pub fn local(seconds: libc::time_t) -> libc::tm {
    // SAFETY: tzset reads the environment and /etc/localtime again, which
    // localtime_r alone does once only; localtime_r writes only into `tm`.
    unsafe {
        tzset();
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&seconds, &mut tm);
        tm
    }
}

/// `tm`, local, as seconds since the epoch.
fn seconds(mut tm: libc::tm) -> i64 {
    tm.tm_isdst = -1;
    // SAFETY: mktime normalises the fields of a valid `tm` in place.
    unsafe { libc::mktime(&mut tm) as i64 }
}

/// The date as typed, "2026-10-22", and the time of day now: seconds since
/// the epoch; None when it is no date.
pub fn with_date(typed: &str) -> Option<i64> {
    let mut parts = typed.trim().splitn(3, ['-', '/', '.']);
    let year: i32 = parts.next()?.trim().parse().ok()?;
    let month: i32 = parts.next()?.trim().parse().ok()?;
    let day: i32 = parts.next()?.trim().parse().ok()?;
    if !(2000..=2100).contains(&year) || !(1..=12).contains(&month) || day < 1 {
        return None;
    }
    let mut tm = now();
    tm.tm_year = year - 1900;
    tm.tm_mon = month - 1;
    tm.tm_mday = day;
    let seconds = seconds(tm);
    // A day past the month's end rolls into the next one.
    (local(seconds as libc::time_t).tm_mday == day).then_some(seconds)
}

/// Today at the time as typed, "17:45": seconds since the epoch; None when
/// it is no time.
pub fn with_time(typed: &str) -> Option<i64> {
    let (hour, minute) = typed.trim().split_once([':', '.'])?;
    let (hour, minute): (i32, i32) = (hour.trim().parse().ok()?, minute.trim().parse().ok()?);
    if !(0..24).contains(&hour) || !(0..60).contains(&minute) {
        return None;
    }
    let mut tm = now();
    tm.tm_hour = hour;
    tm.tm_min = minute;
    tm.tm_sec = 0;
    Some(seconds(tm))
}

/// The local day of `ms` milliseconds since the epoch: "2026-10-23".
pub fn day_of(ms: i64) -> String {
    date_text(&local((ms / 1000) as libc::time_t))
}

/// "2026-10-22".
pub fn date_text(tm: &libc::tm) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday
    )
}

/// "17:45", or "5:45 PM" on a 12-hour clock.
pub fn time_text(tm: &libc::tm, twenty_four: bool) -> String {
    if twenty_four {
        return format!("{:02}:{:02}", tm.tm_hour, tm.tm_min);
    }
    let hour = match tm.tm_hour % 12 {
        0 => 12,
        hour => hour,
    };
    let half = tr(if tm.tm_hour < 12 { "AM" } else { "PM" });
    fill(
        "{hour}:{minute} {half}",
        &[
            ("hour", &hour.to_string()),
            ("minute", &format!("{:02}", tm.tm_min)),
            ("half", half),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_typed_times() {
        assert!(with_time("17:45").is_some());
        assert!(with_time("7.05").is_some());
        assert!(with_time("24:00").is_none());
        assert!(with_date("2026-10-22").is_some());
        assert!(with_date("2026-02-30").is_none());
        assert!(with_date("22-10-2026").is_none());
        let mut tm = now();
        tm.tm_hour = 0;
        tm.tm_min = 5;
        assert_eq!(time_text(&tm, false), "12:05 AM");
        assert_eq!(time_text(&tm, true), "00:05");
        assert_eq!(city("Europe/Rome"), "Rome");
        assert_eq!(city("America/Argentina/Buenos_Aires"), "Buenos Aires");
    }
}
