//! Shortcut settings use the compositor's native validation and revision checks.

use std::rc::Rc;
use std::time::Duration;

use denial_look::words::fill;
use denial_look_gpui::{island, say};
use gpui::{Context, Div, Focusable, SharedString, Task, Window, prelude::*};
use serde_json::{Value, json};

use super::{Dialog, Layer, Settings};
use crate::denial::{self, Refusal};

/// Where Denial's settings keep the shortcut preset: "standard", or "mac"
/// for Like a Mac.
const PRESET: &str = "shortcuts.preset";

#[derive(Default)]
pub(super) struct Shortcuts {
    pub(super) snapshot: Option<Value>,
    watcher: Option<Task<()>>,
    saving: bool,
}

pub(super) fn list<'a>(snapshot: &'a Value, key: &str) -> &'a [Value] {
    snapshot[key].as_array().map_or(&[], Vec::as_slice)
}

/// The English words naming one of Denial's actions, as the shell names it;
/// `say` gives them in the app's language. Numbered workspace actions come
/// already filled in.
pub(super) fn action_label(action: &str) -> String {
    if let Some(number) = action.strip_prefix("switchWorkspace") {
        return fill("Switch to workspace {workspace}", &[("workspace", number)]);
    }
    if let Some(number) = action.strip_prefix("moveToWorkspace") {
        return fill(
            "Move window to workspace {workspace}",
            &[("workspace", number)],
        );
    }
    let words = match action {
        "shutdown" => "Shut down",
        "openApplications" => "Open applications",
        "openDashboard" => "Open dashboard",
        "openSettings" => "Open Settings",
        "openOverview" => "Open overview",
        "toggleVerticalMaximize" => "Maximize vertically",
        "windowSwitcher" => "Switch windows",
        "openClipboard" => "Open clipboard",
        "captureRegion" => "Capture region",
        "closeWindow" => "Close window",
        "minimizeWindow" => "Minimize window",
        "minimizeAllWindows" => "Minimize all windows",
        "toggleMaximize" => "Maximize or restore",
        "toggleFullscreen" => "Enter or leave fullscreen",
        "toggleWindowAlwaysOnTop" => "Toggle always on top",
        "releasePointer" => "Release pointer",
        // "Lock screen" names the lock screen's page.
        "lockScreen" => "Lock the screen",
        "volumeUp" => "Increase volume",
        "volumeDown" => "Decrease volume",
        "volumeMute" => "Mute or unmute",
        "brightnessUp" => "Increase brightness",
        "brightnessDown" => "Decrease brightness",
        "nextKeyboardLayout" => "Next keyboard layout",
        "previousKeyboardLayout" => "Previous keyboard layout",
        "focusLeft" => "Focus window left",
        "focusRight" => "Focus window right",
        "focusUp" => "Focus window above",
        "focusDown" => "Focus window below",
        "swapLeft" => "Swap window left",
        "swapRight" => "Swap window right",
        "swapUp" => "Swap window upward",
        "swapDown" => "Swap window downward",
        "previousWorkspace" => "Previous workspace",
        "nextWorkspace" => "Next workspace",
        "moveToPreviousWorkspace" => "Move window to previous workspace",
        "moveToNextWorkspace" => "Move window to next workspace",
        "resizeGrowWidth" => "Grow window width",
        "resizeShrinkWidth" => "Shrink window width",
        "resizeGrowHeight" => "Grow window height",
        "resizeShrinkHeight" => "Shrink window height",
        "resetWindowHeight" => "Reset window height",
        "resetWindowWidth" => "Reset window width",
        // An action of a newer Denial: its name, spaced.
        _ => return spaced(action),
    };
    words.to_owned()
}

