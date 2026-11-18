//! The Location page: where the GPS is, a switch that has it locate the
//! phone while the page is open, the position it found, and each app's
//! answer to Denial's location question. The GPS goes through dmail's
//! `location::Link`; the answers live in the portal permission store, where
//! denial-portal's GeoClue agent writes them.

use denial_look::words::fill;
use denial_look_gpui::{Icon, island};
use gpui::{AsyncApp, Context, Div, SharedString, WeakEntity, prelude::*};

use super::cellular::Info;
use super::parts::{ROW_GAP, ROW_PAD, TILE};
use super::{Layer, Settings};
use crate::location::{self, Fix, Gps};

const SEPARATOR: f32 = ROW_PAD + TILE + ROW_GAP;
/// Slower than this, the GPS's speed is its noise.
const STILL: f64 = 0.5;

impl Settings {
    /// Follows dmail's location for as long as the window lives.
    pub(super) async fn follow_location(this: WeakEntity<Self>, cx: &mut AsyncApp) {
        let (updates, received) = async_channel::unbounded();
        let _follow = cx.background_spawn(location::follow(updates));
        while let Ok(update) = received.recv().await {
            let taken = this.update(cx, |view, cx| {
                let (link, state) = update.unzip();
                let location = &mut view.location;
                // A dmail that started again knows no clients: ask anew.
                let ask_again = location.locating && location.link.is_none() && link.is_some();
                location.state = state.unwrap_or_default();
                location.link = link;
                location.read = true;
                if ask_again {
                    view.ask_location(true, cx);
                }
                cx.notify();
            });
            if taken.is_err() {
                return;
            }
        }
    }

    /// Locating stops once the page closes.
    pub(super) fn follow_locating(&mut self, cx: &mut Context<Self>) {
        if self.location.locating && !self.screen.is_page("location") {
            self.set_locating(false, cx);
        }
    }

    fn set_locating(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.location.locating == on {
            return;
        }
        self.location.locating = on;
        self.ask_location(on, cx);
        cx.notify();
    }

