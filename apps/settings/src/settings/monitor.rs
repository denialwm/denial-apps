//! The Monitor page: a monitor plugged in beside the phone shows a desktop,
//! set apart from the phone here. Its scale and refresh rate go to deniald
//! as the panel's rate does, kept only once the user says so; its windows, roundness and
//! wallpaper are Denial's settings and the shell's wallpaper file. The page
//! is listed only while a monitor is plugged in, which Settings asks
//! deniald every few seconds.

use std::rc::Rc;
use std::time::Duration;

use denial_look::words::fill;
use denial_look_gpui::{Icon, island};
use gpui::{AsyncWindowContext, Context, Div, SharedString, WeakEntity, prelude::*};

use super::panel::hertz;
use super::{Dialog, Layer, Screen, Settings, scales};
use crate::catalog::{self, Section};
use crate::denial;
use crate::wallpaper;

/// How often Settings looks for a monitor.
const LOOK_AGAIN: Duration = Duration::from_secs(2);

impl Settings {
    /// Looks for a monitor beside the phone for as long as the window
    /// lives. When it goes, its screens close.
    pub(super) async fn follow_monitor(this: WeakEntity<Self>, cx: &mut AsyncWindowContext) {
        loop {
            let Ok(link) = this.update(cx, |view, _| view.denial.link.clone()) else {
                return;
            };
            let monitor = link.monitor().await.ok().flatten();
            let taken = this.update_in(cx, |view, window, cx| {
                if view.monitor == monitor {
                    return;
                }
                catalog::set_monitor(monitor.is_some());
                view.monitor = monitor;
                let on_monitor =
                    view.screen.is_page("monitor") || view.screen == Screen::MonitorWallpapers;
                if view.monitor.is_none() && on_monitor {
                    view.open_at(Screen::Home, true, window, cx);
                }
                cx.notify();
            });
            if taken.is_err() {
                return;
            }
            cx.background_executor().timer(LOOK_AGAIN).await;
        }
    }

    pub(super) fn reread_monitor(&mut self, cx: &mut Context<Self>) {
        let link = self.denial.link.clone();
        cx.spawn(async move |this, cx| {
            let monitor = link.monitor().await.ok().flatten();
            let _ = this.update(cx, |view, cx| {
                catalog::set_monitor(monitor.is_some());
                view.monitor = monitor;
                cx.notify();
            });
        })
        .detach();
    }

    /// The home row's line: the monitor's size, scale and refresh rate.
    pub(super) fn monitor_summary(&self) -> Option<SharedString> {
        let monitor = self.monitor.as_ref()?;
        Some(
            format!(
                "{} × {} · {} · {}",
                monitor.size.0,
                monitor.size.1,
                percent(monitor.scale),
                hertz(monitor.rate)
            )
            .into(),
        )
    }

    pub(super) fn read_monitor_wallpaper(&mut self) {
        let output = self.monitor.as_ref().map(|monitor| monitor.name.as_str());
        self.wallpapers.monitor = output.and_then(|name| wallpaper::current(Some(name)));
    }

    /// The Monitor page's islands: the scale and refresh rate, then its
    /// sections.
    pub(super) fn monitor_page(
        &self,
        sections: &'static [Section],
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let Some(monitor) = &self.monitor else {
            return Vec::new();
        };
        let scale = self
            .row(
                "monitor.scale",
                Some(Icon::Computer),
                "Scale",
                Some(scales::summary(monitor.pixels, monitor.scale)),
                (true, monitor.rates.len() <= 1),
            )
            .child(self.value(percent(monitor.scale)))
            .child(self.chevron());
        let shown = monitor.clone();
        let mut screen = island(&self.look).child(self.live(
            scale,
            layer.live(),
            cx,
            move |this, _, cx| {
                this.pick_scale(
                    shown.name.clone(),
                    shown.pixels,
                    shown.scale,
                    shown.default_scale,
                    cx,
                )
            },
        ));
        if monitor.rates.len() > 1 {
            let rate = self
                .row("monitor.rate", None, "Refresh rate", None, (false, true))
                .child(self.value(hertz(monitor.rate)))
                .child(self.chevron());
            screen = screen.child(self.separator(0.)).child(self.live(
                rate,
                layer.live(),
                cx,
                |this, _, cx| this.choose_monitor_rate(cx),
            ));
        }
        let mut parts = vec![screen];
        parts.extend(self.page(sections, layer, cx));
        parts
    }
}

impl Settings {
    fn choose_monitor_rate(&mut self, cx: &mut Context<Self>) {
        let Some(monitor) = self.monitor.clone() else {
            return;
        };
        let picked = monitor
            .rates
            .iter()
            .position(|&rate| rate == monitor.rate)
            .unwrap_or(0);
        let rates = monitor.rates.clone();
        self.show(
            Dialog::Pick {
                title: "Refresh rate".into(),
                options: rates.iter().map(|&rate| hertz(rate)).collect(),
                picked,
                pick: Rc::new(move |this, option, _, cx| {
                    this.try_monitor_rate(&monitor, rates[option], cx)
                }),
            },
            cx,
        );
    }

    /// Moves the monitor to `rate` and asks whether to keep it, as the
    /// panel's rate does.
    fn try_monitor_rate(&mut self, monitor: &denial::Monitor, rate: u32, cx: &mut Context<Self>) {
        if rate == monitor.rate {
            return;
        }
        let (link, name, back) = (self.denial.link.clone(), monitor.name.clone(), monitor.rate);
        cx.spawn(async move |this, cx| {
            let tried = link.try_rate(&name, rate).await;
            let _ = this.update(cx, |view, cx| match tried {
                Ok(token) => {
                    view.reread_monitor(cx);
                    view.ask_keep(token, hertz(rate), hertz(back), cx)
                }
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
}

/// A scale as people say it: "150%".
pub(super) fn percent(scale: f64) -> SharedString {
    format!("{:.0}%", scale * 100.0).into()
}

pub(super) fn same(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.005
}
