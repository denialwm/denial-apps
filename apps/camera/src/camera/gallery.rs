//! The gallery: the photos one by one, swiped sideways, with their date,
//! details and Delete; and all of them as a grid by day. Photos are media,
//! bare on black; the bars and panels are smoke over the backdrop blur, so
//! a photo zoomed or scrolled under them shows through.
//!
//! A double tap zooms a photo in around the tap, and out again. A zoomed
//! photo claims the touch as a free drag (GPUI's touch drag, which still
//! reports taps), so it pans on both axes and keeps going after a fling.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use denial_look::mobile::{GESTURE_INSET, GROUP_HEIGHT};
use denial_look::{INSET, MIN_RADIUS, Look, Tween, decelerate, floating, lerp, motion, on, palette};
use denial_look_gpui::{Icon, color, dp, icon, island, shadow, solid};
use gpui::{
    ClickEvent, Context, DispatchPhase, Div, FontWeight, HitboxBehavior, IntoElement, ObjectFit, Pixels, Point,
    ScrollWheelEvent, Stateful, Task, TouchDragEvent, TouchPhase, Window, canvas, div, img, prelude::*, px,
};

use super::{Camera, Screen};
use crate::library::{self, Info};

/// Space between photos while they are swiped, lp.
const GAP: f32 = 16.0;
/// A swipe past this part of the width turns the page.
const TURN: f32 = 0.18;
/// Past the first and the last photo the page follows the finger this much.
const EDGE_GIVE: f32 = 0.35;
/// The bottom bar's height, dp.
const BOTTOM_BAR: f32 = 56.0;
/// Between grid tiles, lp.
const TILE_GAP: f32 = 2.0;
const TILES_A_ROW: usize = 3;
/// The confirmation dims what is under it to this.
const DIM: f32 = 0.6;
/// A double tap zooms to this, around where it lands.
const ZOOM_IN: f32 = 2.5;
/// A single tap waits this long for a second one before it hides the bars.
const DOUBLE_TAP: Duration = Duration::from_millis(300);
/// A zoomed photo flung keeps going this many seconds' worth of its speed:
/// `decelerate` starts at four times its mean speed, so over a slide the
/// photo leaves the finger at the finger's speed.
const FLING: f32 = motion::SLIDE as f32 / 1000.0 / 4.0;
/// A finger still this long before it lifts flings nothing.
const STILL: Duration = Duration::from_millis(60);

const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November",
    "December",
];

/// How the shown photo is zoomed: its scale over fitting the window, and
/// how far its centre is from the window's, lp.
#[derive(Clone, Copy)]
struct View {
    scale: f32,
    x: f32,
    y: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            scale: 1.0,
            x: 0.0,
            y: 0.0,
        }
    }
}

impl View {
    fn zoomed(&self) -> bool {
        self.scale > 1.0
    }

    fn toward(self, to: View, t: f32) -> View {
        View {
            scale: lerp(self.scale, to.scale, t),
            x: lerp(self.x, to.x, t),
            y: lerp(self.y, to.y, t),
        }
    }

    /// Kept so the photo covers the window where it is larger than it.
    fn held(self, fitted: (f32, f32), window: (f32, f32)) -> View {
        let room = |fitted: f32, window: f32| ((fitted * self.scale - window) / 2.0).max(0.0);
        let (x_room, y_room) = (room(fitted.0, window.0), room(fitted.1, window.1));
        View {
            x: self.x.clamp(-x_room, x_room),
            y: self.y.clamp(-y_room, y_room),
            ..self
        }
    }
}

/// A finger panning a zoomed photo.
struct Pan {
    last: (f32, f32),
    at: Instant,
    /// lp a second.
    velocity: (f32, f32),
}

#[derive(Default)]
pub(super) struct Viewer {
    pub(super) index: usize,
    from_grid: bool,
    /// The bars are hidden; a tap on the photo brings them back.
    bare: bool,
    bars: Option<Tween>,
    info: Option<Info>,
    confirm: bool,
    dragging: bool,
    /// How far the photos are moved sideways, lp.
    drag: f32,
    /// Coming back to rest from this offset.
    settle: Option<(f32, Tween)>,
    /// Where the zoom is going, from where, and when it gets there.
    view: View,
    view_from: View,
    view_tween: Option<Tween>,
    pan: Option<Pan>,
    /// A single tap waiting to see whether a second one follows.
    _single_tap: Option<Task<()>>,
}