    /// Starts or stops dmail's GPS for Settings.
    fn ask_location(&mut self, start: bool, cx: &mut Context<Self>) {
        let Some(link) = self.location.link.clone() else {
            return;
        };
        self.location.asking = true;
        cx.spawn(async move |this, cx| {
            let result = if start {
                link.start().await
            } else {
                link.stop().await
            };
            let _ = this.update(cx, |view, cx| {
                view.location.asking = false;
                if let Err(error) = result {
                    eprintln!("settings: asking dmail for location: {error}");
                    if start {
                        view.location.locating = false;
                        view.note("Could not start the GPS", cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn location_page(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let mut parts = Vec::new();
        let location = &self.location;
        let state = &location.state;
        if location.read {
            if location.link.is_none() || state.gps == Gps::Unavailable {
                let why = if location.link.is_none() {
                    "dmail, the modem service, does not run here"
                } else {
                    "The modem has no location engine, or it stopped"
                };
                parts.push(self.missing(Icon::Location, "No GPS", why));
            } else {
                parts.push(self.gps_island(layer, cx));
                if let Some(fix) = state.fix {
                    parts.push(self.chip_row("Position"));
                    parts.push(self.info_island(self.position_rows(&fix, state)));
                }
            }
        }

        parts.push(self.chip_row("Apps"));
        let description = "Denial asks the first time an app wants your location. Apps that asked are listed here.";
        match self.permission_status() {
            Some(status) => parts.push(island(&self.look).child(self.row(
                "location-apps",
                Some(Icon::Shield),
                status,
                Some(description.into()),
                (true, true),
            ))),
            None => parts.extend(self.permission_answers(layer, cx)),
        }
        parts
    }

    /// Where the GPS is, and the switch that has it locate the phone.
    fn gps_island(&self, layer: Layer, cx: &mut Context<Self>) -> Div {
        let location = &self.location;
        let state = &location.state;
        let satellites = |count: u32| fill("{count} satellites", &[("count", &count.to_string())]);
        let (title, detail): (SharedString, SharedString) = match state.gps {
            Gps::Fixed => (
                "Located".into(),
                match state.fix.and_then(|fix| fix.accuracy) {
                    Some(accuracy) => fill(
                        "Within {distance}, from {satellites}",
                        &[
                            ("distance", &distance(accuracy)),
                            ("satellites", &satellites(state.used)),
                        ],
                    )
                    .into(),
                    None => fill(
                        "From {satellites}",
                        &[("satellites", &satellites(state.used))],
                    )
                    .into(),
                },
            ),
            Gps::Searching if state.tracked > 0 => (
                "Looking for satellites…".into(),
                fill(
                    "Hearing {satellites}",
                    &[("satellites", &satellites(state.tracked))],
                )
                .into(),
            ),
            Gps::Searching => (
                "Looking for satellites…".into(),
                "Outdoors, the first position can take a few minutes".into(),
            ),
            Gps::Off | Gps::Unavailable => (
                "GPS is off".into(),
                "It runs only while an app or this page asks for your location".into(),
            ),
        };
        let status = self.row(
            "gps",
            Some(Icon::Location),
            title,
            Some(detail),
            (true, false),
        );
        let locating = location.locating;
        let switch = self
            .row(
                "locate",
                Some(Icon::Search),
                "Locate this device",
                Some("While this page is open".into()),
                (false, true),
            )
            .child(self.toggle(locating));
        let live = layer.live() && !location.asking;
        island(&self.look)
            .child(status)
            .child(self.separator(SEPARATOR))
            .child(self.live(switch, live, cx, move |this, _, cx| {
                this.set_locating(!locating, cx)
            }))
    }

    fn position_rows(&self, fix: &Fix, state: &location::State) -> Vec<Info> {
        let mut rows = vec![
            Info::Value("Latitude", degrees(fix.latitude, ['N', 'S'])),
            Info::Value("Longitude", degrees(fix.longitude, ['E', 'W'])),
        ];
        if let Some(accuracy) = fix.accuracy {
            rows.push(Info::Value("Accuracy", format!("± {}", distance(accuracy))));
        }
        if let Some(speed) = fix.speed.filter(|speed| *speed >= STILL) {
            let heading = fix
                .heading
                .map_or(String::new(), |heading| format!(" · {}", compass(heading)));
            rows.push(Info::Value(
                "Speed",
                format!("{:.0} km/h{heading}", speed * 3.6),
            ));
        }
        rows.push(Info::Value(
            "Satellites",
            fill(
                "{used} used, {heard} heard",
                &[
                    ("used", &state.used.to_string()),
                    ("heard", &state.tracked.to_string()),
                ],
            ),
        ));
        if fix.time > 0 {
            rows.push(Info::Value("Time of the position", self.when(fix.time)));
        }
        rows
    }

    pub(super) fn location_summary(&self) -> Option<SharedString> {
        let location = &self.location;
        if !location.read || location.link.is_none() {
            return None;
        }
        match location.state.gps {
            Gps::Fixed => Some("In use · located".into()),
            Gps::Searching => Some("In use · looking for satellites".into()),
            Gps::Off | Gps::Unavailable => None,
        }
    }
}

/// "45.46423° N".
fn degrees(value: f64, sides: [char; 2]) -> String {
    let side = if value < 0.0 { sides[1] } else { sides[0] };
    format!("{:.5}° {side}", value.abs())
}

/// "12 m", "1.4 km".
fn distance(metres: f64) -> String {
    if metres < 1000.0 {
        format!("{} m", metres.round().max(1.0))
    } else {
        format!("{:.1} km", metres / 1000.0)
    }
}

/// The nearest of the eight compass points.
fn compass(heading: f64) -> &'static str {
    const POINTS: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    POINTS[((heading.rem_euclid(360.0) + 22.5) / 45.0) as usize % 8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_positions() {
        assert_eq!(degrees(45.464_234, ['N', 'S']), "45.46423° N");
        assert_eq!(degrees(-9.19, ['E', 'W']), "9.19000° W");
        assert_eq!(distance(11.6), "12 m");
        assert_eq!(distance(0.2), "1 m");
        assert_eq!(distance(1450.0), "1.4 km");
        assert_eq!(compass(359.0), "N");
        assert_eq!(compass(84.4), "E");
        assert_eq!(compass(200.0), "S");
    }
}
