//! The Camera window. A camera is judged on its picture, so it keeps to
//! Denial's mobile materials (denial/docs/MOBILE_APP_MATERIALS.md) as an
//! opaque window: always the dark palette on black, raised islands for the
//! controls, glass only over its own picture. The preview is centred, the
//! top pill holds the timer and the grid, the lenses sit on the preview, and
//! the last photo, the shutter and the camera switch are centred under it.
//! The gallery (`gallery`) shows the photos one by one and as a grid.

mod gallery;

use denial_look::words::{fill, tr};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use denial_look::mobile::GESTURE_INSET;
use denial_look::{Appearance, INSET, Look, SettingsWatch, Tween, decelerate, floating, motion, palette};
use denial_look_gpui::{Icon, chip, color, dp, icon, island, solid};
use gpui::{
    App, AsyncApp, AsyncWindowContext, ClickEvent, Context, FocusHandle, Focusable, Font, FontWeight, ObjectFit,
    RenderImage, SharedString, Stateful, Subscription, Task, Transformation, WeakEntity, Window, actions, div, img,
    prelude::*, px, radians, relative,
};

use braun::client::Info;

use crate::engine::{Command, Commands, Event, Facing};
use crate::library::{self, Loaded, Request};

use gallery::Viewer;

actions!(camera, [Back]);

/// The top pill, which holds the timer and the grid, dp.
const BAR: f32 = 44.0;
/// The shutter: a ring, a gap and the release disc, dp.
const SHUTTER: f32 = 80.0;
const RING: f32 = 4.0;
const RELEASE: f32 = 64.0;
const RELEASE_PRESSED: f32 = 56.0;
const RELEASE_BUSY: f32 = 44.0;
/// The stop square the shutter shows during the timer, dp.
const STOP: f32 = 26.0;
/// The last photo and the camera switch, dp.
const SIDE: f32 = 52.0;
const SIDE_RADIUS: f32 = 12.0;
const ICON: f32 = 24.0;
/// Room under the preview for a chip above the shutter, dp.
const CHIP_ROOM: f32 = 30.0;
/// The lens buttons on the preview, dp, and their glass.
const ZOOM: f32 = 38.0;
const ZOOM_TINT: f32 = 0.35;
/// The crops each way's own camera offers.
const ZOOMS: [f32; 2] = [1.0, 2.0];
/// The timer's choices, seconds; 0 is off.
const TIMERS: [u32; 3] = [0, 3, 10];
/// The last photo grows from this part of its size when a photo is saved.
const ARRIVAL_FROM: f32 = 0.5;
/// The focus mark, dp: it lands this much larger and settles.
const MARK: f32 = 64.0;
const MARK_LAND: f32 = 1.3;
const MARK_FOR: Duration = Duration::from_millis(900);
/// The rule-of-thirds lines.
const GRID_ALPHA: f32 = 0.4;
/// Tint over the blurred last frame while the camera opens.
const OPENING_TINT: f32 = 0.35;
/// The preview blinks dark this deep when the burst starts.
const BLINK: f32 = 0.75;
/// One turn of the developing arc.
const SPIN: Duration = Duration::from_millis(900);
const NOTICE: Duration = Duration::from_millis(motion::NOTICE);

enum State {
    Opening,
    Live,
    Failed(SharedString),
}

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Camera,
    Viewer,
    Grid,
}

struct Mark {
    x: f32,
    y: f32,
    at: Instant,
    settle: Tween,
}

pub struct Camera {
    look: Look,
    font: Font,
    focus_handle: FocusHandle,
    commands: Commands,
    library: async_channel::Sender<Request>,
    screen: Screen,
    /// The screen fading in.
    entered: Option<Tween>,
    /// The window can be seen; Denial suspends it otherwise.
    shown: bool,
    /// What the camera was last told: closed (true) or open.
    closed: bool,

