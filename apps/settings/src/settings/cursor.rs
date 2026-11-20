//! The cursor themes as cards: each one's name and its normal, link, text,
//! working and busy cursors as they move, the chosen one ringed in the
//! accent. A tap chooses a theme; an imported one can be removed from its
//! card.

use std::rc::Rc;
use std::time::{Duration, Instant};

use denial_look_gpui::{Icon, color, control_fill, icon, say, solid};
use gpui::{
    Context, Div, FontWeight, ObjectFit, PathPromptOptions, SharedString, Stateful, div, img,
    prelude::*, px,
};
use serde_json::{Value, json};

use super::parts::ROW_PAD;
use super::{Dialog, Settings};
use crate::cursor_theme::{self, PreviewFrame, Theme};
use crate::denial;

const THEME: &str = "appearance.cursorThemeId";
/// The disclosure the cards are drawn under; their cursors move only while
/// it is open.
const SHOWN_UNDER: &str = "Windows and pointer";

const CARD_WIDTH: f32 = 236.0;
const CARD_GAP: f32 = 10.0;
const CARD_PAD: f32 = 12.0;
/// Each cursor's box on a card, and the longest edge it is drawn at.
const PREVIEW_BOX: f32 = 34.0;
const PREVIEW: f32 = 28.0;
const REMOVE: f32 = 28.0;
/// The shortest wait between two frames, so a theme with very short frames
/// cannot keep Settings drawing without rest.
const FASTEST_FRAME: Duration = Duration::from_millis(16);

#[derive(Default)]
pub(super) struct Cursors {
    themes: Vec<Theme>,
    /// Every card's animation starts here, so the cards move together.
    clock: Option<Instant>,
    /// A timer is redrawing the cards at each frame change.
    ticking: bool,
}

/// The frame `frames` shows `elapsed` microseconds into its loop, and how
/// long until the next one; no wait for a single frame.
fn frame_at(frames: &[PreviewFrame], elapsed: u64) -> (usize, Option<u64>) {
    let total: u64 = frames.iter().map(|frame| frame.duration).sum();
    if frames.len() < 2 || total == 0 {
        return (0, None);
    }
    let mut into = elapsed % total;
    for (index, frame) in frames.iter().enumerate() {
        if into < frame.duration {
            return (index, Some(frame.duration - into));
        }
        into -= frame.duration;
    }
    (0, Some(frames[0].duration))
}

impl Settings {
    pub(super) fn current_cursor(&self) -> &str {
        self.kept(THEME)
            .and_then(Value::as_str)
            .unwrap_or(cursor_theme::DEFAULT)
    }

