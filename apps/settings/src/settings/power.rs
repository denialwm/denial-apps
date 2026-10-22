//! The Battery page: the level and what the battery is doing, then how worn
//! it is. Everything comes from UPower as a fresh `battery::State`; nothing
//! here changes the phone.

use std::time::Duration;

use denial_look_gpui::{Icon, island};
use gpui::{Context, Div, SharedString, prelude::*};

use super::{Layer, Settings};
use crate::battery::{Battery, Charge};

impl Settings {
    /// The Battery page's islands, top to bottom: the level, then the
    /// battery's details.
    pub(super) fn battery_page(&self, _layer: Layer, _cx: &mut Context<Self>) -> Vec<Div> {
        let power = &self.power;
        if !power.read {
            return Vec::new();
        }
        let Some(state) = &power.state else {
            return vec![self.missing(
                Icon::Battery,
                "Battery is not available",
                "UPower, which reads the battery, does not run on this phone",
            )];
        };
        let Some(battery) = &state.battery else {
            return vec![self.missing(
                Icon::Battery,
                "No battery",
                "This device runs on its charger",
            )];
        };
        let level = self.row(
            "level",
            Some(Icon::Battery),
            percent(battery.level),
            Some(doing(battery, state.plugged).into()),
            (true, true),
        );
        let mut parts = vec![island(&self.look).child(level)];

        let mut details: Vec<(&str, String)> = Vec::new();
        if let Some(health) = battery.health {
            details.push(("Health", percent(health)));
        }
        if let Some(cycles) = battery.cycles {
            details.push(("Charge cycles", cycles.to_string()));
        }
        if let Some(celsius) = battery.temperature {
            details.push(("Temperature", format!("{celsius:.1} °C")));
        }
        if battery.power > 0.0 {
            let title = match battery.charge {
                Charge::Charging => "Charging at",
                _ => "Using",
            };
            details.push((title, format!("{:.2} W", battery.power)));
        }
        if !details.is_empty() {
            parts.push(self.chip_row("Battery"));
            parts.push(
                self.rows_island(details.into_iter(), 0., |(title, value), ends| {
                    self.row(title, None, title, None, ends)
                        .child(self.value(value))
                }),
            );
        }
        parts
    }

    /// Battery's line under its title on the home screen.
    pub(super) fn battery_summary(&self) -> Option<SharedString> {
        let power = &self.power;
        if !power.read {
            return None;
        }
        let Some(state) = &power.state else {
            return Some("Not available".into());
        };
        let Some(battery) = &state.battery else {
            return Some("No battery".into());
        };
        Some(
            format!(
                "{} · {}",
                percent(battery.level),
                doing(battery, state.plugged)
            )
            .into(),
        )
    }
}

fn percent(level: f64) -> String {
    format!("{}%", level.round())
}

/// What the battery is doing, and for how long, as one line.
fn doing(battery: &Battery, plugged: bool) -> String {
    match (battery.charge, battery.until) {
        (Charge::Charging, Some(until)) => format!("Charging · full in {}", lasting(until)),
        (Charge::Charging, None) => "Charging".to_owned(),
        (Charge::Full, _) if plugged => "Fully charged".to_owned(),
        (Charge::Held, _) => "Plugged in, not charging".to_owned(),
        (_, Some(until)) => format!("About {} left", lasting(until)),
        (_, None) if plugged => "Plugged in".to_owned(),
        (_, None) => "On battery".to_owned(),
    }
}

/// `time` as people say it: "40 min", "2 h 15 min", "23 h", "3 days".
fn lasting(time: Duration) -> String {
    let minutes = (time.as_secs() + 30) / 60;
    let (hours, minutes) = (minutes / 60, minutes % 60);
    match hours {
        0 => format!("{minutes} min"),
        1..10 if minutes > 0 => format!("{hours} h {minutes} min"),
        1..48 => format!("{hours} h"),
        _ => format!("{} days", (hours + 12) / 24),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_how_long() {
        let minutes = |minutes: u64| lasting(Duration::from_secs(minutes * 60));
        assert_eq!(minutes(40), "40 min");
        assert_eq!(minutes(135), "2 h 15 min");
        assert_eq!(minutes(120), "2 h");
        assert_eq!(minutes(23 * 60 + 31), "23 h");
        assert_eq!(minutes(72 * 60), "3 days");
    }
}