    /// The cameras braun offers.
    cameras: Vec<Info>,
    /// The camera open or opening.
    camera: Option<String>,
    facing: Facing,
    /// How many times as tall as it is wide the open camera's frame is,
    /// upright.
    aspect: f32,
    /// The open camera can focus.
    lens: bool,
    state: State,
    preview: Option<Arc<RenderImage>>,
    zoom: f32,
    grid: bool,
    /// Seconds the timer waits; 0 is off.
    timer: u32,
    /// When the timer takes the photo.
    countdown: Option<Instant>,
    _countdown: Option<Task<()>>,
    /// The burst is being taken.
    capturing: bool,
    /// Photos being developed.
    developing: usize,
    flash: Option<Tween>,
    mark: Option<Mark>,
    /// The switch's half turn when the cameras change.
    turn: Option<Tween>,
    /// The last photo growing into its place when it is saved.
    arrival: Option<Tween>,
    notice: Option<(SharedString, Instant)>,

    /// Newest first.
    photos: Vec<PathBuf>,
    thumbnails: HashMap<PathBuf, Arc<RenderImage>>,
    /// The photos the viewer shows and its neighbours, screen-sized.
    screens: HashMap<PathBuf, Arc<RenderImage>>,
    asked_thumbnails: HashSet<PathBuf>,
    asked_screens: HashSet<PathBuf>,
    /// The zoomed photo at its full size, and the one asked for.
    full: Option<(PathBuf, Arc<RenderImage>)>,
    asked_full: Option<PathBuf>,
    viewer: Viewer,

    _events: Task<()>,
    _loaded: Task<()>,
    _settings: Task<()>,
    _visibility: Subscription,
}

impl Focusable for Camera {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Denial's look as the camera wears it: dark and opaque whatever the
/// appearance, so nothing but the picture colours what is seen.
fn camera_look() -> Look {
    let mut look = Look::load();
    look.appearance = Appearance::Dark;
    look.palette = palette::DARK;
    look.window_alpha = 1.0;
    look.panel_alpha = 1.0;
    look
}

impl Camera {
    pub fn new(
        commands: Commands,
        events: async_channel::Receiver<Event>,
        library: async_channel::Sender<Request>,
        loaded: async_channel::Receiver<Loaded>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let look = camera_look();
        let mut camera = Self {
            font: gpui::font(look.font_family(&cx.text_system().all_font_names())),
            look,
            focus_handle: cx.focus_handle(),
            commands,
            library,
            screen: Screen::Camera,
            entered: None,
            shown: true,
            closed: false,
            cameras: Vec::new(),
            camera: None,
            facing: Facing::Back,
            aspect: 4.0 / 3.0,
            lens: false,
            state: State::Opening,
            preview: None,
            zoom: 1.0,
            grid: false,
            timer: 0,
            countdown: None,
            _countdown: None,
            capturing: false,
            developing: 0,
            flash: None,
            mark: None,
            turn: None,
            arrival: None,
            notice: None,
            photos: library::photos(),
            thumbnails: HashMap::new(),
            screens: HashMap::new(),
            asked_thumbnails: HashSet::new(),
            asked_screens: HashSet::new(),
            full: None,
            asked_full: None,
            viewer: Viewer::default(),
            _events: cx.spawn_in(window, async move |this, cx| Self::follow(this, events, cx).await),
            _loaded: cx.spawn_in(window, async move |this, cx| Self::follow_library(this, loaded, cx).await),
            _settings: cx.spawn(async move |this, cx| Self::follow_settings(this, cx).await),
            // Denial suspends a window no one can see; the camera closes
            // with it.
            _visibility: cx.observe_window_visibility(window, |this, visibility, _, _| {
                this.shown = visibility.is_visible();
                this.sync_camera();
            }),
        };
        if let Some(newest) = camera.photos.first().cloned() {
            camera.ask_thumbnail(newest);
        }
        camera
    }

