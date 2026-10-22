//! The Display page: the panel's brightness, the dark or light appearance,
//! when the idle screen turns off, and the refresh rate. What the user
//! does goes to deniald through `denial::Link`; the appearance and timeout
//! come back through Denial's settings, the panel by reading it again.

use denial_look::words::fill;
use std::rc::Rc;

use denial_look_gpui::{Icon, island};
use gpui::{Context, Div, SharedString, Window, prelude::*};

use super::{Dialog, Layer, Settings};
use crate::denial::{self, Output};

/// The slider's key while the finger holds it.
const BRIGHTNESS: &str = "display.brightness";
/// Seconds the idle screen may stay on; None for always.
const SCREEN_OFF: [Option<u32>; 7] = [
    Some(30),
    Some(60),
    Some(2 * 60),
    Some(5 * 60),
    Some(10 * 60),
    Some(30 * 60),
    None,
];

impl Settings {
    /// The Display page's islands, top to bottom: brightness, appearance,
    /// then the screen's timeout and refresh rate.
    pub(super) fn display_page(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let display = &self.denial;
        if !display.read {
            return Vec::new();
        }
        let Some(prefs) = display.prefs() else {
            return vec![self.missing(
                Icon::Display,
                "Display settings are not available",
                "Denial, which runs the screen, does not answer",
            )];
        };
        let live = layer.live();
        let mut parts = Vec::new();
        if let Some(level) = display.brightness {
            parts.push(island(&self.look).child(self.level_row(
                BRIGHTNESS,
                Some(Icon::Display),
                "Brightness",
                None,
                level,
                live,
                (true, true),
                cx,
                Rc::new(|this, level, _, cx| this.set_brightness(level, cx)),
            )));
        }

        parts.push(self.chip_row("Appearance"));
        parts.push(island(&self.look).child(self.options_row(
            "display.scheme",
            "Appearance",
            None,
            &["Dark", "Light"],
            usize::from(!prefs.dark),
            live,
            (true, true),
            cx,
            Rc::new(|this, option, _, cx| this.set_dark(option == 0, cx)),
        )));

        parts.push(self.chip_row("Screen"));
        let timeout = self
            .row(
                "display.timeout",
                None,
                "Screen timeout",
                Some("When nothing touches it".into()),
                (true, display.output.is_none()),
            )
            .child(self.value(screen_off(prefs.screen_off)))
            .child(self.chevron());
        let mut screen = island(&self.look)
            .child(self.live(timeout, live, cx, |this, _, cx| this.choose_screen_off(cx)));
        if let Some(output) = display
            .output
            .as_ref()
            .filter(|output| output.rates.len() > 1)
        {
            let rate = self
                .row("display.rate", None, "Refresh rate", None, (false, true))
                .child(self.value(hertz(output.rate)))
                .child(self.chevron());
            screen =
                screen
                    .child(self.separator(0.))
                    .child(self.live(rate, live, cx, |this, _, cx| this.choose_rate(cx)));
        }
        parts.push(screen);
        parts
    }

    /// Reads the panel and its brightness again: on the Display page, every
    /// second, so the shade's slider shows here too.
    pub(super) async fn read_panel(link: denial::Link) -> Option<(Output, Option<f32>)> {
        let output = link.output().await.ok().flatten()?;
        Some((output, denial::backlight()))
    }

    pub(super) fn take_panel(
        &mut self,
        panel: Option<(Output, Option<f32>)>,
        cx: &mut Context<Self>,
    ) {
        let display = &mut self.denial;
        let (output, level) = panel.unzip();
        display.output = output;
        // The finger's level stands until it lets go.
        if self.sliding != Some(BRIGHTNESS) {
            display.brightness = level.flatten();
        }
        cx.notify();
    }

    /// Moves the slider to `level` at once; the panel follows, one request at
    /// a time, ending on the last level asked.
    fn set_brightness(&mut self, level: f32, cx: &mut Context<Self>) {
        let level = level.clamp(0.0, 1.0);
        self.denial.brightness = Some(level);
        self.denial.wanted = Some((level * 100.0).round() as u8);
        cx.notify();
        if self.denial.sending {
            return;
        }
        let (Some(output), link) = (self.denial.output.clone(), self.denial.link.clone()) else {
            return;
        };
        self.denial.sending = true;
        cx.spawn(async move |this, cx| {
            let mut sent = None;
            loop {
                let wanted = this.update(cx, |view, _| view.denial.wanted).ok().flatten();
                if wanted.is_none() || wanted == sent {
                    let _ = this.update(cx, |view, _| view.denial.sending = false);
                    return;
                }
                let percent = wanted.unwrap_or_default();
                if let Err(denial::Refusal(said)) = link.set_brightness(&output, percent).await {
                    eprintln!("settings: setting the brightness: {said}");
                }
                sent = wanted;
            }
        })
        .detach();
    }