impl Viewer {
    pub(super) fn animating(&self, now: Instant) -> bool {
        self.bars.is_some_and(|bars| bars.running(now))
            || self.settle.is_some_and(|(_, settle)| settle.running(now))
            || self.view_tween.is_some_and(|tween| tween.running(now))
    }

    fn view_at(&self, now: Instant) -> View {
        match self.view_tween {
            Some(tween) => self.view_from.toward(self.view, decelerate(tween.progress(now))),
            None => self.view,
        }
    }

    /// Moves the zoom to `to` over the slide's time.
    fn animate_view(&mut self, to: View, look: &Look) {
        self.view_from = self.view_at(Instant::now());
        self.view = to;
        self.view_tween = Some(look.tween(motion::SLIDE));
    }

    fn offset(&self, now: Instant) -> f32 {
        if self.dragging {
            return self.drag;
        }
        self.settle
            .map_or(0.0, |(from, settle)| from * (1.0 - decelerate(settle.progress(now))))
    }

    /// How much the bars show.
    fn bars_shown(&self, now: Instant) -> f32 {
        let t = self.bars.map_or(1.0, |bars| bars.progress(now));
        if self.bare { 1.0 - t } else { t }
    }
}

/// The gallery's bars and panels: the glass tint at the island floor over
/// the backdrop blur, with the floating edge and shadow. It takes the
/// touches over the photo under it.
fn smoke(look: &Look) -> Div {
    div()
        .occlude()
        .backdrop_blur(px(floating::BACKDROP_BLUR))
        .bg(color(look.palette.glass, look.palette.island_floor))
        .rounded(px(look.group_radius()))
        .border(px(floating::EDGE_WIDTH))
        .border_color(color(look.palette.foreground, look.palette.edge_alpha))
        .shadow(shadow(look))
}

impl Camera {
    pub(super) fn open_viewer(&mut self, index: usize, from_grid: bool, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.photos.len() {
            return;
        }
        self.viewer = Viewer {
            index,
            from_grid,
            ..Viewer::default()
        };
        self.go(Screen::Viewer, cx);
        self.keep_screens(window);
    }

    pub(super) fn open_grid(&mut self, cx: &mut Context<Self>) {
        for path in self.photos.clone() {
            self.ask_thumbnail(path);
        }
        self.go(Screen::Grid, cx);
    }