    async fn follow(this: WeakEntity<Self>, events: async_channel::Receiver<Event>, cx: &mut AsyncWindowContext) {
        while let Ok(event) = events.recv().await {
            if this
                .update_in(cx, |view, window, cx| view.event(event, window, cx))
                .is_err()
            {
                return;
            }
        }
    }

    async fn follow_library(
        this: WeakEntity<Self>,
        loaded: async_channel::Receiver<Loaded>,
        cx: &mut AsyncWindowContext,
    ) {
        while let Ok(image) = loaded.recv().await {
            if this
                .update_in(cx, |view, window, cx| view.loaded(image, window, cx))
                .is_err()
            {
                return;
            }
        }
    }

    /// Takes Denial's look afresh whenever its settings change.
    async fn follow_settings(this: WeakEntity<Self>, cx: &mut AsyncApp) {
        let mut watch = match SettingsWatch::new() {
            Ok(watch) => watch,
            Err(error) => return eprintln!("camera: following Denial's settings: {error}"),
        };
        loop {
            if let Err(error) = watch.changed().await {
                return eprintln!("camera: following Denial's settings: {error}");
            }
            let look = cx.background_spawn(async { camera_look() }).await;
            let taken = this.update(cx, |view, cx| {
                view.font = gpui::font(look.font_family(&cx.text_system().all_font_names()));
                view.look = look;
                cx.notify();
            });
            if taken.is_err() {
                return;
            }
        }
    }

    fn event(&mut self, event: Event, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            // Opening lasts until the first frame: the last frame shown
            // stays blurred until then.
            Event::Cameras(cameras) => {
                // The camera thread opens the main back camera until told
                // otherwise.
                if self.camera.is_none() {
                    self.camera = Facing::Back.camera(&cameras).map(|camera| camera.id.clone());
                }
                self.cameras = cameras;
            }
            Event::Opened { id, aspect, lens } => {
                if let Some(camera) = self.cameras.iter().find(|camera| camera.id == id) {
                    self.facing = Facing::of(camera);
                }
                self.camera = Some(id);
                self.aspect = aspect;
                self.lens = lens;
            }
            Event::Preview(image) => {
                if let Some(old) = self.preview.replace(image) {
                    let _ = window.drop_image(old);
                }
                self.state = State::Live;
            }
            Event::Captured => self.capturing = false,
            Event::Saved(path) => {
                self.developing = self.developing.saturating_sub(1);
                if !self.photos.contains(&path) {
                    self.photos.insert(0, path.clone());
                    // The viewer keeps showing the photo it showed.
                    if self.screen != Screen::Camera {
                        self.viewer.index += 1;
                    }
                }
                self.ask_thumbnail(path);
                self.arrival = Some(self.look.tween(motion::NEW_MESSAGE));
                self.note(tr("Saved to Pictures"));
            }
            Event::PhotoFailed(error) => {
                self.capturing = false;
                self.developing = self.developing.saturating_sub(1);
                self.note(error);
            }
            Event::Failed(error) => self.state = State::Failed(error.into()),
        }
        cx.notify();
    }

    fn loaded(&mut self, image: Loaded, window: &mut Window, cx: &mut Context<Self>) {
        match image {
            Loaded::Thumbnail(path, image) => {
                if self.photos.contains(&path) {
                    self.thumbnails.insert(path, image);
                } else {
                    let _ = window.drop_image(image);
                }
            }
            Loaded::Screen(path, image) => {
                if self.wanted_screens().contains(&path) {
                    self.screens.insert(path, image);
                } else {
                    self.asked_screens.remove(&path);
                    let _ = window.drop_image(image);
                }
            }
            Loaded::Full(path, image) => {
                if self.wanted_full().as_ref() == Some(&path) {
                    if let Some((_, old)) = self.full.replace((path, image)) {
                        let _ = window.drop_image(old);
                    }
                } else {
                    let _ = window.drop_image(image);
                }
            }
            Loaded::Failed(path) => eprintln!("camera: cannot show {}", path.display()),
        }
        cx.notify();
    }

