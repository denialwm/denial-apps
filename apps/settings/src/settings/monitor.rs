//! The Monitor page: a monitor plugged in beside the phone shows a desktop,
//! set apart from the phone here. Its scale goes to deniald as the refresh
//! rate does, kept only once the user says so; its windows, roundness and
//! wallpaper are Denial's settings and the shell's wallpaper file. The page
//! is listed only while a monitor is plugged in, which Settings asks
//! deniald every few seconds.

use std::rc::Rc;
use std::time::Duration;

use denial_look::words::{fill, tr};
use denial_look_gpui::{Icon, island, solid};
use gpui::{AsyncWindowContext, Context, Div, SharedString, WeakEntity, div, prelude::*};

use super::android::{DEFAULT_DPI, SIZE_SCALES, SIZES};
use super::{Dialog, Layer, Screen, Settings};
use crate::catalog::{self, Section};
use crate::denial;
use crate::droidloom;
use crate::wallpaper;

/// How often Settings looks for a monitor.
const LOOK_AGAIN: Duration = Duration::from_secs(2);

/// The scales offered; one set otherwise joins them.
const SCALES: [f64; 7] = [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];

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

    /// The home row's line: the monitor's size and scale.
    pub(super) fn monitor_summary(&self) -> Option<SharedString> {
        let monitor = self.monitor.as_ref()?;
        Some(
            format!(
                "{} × {} · {}",
                monitor.size.0,
                monitor.size.1,
                percent(monitor.scale)
            )
            .into(),
        )
    }

    pub(super) fn read_monitor_wallpaper(&mut self) {
        let output = self.monitor.as_ref().map(|monitor| monitor.name.as_str());
        self.wallpapers.monitor = output.and_then(|name| wallpaper::current(Some(name)));
    }

    /// The Monitor page's islands: the scale, then its sections.
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
                Some("Larger text and windows on the monitor".into()),
                (true, true),
            )
            .child(self.value(percent(monitor.scale)))
            .child(self.chevron());
        let mut parts =
            vec![
                island(&self.look)
                    .child(self.live(scale, layer.live(), cx, |this, _, cx| this.choose_scale(cx))),
            ];
        if droidloom::installed() {
            parts.push(self.android_size_island(monitor, layer, cx));
        }
        parts.extend(self.page(sections, layer, cx));
        parts
    }

    /// How large Android apps are on the monitor, apart from the phone.
    fn android_size_island(
        &self,
        monitor: &denial::Monitor,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Div {
        let per_scale = self
            .densities
            .outputs
            .get(&monitor.name)
            .copied()
            .or(self.densities.every);
        let picked = SIZE_SCALES
            .iter()
            .position(|share| per_scale.is_some_and(|per| (DEFAULT_DPI * share - per).abs() < 0.5))
            .unwrap_or(SIZES.len());
        let dpi = per_scale.map(|per| {
            let dpi = (f64::from(per) * monitor.scale).round();
            fill("{dpi} dpi", &[("dpi", &dpi.to_string())]).into()
        });
        island(&self.look).child(
            self.options_row(
                "monitor.android",
                "Android apps",
                dpi,
                &SIZES,
                picked,
                layer.live(),
                (true, true),
                cx,
                Rc::new(|this, option, _, cx| this.pick_monitor_android(option, cx)),
            )
            .child(
                div()
                    .text_color(solid(self.look.palette.secondary))
                    .child(tr("Open Android apps may restart to take the new size")),
            ),
        )
    }

    /// Draws Android apps on the monitor at the size picked from `SIZES`.
    fn pick_monitor_android(&mut self, option: usize, cx: &mut Context<Self>) {
        let (Some(monitor), Some(share)) = (self.monitor.clone(), SIZE_SCALES.get(option)) else {
            return;
        };
        let per_scale = DEFAULT_DPI * share;
        self.densities
            .outputs
            .insert(monitor.name.clone(), per_scale);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let set = cx
                .background_spawn(async move {
                    droidloom::set_densities(Some(&monitor.name), Some(per_scale))
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if let Err(error) = set {
                    view.densities = droidloom::densities();
                    view.note(
                        fill(
                            "Could not change the size: {why}",
                            &[("why", &error.to_string())],
                        ),
                        cx,
                    );
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads the densities by screen. Until one is set for every screen,
    /// it is worked out from Android's density on the phone, so the
    /// monitor starts with apps the size they are there.
    pub(super) fn read_densities(&mut self, cx: &mut Context<Self>) {
        self.densities = droidloom::densities();
        let Some(panel_scale) = self.monitor.as_ref().map(|monitor| monitor.panel_scale) else {
            return;
        };
        if self.densities.every.is_some() || !droidloom::installed() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let seeded = cx
                .background_spawn(async move {
                    let density = droidloom::density().ok()?;
                    let per_scale = (f64::from(density.dpi) / panel_scale) as f32;
                    droidloom::set_densities(None, Some(per_scale)).ok()?;
                    Some(droidloom::densities())
                })
                .await;
            if let Some(densities) = seeded {
                let _ = this.update(cx, |view, cx| {
                    view.densities = densities;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn choose_scale(&mut self, cx: &mut Context<Self>) {
        let Some(monitor) = self.monitor.clone() else {
            return;
        };
        let mut scales = SCALES.to_vec();
        if !scales.iter().any(|&scale| same(scale, monitor.scale)) {
            scales.push(monitor.scale);
            scales.sort_by(f64::total_cmp);
        }
        let picked = scales
            .iter()
            .position(|&scale| same(scale, monitor.scale))
            .unwrap_or(0);
        self.show(
            Dialog::Pick {
                title: "Scale".into(),
                options: scales.iter().map(|&scale| percent(scale)).collect(),
                picked,
                pick: Rc::new(move |this, option, _, cx| {
                    this.try_scale(&monitor, scales[option], cx)
                }),
            },
            cx,
        );
    }

    /// Scales the monitor to `scale` and asks whether to keep it; Denial
    /// goes back by itself if nobody answers.
    fn try_scale(&mut self, monitor: &denial::Monitor, scale: f64, cx: &mut Context<Self>) {
        if same(scale, monitor.scale) {
            return;
        }
        let (link, name, back) = (
            self.denial.link.clone(),
            monitor.name.clone(),
            monitor.scale,
        );
        cx.spawn(async move |this, cx| {
            let tried = link.try_scale(&name, scale).await;
            let _ = this.update(cx, |view, cx| match tried {
                Ok(token) => {
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

/// A scale as people say it: "150%".
fn percent(scale: f64) -> SharedString {
    format!("{:.0}%", scale * 100.0).into()
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.005
}