    /// Back in the viewer: the innermost thing first.
    pub(super) fn viewer_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.viewer.confirm {
            self.viewer.confirm = false;
        } else if self.viewer.info.is_some() {
            self.viewer.info = None;
        } else {
            self.leave_viewer(window, cx);
            return;
        }
        cx.notify();
    }

    fn leave_viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let to = if self.viewer.from_grid { Screen::Grid } else { Screen::Camera };
        self.go(to, cx);
        self.keep_screens(window);
    }

    /// The screen-sized photos the viewer wants: the one shown and its
    /// neighbours.
    pub(super) fn wanted_screens(&self) -> Vec<PathBuf> {
        if self.screen != Screen::Viewer {
            return Vec::new();
        }
        let index = self.viewer.index;
        (index.saturating_sub(1)..=index + 1)
            .filter_map(|at| self.photos.get(at).cloned())
            .collect()
    }

    /// Lets go of the screen-sized photos no longer wanted and asks for the
    /// wanted ones, the shown one first.
    fn keep_screens(&mut self, window: &mut Window) {
        let mut wanted = self.wanted_screens();
        let gone: Vec<PathBuf> = self.screens.keys().filter(|path| !wanted.contains(path)).cloned().collect();
        for path in gone {
            if let Some(image) = self.screens.remove(&path) {
                let _ = window.drop_image(image);
            }
            self.asked_screens.remove(&path);
        }
        if let Some(shown) = self.photos.get(self.viewer.index) {
            wanted.sort_by_key(|path| path != shown);
        }
        for path in wanted {
            if self.asked_screens.insert(path.clone()) {
                let _ = self.library.try_send(library::Request::Screen(path));
            }
        }
        self.keep_full(window);
    }

    /// The photo the viewer wants at full size: the shown one, while zoomed.
    pub(super) fn wanted_full(&self) -> Option<PathBuf> {
        (self.screen == Screen::Viewer && self.viewer.view.zoomed())
            .then(|| self.photos.get(self.viewer.index).cloned())
            .flatten()
    }

    fn keep_full(&mut self, window: &mut Window) {
        let wanted = self.wanted_full();
        if self.full.as_ref().map(|(path, _)| path) != wanted.as_ref()
            && let Some((_, image)) = self.full.take()
        {
            let _ = window.drop_image(image);
        }
        if let Some(path) = wanted {
            if self.full.is_none() && self.asked_full.as_ref() != Some(&path) {
                self.asked_full = Some(path.clone());
                let _ = self.library.try_send(library::Request::Full(path));
            }
        } else {
            self.asked_full = None;
        }
    }

    /// The shown photo fitted to the window, lp, once it is loaded.
    fn fitted(&self, window: (f32, f32)) -> Option<(f32, f32)> {
        let path = self.photos.get(self.viewer.index)?;
        let image = self.screens.get(path)?;
        let size = image.size(0);
        let (width, height) = (size.width.0 as f32, size.height.0 as f32);
        let fit = (window.0 / width).min(window.1 / height);
        Some((width * fit, height * fit))
    }

    fn photo_tap(&mut self, event: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.click_count().is_multiple_of(2) {
            self.viewer._single_tap = None;
            self.double_tap(event.position(), window, cx);
            return;
        }
        // A tap hides the bars, unless it is the first of a double tap.
        self.viewer._single_tap = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DOUBLE_TAP).await;
            let _ = this.update(cx, |view, cx| view.toggle_bars(cx));
        }));
    }

    /// Zooms in around `at`, or back out to the whole photo.
    fn double_tap(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let size = window.viewport_size();
        let area = (f32::from(size.width), f32::from(size.height));
        let Some(fitted) = self.fitted(area) else {
            return;
        };
        let now = self.viewer.view_at(Instant::now());
        let to = if now.zoomed() {
            View::default()
        } else {
            // The point under the finger stays under it.
            let (x, y) = (f32::from(at.x) - area.0 / 2.0, f32::from(at.y) - area.1 / 2.0);
            let grow = ZOOM_IN / now.scale;
            View {
                scale: ZOOM_IN,
                x: x - (x - now.x) * grow,
                y: y - (y - now.y) * grow,
            }
            .held(fitted, area)
        };
        self.viewer.animate_view(to, &self.look);
        self.keep_full(window);
        cx.notify();
    }

    /// The free drag of a zoomed photo; true while it is ours.
    fn touch_drag(&mut self, event: &TouchDragEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let now = Instant::now();
        let at = (f32::from(event.position.x), f32::from(event.position.y));
        let size = window.viewport_size();
        let area = (f32::from(size.width), f32::from(size.height));
        let fitted = self.fitted(area);
        let viewer = &mut self.viewer;
        match event.phase {
            TouchPhase::Started => {
                if !viewer.view.zoomed() || viewer.confirm {
                    return false;
                }
                viewer.view = viewer.view_at(now);
                viewer.view_tween = None;
                viewer.pan = Some(Pan {
                    last: at,
                    at: now,
                    velocity: (0.0, 0.0),
                });
            }
            TouchPhase::Moved => {
                let Some(pan) = &mut viewer.pan else {
                    return false;
                };
                let (dx, dy) = (at.0 - pan.last.0, at.1 - pan.last.1);
                let dt = (now - pan.at).as_secs_f32().max(0.001);
                // Weighted toward the newest movement, as a finger speeds up
                // and slows down.
                pan.velocity = (
                    lerp(pan.velocity.0, dx / dt, 0.7),
                    lerp(pan.velocity.1, dy / dt, 0.7),
                );
                pan.last = at;
                pan.at = now;
                let moved = View {
                    x: viewer.view.x + dx,
                    y: viewer.view.y + dy,
                    ..viewer.view
                };
                viewer.view = match fitted {
                    Some(fitted) => moved.held(fitted, area),
                    None => moved,
                };
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let Some(pan) = viewer.pan.take() else {
                    return false;
                };
                let velocity = if now - pan.at > STILL { (0.0, 0.0) } else { pan.velocity };
                let flung = View {
                    x: viewer.view.x + velocity.0 * FLING,
                    y: viewer.view.y + velocity.1 * FLING,
                    ..viewer.view
                };
                if let Some(fitted) = fitted {
                    viewer.animate_view(flung.held(fitted, area), &self.look);
                }
            }
        }
        cx.notify();
        true
    }


    fn swipe(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.viewer.confirm || self.viewer.view.zoomed() {
            return;
        }
        let width = f32::from(window.viewport_size().width) + GAP;
        let viewer = &mut self.viewer;
        match event.touch_phase {
            TouchPhase::Started => {
                viewer.drag = viewer.offset(Instant::now());
                viewer.dragging = true;
                viewer.settle = None;
            }
            TouchPhase::Moved if viewer.dragging => {
                let dx = f32::from(event.delta.pixel_delta(px(20.)).x);
                let last = self.photos.len().saturating_sub(1);
                let past_edge = (viewer.drag + dx > 0.0 && viewer.index == 0)
                    || (viewer.drag + dx < 0.0 && viewer.index == last);
                viewer.drag = (viewer.drag + if past_edge { dx * EDGE_GIVE } else { dx }).clamp(-width, width);
            }
            TouchPhase::Moved => return,
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if !viewer.dragging {
                    return;
                }
                viewer.dragging = false;
                let turn = event.touch_phase == TouchPhase::Ended;
                if turn && viewer.drag < -width * TURN && viewer.index + 1 < self.photos.len() {
                    viewer.index += 1;
                    viewer.drag += width;
                } else if turn && viewer.drag > width * TURN && viewer.index > 0 {
                    viewer.index -= 1;
                    viewer.drag -= width;
                }
                viewer.settle = Some((viewer.drag, self.look.tween(motion::SLIDE)));
                self.keep_screens(window);
            }
        }
        cx.notify();
    }

    fn toggle_bars(&mut self, cx: &mut Context<Self>) {
        let viewer = &mut self.viewer;
        viewer.bare = !viewer.bare;
        viewer.info = None;
        viewer.bars = Some(self.look.tween(motion::GROUP_FADE));
        cx.notify();
    }

    fn toggle_info(&mut self, cx: &mut Context<Self>) {
        self.viewer.info = match self.viewer.info {
            Some(_) => None,
            None => self.photos.get(self.viewer.index).and_then(|path| library::info(path)),
        };
        cx.notify();
    }

    fn delete_shown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.viewer.confirm = false;
        let Some(path) = self.photos.get(self.viewer.index).cloned() else {
            return;
        };
        if let Err(error) = library::delete(&path) {
            self.note(format!("Not deleted: {error}"));
            cx.notify();
            return;
        }
        self.photos.remove(self.viewer.index);
        if let Some(image) = self.thumbnails.remove(&path) {
            let _ = window.drop_image(image);
        }
        self.asked_thumbnails.remove(&path);
        self.viewer.info = None;
        self.viewer.view = View::default();
        self.viewer.view_tween = None;
        self.note("Photo deleted");
        if self.photos.is_empty() {
            self.viewer.from_grid = false;
            self.leave_viewer(window, cx);
            return;
        }
        // The next older photo takes its place; after the oldest, the one
        // before.
        self.viewer.index = self.viewer.index.min(self.photos.len() - 1);
        self.keep_screens(window);
        if let Some(newest) = self.photos.first().cloned() {
            self.ask_thumbnail(newest);
        }
        cx.notify();
    }

    /// A raised bar along the top: Back, then what the screen shows.
    fn top_group(&self, title: String, detail: String, cx: &mut Context<Self>) -> impl IntoElement {
        let look = &self.look;
        smoke(look)
            .absolute()
            .top(px(INSET))
            .left(px(INSET))
            .right(px(INSET))
            .h(dp(GROUP_HEIGHT))
            .px(dp(4.))
            .flex()
            .items_center()
            .gap(dp(4.))
            .child(
                self.bar_button("back", Icon::Back, look.palette.foreground)
                    .on_click(cx.listener(|this, _, window, cx| this.back(&super::Back, window, cx))),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(dp(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(solid(look.palette.foreground))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(dp(13.))
                            .text_color(solid(look.palette.secondary))
                            .child(detail),
                    ),
            )
    }

    fn bar_button(&self, id: &'static str, glyph: Icon, ink: u32) -> Stateful<gpui::Div> {
        let pressed = solid(self.look.palette.control);
        div()
            .id(id)
            .flex_none()
            .size(dp(48.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .active(move |button| button.bg(pressed))
            .child(icon(glyph, dp(24.), solid(ink)))
    }

    pub(super) fn viewer_screen(&self, window: &Window, now: Instant, cx: &mut Context<Self>) -> impl IntoElement {
        let look = &self.look;
        let viewer = &self.viewer;
        let size = window.viewport_size();
        let area = (f32::from(size.width), f32::from(size.height));
        let width = area.0;
        let offset = viewer.offset(now);
        let view = viewer.view_at(now);
        let fitted = self.fitted(area);
        let pages = (-1isize..=1).filter_map(|step| {
            let at = viewer.index.checked_add_signed(step)?;
            let path = self.photos.get(at)?;
            let screen = self.screens.get(path).cloned();
            let page = div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(offset + step as f32 * (width + GAP)))
                .w(px(width));
            // The shown photo is placed by its zoom, from the whole photo
            // once it is decoded; the others fit the window.
            Some(match (step, fitted) {
                (0, Some((fitted_width, fitted_height))) => {
                    let image = self
                        .full
                        .as_ref()
                        .filter(|(full, _)| full == path)
                        .map(|(_, image)| image.clone())
                        .or(screen);
                    let (shown_width, shown_height) = (fitted_width * view.scale, fitted_height * view.scale);
                    page.children(image.map(|image| {
                        img(image)
                            .absolute()
                            .left(px((width - shown_width) / 2.0 + view.x))
                            .top(px((area.1 - shown_height) / 2.0 + view.y))
                            .w(px(shown_width))
                            .h(px(shown_height))
                    }))
                }
                _ => page.children(screen.map(|image| img(image).size_full().object_fit(ObjectFit::Contain))),
            })
        });
        let this = cx.entity().downgrade();
        // Claims the touch while the photo is zoomed, so it pans freely; the
        // bars occlude it, so their buttons keep their taps.
        let pan = canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |_, hitbox, window, _| {
                window.on_mouse_event(move |event: &TouchDragEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble
                        || (event.phase == TouchPhase::Started && !hitbox.is_hovered(window))
                    {
                        return;
                    }
                    let claimed = this
                        .update(cx, |view, cx| view.touch_drag(event, window, cx))
                        .unwrap_or(false);
                    if claimed && event.phase == TouchPhase::Started {
                        window.prevent_default();
                    }
                });
            },
        )
        .absolute()
        .size_full();
        let photos = div()
            .id("photos")
            .absolute()
            .inset_0()
            .overflow_hidden()
            .children(pages)
            .child(pan)
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| this.swipe(event, window, cx)))
            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| this.photo_tap(event, window, cx)));

        let shown = viewer.bars_shown(now);
        let bars = (shown > 0.0).then(|| {
            let path = self.photos.get(viewer.index);
            let taken = path.and_then(|path| library::taken(path));
            let title = taken.map_or_else(|| "Photo".to_string(), day);
            let place = format!("{} of {}", viewer.index + 1, self.photos.len());
            let detail = match taken {
                Some([.., hour, minute]) => format!("{hour:02}:{minute:02} · {place}"),
                None => place,
            };
            let info_ink = if viewer.info.is_some() { look.accent } else { look.palette.foreground };
            let bottom = smoke(look)
                .absolute()
                .left(px(INSET))
                .right(px(INSET))
                .bottom(px(GESTURE_INSET))
                .h(dp(BOTTOM_BAR))
                .flex()
                .items_center()
                .justify_around()
                .child(
                    self.bar_button("all", Icon::Photos, look.palette.foreground)
                        .on_click(cx.listener(|this, _, _, cx| this.open_grid(cx))),
                )
                .child(
                    self.bar_button("info", Icon::Info, info_ink)
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_info(cx))),
                )
                .child(self.bar_button("delete", Icon::Delete, look.palette.foreground).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.viewer.confirm = true;
                        this.viewer.info = None;
                        cx.notify();
                    },
                )));
            div()
                .absolute()
                .inset_0()
                .opacity(shown)
                .child(self.top_group(title, detail, cx))
                .children(viewer.info.as_ref().map(|info| self.info_panel(info)))
                .child(bottom)
        });
        div()
            .size_full()
            .relative()
            .child(photos)
            .children(bars)
            .children(viewer.confirm.then(|| self.confirmation(cx)))
    }

    fn info_panel(&self, info: &Info) -> impl IntoElement {
        let look = &self.look;
        let path = &self.photos[self.viewer.index];
        let row = |label: &'static str, value: String| {
            div()
                .flex()
                .flex_col()
                .gap(dp(2.))
                .child(div().text_size(dp(12.)).text_color(solid(look.palette.secondary)).child(label))
                .child(div().text_size(dp(15.)).text_color(solid(look.palette.foreground)).child(value))
        };
        let megapixels = info.width as f32 * info.height as f32 / 1e6;
        let name = path.file_name().map_or(String::new(), |name| name.to_string_lossy().into_owned());
        let mut panel = smoke(look)
            .absolute()
            .left(px(INSET))
            .right(px(INSET))
            .bottom(px(GESTURE_INSET + f32::from(dp(BOTTOM_BAR)) + INSET))
            .p(dp(16.))
            .flex()
            .flex_col()
            .gap(dp(12.))
            .children(library::taken(path).map(|taken| row("Taken", long_date(taken))))
            .child(row("Size", format!("{} × {} · {megapixels:.1} MP", info.width, info.height)))
            .child(row("File", format!("{name} · {}", file_size(info.bytes))));
        if let Some((sensor, micros, gain, digital)) = &info.exposure {
            let mut exposure = format!("{sensor} · {} · gain {gain:.1}×", shutter_speed(*micros));
            if *digital > 1.0 {
                exposure.push_str(&format!(" · digital {digital:.1}×"));
            }
            panel = panel.child(row("Exposure", exposure));
        }
        panel
    }

    /// Delete's confirmation: an island above the gesture zone, the rest
    /// dimmed.
    fn confirmation(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let look = &self.look;
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .flex_1()
                .h(dp(48.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(dp(15.))
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .child(label)
        };
        let control = look.palette.control;
        let destructive = palette::DESTRUCTIVE;
        div()
            .id("dim")
            .absolute()
            .inset_0()
            .occlude()
            .bg(color(0x000000, DIM))
            .on_click(cx.listener(|this, _, _, cx| {
                this.viewer.confirm = false;
                cx.notify();
            }))
            .child(
                smoke(look)
                    .id("confirm")
                    .absolute()
                    .left(px(INSET))
                    .right(px(INSET))
                    .bottom(px(GESTURE_INSET))
                    .p(dp(20.))
                    .flex()
                    .flex_col()
                    .gap(dp(6.))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_size(dp(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Delete this photo?"),
                    )
                    .child(
                        div()
                            .text_size(dp(14.))
                            .text_color(solid(look.palette.secondary))
                            .child("It is removed from Pictures/Camera. This cannot be undone."),
                    )
                    .child(
                        div()
                            .mt(dp(14.))
                            .flex()
                            .gap(dp(8.))
                            .child(
                                button("cancel", "Cancel")
                                    .bg(solid(control))
                                    .text_color(solid(look.palette.foreground))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.viewer.confirm = false;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("confirm-delete", "Delete")
                                    .bg(solid(destructive))
                                    .text_color(solid(on(destructive)))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.delete_shown(window, cx);
                                    })),
                            ),
                    ),
            )
    }

    pub(super) fn grid_screen(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = &self.look;
        let width = f32::from(window.viewport_size().width) - 2.0 * INSET;
        let tile = (width - TILE_GAP * (TILES_A_ROW - 1) as f32) / TILES_A_ROW as f32;
        let radius = px(MIN_RADIUS);
        // Photos by the day they were taken, newest first.
        let mut days: Vec<(String, Vec<usize>)> = Vec::new();
        for (index, path) in self.photos.iter().enumerate() {
            let label = library::taken(path).map_or_else(|| "Earlier".to_string(), day);
            match days.last_mut() {
                Some((last, indices)) if *last == label => indices.push(index),
                _ => days.push((label, vec![index])),
            }
        }
        let sections = days.into_iter().map(|(label, indices)| {
            let rows = indices.chunks(TILES_A_ROW).map(|row| {
                div().flex().gap(px(TILE_GAP)).children(row.iter().map(|&index| {
                    let path = &self.photos[index];
                    div()
                        .id(("tile", index))
                        .flex_none()
                        .size(px(tile))
                        .rounded(radius)
                        .bg(solid(look.palette.raised))
                        .cursor_pointer()
                        .active(|tile| tile.opacity(0.7))
                        .children(
                            self.thumbnails
                                .get(path)
                                .map(|thumbnail| img(thumbnail.clone()).size_full().rounded(radius).object_fit(ObjectFit::Cover)),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| this.open_viewer(index, true, window, cx)))
                }))
            });
            div()
                .flex()
                .flex_col()
                .gap(px(TILE_GAP))
                .child(
                    div()
                        .pt(dp(16.))
                        .pb(dp(8.))
                        .px(dp(4.))
                        .text_size(dp(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(label),
                )
                .children(rows)
        });
        let count = match self.photos.len() {
            1 => "1 photo".to_string(),
            count => format!("{count} photos"),
        };
        let content = div()
            .id("grid")
            .absolute()
            .inset_0()
            .overflow_y_scroll()
            .px(px(INSET))
            .pt(px(INSET + f32::from(dp(GROUP_HEIGHT))))
            .pb(px(GESTURE_INSET + INSET))
            .children(sections)
            .when(self.photos.is_empty(), |content| {
                content.child(
                    island(look)
                        .mt(dp(16.))
                        .p(dp(16.))
                        .text_size(dp(15.))
                        .child("No photos yet. The ones you take appear here."),
                )
            });
        div()
            .size_full()
            .relative()
            .child(content)
            .child(self.top_group("Photos".to_string(), count, cx))
    }
}