    fn ask_thumbnail(&mut self, path: PathBuf) {
        if self.asked_thumbnails.insert(path.clone()) {
            let _ = self.library.try_send(Request::Thumbnail(path));
        }
    }

    fn note(&mut self, text: impl Into<SharedString>) {
        self.notice = Some((text.into(), Instant::now()));
    }

    /// Opens the camera on the camera screen while the window is seen, and
    /// closes it otherwise.
    fn sync_camera(&mut self) {
        let closed = !self.shown || self.screen != Screen::Camera;
        if closed != self.closed && self.commands.try_send(Command::Hide(closed)).is_ok() {
            self.closed = closed;
            if !closed {
                self.state = State::Opening;
            }
        }
    }

    fn go(&mut self, screen: Screen, cx: &mut Context<Self>) {
        if screen != Screen::Camera {
            self.cancel_countdown();
        }
        self.screen = screen;
        self.entered = Some(self.look.tween(motion::IN));
        self.sync_camera();
        cx.notify();
    }

    /// The shutter: the photo now, or after the timer; during the timer, a
    /// stop.
    fn release(&mut self, cx: &mut Context<Self>) {
        if self.countdown.is_some() {
            self.cancel_countdown();
            cx.notify();
        } else if self.timer > 0 {
            if matches!(self.state, State::Live) && !self.capturing {
                self.start_countdown(cx);
            }
        } else {
            self.shoot(cx);
        }
    }

