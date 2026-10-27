//! The Sensors window: one island per sensor steinerd offers, drawn to
//! Denial's mobile materials (Islands, denial/docs/MOBILE_APP_MATERIALS.md).
//! Orientation shows the phone turned as it is held; ambient light, the lux
//! on a meter and the last minute as a graph; proximity, rings that light
//! up while something is near. The sensors run while the window shows.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use async_channel::Sender;
use denial_look::mobile::{GROUP_HEIGHT, content_bottom, content_top, dp as dp_lp};
use denial_look::{INSET, Look, SettingsWatch, Tween, decelerate, ease_out_cubic, motion, stagger};
use denial_look_gpui::{backing, control_fill, dp, glass_group, island, say, solid};
use gpui::{
    App, AsyncApp, Context, Div, FocusHandle, Focusable, Font, FontWeight, SharedString,
    Subscription, Task, WeakEntity, Window, actions, canvas, div, prelude::*, px, relative,
};

use crate::draw::{self, Ink};
use crate::steiner::{Orientation, State};

actions!(sensors, [Back]);

/// The phone and the rings, in dp.
const DRAWING: f32 = 112.0;
/// An island's padding, in dp.
const PAD: f32 = 16.0;
/// The column's widest, in dp, for a window turned to landscape.
const COLUMN: f32 = 560.0;
/// How often the light graph moves on while it shows.
const GRAPH_TICK: Duration = Duration::from_millis(250);
/// The light meter's top, in decades of lux above 1: 100,000 lx is direct
/// sun.
const METER_DECADES: f64 = 5.0;

/// A value that glides to where it was last set.
#[derive(Clone, Copy)]
struct Glide {
    from: f32,
    to: f32,
    motion: Tween,
}

impl Glide {
    fn new(value: f32, look: &Look) -> Self {
        Self {
            from: value,
            to: value,
            motion: look.tween(0),
        }
    }

    fn at(&self, now: Instant) -> f32 {
        denial_look::lerp(self.from, self.to, decelerate(self.motion.progress(now)))
    }

    fn set(&mut self, to: f32, millis: u64, look: &Look, now: Instant) {
        if to != self.to {
            *self = Self {
                from: self.at(now),
                to,
                motion: look.tween(millis),
            };
        }
    }
}

/// steinerd as the window knows it.
enum Service {
    /// Not asked yet.
    Looking,
    /// Not on the system bus.
    Missing,
    Running(State),
}

