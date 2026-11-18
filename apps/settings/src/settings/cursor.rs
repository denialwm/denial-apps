use std::rc::Rc;

use denial_look_gpui::say;
use gpui::{Context, PathPromptOptions, prelude::*};
use serde_json::{Value, json};

use super::{Dialog, Settings};
use crate::{cursor_theme, denial};

const THEME: &str = "appearance.cursorThemeId";

impl Settings {
    pub(super) fn current_cursor(&self) -> &str {
        self.kept(THEME)
            .and_then(Value::as_str)
            .unwrap_or(cursor_theme::DEFAULT)
    }

    pub(super) fn choose_cursor(&mut self, remove: bool, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let mut themes = cx
                .background_spawn(async { cursor_theme::discovered() })
                .await;
            if remove {
                themes.retain(|theme| theme.id != cursor_theme::DEFAULT);
            }
            let _ = this.update(cx, |view, cx| {
                if !view.screen.is_page("style") || view.dialog.is_some() {
                    return;
                }
                if themes.is_empty() {
                    view.note("No imported cursor themes", cx);
                    return;
                }
                view.show(
                    Dialog::Pick {
                        title: if remove {
                            "Remove imported cursor theme"
                        } else {
                            "Cursor theme"
                        }
                        .into(),
                        options: themes
                            .iter()
                            .map(|theme| theme.label.clone().into())
                            .collect(),
                        picked: if remove {
                            themes.len()
                        } else {
                            themes
                                .iter()
                                .position(|theme| theme.id == view.current_cursor())
                                .unwrap_or(themes.len())
                        },
                        pick: Rc::new(move |this, index, _, cx| {
                            if let Some(theme) = themes.get(index) {
                                let id = theme.id.clone();
                                if remove {
                                    this.show(
                                        Dialog::Confirm {
                                            title: "Remove imported cursor theme".into(),
                                            consequence: theme.label.clone().into(),
                                            action: "Remove",
                                            confirm: Rc::new(move |this, _, cx| {
                                                this.remove_cursor(id.clone(), cx)
                                            }),
                                        },
                                        cx,
                                    );
                                } else {
                                    this.select_cursor(id, cx);
                                }
                            }
                        }),
                    },
                    cx,
                );
            });
        })
        .detach();
    }

    fn select_cursor(&mut self, id: String, cx: &mut Context<Self>) {
        self.change(
            "Cursor theme",
            move |document| denial::put(document, THEME, json!(id)),
            cx,
        );
    }

    pub(super) fn import_cursor(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(say("Import cursor theme ZIP")),
        });
        cx.spawn(async move |this, cx| {
            let path = match prompt.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) => return,
                _ => {
                    let _ = this.update(cx, |view, cx| {
                        view.note("Could not open the file picker", cx)
                    });
                    return;
                }
            };
            let Some(path) = path else {
                return;
            };
            let result = cx
                .background_spawn(async move { cursor_theme::import(&path) })
                .await;
            let _ = this.update(cx, |view, cx| match result {
                Ok(theme) => view.select_cursor(theme.id, cx),
                Err(error) => view.note(error.to_string(), cx),
            });
        })
        .detach();
    }

    fn remove_cursor(&mut self, id: String, cx: &mut Context<Self>) {
        let link = self.denial.link.clone();
        cx.spawn(async move |this, cx| {
            let reset = link
                .edit(|document| {
                    if denial::at(document, THEME) == &json!(id) {
                        denial::put(document, THEME, json!(cursor_theme::DEFAULT));
                    }
                })
                .await;
            if let Err(error) = reset {
                let _ = this.update(cx, |view, cx| view.note(error.0, cx));
                return;
            }
            let result = cx
                .background_spawn(async move { cursor_theme::remove(&id) })
                .await;
            let _ = this.update(cx, |view, cx| match result {
                Ok(()) => view.note("Cursor theme removed", cx),
                Err(error) => view.note(error.to_string(), cx),
            });
        })
        .detach();
    }
}