    fn start_countdown(&mut self, cx: &mut Context<Self>) {
        self.countdown = Some(Instant::now() + Duration::from_secs(self.timer.into()));
        self._countdown = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                let waiting = this.update(cx, |view, cx| {
                    let Some(end) = view.countdown else {
                        return false;
                    };
                    cx.notify();
                    if Instant::now() < end {
                        return true;
                    }
                    view.countdown = None;
                    view.shoot(cx);
                    false
                });
                if !matches!(waiting, Ok(true)) {
                    return;
                }
            }
        }));
        cx.notify();
    }

    fn cancel_countdown(&mut self) {
        self.countdown = None;
        self._countdown = None;
    }

    fn shoot(&mut self, cx: &mut Context<Self>) {
        if self.capturing || !matches!(self.state, State::Live) {
            return;
        }
        if self.commands.try_send(Command::Shoot).is_ok() {
            self.capturing = true;
            self.developing += 1;
            self.flash = Some(self.look.tween(motion::OUT));
            cx.notify();
        }
    }

    /// Turns to the other way's own camera.
    fn switch(&mut self, cx: &mut Context<Self>) {
        let next = self.facing.other();
        let Some(id) = next.camera(&self.cameras).map(|camera| camera.id.clone()) else {
            return;
        };
        if self.open(id) {
            self.facing = next;
            self.turn = Some(self.look.tween(motion::SLIDE));
            cx.notify();
        }
    }

    /// Opens camera `id`, unless a burst is being taken; true if asked.
    fn open(&mut self, id: String) -> bool {
        if self.capturing || self.commands.try_send(Command::Open(id.clone())).is_err() {
            return false;
        }
        self.camera = Some(id);
        self.state = State::Opening;
        self.mark = None;
        true
    }

    /// A lens button: camera `id` at `crop`.
    fn pick(&mut self, id: &str, crop: f32, cx: &mut Context<Self>) {
        if self.camera.as_deref() != Some(id) && !self.open(id.to_string()) {
            return;
        }
        self.set_zoom(crop, cx);
        cx.notify();
    }

    /// The lens buttons facing this way, widest first: this way's own
    /// camera at each of `ZOOMS`, and each other camera the device places
    /// against the main one. Each is the camera, its crop and the main
    /// camera's zoom that shows the same.
    fn lenses(&self) -> Vec<(String, f32, f32)> {
        let Some(own) = self.facing.camera(&self.cameras) else {
            return Vec::new();
        };
        let base = own.zoom.unwrap_or(1.0);
        let mut lenses: Vec<_> = ZOOMS.iter().map(|&crop| (own.id.clone(), crop, base * crop)).collect();
        lenses.extend(
            self.facing
                .cameras(&self.cameras)
                .filter(|camera| camera.id != own.id)
                .filter_map(|camera| Some((camera.id.clone(), 1.0, camera.zoom?))),
        );
        lenses.sort_by(|a, b| a.2.total_cmp(&b.2));
        lenses
    }

    fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        if zoom != self.zoom && self.commands.try_send(Command::Zoom(zoom)).is_ok() {
            self.zoom = zoom;
            self.mark = None;
            cx.notify();
        }
    }

    fn focus_at(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        if !self.lens || !matches!(self.state, State::Live) {
            return;
        }
        if self.commands.try_send(Command::Focus(x, y)).is_ok() {
            self.mark = Some(Mark {
                x,
                y,
                at: Instant::now(),
                settle: self.look.tween(motion::LIFT),
            });
            cx.notify();
        }
    }

    fn back(&mut self, _: &Back, window: &mut Window, cx: &mut Context<Self>) {
        match self.screen {
            Screen::Camera => {}
            Screen::Viewer => self.viewer_back(window, cx),
            Screen::Grid => self.go(Screen::Camera, cx),
        }
    }

    /// The preview's place: centred in the window, as wide as the window
    /// allows, in the camera's upright aspect, leaving room for the top
    /// pill above and the controls below.
    fn preview_bounds(&self, width: f32, height: f32) -> (f32, f32, f32, f32) {
        let aspect = self.aspect;
        let above = 2.0 * INSET + f32::from(dp(BAR));
        let below = GESTURE_INSET + f32::from(dp(SHUTTER + 2.0 * CHIP_ROOM));
        let room = (height - above - below).max(0.0);
        let mut preview_width = width - 2.0 * INSET;
        let mut preview_height = preview_width * aspect;
        if preview_height > room {
            preview_height = room;
            preview_width = preview_height / aspect;
        }
        let top = ((height - preview_height) / 2.0).clamp(above, (height - below - preview_height).max(above));
        ((width - preview_width) / 2.0, top, preview_width, preview_height)
    }

    fn animating(&self, now: Instant) -> bool {
        let running = |tween: &Option<Tween>| tween.is_some_and(|tween| tween.running(now));
        running(&self.entered)
            || match self.screen {
                Screen::Camera => {
                    running(&self.flash)
                        || running(&self.turn)
                        || running(&self.arrival)
                        || self.developing > 0
                        || self.mark.as_ref().is_some_and(|mark| now - mark.at < MARK_FOR)
                        || self.notice.as_ref().is_some_and(|(_, at)| now - *at < NOTICE)
                }
                Screen::Viewer => self.viewer.animating(now),
                Screen::Grid => false,
            }
    }

    /// A raised round control: its pressed fill is the control colour.
    fn round(&self, id: &'static str, diameter: f32) -> Stateful<gpui::Div> {
        let pressed = solid(self.look.palette.control);
        island(&self.look)
            .id(id)
            .flex_none()
            .size(dp(diameter))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .active(move |control| control.bg(pressed))
    }

    fn top_bar(&self, top: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let look = &self.look;
        let tint = |on: bool| solid(if on { look.accent } else { look.palette.foreground });
        let pressed = solid(look.palette.control);
        let button = |id: &'static str| {
            div()
                .id(id)
                .h_full()
                .min_w(dp(BAR))
                .px(dp(10.))
                .flex()
                .items_center()
                .justify_center()
                .gap(dp(6.))
                .rounded_full()
                .cursor_pointer()
                .active(move |button| button.bg(pressed))
        };
        let timer_on = self.timer > 0;
        let timer = button("timer")
            .child(icon(Icon::Timer, dp(20.), tint(timer_on)))
            .when(timer_on, |button| {
                button
                    .text_color(tint(true))
                    .text_size(dp(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(fill("{seconds} s", &[("seconds", &self.timer.to_string())]))
            })
            .on_click(cx.listener(|this, _, _, cx| {
                let at = TIMERS.iter().position(|&timer| timer == this.timer).unwrap_or(0);
                this.timer = TIMERS[(at + 1) % TIMERS.len()];
                this.note(match this.timer {
                    0 => tr("Timer off").to_string(),
                    seconds => fill("Timer {seconds} s", &[("seconds", &seconds.to_string())]),
                });
                cx.notify();
            }));
        let grid = button("grid")
            .child(icon(Icon::Gridlines, dp(20.), tint(self.grid)))
            .on_click(cx.listener(|this, _, _, cx| {
                this.grid = !this.grid;
                cx.notify();
            }));
        div()
            .absolute()
            .top(px(top))
            .left_0()
            .right_0()
            .h(dp(BAR))
            .flex()
            .justify_center()
            .child(
                island(look)
                    .h_full()
                    .rounded_full()
                    .p(dp(2.))
                    .flex()
                    .items_center()
                    .gap(dp(2.))
                    .child(timer)
                    .child(grid),
            )
    }

    fn preview(&self, bounds: (f32, f32, f32, f32), now: Instant, cx: &mut Context<Self>) -> impl IntoElement {
        let (left, top, width, height) = bounds;
        let look = &self.look;
        // GPUI clips to rectangles: everything inside takes the radius too.
        let radius = px(look.group_radius());
        let layer = || div().absolute().inset_0().rounded(radius);
        let mut area = div()
            .id("preview")
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(width))
            .h(px(height))
            .rounded(radius)
            .bg(solid(look.palette.canvas))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                let at = event.position();
                let x = (f32::from(at.x) - left) / width;
                let y = (f32::from(at.y) - top) / height;
                this.focus_at(x.clamp(0.0, 1.0), y.clamp(0.0, 1.0), cx);
            }));
        if let Some(image) = &self.preview
            && !matches!(self.state, State::Failed(_))
        {
            area = area.child(img(image.clone()).size_full().rounded(radius).object_fit(ObjectFit::Cover));
            if matches!(self.state, State::Opening) {
                area = area.child(
                    layer()
                        .backdrop_blur(px(floating::BACKDROP_BLUR))
                        .bg(color(look.palette.glass, OPENING_TINT)),
                );
            }
        }
        if self.grid {
            let line = || div().absolute().bg(color(0xffffff, GRID_ALPHA));
            for third in [1.0 / 3.0, 2.0 / 3.0] {
                area = area
                    .child(line().top(px(height * third)).left_0().right_0().h(px(1.)))
                    .child(line().left(px(width * third)).top_0().bottom_0().w(px(1.)));
            }
        }
        if let Some(flash) = self.flash.filter(|flash| flash.running(now)) {
            let t = flash.progress(now);
            area = area.child(layer().bg(color(0x000000, BLINK * (1.0 - t))));
        }
        if let Some(mark) = self.mark.as_ref().filter(|mark| now - mark.at < MARK_FOR) {
            let t = (now - mark.at).as_secs_f32() / MARK_FOR.as_secs_f32();
            let land = MARK_LAND + (1.0 - MARK_LAND) * decelerate(mark.settle.progress(now));
            let size = f32::from(dp(MARK)) * land;
            area = area.child(
                div()
                    .absolute()
                    .left(px(mark.x * width - size / 2.0))
                    .top(px(mark.y * height - size / 2.0))
                    .size(px(size))
                    .rounded_full()
                    .border(dp(1.5))
                    .border_color(color(0xffffff, 0.9 * (1.0 - t * t))),
            );
        }
        if let Some(end) = self.countdown {
            let left = end.saturating_duration_since(now).as_secs_f32().ceil().max(1.0);
            area = area.child(
                layer().flex().items_center().justify_center().child(
                    div()
                        .text_size(dp(120.))
                        .font_weight(FontWeight::LIGHT)
                        .text_color(solid(0xffffff))
                        .child(format!("{left}")),
                ),
            );
        }
        let message = match &self.state {
            State::Failed(error) => Some(error.clone()),
            State::Opening | State::Live => None,
        };
        area = area.children(message.map(|message| {
            div().absolute().inset_0().flex().items_center().justify_center().p(dp(24.)).child(
                island(look)
                    .p(dp(16.))
                    .text_size(dp(15.))
                    .text_color(solid(look.palette.foreground))
                    .child(message),
            )
        }));
        area.child(self.lens_buttons(cx))
    }

    /// The lenses on the preview, on glass over the picture: .6, 1 and 2
    /// with an ultrawide.
    fn lens_buttons(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let look = &self.look;
        let buttons = self.lenses().into_iter().enumerate().map(|(index, (id, crop, zoom))| {
            let chosen = self.camera.as_deref() == Some(id.as_str()) && crop == self.zoom;
            div()
                .id(("lens", index))
                .h(dp(ZOOM))
                .min_w(dp(ZOOM))
                .px(dp(6.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .when(chosen, |button| button.bg(color(look.palette.glass, ZOOM_TINT)))
                .text_size(dp(if chosen { 13. } else { 12. }))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(solid(if chosen { look.accent } else { look.palette.foreground }))
                .child(zoom_label(zoom, chosen))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.pick(&id, crop, cx);
                }))
        });
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(dp(12.))
            .flex()
            .justify_center()
            .child(
                div()
                    .occlude()
                    .p(dp(3.))
                    .gap(dp(4.))
                    .flex()
                    .rounded_full()
                    .backdrop_blur(px(floating::BACKDROP_BLUR))
                    .bg(color(look.palette.glass, ZOOM_TINT))
                    .children(buttons),
            )
    }

    /// The last photo, the shutter and the switch, centred in the room
    /// between the preview and the gesture zone.
    fn controls(&self, room: (f32, f32), now: Instant, cx: &mut Context<Self>) -> impl IntoElement {
        let look = &self.look;
        let foreground = look.palette.foreground;
        let ready = matches!(self.state, State::Live) && !self.capturing;
        let newest = self.photos.first().and_then(|path| self.thumbnails.get(path)).cloned();
        let last = newest.map(|thumbnail| {
            let grow = self.arrival.map_or(1.0, |arrival| decelerate(arrival.progress(now)));
            let size = SIDE * (ARRIVAL_FROM + (1.0 - ARRIVAL_FROM) * grow);
            let radius = dp(SIDE_RADIUS * size / SIDE);
            div()
                .id("last")
                .flex_none()
                .size(dp(SIDE))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .active(|last| last.opacity(0.7))
                .child(
                    div()
                        .relative()
                        .size(dp(size))
                        .child(img(thumbnail).size_full().rounded(radius).object_fit(ObjectFit::Cover))
                        .when(self.developing > 0, |frame| frame.child(self.developing_arc(radius))),
                )
                .on_click(cx.listener(|this, _, window, cx| this.open_viewer(0, false, window, cx)))
        });
        let disc = if self.countdown.is_some() {
            div().size(dp(STOP)).rounded(dp(6.)).bg(solid(foreground))
        } else {
            let release = if self.capturing { RELEASE_BUSY } else { RELEASE };
            div()
                .size(dp(release))
                .rounded_full()
                .bg(color(foreground, if ready { 1.0 } else { 0.4 }))
        };
        let shutter = div()
            .id("shutter")
            .group("shutter")
            .flex_none()
            .size(dp(SHUTTER))
            .rounded_full()
            .border(dp(RING))
            .border_color(solid(foreground))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(div().id("release").child(disc).when(ready && self.countdown.is_none(), |disc| {
                disc.group_active("shutter", |disc| disc.size(dp(RELEASE_PRESSED)))
            }))
            .on_click(cx.listener(|this, _, _, cx| this.release(cx)));
        let turn = self.turn.map_or(0.0, |turn| decelerate(turn.progress(now)));
        let flip = self
            .round("flip", SIDE)
            .child(
                icon(Icon::Flip, dp(ICON), solid(foreground))
                    .with_transformation(Transformation::rotate(radians(std::f32::consts::PI * turn))),
            )
            .on_click(cx.listener(|this, _, _, cx| this.switch(cx)));
        let (top, bottom) = room;
        let side = |child: Option<Stateful<gpui::Div>>| div().flex_1().flex().justify_center().children(child);
        div()
            .absolute()
            .left(px(INSET))
            .right(px(INSET))
            .top(px(top))
            .h(px(bottom - top))
            .flex()
            .items_center()
            .child(side(last))
            .child(shutter)
            .child(side(Some(flip)))
    }

    /// A turning arc over the last photo while photos are developed.
    fn developing_arc(&self, radius: gpui::Pixels) -> impl IntoElement {
        let turn = epoch().elapsed().as_secs_f32() / SPIN.as_secs_f32();
        div()
            .absolute()
            .inset_0()
            .rounded(radius)
            .bg(color(0x000000, 0.45))
            .flex()
            .items_center()
            .justify_center()
            .child(
                icon(Icon::Progress, dp(ICON), solid(0xffffff))
                    .with_transformation(Transformation::rotate(radians(std::f32::consts::TAU * turn.fract()))),
            )
    }

    /// A chip a gutter under the preview: what was just done, or that
    /// photos are developing.
    fn notice(&self, top: f32, now: Instant) -> Option<impl IntoElement> {
        let text: SharedString = match &self.notice {
            Some((text, at)) if now - *at < NOTICE => text.clone(),
            _ if self.developing > 1 => fill("Developing {count} photos", &[("count", &self.developing.to_string())]).into(),
            _ if self.developing > 0 => tr("Developing").into(),
            _ => return None,
        };
        Some(
            div()
                .absolute()
                .top(px(top + INSET))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(chip(&self.look).bg(solid(self.look.palette.raised)).child(text)),
        )
    }

    fn camera_screen(&self, window: &Window, now: Instant, cx: &mut Context<Self>) -> impl IntoElement {
        let size = window.viewport_size();
        let height = f32::from(size.height);
        let bounds = self.preview_bounds(f32::from(size.width), height);
        let below = bounds.1 + bounds.3;
        let bar_top = ((bounds.1 - f32::from(dp(BAR))) / 2.0).max(INSET);
        div()
            .size_full()
            .relative()
            .child(self.top_bar(bar_top, cx))
            .child(self.preview(bounds, now, cx))
            .children(self.notice(below, now))
            .child(self.controls((below, height - GESTURE_INSET), now, cx))
    }
}

/// A zoom as a lens button shows it: ".6" and "2", and chosen "0.6×" and
/// "2×".
fn zoom_label(zoom: f32, chosen: bool) -> String {
    let number = if zoom.fract() == 0.0 { format!("{zoom}") } else { format!("{zoom:.1}") };
    if chosen {
        format!("{number}×")
    } else {
        number.trim_start_matches('0').to_string()
    }
}

/// A fixed instant for motion that only turns, such as the developing arc.
fn epoch() -> Instant {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

impl Render for Camera {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        if self.animating(now) {
            window.request_animation_frame();
        }
        let entered = self.entered.map_or(1.0, |entered| entered.progress(now));
        let screen = match self.screen {
            Screen::Camera => self.camera_screen(window, now, cx).into_any_element(),
            Screen::Viewer => self.viewer_screen(window, now, cx).into_any_element(),
            Screen::Grid => self.grid_screen(window, cx).into_any_element(),
        };
        div()
            .size_full()
            .relative()
            .key_context("Camera")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::back))
            .bg(solid(0x000000))
            .font(self.font.clone())
            .text_color(solid(self.look.palette.foreground))
            .line_height(relative(1.3))
            .child(div().size_full().opacity(entered).child(screen))
    }
}
