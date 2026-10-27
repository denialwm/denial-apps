//! The drawings inside the islands, painted as paths: the phone turned as it
//! is held, the proximity rings, and the light over the last minute.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use gpui::{
    Bounds, Hsla, PathBuilder, Pixels, Point, linear_color_stop, linear_gradient, point, px,
};

/// How much of the past the light graph shows.
pub const HISTORY: Duration = Duration::from_secs(60);
/// The light graph's lowest level, in lux: darker reads as this.
const FLOOR_LUX: f64 = 0.1;

/// The colours a drawing takes from the look.
#[derive(Clone, Copy)]
pub struct Ink {
    pub foreground: Hsla,
    pub secondary: Hsla,
    pub control: Hsla,
    pub accent: Hsla,
}

/// A point `x`, `y` lp from the centre of a shape turned `turn` degrees
/// clockwise about `center`.
fn turned(center: Point<Pixels>, turn: f32, x: f32, y: f32) -> Point<Pixels> {
    let (sin, cos) = turn.to_radians().sin_cos();
    point(
        center.x + px(x * cos - y * sin),
        center.y + px(x * sin + y * cos),
    )
}

/// A rounded rectangle `w` by `h` lp, its corners `r`, centred `x`, `y` lp
/// from `center` before the turn.
fn rounded(
    builder: &mut PathBuilder,
    center: Point<Pixels>,
    turn: f32,
    (x, y, w, h, r): (f32, f32, f32, f32, f32),
) {
    let at = |dx: f32, dy: f32| turned(center, turn, x + dx, y + dy);
    let (left, right, top, bottom) = (-w / 2., w / 2., -h / 2., h / 2.);
    let radii = point(px(r), px(r));
    let corner = |builder: &mut PathBuilder, to: Point<Pixels>| {
        builder.arc_to(radii, px(0.), false, true, to)
    };
    builder.move_to(at(left + r, top));
    builder.line_to(at(right - r, top));
    corner(builder, at(right, top + r));
    builder.line_to(at(right, bottom - r));
    corner(builder, at(right - r, bottom));
    builder.line_to(at(left + r, bottom));
    corner(builder, at(left, bottom - r));
    builder.line_to(at(left, top + r));
    corner(builder, at(left + r, top));
    builder.close();
}

fn circle(builder: &mut PathBuilder, center: Point<Pixels>, r: f32) {
    let radii = point(px(r), px(r));
    builder.move_to(point(center.x + px(r), center.y));
    builder.arc_to(
        radii,
        px(0.),
        false,
        true,
        point(center.x - px(r), center.y),
    );
    builder.arc_to(
        radii,
        px(0.),
        false,
        true,
        point(center.x + px(r), center.y),
    );
    builder.close();
}

/// The phone, turned `turn` degrees clockwise from upright, in the middle
/// of `bounds`: its body, its screen, and the earpiece and camera that
/// mark its top edge.
pub fn phone(bounds: Bounds<Pixels>, turn: f32, ink: Ink, window: &mut gpui::Window) {
    let center = bounds.center();
    let side = f32::from(bounds.size.width.min(bounds.size.height));
    let h = side * 0.84;
    let w = h * 0.5;
    let line = (side / 64.).max(1.5);

    let mut screen = PathBuilder::fill();
    rounded(&mut screen, center, turn, (0., 0., w, h, w * 0.2));
    if let Ok(path) = screen.build() {
        window.paint_path(path, ink.control);
    }
    let mut body = PathBuilder::stroke(px(line));
    rounded(&mut body, center, turn, (0., 0., w, h, w * 0.2));
    if let Ok(path) = body.build() {
        window.paint_path(path, ink.foreground);
    }
    let mut earpiece = PathBuilder::fill();
    let slot = w * 0.07;
    rounded(
        &mut earpiece,
        center,
        turn,
        (0., -h / 2. + w * 0.13, w * 0.3, slot, slot / 2.),
    );
    if let Ok(path) = earpiece.build() {
        window.paint_path(path, ink.foreground);
    }
    let mut camera = PathBuilder::fill();
    circle(
        &mut camera,
        turned(center, turn, w * 0.27, -h / 2. + w * 0.13),
        w * 0.06,
    );
    if let Ok(path) = camera.build() {
        window.paint_path(path, ink.accent);
    }
}

