//! The Battery page: the level and what the battery is doing, then how worn
//! it is. UPower supplies the battery readings and charge-limit controls.

use denial_look::words::{fill, tr};
use std::time::Duration;

use denial_look_gpui::{Icon, island};
use gpui::{Context, Div, SharedString, prelude::*};

use super::{Layer, Settings};
use crate::battery::{self, Battery, Charge};

impl Settings {
    /// The Battery page's islands, top to bottom: the level, then the
    /// battery's details.
    pub(super) fn battery_page(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
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
            if !state.charge_limits.is_empty() {
                return self.charge_limit_parts(layer, cx);
            }
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
        parts.extend(self.charge_limit_parts(layer, cx));
        parts
    }

    pub(super) fn charge_limit_parts(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let Some(state) = self
            .power
            .state
            .as_ref()
            .filter(|state| !state.charge_limits.is_empty())
        else {
            return Vec::new();
        };
        let group = self.rows_island(state.charge_limits.iter(), 0., |limit, ends| {
            let detail = match (limit.start, limit.end, limit.firmware_optimized) {
                (Some(start), Some(end), _) => fill(
                    "Resume charging below {start}% and stop at {end}%.",
                    &[("start", &start.to_string()), ("end", &end.to_string())],
                ),
                (_, Some(end), _) => fill("Stop charging at {end}%.", &[("end", &end.to_string())]),
                (Some(start), _, _) => fill(
                    "Resume charging below {start}%.",
                    &[("start", &start.to_string())],
                ),
                (_, _, true) => tr("Firmware chooses the charging pattern.").to_owned(),
                _ => tr("Limit the full charge level to reduce battery wear.").to_owned(),
            };
            let title = if state.charge_limits.len() == 1 || limit.name.is_empty() {
                tr("Limit battery charging").to_owned()
            } else {
                fill("Limit charging for {battery}", &[("battery", &limit.name)])
            };
            let row = self
                .row(
                    SharedString::from(format!("charge-limit:{}", limit.path)),
                    Some(Icon::Battery),
                    title,
                    Some(detail.into()),
                    ends,
                )
                .child(self.toggle(limit.enabled));
            let path = limit.path.clone();
            let enabled = !limit.enabled;
            self.live(
                row,
                layer.live() && !self.power.changing_limits.contains(&path),
                cx,
                move |this, _, cx| this.set_charge_limit(path.clone(), enabled, cx),
            )
        });
        vec![self.chip_row("Charging limits"), group]
    }

    fn set_charge_limit(
        &mut self,
        path: zbus::zvariant::OwnedObjectPath,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.power.changing_limits.insert(path.clone()) {
            return;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = battery::set_charge_limit(&path, enabled).await;
            let _ = this.update(cx, |view, cx| {
                view.power.changing_limits.remove(&path);
                match result {
                    Ok(()) => {
                        if let Some(limit) = view.power.state.as_mut().and_then(|state| {
                            state
                                .charge_limits
                                .iter_mut()
                                .find(|limit| limit.path == path)
                        }) {
                            limit.enabled = enabled;
                        }
                        cx.notify();
                    }
                    Err(error) => {
                        eprintln!("settings: setting charge limit: {error}");
                        view.note("Could not change the battery charge limit", cx);
                    }
                }
            });
        })
        .detach();
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
        (Charge::Charging, Some(until)) => {
            fill("Charging · full in {time}", &[("time", &lasting(until))])
        }
        (Charge::Charging, None) => tr("Charging").to_owned(),
        (Charge::Full, _) if plugged => tr("Fully charged").to_owned(),
        (Charge::Held, _) => tr("Plugged in, not charging").to_owned(),
        (_, Some(until)) => fill("About {time} left", &[("time", &lasting(until))]),
        (_, None) if plugged => tr("Plugged in").to_owned(),
        (_, None) => tr("On battery").to_owned(),
    }
}

/// `time` as people say it: "40 min", "2 h 15 min", "23 h", "3 days".
fn lasting(time: Duration) -> String {
    let minutes = (time.as_secs() + 30) / 60;
    let (hours, minutes) = (minutes / 60, minutes % 60);
    match hours {
        0 => fill("{minutes} min", &[("minutes", &minutes.to_string())]),
        1..10 if minutes > 0 => fill(
            "{hours} h {minutes} min",
            &[
                ("hours", &hours.to_string()),
                ("minutes", &minutes.to_string()),
            ],
        ),
        1..48 => fill("{hours} h", &[("hours", &hours.to_string())]),
        _ => fill("{days} days", &[("days", &((hours + 12) / 24).to_string())]),
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
