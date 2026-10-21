//! The call log, and how its numbers, times and lengths read.

/// What happened to a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Outgoing,
    Incoming,
    /// Rang here and was not answered.
    Missed,
    /// Rang here and was turned down.
    Declined,
}

impl Direction {
    pub fn label(self) -> &'static str {
        match self {
            Direction::Outgoing => "Outgoing",
            Direction::Incoming => "Incoming",
            Direction::Missed => "Missed",
            Direction::Declined => "Declined",
        }
    }
}

/// One call in the log.
pub struct Call {
    pub id: u64,
    /// As dialled, or as the caller's number came in.
    pub number: String,
    pub direction: Direction,
    /// Unix time it started ringing.
    pub time: i64,
    /// Seconds talked; 0 for one that was never answered.
    pub talked: u64,
    /// One of the made-up calls of `samples`.
    pub sample: bool,
}

impl Call {
    /// "Outgoing call, 4 min", "Missed call", as a call's history reads.
    pub fn summary(&self) -> String {
        match (self.direction, self.talked) {
            (Direction::Missed, _) => "Missed call".into(),
            (Direction::Declined, _) => "Declined call".into(),
            (Direction::Outgoing, 0) => "Outgoing call, not answered".into(),
            (direction, talked) => format!("{} call, {}", direction.label(), length(talked)),
        }
    }
}

/// A call's length: "35 s", "4 min", "1 h 12 min".
pub fn length(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds} s"),
        60..3600 => format!("{} min", seconds / 60),
        _ if seconds % 3600 < 60 => format!("{} h", seconds / 3600),
        _ => format!("{} h {} min", seconds / 3600, seconds % 3600 / 60),
    }
}

/// A running call's clock: "02:14", "1:02:14".
pub fn timer(seconds: u64) -> String {
    let (hours, minutes, seconds) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// A name's initials; a number has none.
pub fn initials(name: &str) -> String {
    if !name.chars().any(char::is_alphabetic) {
        return "#".into();
    }
    name.split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .collect()
}

/// The letter a name lists under: its first letter, upper case, or # when
/// it starts with none.
pub fn letter(name: &str) -> String {
    match name.chars().next() {
        Some(first) if first.is_alphabetic() => first.to_uppercase().collect(),
        _ => "#".into(),
    }
}

/// The digits of a number as typed, for matching part of one.
pub fn digits(number: &str) -> String {
    number.chars().filter(char::is_ascii_digit).collect()
}

/// A Unix time in the phone's time zone.
struct Local {
    /// Days since 1970-01-01 in local time.
    day: i64,
    weekday: usize,
    month: usize,
    month_day: i32,
    year: i32,
    hour: i32,
    minute: i32,
}

fn local(time: i64) -> Local {
    // SAFETY: localtime_r only writes the tm it is given.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&(time as libc::time_t), &mut tm);
        tm
    };
    Local {
        day: (time + tm.tm_gmtoff).div_euclid(86_400),
        weekday: tm.tm_wday.clamp(0, 6) as usize,
        month: tm.tm_mon.clamp(0, 11) as usize,
        month_day: tm.tm_mday,
        year: tm.tm_year + 1900,
        hour: tm.tm_hour,
        minute: tm.tm_min,
    }
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Days from `time` to `now`, by the calendar: 0 today, 1 yesterday.
pub fn days_ago(time: i64, now: i64) -> i64 {
    local(now).day - local(time).day
}

/// "18:02".
pub fn clock(time: i64) -> String {
    let t = local(time);
    format!("{:02}:{:02}", t.hour, t.minute)
}

/// Today, Yesterday, a weekday within the week, else the date.
pub fn day(time: i64, now: i64) -> String {
    let (t, today) = (local(time), local(now));
    match today.day - t.day {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        2..=6 => WEEKDAYS[t.weekday].into(),
        _ if t.year == today.year => format!("{} {}", t.month_day, MONTHS[t.month]),
        _ => format!("{} {} {}", t.month_day, MONTHS[t.month], t.year),
    }
}

/// "Today 18:02", or "21 Oct 2026, 18:02" beyond the week.
pub fn full_time(time: i64, now: i64) -> String {
    let t = local(time);
    if local(now).day - t.day <= 6 {
        return format!("{} {:02}:{:02}", day(time, now), t.hour, t.minute);
    }
    format!(
        "{} {} {}, {:02}:{:02}",
        t.month_day, MONTHS[t.month], t.year, t.hour, t.minute
    )
}

/// The clock for today, the day otherwise.
pub fn list_time(time: i64, now: i64) -> String {
    if days_ago(time, now) == 0 {
        clock(time)
    } else {
        day(time, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_and_timers_read_as_on_a_phone() {
        assert_eq!(length(35), "35 s");
        assert_eq!(length(245), "4 min");
        assert_eq!(length(3600), "1 h");
        assert_eq!(length(4330), "1 h 12 min");
        assert_eq!(timer(134), "02:14");
        assert_eq!(timer(3734), "1:02:14");
    }

    #[test]
    fn names_list_under_their_first_letter() {
        assert_eq!(letter("élodie"), "É");
        assert_eq!(letter("+44 7700"), "#");
        assert_eq!(initials("Giulia Rinaldi"), "GR");
        assert_eq!(initials("+447700900123"), "#");
    }
}
