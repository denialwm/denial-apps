//! Denial's desktop idle policy and power-button preferences.

use std::rc::Rc;

use gpui::{AppContext, Context, SharedString};
use serde_json::{Value, json};

use super::panel::screen_off;
use super::{Dialog, Settings};
use crate::catalog::Item;
use crate::denial;

const MIN: u32 = 30;
const MAX: u32 = 7200;
const LOCK: (&str, u32) = ("power.idleLockTimeoutSeconds", 960);
const DISPLAY: (&str, u32) = ("power.idleDpmsTimeoutSeconds", 900);
const SUSPEND: (&str, u32) = ("power.idleSuspendTimeoutSeconds", 1800);
const TIMERS: &[u32] = &[
    30, 60, 120, 180, 300, 600, 900, 960, 1200, 1800, 2700, 3600, 5400, 7200,
];
const MODES: &[(&str, &str)] = &[
    ("systemDefault", "System default"),
    ("s2idle", "Suspend to idle"),
    ("shallow", "Standby"),
    ("deep", "Suspend to RAM"),
];
const PRESS: &[(&str, &str)] = &[
    ("dpms", "Turn off display"),
    ("suspend", "Suspend"),
    ("hibernate", "Hibernate"),
    ("powerOff", "Power off"),
];
const HOLD: &[(&str, &str)] = &[
    ("nothing", "Do nothing"),
    ("powerMenu", "Power menu"),
    ("suspend", "Suspend"),
    ("hibernate", "Hibernate"),
    ("powerOff", "Power off"),
];

fn timer_for(key: &str) -> Option<(&'static str, u32)> {
    match key {
        "power.lockTimeout" => Some(LOCK),
        "power.displayTimeout" => Some(DISPLAY),
        "power.suspendTimeout" => Some(SUSPEND),
        _ => None,
    }
}

fn choice_for(
    key: &str,
) -> Option<(
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
)> {
    match key {
        "power.suspendMode" => Some(("power.suspendMode", "deep", MODES)),
        "power.press" => Some(("power.powerButtonAction", "powerOff", PRESS)),
        "power.hold" => Some(("power.powerButtonLongPressAction", "nothing", HOLD)),
        _ => None,
    }
}

fn seconds(document: &Value, (path, fallback): (&str, u32)) -> u32 {
    let configured = denial::at(document, path)
        .as_u64()
        .map_or(fallback, |seconds| {
            seconds.clamp(u64::from(MIN), u64::from(MAX)) as u32
        });
    match path {
        path if path == SUSPEND.0 => configured.max(seconds(document, DISPLAY)),
        path if path == LOCK.0 => configured.min(seconds(document, SUSPEND)),
        _ => configured,
    }
}

async fn can_hibernate() -> bool {
    let Ok(bus) = zbus::Connection::system().await else {
        return false;
    };
    let Ok(reply) = bus
        .call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "CanHibernate",
            &(),
        )
        .await
    else {
        return false;
    };
    matches!(reply.body().deserialize::<&str>(), Ok("yes" | "challenge"))
}

impl Settings {
    pub(super) fn idle_power_value(&self, key: &str) -> Option<SharedString> {
        if let Some(timer) = timer_for(key) {
            let value = self
                .denial
                .document
                .as_ref()
                .map_or(timer.1, |doc| seconds(doc, timer));
            return Some(screen_off(Some(value)));
        }
        let (path, default, choices) = choice_for(key)?;
        let value = self.kept(path).and_then(Value::as_str).unwrap_or(default);
        choices
            .iter()
            .find(|(word, _)| *word == value)
            .map(|(_, label)| (*label).into())
    }

    pub(super) fn choose_idle_power(&mut self, item: &'static Item, cx: &mut Context<Self>) {
        let Some(document) = self.denial.document.as_ref() else {
            return self.note("Denial does not answer", cx);
        };
        if let Some(timer) = timer_for(item.key) {
            let current = seconds(document, timer);
            let suspend = seconds(document, SUSPEND);
            let (low, high) = if timer == SUSPEND {
                (seconds(document, DISPLAY), MAX)
            } else {
                (MIN, suspend)
            };
            let mut choices: Vec<u32> = TIMERS
                .iter()
                .copied()
                .filter(|&seconds| (low..=high).contains(&seconds))
                .collect();
            choices.extend([low, high, current.clamp(low, high)]);
            choices.sort_unstable();
            choices.dedup();
            let picked = choices
                .iter()
                .position(|&value| value == current)
                .unwrap_or(choices.len());
            self.show(
                Dialog::Pick {
                    title: item.title.into(),
                    options: choices
                        .iter()
                        .map(|&value| screen_off(Some(value)))
                        .collect(),
                    picked,
                    pick: Rc::new(move |this, index, _, cx| {
                        let value = choices[index];
                        this.change(
                            item.title,
                            move |document| {
                                let suspend = seconds(document, SUSPEND);
                                let value = if timer == SUSPEND {
                                    value.max(seconds(document, DISPLAY))
                                } else {
                                    value.min(suspend)
                                };
                                if timer == SUSPEND && seconds(document, LOCK) > value {
                                    denial::put(document, LOCK.0, json!(value));
                                }
                                denial::put(document, timer.0, json!(value));
                            },
                            cx,
                        );
                    }),
                },
                cx,
            );
            return;
        }
        let Some((path, default, choices)) = choice_for(item.key) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let available: Vec<_> = if choices == MODES {
                let modes = cx
                    .background_spawn(async {
                        std::fs::read_to_string("/sys/power/mem_sleep").unwrap_or_default()
                    })
                    .await;
                choices
                    .iter()
                    .copied()
                    .filter(|(word, _)| {
                        *word == "systemDefault"
                            || modes
                                .split_whitespace()
                                .any(|mode| mode.trim_matches(['[', ']']) == *word)
                    })
                    .collect()
            } else {
                let hibernate = can_hibernate().await;
                choices
                    .iter()
                    .copied()
                    .filter(|(word, _)| *word != "hibernate" || hibernate)
                    .collect()
            };
            let _ = this.update(cx, |view, cx| {
                if !view.screen.is_page("power") || view.dialog.is_some() {
                    return;
                }
                let current = view.kept(path).and_then(Value::as_str).unwrap_or(default);
                let picked = available
                    .iter()
                    .position(|(word, _)| *word == current)
                    .unwrap_or(available.len());
                view.show(
                    Dialog::Pick {
                        title: item.title.into(),
                        options: available.iter().map(|(_, label)| (*label).into()).collect(),
                        picked,
                        pick: Rc::new(move |this, index, _, cx| {
                            let value = available[index].0;
                            this.change(
                                item.title,
                                move |document| denial::put(document, path, json!(value)),
                                cx,
                            );
                        }),
                    },
                    cx,
                );
            });
        })
        .detach();
    }

    pub(super) fn reset_idle_power(&mut self, cx: &mut Context<Self>) {
        self.change(
            "Power preferences",
            |document| {
                for (path, default) in [LOCK, DISPLAY, SUSPEND] {
                    denial::put(document, path, json!(default));
                }
                for (path, default) in [
                    ("power.idleLockEnabled", true),
                    ("power.idleDpmsEnabled", true),
                    ("power.idleSuspendEnabled", false),
                ] {
                    denial::put(document, path, json!(default));
                }
                for (path, default) in [
                    ("power.powerButtonAction", "powerOff"),
                    ("power.powerButtonLongPressAction", "nothing"),
                    ("power.suspendMode", "deep"),
                ] {
                    denial::put(document, path, json!(default));
                }
            },
            cx,
        );
    }
}