/// The proximity sensor as rings around a dot: faint while nothing is near;
/// as `near` goes to 1 the rings take the accent and the dot grows over
/// them.
pub fn rings(bounds: Bounds<Pixels>, near: f32, ink: Ink, window: &mut gpui::Window) {
    let center = bounds.center();
    let radius = f32::from(bounds.size.width.min(bounds.size.height)) / 2. - 2.;
    let line = (radius / 28.).max(1.5);
    for (ix, share) in [0.42f32, 0.71, 1.0].into_iter().enumerate() {
        let mut ring = PathBuilder::stroke(px(line));
        circle(&mut ring, center, radius * share - line / 2.);
        if let Ok(path) = ring.build() {
            // The inner rings light first.
            let lit = (near * 1.6 - ix as f32 * 0.3).clamp(0., 1.);
            let color = mix(ink.secondary.opacity(0.35), ink.accent, lit);
            window.paint_path(path, color);
        }
    }
    let mut dot = PathBuilder::fill();
    circle(
        &mut dot,
        center,
        radius * denial_look::lerp(0.14, 0.32, near),
    );
    if let Ok(path) = dot.build() {
        window.paint_path(path, mix(ink.secondary, ink.accent, near));
    }
}

fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b) = (a.to_rgb(), b.to_rgb());
    gpui::Rgba {
        r: denial_look::lerp(a.r, b.r, t),
        g: denial_look::lerp(a.g, b.g, t),
        b: denial_look::lerp(a.b, b.b, t),
        a: denial_look::lerp(a.a, b.a, t),
    }
    .into()
}

/// Where `lux` sits on a log scale: decades above `FLOOR_LUX`.
fn level(lux: f64) -> f32 {
    (lux.max(FLOOR_LUX) / FLOOR_LUX).log10() as f32
}

/// The light over `HISTORY` up to `now`, each reading held until the next,
/// on a log scale fitted to what the minute saw (a decade at least):
/// a line in the accent over a fill that fades down.
pub fn light(
    bounds: Bounds<Pixels>,
    history: &VecDeque<(Instant, f64)>,
    now: Instant,
    ink: Ink,
    window: &mut gpui::Window,
) {
    let Some(&(_, last)) = history.back() else {
        return;
    };
    let start = now - HISTORY;
    // The reading in force at the window's left edge, then the ones after.
    let first = history
        .partition_point(|(at, _)| *at <= start)
        .saturating_sub(1);
    let shown = || history.range(first..);
    let (low, high) = shown().fold((f32::MAX, f32::MIN), |(low, high), &(_, lux)| {
        (low.min(level(lux)), high.max(level(lux)))
    });
    let middle = (low + high) / 2.;
    let span = (high - low).max(1.) * 1.2;
    let (low, high) = (middle - span / 2., middle + span / 2.);

    let line = 2.;
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height) - line;
    let x = |at: Instant| {
        let age = now.saturating_duration_since(at.max(start)).as_secs_f32();
        bounds.origin.x + px(width * (1. - age / HISTORY.as_secs_f32()))
    };
    let y = |lux: f64| {
        bounds.origin.y + px(line / 2. + height * (1. - (level(lux) - low) / (high - low)))
    };
    let mut points = Vec::new();
    for (ix, &(at, lux)) in shown().enumerate() {
        if ix > 0 {
            // Held flat until this reading came.
            let held = points.last().map(|p: &Point<Pixels>| p.y).unwrap_or(y(lux));
            points.push(point(x(at), held));
        }
        points.push(point(x(at), y(lux)));
    }
    points.push(point(x(now), y(last)));

    let mut fill = PathBuilder::fill();
    fill.move_to(point(points[0].x, bounds.bottom()));
    for p in &points {
        fill.line_to(*p);
    }
    fill.line_to(point(x(now), bounds.bottom()));
    fill.close();
    if let Ok(path) = fill.build() {
        window.paint_path(
            path,
            linear_gradient(
                180.,
                linear_color_stop(ink.accent.opacity(0.32), 0.),
                linear_color_stop(ink.accent.opacity(0.), 1.),
            ),
        );
    }
    let mut stroke = PathBuilder::stroke(px(line));
    stroke.move_to(points[0]);
    for p in &points[1..] {
        stroke.line_to(*p);
    }
    if let Ok(path) = stroke.build() {
        window.paint_path(path, ink.accent);
    }
}