pub struct Sensors {
    look: Look,
    font: Font,
    focus_handle: FocusHandle,
    service: Service,
    /// The islands rising in, from steinerd's first answer.
    launch: Option<Tween>,
    /// Degrees the drawn phone is turned.
    turn: Glide,
    /// The light meter's fill, 0 to 1.
    meter: Glide,
    /// 1 while something is near.
    near: Glide,
    /// The light readings of the last minute, with when they came, and the
    /// one before them.
    history: VecDeque<(Instant, f64)>,
    /// Whether the window is being shown: the sensors run only then.
    visible: bool,
    shown: Sender<bool>,
    _settings: Task<()>,
    _steiner: Task<()>,
    _graph: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for Sensors {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Sensors {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let look = Look::load();
        let font = gpui::font(look.font_family(&cx.text_system().all_font_names()));
        let (shown, showing) = async_channel::unbounded();
        let (updates, received) = async_channel::unbounded();
        let visible = window.visibility().is_visible();
        let _ = shown.try_send(visible);
        let steiner = cx.spawn(async move |this, cx| {
            let _follow = cx.background_spawn(crate::steiner::follow(updates, showing));
            while let Ok(update) = received.recv().await {
                if this.update(cx, |view, cx| view.take(update, cx)).is_err() {
                    return;
                }
            }
        });
        let subscriptions =
            vec![
                cx.observe_window_visibility(window, |this, visibility, _, cx| {
                    this.visible = visibility.is_visible();
                    let _ = this.shown.try_send(this.visible);
                    cx.notify();
                }),
            ];
        Self {
            turn: Glide::new(0.0, &look),
            meter: Glide::new(0.0, &look),
            near: Glide::new(0.0, &look),
            look,
            font,
            focus_handle: cx.focus_handle(),
            service: Service::Looking,
            launch: None,
            history: VecDeque::new(),
            visible,
            shown,
            _settings: cx.spawn(async move |this, cx| Self::follow_settings(this, cx).await),
            _steiner: steiner,
            _graph: cx.spawn(async move |this, cx| Self::tick_graph(this, cx).await),
            _subscriptions: subscriptions,
        }
    }

    /// Takes Denial's look afresh whenever its settings change.
    async fn follow_settings(this: WeakEntity<Self>, cx: &mut AsyncApp) {
        let mut watch = match SettingsWatch::new() {
            Ok(watch) => watch,
            Err(error) => return eprintln!("sensors: following Denial's settings: {error}"),
        };
        loop {
            if let Err(error) = watch.changed().await {
                return eprintln!("sensors: following Denial's settings: {error}");
            }
            let look = cx.background_spawn(async { Look::load() }).await;
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

    /// Redraws the light graph so it moves on while it shows, as readings
    /// come only when the light changes.
    async fn tick_graph(this: WeakEntity<Self>, cx: &mut AsyncApp) {
        loop {
            async_io::Timer::after(GRAPH_TICK).await;
            let ticked = this.update(cx, |view, cx| {
                if view.visible && matches!(&view.service, Service::Running(s) if s.has_light) {
                    cx.notify();
                }
            });
            if ticked.is_err() {
                return;
            }
        }
    }

    /// What steinerd reports now, or None while it does not run.
    fn take(&mut self, update: Option<State>, cx: &mut Context<Self>) {
        let now = Instant::now();
        if self.launch.is_none() {
            self.launch = Some(self.look.tween(0));
        }
        let Some(state) = update else {
            self.service = Service::Missing;
            self.history.clear();
            return cx.notify();
        };
        if state.has_light {
            if self
                .history
                .back()
                .is_none_or(|&(_, lux)| lux != state.light)
            {
                self.history.push_back((now, state.light));
            }
            // One reading from before the minute is kept: it was in force
            // at the graph's left edge.
            while self.history.len() > 1 && self.history[1].0 <= now - draw::HISTORY {
                self.history.pop_front();
            }
        }
        let meter = ((state.light.max(1.0).log10() / METER_DECADES) as f32).clamp(0.0, 1.0);
        self.meter.set(meter, motion::LAUNCH, &self.look, now);
        self.near.set(
            if state.near { 1.0 } else { 0.0 },
            motion::IN,
            &self.look,
            now,
        );
        if state.orientation != Orientation::Undefined {
            // The short way round.
            let mut turn = state.orientation.turn();
            while turn - self.turn.to > 180.0 {
                turn -= 360.0;
            }
            while turn - self.turn.to < -180.0 {
                turn += 360.0;
            }
            self.turn.set(turn, motion::LAUNCH, &self.look, now);
        }
        self.service = Service::Running(state);
        cx.notify();
    }

    /// Denial's Back gesture and Escape: Sensors has one screen, so the
    /// window closes.
    fn back(&mut self, _: &Back, window: &mut Window, _: &mut Context<Self>) {
        window.remove_window();
    }

    fn animating(&self, now: Instant) -> bool {
        let launching = self.launch.is_some_and(|launch| {
            let end = stagger(motion::LAUNCH_DELAY, motion::LAUNCH_STEP, 3) + motion::LAUNCH;
            launch.segment(end, 0).running(now)
        });
        launching
            || [self.turn, self.meter, self.near]
                .iter()
                .any(|glide| glide.motion.running(now))
    }

    fn ink(&self) -> Ink {
        Ink {
            foreground: solid(self.look.palette.foreground),
            secondary: solid(self.look.palette.secondary),
            control: control_fill(&self.look),
            accent: solid(self.look.accent),
        }
    }

    /// Island `index`, rising and fading in at launch.
    fn place(&self, index: usize, island: Div, now: Instant) -> Div {
        let t = self.launch.map_or(1.0, |launch| {
            let delay = stagger(motion::LAUNCH_DELAY, motion::LAUNCH_STEP, index);
            ease_out_cubic(launch.segment(delay, motion::LAUNCH).progress(now))
        });
        div()
            .w_full()
            .max_w(dp(COLUMN))
            .relative()
            .top(px(dp_lp(motion::LAUNCH_RISE) * (1.0 - t)))
            .opacity(t)
            .child(island)
    }

    /// The name of an island, inside it, in accent text.
    fn label(&self, label: &'static str) -> Div {
        div()
            .text_size(dp(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(solid(self.look.accent_text))
            .child(say(label).to_uppercase())
    }

    /// What a sensor reads, large.
    fn reading(&self, text: impl Into<SharedString>) -> Div {
        div()
            .text_size(dp(26.))
            .line_height(relative(1.2))
            .font_weight(FontWeight::SEMIBOLD)
            .child(text.into())
    }

    fn secondary(&self, text: impl Into<SharedString>, size: f32) -> Div {
        div()
            .text_size(dp(size))
            .text_color(solid(self.look.palette.secondary))
            .child(text.into())
    }

    /// A drawing on the left, the label and the reading on the right.
    fn drawn(
        &self,
        drawing: impl IntoElement,
        label: &'static str,
        reading: &'static str,
        detail: &'static str,
    ) -> Div {
        island(&self.look)
            .p(dp(PAD))
            .flex()
            .items_center()
            .gap(dp(PAD))
            .child(div().flex_none().size(dp(DRAWING)).child(drawing))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(dp(4.))
                    .child(self.label(label))
                    .child(self.reading(say(reading)))
                    .child(self.secondary(say(detail), 15.)),
            )
    }

    fn orientation(&self, orientation: Orientation, now: Instant) -> Div {
        let (reading, detail) = match orientation {
            Orientation::Undefined => ("No reading yet", "Tilt the phone to take one"),
            Orientation::Normal => ("Upright", "Portrait"),
            Orientation::BottomUp => ("Upside down", "Portrait"),
            Orientation::LeftUp => ("Left edge up", "Landscape"),
            Orientation::RightUp => ("Right edge up", "Landscape"),
        };
        let (turn, ink) = (self.turn.at(now), self.ink());
        let faint = orientation == Orientation::Undefined;
        let phone = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| draw::phone(bounds, turn, ink, window),
        )
        .size_full();
        self.drawn(
            div()
                .size_full()
                .when(faint, |d| d.opacity(0.4))
                .child(phone),
            "Orientation",
            reading,
            detail,
        )
    }

    fn proximity(&self, near: bool, now: Instant) -> Div {
        let (reading, detail) = if near {
            ("Near", "Something covers the sensor")
        } else {
            ("Clear", "Nothing in front of the sensor")
        };
        let (near, ink) = (self.near.at(now), self.ink());
        let rings = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| draw::rings(bounds, near, ink, window),
        )
        .size_full();
        self.drawn(rings, "Proximity", reading, detail)
    }

    /// The lux and what light that is, the meter from dark to direct sun,
    /// and the last minute.
    fn light(&self, lux: f64, now: Instant) -> Div {
        let ink = self.ink();
        let history = self.history.clone();
        let graph = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| draw::light(bounds, &history, now, ink, window),
        )
        .size_full();
        let ends = |left: &'static str, right: &'static str| {
            div()
                .flex()
                .justify_between()
                .child(self.secondary(say(left), 12.))
                .child(self.secondary(say(right), 12.))
        };
        island(&self.look)
            .p(dp(PAD))
            .flex()
            .flex_col()
            .gap(dp(10.))
            .child(self.label("Ambient light"))
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .gap(dp(12.))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(dp(6.))
                            .child(
                                div()
                                    .text_size(dp(44.))
                                    .line_height(relative(1.1))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(lux_text(lux)),
                            )
                            .child(self.secondary("lx", 20.)),
                    )
                    .child(
                        div()
                            .pb(dp(6.))
                            .text_size(dp(17.))
                            .truncate()
                            .child(say(kind_of_light(lux))),
                    ),
            )
            .child(
                div()
                    .relative()
                    .h(dp(8.))
                    .rounded_full()
                    .bg(control_fill(&self.look))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left_0()
                            .w(relative(self.meter.at(now).max(0.02)))
                            .rounded_full()
                            .bg(solid(self.look.accent)),
                    ),
            )
            .child(ends("Dark", "Direct sun"))
            .child(div().mt(dp(6.)).h(dp(96.)).child(graph))
            .child(ends("1 min ago", "Now"))
    }

    /// One island saying why there is nothing to show.
    fn blocked(&self, title: &'static str, detail: &'static str) -> Div {
        island(&self.look)
            .p(dp(PAD))
            .flex()
            .flex_col()
            .gap(dp(4.))
            .child(
                div()
                    .text_size(dp(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(say(title)),
            )
            .child(self.secondary(say(detail), 15.))
    }

    fn islands(&self, now: Instant) -> Vec<Div> {
        let state = match &self.service {
            Service::Looking => return Vec::new(),
            Service::Missing => {
                return vec![self.blocked(
                    "The sensor service is not running",
                    "Readings show here once steinerd starts.",
                )];
            }
            Service::Running(state) => state,
        };
        let mut islands = Vec::new();
        if state.has_orientation {
            islands.push(self.orientation(state.orientation, now));
        }
        if state.has_light {
            islands.push(self.light(state.light, now));
        }
        if state.has_proximity {
            islands.push(self.proximity(state.near, now));
        }
        if islands.is_empty() {
            islands.push(self.blocked(
                "No sensors found",
                "steinerd reaches no orientation, light or proximity sensor.",
            ));
        }
        islands
    }

    fn top_group(&self) -> Div {
        let fade = self.launch.map_or(0.0, |launch| {
            launch
                .segment(0, motion::GROUP_FADE)
                .progress(Instant::now())
        });
        div()
            .absolute()
            .top(px(INSET))
            .left(px(INSET))
            .right(px(INSET))
            .h(dp(GROUP_HEIGHT))
            .child(
                glass_group(&self.look)
                    .size_full()
                    .flex()
                    .items_center()
                    .px(dp(17.))
                    .child(
                        div()
                            .opacity(fade)
                            .text_size(dp(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(say("Sensors")),
                    ),
            )
    }
}

impl Render for Sensors {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        if self.animating(now) {
            window.request_animation_frame();
        }
        let content = div()
            .id("content")
            .size_full()
            .overflow_y_scroll()
            .px(px(INSET))
            .pt(px(content_top()))
            .pb(px(content_bottom(0.0, 0.0, 0.0)))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(INSET))
            .children(
                self.islands(now)
                    .into_iter()
                    .enumerate()
                    .map(|(ix, island)| self.place(ix, island, now)),
            );
        div()
            .size_full()
            .relative()
            .key_context("Sensors")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::back))
            .bg(backing(&self.look))
            .font(self.font.clone())
            .text_color(solid(self.look.palette.foreground))
            // Above the top group the gutter stays on the backing, which
            // Denial paints the status strip with.
            .child(
                div()
                    .absolute()
                    .top(px(INSET))
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .child(content),
            )
            .child(self.top_group())
    }
}