/// "Today", "Yesterday", "21 October", or with the year when it is not this
/// one.
fn day([year, month, date, ..]: [u32; 5]) -> String {
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    if local_date(now) == [year, month, date] {
        return "Today".into();
    }
    if local_date(now - 24 * 60 * 60) == [year, month, date] {
        return "Yesterday".into();
    }
    let name = MONTHS[(month as usize).clamp(1, 12) - 1];
    if local_date(now)[0] == year {
        format!("{date} {name}")
    } else {
        format!("{date} {name} {year}")
    }
}

fn long_date([year, month, date, hour, minute]: [u32; 5]) -> String {
    let name = MONTHS[(month as usize).clamp(1, 12) - 1];
    format!("{date} {name} {year}, {hour:02}:{minute:02}")
}

fn local_date(time: libc::time_t) -> [u32; 3] {
    // SAFETY: localtime_r fills `tm` from `time`.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&time, &mut tm) };
    [(tm.tm_year + 1900) as u32, (tm.tm_mon + 1) as u32, tm.tm_mday as u32]
}

fn shutter_speed(micros: f32) -> String {
    if micros >= 1e6 {
        format!("{:.1} s", micros / 1e6)
    } else {
        format!("1/{} s", (1e6 / micros.max(1.0)).round())
    }
}

fn file_size(bytes: u64) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f32 / (1 << 20) as f32)
    } else {
        format!("{} KB", bytes.div_ceil(1 << 10))
    }
}
