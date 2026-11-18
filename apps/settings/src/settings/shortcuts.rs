//! Shortcut settings use the compositor's native validation and revision checks.

use std::rc::Rc;
use std::time::Duration;

use denial_look_gpui::{island, say};
use gpui::{Context, Div, Focusable, SharedString, Task, Window, prelude::*};
use serde_json::{Value, json};

use super::{Dialog, Layer, Settings};
use crate::denial::Refusal;

#[derive(Default)]
pub(super) struct Shortcuts {
    snapshot: Option<Value>,
    watcher: Option<Task<()>>,
    saving: bool,
}

fn list<'a>(snapshot: &'a Value, key: &str) -> &'a [Value] {
    snapshot[key].as_array().map_or(&[], Vec::as_slice)
}

fn action_label(action: &str) -> String {
    let mut label = String::new();
    for ch in action.chars() {
        if label.is_empty() {
            label.extend(ch.to_uppercase());
        } else {
            if ch.is_ascii_uppercase() {
                label.push(' ');
            }
            label.push(ch);
        }
    }
    label
}

fn target_label(snapshot: &Value, target: &Value) -> String {
    match target["type"].as_str() {
        Some("denialAction") => action_label(target["action"].as_str().unwrap_or("")),
        Some("pluginAction") => {
            let id = target["id"].as_str().unwrap_or("");
            list(snapshot, "plugin_actions")
                .iter()
                .find(|descriptor| descriptor["id"] == id)
                .and_then(|descriptor| descriptor["label"].as_str())
                .unwrap_or(id)
                .to_owned()
        }
        Some("spawnSh") => target["command"].as_str().unwrap_or("").to_owned(),
        Some("spawn") => target["desktopFileId"]
            .as_str()
            .or_else(|| target["command"][0].as_str())
            .unwrap_or("")
            .to_owned(),
        _ => String::new(),
    }
}