/// `camelCase` as "Camel Case".
fn spaced(action: &str) -> String {
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
            // The shell's own actions come in its language; Denial's are
            // English for `say`.
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
        // Welcome shows the bindings too.
        if !self.screen.is_page("shortcuts") && self.welcome.is_none() {
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
        // The document follows a change at once; the snapshot once Denial
        // applied it.
        let mac = self
            .denial
            .document
            .as_ref()
            .map(|document| denial::at(document, PRESET).as_str() == Some("mac"))
            .unwrap_or_else(|| snapshot["preset"].as_str() == Some("mac"));
        let preset = self
            .row(
                "shortcut-preset-mac",
                None,
                "Like a Mac",
                Some(say(
                    "Alt works as ⌘ and Super as ⌥: Alt+C copies, Alt+V pastes, Alt+Tab \
                     switches apps and Alt+F searches. Alt+Space still switches the \
                     keyboard layout.",
                )),
                (true, true),
            )
            .child(self.toggle(mac));
        let add = self
            .row("shortcut-add", None, "Add shortcut", None, (true, true))
            .child(self.chevron());
        let mut parts = vec![
            island(&self.look).child(self.live(
                preset,
                live && self.denial.document.is_some(),
                cx,
                move |this, _, cx| {
                    let word = if mac { "standard" } else { "mac" };
                    this.change(
                        "Like a Mac",
                        move |document| denial::put(document, PRESET, json!(word)),
                        cx,
                    );
                },
            )),
            island(&self.look).child(self.live(add, live, cx, |this, window, cx| {
                this.edit_shortcut_chord(None, window, cx);
            })),
        ];
        // The preset's shortcuts that the configured ones leave in effect.
        // Denial edits and deletes them through the configured ones.
        let mut presets: Vec<_> = list(snapshot, "preset_shortcuts")
            .iter()
            .filter_map(|binding| Some((binding["shortcut"].as_str()?, &binding["target"])))
            .collect();
        presets.sort_unstable_by_key(|(key, _)| *key);
        if !presets.is_empty() {
            parts.push(
                self.chip_row(if snapshot["preset"].as_str() == Some("mac") {
                    "Built-in shortcuts: Like a Mac"
                } else {
                    "Built-in shortcuts"
                }),
            );
            parts.push(self.binding_rows("preset", snapshot, presets, live, cx));
        }
        let mut bindings: Vec<_> = list(snapshot, "shortcuts")
            .iter()
            .filter_map(|binding| Some((binding["shortcut"].as_str()?, &binding["target"])))
            .collect();
        bindings.sort_unstable_by_key(|(key, _)| *key);
        if !bindings.is_empty() {
            parts.push(self.chip_row("Configured shortcuts"));
            parts.push(self.binding_rows("shortcut", snapshot, bindings, live, cx));
        }
        parts
    }

    /// An island of shortcuts, each opening its edit and delete choices.
    fn binding_rows(
        &self,
        kind: &'static str,
        snapshot: &Value,
        bindings: Vec<(&str, &Value)>,
        live: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        self.rows_island(bindings.into_iter(), 0., |(chord, target), ends| {
            let row = self
                .row(
                    SharedString::from(format!("{kind}:{chord}")),
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
        })
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
                        say(action_label(action)),
                        json!({"type": "denialAction", "action": action}),
                    ))
                })
                .chain(
                    list(snapshot, "plugin_actions")
                        .iter()
                        .filter_map(|action| {
                            Some((
                                say(action["label"].as_str()?.to_owned()),
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::action_label;

    /// deniald's actions (`ShortcutAction`), but the numbered workspaces.
    const DENIAL_ACTIONS: &[&str] = &[
        "shutdown",
        "openApplications",
        "openDashboard",
        "openSettings",
        "openOverview",
        "toggleVerticalMaximize",
        "windowSwitcher",
        "openClipboard",
        "captureRegion",
        "closeWindow",
        "minimizeWindow",
        "minimizeAllWindows",
        "toggleMaximize",
        "toggleFullscreen",
        "toggleWindowAlwaysOnTop",
        "releasePointer",
        "lockScreen",
        "volumeUp",
        "volumeDown",
        "volumeMute",
        "brightnessUp",
        "brightnessDown",
        "nextKeyboardLayout",
        "previousKeyboardLayout",
        "focusLeft",
        "focusRight",
        "focusUp",
        "focusDown",
        "swapLeft",
        "swapRight",
        "swapUp",
        "swapDown",
        "previousWorkspace",
        "nextWorkspace",
        "moveToPreviousWorkspace",
        "moveToNextWorkspace",
        "resizeGrowWidth",
        "resizeShrinkWidth",
        "resizeGrowHeight",
        "resizeShrinkHeight",
        "resetWindowHeight",
        "resetWindowWidth",
    ];

    /// The labels deniald gives its own actions (`BuiltinAction::label`).
    const DENIALD_LABELS: &[&str] = &[
        "Switch apps",
        "Switch windows of the app",
        "Search",
        "Hide the app",
        "Screenshot of the screen",
        "Windows of the app",
        "Send to the app",
    ];

    #[test]
    fn every_action_reads_in_each_language() {
        let chinese: HashSet<&str> = crate::words::CHINESE
            .iter()
            .map(|(english, _)| *english)
            .collect();
        let russian: HashSet<&str> = crate::words_ru::RUSSIAN
            .iter()
            .map(|(english, _)| *english)
            .collect();
        let labels = DENIAL_ACTIONS
            .iter()
            .map(|action| action_label(action))
            .chain(DENIALD_LABELS.iter().map(|label| (*label).to_owned()))
            .chain(
                [
                    "Switch to workspace {workspace}",
                    "Move window to workspace {workspace}",
                ]
                .map(str::to_owned),
            );
        for label in labels {
            assert!(chinese.contains(label.as_str()), "no Chinese for {label}");
            assert!(russian.contains(label.as_str()), "no Russian for {label}");
        }
        assert!(action_label("switchWorkspace3").contains('3'));
        assert!(action_label("moveToWorkspace7").contains('7'));
        // An action of a newer Denial still reads.
        assert_eq!(action_label("frobnicateWindows"), "Frobnicate Windows");
    }
}