    /// Reads the bundled and imported themes for the cards.
    pub(super) fn read_cursors(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let themes = cx
                .background_spawn(async { cursor_theme::discovered() })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.cursors.themes = themes;
                view.cursors.clock.get_or_insert_with(Instant::now);
                view.follow_cursors(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn cursors_shown(&self) -> bool {
        self.screen.is_page("style") && self.expanded_sections.contains(SHOWN_UNDER)
    }

    fn cursor_elapsed(&self) -> u64 {
        self.cursors
            .clock
            .map_or(0, |clock| clock.elapsed().as_micros() as u64)
    }

    /// How long until any card's cursor changes frame; none when nothing
    /// moves.
    fn next_cursor_frame(&self) -> Option<Duration> {
        let elapsed = self.cursor_elapsed();
        self.cursors
            .themes
            .iter()
            .flat_map(|theme| &theme.previews)
            .filter_map(|frames| frame_at(frames, elapsed).1)
            .min()
            .map(|wait| Duration::from_micros(wait).max(FASTEST_FRAME))
    }

    /// Redraws the cards at each frame change while they can be seen, and
    /// stops once they cannot.
    pub(super) fn follow_cursors(&mut self, cx: &mut Context<Self>) {
        if self.cursors.ticking || !self.cursors_shown() || self.next_cursor_frame().is_none() {
            return;
        }
        self.cursors.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                let wait = this.update(cx, |view, cx| {
                    let wait = view
                        .cursors_shown()
                        .then(|| view.next_cursor_frame())
                        .flatten();
                    if wait.is_none() {
                        view.cursors.ticking = false;
                    }
                    cx.notify();
                    wait
                });
                let Ok(Some(wait)) = wait else {
                    return;
                };
                cx.background_executor().timer(wait).await;
            }
        })
        .detach();
    }

    /// The cards, in the row `key` stands for.
    pub(super) fn cursor_cards(
        &self,
        key: &'static str,
        (first, last): (bool, bool),
        live: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let radius = self.row_radius();
        let current = self.current_cursor();
        let elapsed = self.cursor_elapsed();
        let cards = self.cursors.themes.iter().map(|theme| {
            let picked = theme.id == current;
            self.cursor_card(theme, picked, elapsed, live, cx)
        });
        div()
            .id(key)
            .p(px(ROW_PAD))
            .when(first, |row| row.rounded_t(radius))
            .when(last, |row| row.rounded_b(radius))
            .flex()
            .flex_wrap()
            .gap(px(CARD_GAP))
            .children(cards)
    }

    fn cursor_card(
        &self,
        theme: &Theme,
        picked: bool,
        elapsed: u64,
        live: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let look = &self.look;
        let fill = control_fill(look);
        let radius = px((look.group_radius() - CARD_PAD).max(8.0));
        let previews = theme.previews.iter().map(|frames| {
            let frame = &frames[frame_at(frames, elapsed).0];
            div()
                .flex_none()
                .size(px(PREVIEW_BOX))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    img(frame.path.clone())
                        .size(px(PREVIEW))
                        .object_fit(ObjectFit::Contain),
                )
        });
        let remove = (theme.imported() && live).then(|| {
            let id = theme.id.clone();
            let label: SharedString = theme.label.clone().into();
            let button = div()
                .id(SharedString::from(format!("remove-{}", theme.id)))
                .flex_none()
                .size(px(REMOVE))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(REMOVE / 2.0))
                .child(icon(Icon::Delete, px(18.), solid(look.palette.secondary)));
            self.hovered(self.tappable(button, cx, move |this, _, cx| {
                let id = id.clone();
                this.show(
                    Dialog::Confirm {
                        title: "Remove imported cursor theme".into(),
                        consequence: label.clone(),
                        action: "Remove",
                        confirm: Rc::new(move |this, _, cx| this.remove_cursor(id.clone(), cx)),
                    },
                    cx,
                );
            }))
        });
        let card = div()
            .id(SharedString::from(format!("cursor-{}", theme.id)))
            .flex_none()
            .w(px(CARD_WIDTH))
            .p(px(CARD_PAD))
            .flex()
            .flex_col()
            .gap(px(10.))
            .rounded(radius)
            .border(px(if picked { 2.0 } else { 1.0 }))
            .border_color(if picked {
                solid(look.accent)
            } else {
                color(look.palette.separator, look.palette.separator_alpha)
            })
            .bg(if picked {
                color(look.accent, 0.14)
            } else {
                fill
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .min_h(px(REMOVE))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(solid(look.palette.foreground))
                            .child(SharedString::from(theme.label.clone())),
                    )
                    .children(remove),
            )
            .child(div().flex().justify_between().children(previews));
        if live && !picked {
            let id = theme.id.clone();
            self.hovered(self.tappable(card, cx, move |this, _, cx| {
                this.select_cursor(id.clone(), cx)
            }))
        } else {
            card
        }
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
                Ok(theme) => {
                    view.select_cursor(theme.id, cx);
                    view.read_cursors(cx);
                }
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
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(()) => view.note("Cursor theme removed", cx),
                    Err(error) => view.note(error.to_string(), cx),
                }
                view.read_cursors(cx);
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(durations: &[u64]) -> Vec<PreviewFrame> {
        durations
            .iter()
            .map(|&duration| PreviewFrame {
                path: "x.png".into(),
                duration,
            })
            .collect()
    }

    #[test]
    fn frames_follow_their_durations_and_loop() {
        let frames = frames(&[100, 50, 200]);
        assert_eq!(frame_at(&frames, 0), (0, Some(100)));
        assert_eq!(frame_at(&frames, 120), (1, Some(30)));
        assert_eq!(frame_at(&frames, 150), (2, Some(200)));
        assert_eq!(frame_at(&frames, 350 + 10), (0, Some(90)));
        assert_eq!(frame_at(&self::frames(&[100]), 5_000), (0, None));
    }
}