impl Settings {
    pub(super) fn follow_shortcuts(&mut self, cx: &mut Context<Self>) {
        if !self.screen.is_page("shortcuts") {
            self.shortcuts.watcher = None;
            return;
        }
        if self.shortcuts.watcher.is_some() {
            return;
        }
        let link = self.denial.link.clone();
        self.shortcuts.watcher = Some(cx.spawn(async move |this, cx| {
            loop {
                let snapshot = link
                    .shortcut_request("settings.shortcuts.get", None)
                    .await
                    .ok();
                if this
                    .update(cx, |view, cx| {
                        if !view.shortcuts.saving && view.shortcuts.snapshot != snapshot {
                            view.shortcuts.snapshot = snapshot;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    return;
                }
                async_io::Timer::after(Duration::from_secs(2)).await;
            }
        }));
    }

    pub(super) fn shortcuts_page(&self, layer: Layer, cx: &mut Context<Self>) -> Vec<Div> {
        let Some(snapshot) = &self.shortcuts.snapshot else {
            return vec![self.notice_row("Waiting for shortcuts")];
        };
        let live = layer.live() && !self.shortcuts.saving;
        let add = self
            .row("shortcut-add", None, "Add shortcut", None, (true, true))
            .child(self.chevron());
        let mut parts =
            vec![
                island(&self.look).child(self.live(add, live, cx, |this, window, cx| {
                    this.edit_shortcut_chord(None, window, cx);
                })),
            ];
        let mut bindings: Vec<_> = list(snapshot, "shortcuts")
            .iter()
            .filter_map(|binding| Some((binding["shortcut"].as_str()?, &binding["target"])))
            .collect();
        bindings.sort_unstable_by_key(|(key, _)| *key);
        if !bindings.is_empty() {
            parts.push(self.chip_row("Configured shortcuts"));
            parts.push(
                self.rows_island(bindings.into_iter(), 0., |(chord, target), ends| {
                    let row = self
                        .row(
                            SharedString::from(format!("shortcut:{chord}")),
                            None,
                            chord.to_owned(),
                            Some(target_label(snapshot, target).into()),
                            ends,
                        )
                        .child(self.chevron());
                    let chord = chord.to_owned();
                    let target = target.clone();
                    self.live(row, live, cx, move |this, _, cx| {
                        this.shortcut_actions(chord.clone(), target.clone(), cx)
                    })
                }),
            );
        }
        parts
    }

    fn shortcut_actions(&mut self, chord: String, target: Value, cx: &mut Context<Self>) {
        self.show(
            Dialog::Pick {
                title: chord.clone().into(),
                options: vec!["Edit shortcut".into(), "Delete shortcut".into()],
                picked: 2,
                pick: Rc::new(move |this, index, window, cx| {
                    if index == 0 {
                        this.edit_shortcut_chord(Some((chord.clone(), target.clone())), window, cx);
                    } else if index == 1 {
                        let chord = chord.clone();
                        this.show(
                            Dialog::Confirm {
                                title: "Delete shortcut".into(),
                                consequence: chord.clone().into(),
                                action: "Delete",
                                confirm: Rc::new(move |this, _, cx| {
                                    this.save_shortcut(Some(chord.clone()), None, cx)
                                }),
                            },
                            cx,
                        );
                    }
                }),
            },
            cx,
        );
    }

    fn edit_shortcut_chord(
        &mut self,
        existing: Option<(String, Value)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let initial = existing
            .as_ref()
            .map_or_else(String::new, |(chord, _)| chord.clone());
        self.output_field
            .update(cx, |field, cx| field.set_text(initial, cx));
        self.show(
            Dialog::TextEdit {
                title: "Shortcut chord",
                detail: "Enter a chord such as Super+Shift+F.",
                valid: Rc::new(|text| !text.trim().is_empty()),
                save: Rc::new(move |this, chord, cx| {
                    this.choose_shortcut_target(chord.trim().to_owned(), existing.clone(), cx)
                }),
            },
            cx,
        );
        window.focus(&self.output_field.focus_handle(cx), cx);
    }

    fn choose_shortcut_target(
        &mut self,
        chord: String,
        existing: Option<(String, Value)>,
        cx: &mut Context<Self>,
    ) {
        let mut options: Vec<SharedString> = [
            "Built-in or plugin action",
            "Installed application",
            "Executable and arguments",
            "Shell command",
        ]
        .map(say)
        .to_vec();
        if existing.is_some() {
            options.push(say("Keep current target"));
        }
        self.show(
            Dialog::Pick {
                title: "Shortcut target".into(),
                picked: options.len(),
                options,
                pick: Rc::new(move |this, index, window, cx| {
                    let current = existing.as_ref().map(|(key, _)| key.clone());
                    match index {
                        0 | 1 => {
                            this.shortcut_catalog_target(chord.clone(), current, index == 1, cx)
                        }
                        2 | 3 => this.shortcut_command(
                            chord.clone(),
                            existing.clone(),
                            index == 3,
                            window,
                            cx,
                        ),
                        4 => {
                            if let Some((key, target)) = &existing {
                                this.save_shortcut(
                                    Some(key.clone()),
                                    Some(json!({"shortcut": chord, "target": target})),
                                    cx,
                                );
                            }
                        }
                        _ => {}
                    }
                }),
            },
            cx,
        );
    }

    fn shortcut_catalog_target(
        &mut self,
        chord: String,
        existing: Option<String>,
        apps: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(snapshot) = &self.shortcuts.snapshot else {
            return;
        };
        let mut targets: Vec<(SharedString, Value)> = if apps {
            self.installed.apps.iter().filter_map(|app| {
                let path = app.desktop_file.to_str()?;
                Some((app.name.clone().into(), json!({"type": "spawn", "command": ["gio", "launch", path], "desktopFileId": app.id})))
            }).collect()
        } else {
            list(snapshot, "supported_actions")
                .iter()
                .filter_map(|action| {
                    let action = action.as_str()?;
                    Some((
                        action_label(action).into(),
                        json!({"type": "denialAction", "action": action}),
                    ))
                })
                .chain(
                    list(snapshot, "plugin_actions")
                        .iter()
                        .filter_map(|action| {
                            Some((
                                action["label"].as_str()?.to_owned().into(),
                                json!({"type": "pluginAction", "id": action["id"].as_str()?}),
                            ))
                        }),
                )
                .collect()
        };
        if targets.is_empty() {
            self.note("No shortcut targets available", cx);
            return;
        }
        targets.sort_by_cached_key(|(label, _)| label.to_lowercase());
        self.show(
            Dialog::Pick {
                title: if apps {
                    "Installed application".into()
                } else {
                    "Action".into()
                },
                options: targets.iter().map(|(label, _)| label.clone()).collect(),
                picked: targets.len(),
                pick: Rc::new(move |this, index, _, cx| {
                    if let Some((_, target)) = targets.get(index) {
                        this.save_shortcut(
                            existing.clone(),
                            Some(json!({"shortcut": chord, "target": target})),
                            cx,
                        );
                    }
                }),
            },
            cx,
        );
    }

    fn shortcut_command(
        &mut self,
        chord: String,
        existing: Option<(String, Value)>,
        shell: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kind = if shell { "spawnSh" } else { "spawn" };
        let initial = existing
            .as_ref()
            .filter(|(_, target)| target["type"] == kind)
            .map_or_else(String::new, |(_, target)| {
                if shell {
                    target["command"].as_str().unwrap_or("").to_owned()
                } else {
                    target["command"].to_string()
                }
            });
        self.output_field
            .update(cx, |field, cx| field.set_text(initial, cx));
        self.show(Dialog::TextEdit {
            title: if shell { "Shell command" } else { "Executable and arguments" },
            detail: if shell { "This command runs in a shell when you press the shortcut." }
                else { "Enter a JSON list, for example [\"program\", \"argument\"]." },
            valid: Rc::new(move |text| if shell { !text.trim().is_empty() } else { command_arguments(text).is_some() }),
            save: Rc::new(move |this, text, cx| {
                let command = if shell { json!(text) } else if let Some(args) = command_arguments(&text) {
                    json!(args)
                } else { return; };
                this.save_shortcut(existing.as_ref().map(|(key, _)| key.clone()),
                    Some(json!({"shortcut": chord, "target": {"type": kind, "command": command}})), cx);
            }),
        }, cx);
        window.focus(&self.output_field.focus_handle(cx), cx);
    }

    fn save_shortcut(
        &mut self,
        existing: Option<String>,
        binding: Option<Value>,
        cx: &mut Context<Self>,
    ) {
        if self.shortcuts.saving {
            return;
        }
        let Some(revision) = self
            .shortcuts
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot["revision"].as_u64())
        else {
            return;
        };
        self.shortcuts.saving = true;
        cx.notify();
        let link = self.denial.link.clone();
        cx.spawn(async move |this, cx| {
            let result = async {
                if let Some(binding) = binding {
                    let mut params = json!({"shortcut": binding});
                    if let Some(existing) = &existing {
                        params["existing_shortcut"] = json!(existing);
                    }
                    let validation = link
                        .shortcut_request("settings.shortcuts.validate", Some(params.clone()))
                        .await?;
                    match validation["kind"].as_str() {
                        Some("valid") => {}
                        Some("conflict") => {
                            return Err(Refusal(format!(
                                "Shortcut already assigned: {}",
                                validation["canonical"].as_str().unwrap_or_default()
                            )));
                        }
                        _ => {
                            return Err(Refusal(
                                validation["error"]
                                    .as_str()
                                    .unwrap_or("Invalid shortcut")
                                    .to_owned(),
                            ));
                        }
                    }
                    if let Some(canonical) = validation["canonical"].as_str() {
                        params["shortcut"]["shortcut"] = json!(canonical);
                    }
                    params["expected_revision"] = json!(revision);
                    let method = if existing.is_some() {
                        "settings.shortcuts.update"
                    } else {
                        "settings.shortcuts.add"
                    };
                    link.shortcut_request(method, Some(params)).await
                } else {
                    link.shortcut_request(
                        "settings.shortcuts.remove",
                        Some(json!({
                            "expected_revision": revision, "shortcut": existing
                        })),
                    )
                    .await
                }
            }
            .await;
            let _ = this.update(cx, |view, cx| {
                view.shortcuts.saving = false;
                match result {
                    Ok(snapshot) => view.shortcuts.snapshot = Some(snapshot),
                    Err(Refusal(error)) => view.note(error, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn command_arguments(text: &str) -> Option<Vec<String>> {
    let args: Vec<String> = serde_json::from_str(text).ok()?;
    (args.first().is_some_and(|arg| !arg.trim().is_empty())
        && args.iter().all(|arg| !arg.contains('\0')))
    .then_some(args)
}
