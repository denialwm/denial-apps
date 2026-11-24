//! A display's scale, the one size setting: Denial draws the shell and every
//! app at it, Wayland, X11 and Android alike. Like macOS's "looks like" and
//! Android's display size, each step offered says the screen it makes, in
//! logical pixels, and the one Denial picks by itself is marked. The panel's
//! Display page and the Monitor page offer the steps of their screen and try
//! the one picked, kept only once the user says so.

use std::rc::Rc;

use denial_look::words::fill;
use gpui::{Context, SharedString};

use super::monitor::{percent, same};
use super::{Dialog, Settings};
use crate::denial;

/// The steps are quarters, which Wayland's 120ths hold exactly.
const STEP: f64 = 0.25;
const LOWEST: f64 = 1.0;
const HIGHEST: f64 = 4.0;
/// The logical short side a step may make: from a small tablet's (an iPad
/// mini is 744 points across) to a large desktop's.
const SHORTEST: f64 = 720.0;
const LONGEST: f64 = 1440.0;

/// One scale a display offers.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Step {
    pub scale: f64,
    /// The screen it makes, in logical pixels, wide by high.
    pub looks_like: (u32, u32),
    /// The scale Denial gives the display while none is set.
    pub default: bool,
}

impl Step {
    /// "250% · looks like 1357 × 960", and "· default" on the default.
    pub fn label(&self) -> SharedString {
        let (width, height) = self.looks_like;
        let words = if self.default {
            "{percent} · looks like {width} × {height} · default"
        } else {
            "{percent} · looks like {width} × {height}"
        };
        fill(
            words,
            &[
                ("percent", &percent(self.scale)),
                ("width", &width.to_string()),
                ("height", &height.to_string()),
            ],
        )
        .into()
    }
}

/// The screen `pixels` makes at `scale`, rounded as Denial rounds it.
pub(super) fn looks_like(pixels: (u32, u32), scale: f64) -> (u32, u32) {
    let side = |pixels: u32| (f64::from(pixels) / scale).round().max(1.0) as u32;
    (side(pixels.0), side(pixels.1))
}

/// The scales a display of `pixels` offers: the quarter steps whose screen
/// is neither smaller than a small tablet's nor larger than a large
/// desktop's, with the default and the current scale always among them.
pub(super) fn steps(pixels: (u32, u32), current: f64, default: Option<f64>) -> Vec<Step> {
    let short = f64::from(pixels.0.min(pixels.1));
    let mut scales: Vec<f64> = (0..)
        .map(|step| LOWEST + f64::from(step) * STEP)
        .take_while(|&scale| scale <= HIGHEST)
        .filter(|&scale| (SHORTEST..=LONGEST).contains(&(short / scale)))
        .collect();
    if scales.is_empty() {
        scales.push(LOWEST);
    }
    for extra in default.into_iter().chain([current]) {
        if !scales.iter().any(|&scale| same(scale, extra)) {
            scales.push(extra);
        }
    }
    scales.sort_by(f64::total_cmp);
    scales
        .into_iter()
        .map(|scale| Step {
            scale,
            looks_like: looks_like(pixels, scale),
            default: default.is_some_and(|default| same(default, scale)),
        })
        .collect()
}

/// The scale row's line under its title: what the screen looks like now.
pub(super) fn summary(pixels: (u32, u32), scale: f64) -> SharedString {
    let (width, height) = looks_like(pixels, scale);
    fill(
        "Looks like {width} × {height}",
        &[("width", &width.to_string()), ("height", &height.to_string())],
    )
    .into()
}

impl Settings {
    /// Offers the steps of the screen `name`, of `pixels` at `scale`, and
    /// tries the one picked.
    pub(super) fn pick_scale(
        &mut self,
        name: String,
        pixels: (u32, u32),
        scale: f64,
        default: Option<f64>,
        cx: &mut Context<Self>,
    ) {
        let steps = steps(pixels, scale, default);
        let picked = steps
            .iter()
            .position(|step| same(step.scale, scale))
            .unwrap_or(0);
        self.show(
            Dialog::Pick {
                title: "Scale".into(),
                options: steps.iter().map(Step::label).collect(),
                picked,
                pick: Rc::new(move |this, option, _, cx| {
                    this.try_scale(name.clone(), scale, steps[option].scale, cx)
                }),
            },
            cx,
        );
    }

    /// Scales the screen `name` from `back` to `scale` and asks whether to
    /// keep it; Denial goes back by itself if nobody answers.
    fn try_scale(&mut self, name: String, back: f64, scale: f64, cx: &mut Context<Self>) {
        if same(scale, back) {
            return;
        }
        let link = self.denial.link.clone();
        cx.spawn(async move |this, cx| {
            let tried = link.try_scale(&name, scale).await;
            let _ = this.update(cx, |view, cx| match tried {
                Ok(token) => {
                    view.reread_panel(cx);
                    view.reread_monitor(cx);
                    view.ask_keep(token, percent(scale), percent(back), cx)
                }
                Err(denial::Refusal(said)) => view.note(
                    fill("Could not change the scale: {why}", &[("why", &said)]),
                    cx,
                ),
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pad_3_offers_tablet_to_desktop_sizes_around_its_default() {
        let offered: Vec<(f64, (u32, u32), bool)> = steps((3392, 2400), 2.0, Some(2.5))
            .iter()
            .map(|step| (step.scale, step.looks_like, step.default))
            .collect();
        assert_eq!(
            offered,
            [
                (1.75, (1938, 1371), false),
                (2.0, (1696, 1200), false),
                (2.25, (1508, 1067), false),
                (2.5, (1357, 960), true),
                (2.75, (1233, 873), false),
                (3.0, (1131, 800), false),
                (3.25, (1044, 738), false),
            ]
        );
    }

    #[test]
    fn monitors_offer_what_their_resolution_makes_sensible() {
        let scales = |pixels, default| -> Vec<f64> {
            steps(pixels, 1.0, default)
                .iter()
                .map(|step| step.scale)
                .collect()
        };
        assert_eq!(scales((1920, 1080), Some(1.0)), [1.0, 1.25, 1.5]);
        assert_eq!(
            scales((3840, 2160), Some(1.0)),
            [1.0, 1.5, 1.75, 2.0, 2.25, 2.5, 2.75, 3.0]
        );
        // A small panel still offers its own size.
        assert_eq!(scales((1024, 600), None), [1.0]);
    }

    #[test]
    fn a_scale_set_by_hand_stays_offered() {
        let steps = steps((3392, 2400), 1.6, Some(2.5));
        let custom = steps.iter().find(|step| same(step.scale, 1.6)).unwrap();
        assert_eq!(custom.looks_like, (2120, 1500));
        assert!(!custom.default);
        assert_eq!(steps.first().unwrap().scale, 1.6);
    }

    #[test]
    fn a_turned_panel_looks_like_its_turned_size() {
        let turned = denial::turned((3392, 2400), "270");
        assert_eq!(looks_like(turned, 2.5), (960, 1357));
        assert_eq!(summary(turned, 2.0).as_ref(), "Looks like 1200 × 1696");
    }

    #[test]
    fn labels_say_the_size_and_mark_the_default() {
        let steps = steps((3392, 2400), 2.0, Some(2.5));
        assert_eq!(steps[1].label().as_ref(), "200% · looks like 1696 × 1200");
        assert_eq!(
            steps[3].label().as_ref(),
            "250% · looks like 1357 × 960 · default"
        );
    }
}