/// Lux as people read it: a decimal while it is dim, whole lux after, in
/// thousands once bright.
fn lux_text(lux: f64) -> String {
    if lux < 10.0 {
        return format!("{lux:.1}");
    }
    let whole = format!("{:.0}", lux);
    let mut grouped = String::new();
    for (ix, digit) in whole.chars().enumerate() {
        if ix > 0 && (whole.len() - ix) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// What light `lux` is, roughly.
fn kind_of_light(lux: f64) -> &'static str {
    match lux {
        l if l < 1.0 => "Dark",
        l if l < 20.0 => "Dim",
        l if l < 200.0 => "Indoors",
        l if l < 1_000.0 => "Bright indoors",
        l if l < 10_000.0 => "Daylight",
        _ => "Direct sun",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lux_reads() {
        assert_eq!(lux_text(0.0), "0.0");
        assert_eq!(lux_text(5.84), "5.8");
        assert_eq!(lux_text(312.4), "312");
        assert_eq!(lux_text(12_400.0), "12,400");
        assert_eq!(lux_text(104_000.0), "104,000");
    }

    #[test]
    fn words_are_translated() {
        for word in [
            "Sensors",
            "Orientation",
            "Ambient light",
            "Proximity",
            kind_of_light(0.5),
            kind_of_light(10.0),
            kind_of_light(100.0),
            kind_of_light(500.0),
            kind_of_light(5_000.0),
            kind_of_light(50_000.0),
        ] {
            assert!(
                crate::words::CHINESE
                    .iter()
                    .any(|(english, _)| *english == word),
                "{word}"
            );
        }
    }
}