    /// Switches Denial to the dark or light appearance; every app follows,
    /// this one too.
    fn set_dark(&mut self, dark: bool, cx: &mut Context<Self>) {
        self.change(
            "Appearance",
            move |document| denial::set_dark(document, dark),
            cx,
        );
    }

    fn choose_screen_off(&mut self, cx: &mut Context<Self>) {
        let Some(prefs) = self.denial.prefs() else {
            return;
        };
        let mut options = SCREEN_OFF.to_vec();
        if !options.contains(&prefs.screen_off) {
            options.push(prefs.screen_off);
            options.sort_by_key(|seconds| seconds.unwrap_or(u32::MAX));
        }
        let picked = options
            .iter()
            .position(|&seconds| seconds == prefs.screen_off)
            .unwrap_or(0);
        self.show(
            Dialog::Pick {
                title: "Screen timeout".into(),
                options: options.iter().map(|&seconds| screen_off(seconds)).collect(),
                picked,
                pick: Rc::new(move |this, option, _, cx| this.set_screen_off(options[option], cx)),
            },
            cx,
        );
    }

    fn set_screen_off(&mut self, seconds: Option<u32>, cx: &mut Context<Self>) {
        self.change(
            "Screen timeout",
            move |document| denial::set_screen_off(document, seconds),
            cx,
        );
    }

    fn choose_rate(&mut self, cx: &mut Context<Self>) {
        let Some(output) = self.denial.output.clone() else {
            return;
        };
        let picked = output
            .rates
            .iter()
            .position(|&rate| rate == output.rate)
            .unwrap_or(0);
        let rates = output.rates.clone();
        self.show(
            Dialog::Pick {
                title: "Refresh rate".into(),
                options: rates.iter().map(|&rate| hertz(rate)).collect(),
                picked,
                pick: Rc::new(move |this, option, _, cx| this.try_rate(&output, rates[option], cx)),
            },
            cx,
        );
    }

    /// Moves the panel to `rate` and asks whether to keep it. Denial goes
    /// back by itself if nobody answers, so a rate the panel cannot show
    /// does not leave the screen dark.
    fn try_rate(&mut self, output: &Output, rate: u32, cx: &mut Context<Self>) {
        if rate == output.rate {
            return;
        }
        let (link, output, back) = (self.denial.link.clone(), output.clone(), output.rate);
        cx.spawn(async move |this, cx| {
            let tried = link.try_rate(&output, rate).await;
            let _ = this.update(cx, |view, cx| match tried {
                Ok(token) => view.ask_keep(token, rate, back, cx),
                Err(denial::Refusal(said)) => view.note(
                    fill(
                        "Could not change the refresh rate: {why}",
                        &[("why", &said)],
                    ),
                    cx,
                ),
            });
        })
        .detach();
    }

    fn ask_keep(&mut self, token: u64, rate: u32, back: u32, cx: &mut Context<Self>) {
        self.show(Dialog::Keep { token, rate, back }, cx);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(denial::KEEP).await;
            // Denial has gone back on its own; the question is moot.
            let _ = this.update(cx, |view, cx| {
                if matches!(view.dialog, Some((Dialog::Keep { token: asked, .. }, _)) if asked == token)
                {
                    view.dialog = None;
                    view.reread_panel(cx);
                }
            });
        })
        .detach();
    }

    /// Answers the refresh-rate question: keep the new rate, or go back.
    pub(super) fn answer_keep(
        &mut self,
        token: u64,
        keep: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dialog = None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
        let link = self.denial.link.clone();
        cx.spawn(async move |this, cx| {
            let answered = if keep {
                link.keep(token).await
            } else {
                link.undo(token).await
            };
            let _ = this.update(cx, |view, cx| {
                if let Err(denial::Refusal(said)) = answered {
                    view.note(
                        fill("Could not answer Denial: {why}", &[("why", &said)]),
                        cx,
                    );
                }
                view.reread_panel(cx);
            });
        })
        .detach();
    }

    fn reread_panel(&mut self, cx: &mut Context<Self>) {
        let link = self.denial.link.clone();
        cx.spawn(async move |this, cx| {
            let panel = Self::read_panel(link).await;
            let _ = this.update(cx, |view, cx| view.take_panel(panel, cx));
        })
        .detach();
    }
}

/// A refresh rate as people say it: "120 Hz".
pub(super) fn hertz(millihertz: u32) -> SharedString {
    format!("{} Hz", (millihertz + 500) / 1000).into()
}

fn screen_off(seconds: Option<u32>) -> SharedString {
    match seconds {
        None => "Never".into(),
        Some(60) => "1 minute".into(),
        Some(seconds) if seconds % 60 == 0 => {
            fill("{count} minutes", &[("count", &(seconds / 60).to_string())]).into()
        }
        Some(seconds) => fill("{count} seconds", &[("count", &seconds.to_string())]).into(),
    }
}
