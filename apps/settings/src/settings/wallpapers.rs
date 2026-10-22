//! The wallpaper screen: Denial's own wallpaper and the pictures in
//! `~/Pictures/Wallpapers`, three to a row in the screen's shape, the one
//! shown ringed in the accent. A tap shows it on every screen at once.

use denial_look::INSET;
use denial_look_gpui::{dp, island, solid};
use gpui::{Context, Div, ObjectFit, StyledImage, div, img, prelude::*, px};

use super::{Layer, Settings};
use crate::wallpaper::{self, Choice};

/// Pictures to a row, and the space between them, in dp.
const COLUMNS: usize = 3;
const GAP: f32 = 8.0;
const PAD: f32 = 10.0;

impl Settings {
    /// The pictures in one island; `width` is the window's.
    pub(super) fn wallpapers_screen(
        &self,
        width: f32,
        layer: Layer,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let shown = &self.wallpapers;
        if shown.choices.is_empty() {
            return Vec::new();
        }
        let inner = width - 2.0 * INSET - 2.0 * f32::from(dp(PAD));
        let tile_width = (inner - (COLUMNS - 1) as f32 * f32::from(dp(GAP))) / COLUMNS as f32;
        let tile_height = tile_width * 585.0 / 270.0;
        let radius = px((self.look.group_radius() - f32::from(dp(PAD))).max(6.0));
        let rows = shown.choices.chunks(COLUMNS).map(|row| {
            div().flex().gap(dp(GAP)).children(row.iter().map(|choice| {
                let picked = shown.current.as_deref() == Some(choice.resource.as_str());
                let picture = match shown.thumbs.get(&choice.resource) {
                    Some(thumb) => img(thumb.clone())
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .into_any_element(),
                    None => div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(solid(self.look.palette.secondary))
                        .child(choice.name.clone())
                        .into_any_element(),
                };
                let tile = div()
                    .id(gpui::SharedString::from(choice.resource.clone()))
                    .flex_none()
                    .w(px(tile_width))
                    .h(px(tile_height))
                    .rounded(radius)
                    .overflow_hidden()
                    .bg(denial_look_gpui::control_fill(&self.look))
                    .when(picked, |tile| {
                        tile.border(dp(3.)).border_color(solid(self.look.accent))
                    })
                    .child(picture);
                if layer.live() && !picked {
                    let choice = choice.clone();
                    self.tappable(tile, cx, move |this, _, cx| this.set_wallpaper(&choice, cx))
                } else {
                    tile
                }
            }))
        });
        vec![
            island(&self.look)
                .p(dp(PAD))
                .flex()
                .flex_col()
                .gap(dp(GAP))
                .children(rows),
        ]
    }

    /// Lists the pictures, then makes their thumbnails one by one, showing
    /// each as it comes.
    pub(super) fn read_wallpapers(&mut self, cx: &mut Context<Self>) {
        self.wallpapers.current = wallpaper::current();
        if self.wallpapers.reading.is_some() {
            return;
        }
        self.wallpapers.reading = Some(cx.spawn(async move |this, cx| {
            let choices = cx.background_spawn(async { wallpaper::choices() }).await;
            let wanted: Vec<Choice> = choices
                .iter()
                .filter(|choice| choice.file.is_some())
                .cloned()
                .collect();
            if this
                .update(cx, |view, cx| {
                    view.wallpapers.choices = choices;
                    cx.notify();
                })
                .is_err()
            {
                return;
            }
            for choice in wanted {
                let Some(file) = choice.file.clone() else {
                    continue;
                };
                let thumb = cx
                    .background_spawn(async move { wallpaper::thumbnail(&file) })
                    .await;
                let taken = this.update(cx, |view, cx| {
                    if let Some(thumb) = thumb {
                        view.wallpapers.thumbs.insert(choice.resource, thumb);
                        cx.notify();
                    }
                });
                if taken.is_err() {
                    return;
                }
            }
            let _ = this.update(cx, |view, _| view.wallpapers.reading = None);
        }));
    }

    /// Shows `choice` on every screen, and gives the lock screen its depth.
    fn set_wallpaper(&mut self, choice: &Choice, cx: &mut Context<Self>) {
        let choice = choice.clone();
        // The lock screen draws the home wallpaper, with depth, unless the
        // user told it otherwise.
        let depth = self.denial.document.as_ref().is_none_or(|document| {
            crate::denial::at(document, "lockScreen.useSystemWallpaper") != false
        }) && wallpaper::depth_installed();
        self.wallpapers.current = Some(choice.resource.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let set = cx
                .background_spawn(async move {
                    wallpaper::set(&choice)?;
                    if let (true, Some(file)) = (depth, choice.file.as_ref()) {
                        wallpaper::make_depth(file);
                    }
                    Ok::<_, std::io::Error>(())
                })
                .await;
            let _ = this.update(cx, |view, cx| match set {
                Ok(()) => view.note("Wallpaper set", cx),
                Err(error) => {
                    view.wallpapers.current = wallpaper::current();
                    view.note(format!("Could not set the wallpaper: {error}"), cx);
                }
            });
        })
        .detach();
    }
}
