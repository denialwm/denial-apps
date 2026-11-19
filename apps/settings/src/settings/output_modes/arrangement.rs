//! The display map edits output positions in the same draft as the mode controls.

use std::rc::Rc;

use denial_look::words::tr;
use denial_look::{Appearance, INSET, PRESSED_FILL};
use denial_look_gpui::{Icon, color, control_fill, control_radius, icon, island, solid};
use gpui::{
    Context, CursorStyle, Div, Focusable, HitboxBehavior, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, Stateful, TouchDragEvent, TouchPhase,
    Window, canvas, div, prelude::*, px,
};

use super::{ConfiguredOutput, Dialog, OutputConfiguration, Settings};

const PAD: f32 = 10.0;
const FIT_PAD: f64 = 24.0;
const SNAP: f64 = 10.0;
const MIN_SCALE: f64 = 1e-9;
const MAX_SCALE: f64 = 0.60;

type Pair = [f64; 2];

#[derive(Clone, Copy)]
struct View {
    centre: Pair,
    scale: f64,
}

impl View {
    fn in_view(self, world: Pair, viewport: Pair) -> Pair {
        [
            viewport[0] / 2.0 + (world[0] - self.centre[0]) * self.scale,
            viewport[1] / 2.0 + (world[1] - self.centre[1]) * self.scale,
        ]
    }
}

#[derive(Clone)]
struct Tile {
    name: String,
    label: String,
    origin: Pair,
    size: Pair,
    /// An off display shows where it would be, to be chosen and turned on.
    enabled: bool,
}

impl Tile {
    fn of(output: &ConfiguredOutput) -> Option<Self> {
        let mode = output.mode()?;
        let swaps_axes = matches!(
            output.transform.as_str(),
            "90" | "270" | "flipped-90" | "flipped-270"
        );
        let (width, height) = if swaps_axes {
            (mode.height, mode.width)
        } else {
            (mode.width, mode.height)
        };
        if !output.scale.is_finite() || output.scale <= 0.0 {
            return None;
        }
        Some(Self {
            name: output.name.clone(),
            label: output.label(),
            origin: [f64::from(output.x), f64::from(output.y)],
            size: [
                (f64::from(width) / output.scale).round().max(1.0),
                (f64::from(height) / output.scale).round().max(1.0),
            ],
            enabled: output.enabled,
        })
    }

    fn contains(&self, point: Pair, view: View, viewport: Pair) -> bool {
        let origin = view.in_view(self.origin, viewport);
        (0..2).all(|axis| {
            point[axis] >= origin[axis] && point[axis] < origin[axis] + self.size[axis] * view.scale
        })
    }
}

#[derive(Default)]
pub(super) struct Arrangement {
    view: Option<View>,
    drag: Option<Drag>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Device {
    Mouse,
    Touch,
}

struct Drag {
    device: Device,
    start: Pair,
    view: View,
    monitor: Option<Tile>,
    position: Option<[i32; 2]>,
}

fn coordinates(point: Point<Pixels>) -> Pair {
    [f64::from(f32::from(point.x)), f64::from(f32::from(point.y))]
}

fn fit(tiles: &[Tile], viewport: Pair, maximum: f64) -> View {
    let mut lower = [0.0_f64; 2];
    let mut upper = [0.0_f64; 2];
    for tile in tiles {
        for axis in 0..2 {
            lower[axis] = lower[axis].min(tile.origin[axis]);
            upper[axis] = upper[axis].max(tile.origin[axis] + tile.size[axis]);
        }
    }
    if tiles.is_empty() {
        return View {
            centre: [0.0, 0.0],
            scale: maximum,
        };
    }
    let sx = (viewport[0] - FIT_PAD * 2.0).max(1.0) / (upper[0] - lower[0]).max(1.0);
    let sy = (viewport[1] - FIT_PAD * 2.0).max(1.0) / (upper[1] - lower[1]).max(1.0);
    View {
        centre: [(lower[0] + upper[0]) / 2.0, (lower[1] + upper[1]) / 2.0],
        scale: sx.min(sy).clamp(MIN_SCALE, maximum),
    }
}

fn snap_axis(candidate: f64, size: f64, edges: impl Iterator<Item = f64>, scale: f64) -> i32 {
    let maximum = f64::from(i32::MAX) - size;
    let candidate = candidate.clamp(0.0, maximum.max(0.0));
    edges
        .flat_map(|edge| [edge - size, edge])
        .filter(|position| (0.0..=maximum).contains(position))
        .filter(|position| (*position - candidate).abs() * scale <= SNAP)
        .min_by(|a, b| (a - candidate).abs().total_cmp(&(b - candidate).abs()))
        .unwrap_or(candidate)
        .round() as i32
}

fn overlaps(origin: Pair, size: Pair, other: &Tile) -> bool {
    (0..2).all(|axis| {
        origin[axis] < other.origin[axis] + other.size[axis]
            && origin[axis] + size[axis] > other.origin[axis]
    })
}

fn constrain_drag(moving: &Tile, mut position: Pair, target: Pair, tiles: &[Tile]) -> Pair {
    let others = || tiles.iter().filter(|tile| tile.name != moving.name);
    for axis in [0, 1] {
        let cross = 1 - axis;
        let from = position[axis];
        let mut to = target[axis];
        for other in others() {
            if position[cross] >= other.origin[cross] + other.size[cross]
                || position[cross] + moving.size[cross] <= other.origin[cross]
            {
                continue;
            }
            let before = other.origin[axis] - moving.size[axis];
            let after = other.origin[axis] + other.size[axis];
            if from <= before {
                to = to.min(before);
            } else if from >= after {
                to = to.max(after);
            }
        }
        position[axis] = to;
    }
    position
}

struct ArrangementTooltip {
    label: &'static str,
    background: gpui::Hsla,
    foreground: gpui::Hsla,
}

impl Render for ArrangementTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(4.))
            .bg(self.background)
            .text_color(self.foreground)
            .text_size(px(12.))
            .child(denial_look_gpui::say(self.label))
    }
}

impl Settings {
    pub(super) fn output_arrangement(
        &self,
        configuration: &OutputConfiguration,
        selected: &str,
        width: f32,
        live: bool,
        edit: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut tiles: Vec<_> = configuration.outputs.iter().filter_map(Tile::of).collect();
        if tiles.is_empty() {
            return island(&self.look);
        }
        // Off displays draw under the lit ones, and the selected monitor of
        // each draws last, as if raised above any overlapping tile.
        tiles.sort_by_key(|tile| (tile.enabled, tile.name == selected));
        let width = (width - 2.0 * INSET - 2.0 * PAD).max(64.0);
        let height: f32 = if width >= 550.0 { 280.0 } else { 220.0 };
        let viewport = [f64::from(width), f64::from(height)];
        let fitted = fit(&tiles, viewport, MAX_SCALE);
        let arrangement = &self.output_modes.arrangement;
        let view = arrangement
            .view
            .unwrap_or_else(|| fit(&tiles, viewport, 0.15));
        let preview = arrangement
            .drag
            .as_ref()
            .and_then(|drag| Some((drag.monitor.as_ref()?.name.as_str(), drag.position?)));
        let [origin_x, origin_y] = view.in_view([0.0, 0.0], viewport);
        let workspace_left = (origin_x as f32).max(0.);
        let workspace_top = (origin_y as f32).max(0.);
        let (canvas_fill, workspace_fill) = match self.look.appearance {
            Appearance::Dark => (
                control_fill(&self.look),
                color(self.look.palette.foreground, 0.08),
            ),
            Appearance::Light => (
                color(self.look.palette.foreground, 0.16),
                color(0xFFFFFF, 0.30),
            ),
        };
        let mut selected_position = None;
        let diagram = tiles
            .iter()
            .map(|tile| {
                let origin = preview
                    .filter(|(name, _)| *name == tile.name)
                    .map_or(tile.origin, |(_, position)| position.map(f64::from));
                let [left, top] = view.in_view(origin, viewport);
                let chosen = tile.name == selected;
                if chosen {
                    selected_position = Some(format!(
                        "{}  ·  X: {}  Y: {}",
                        tile.label, origin[0], origin[1]
                    ));
                }
                let width = tile.size[0] * view.scale;
                let height = tile.size[1] * view.scale;
                div()
                    .absolute()
                    .left(px(left as f32))
                    .top(px(top as f32))
                    .w(px(width as f32))
                    .h(px(height as f32))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(if chosen {
                        solid(self.look.accent)
                    } else {
                        solid(self.look.palette.secondary)
                    })
                    .bg(if chosen {
                        color(self.look.accent, 0.30)
                    } else {
                        color(self.look.palette.foreground, PRESSED_FILL)
                    })
                    .text_size(px(12.))
                    .text_color(solid(self.look.palette.foreground))
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .when(!tile.enabled, |tile_box| tile_box.opacity(0.55))
                    .when(width >= 38.0 && height >= 15.0, |tile_box| {
                        let label = if tile.enabled {
                            tile.label.clone()
                        } else {
                            format!("{} · {}", tile.label, tr("Off"))
                        };
                        tile_box.child(div().truncate().px(px(4.)).child(label))
                    })
            })
            .collect::<Vec<_>>();
        let active = live && self.dialog.is_none() && !self.output_modes.applying;
        let controls = div().flex().flex_none().gap(px(6.)).children([
            self.arrangement_button(
                "arrangement-zoom-out",
                "Zoom out",
                Icon::Minus,
                active && view.scale > MIN_SCALE,
                cx,
                move |this, _, cx| {
                    this.set_arrangement_view(
                        View {
                            scale: (view.scale / 1.25).max(MIN_SCALE),
                            ..view
                        },
                        cx,
                    )
                },
            ),
            self.arrangement_button(
                "arrangement-fit",
                "Fit",
                Icon::FitScreen,
                active,
                cx,
                move |this, _, cx| this.set_arrangement_view(fitted, cx),
            ),
            self.arrangement_button(
                "arrangement-zoom-in",
                "Zoom in",
                Icon::Plus,
                active && view.scale < MAX_SCALE,
                cx,
                move |this, _, cx| {
                    this.set_arrangement_view(
                        View {
                            scale: (view.scale * 1.25).min(MAX_SCALE),
                            ..view
                        },
                        cx,
                    )
                },
            ),
        ]);
        island(&self.look)
            .p(px(PAD))
            .flex()
            .flex_col()
            .gap(px(PAD))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(height))
                    .overflow_hidden()
                    .rounded(px(6.))
                    .bg(canvas_fill)
                    .when(workspace_left < width && workspace_top < height, |map| {
                        map.child(
                            div()
                                .absolute()
                                .left(px(workspace_left))
                                .top(px(workspace_top))
                                .right_0()
                                .bottom_0()
                                .bg(workspace_fill),
                        )
                    })
                    .children(diagram)
                    .when(active, |diagram| {
                        diagram.child(
                            self.arrangement_events(Rc::new(tiles), view, viewport, edit, cx)
                                .absolute()
                                .inset_0(),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(PAD))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(solid(self.look.palette.foreground))
                            .child(selected_position.unwrap_or_default()),
                    )
                    .child(controls),
            )
    }

    fn arrangement_button(
        &self,
        id: &'static str,
        label: &'static str,
        glyph: Icon,
        enabled: bool,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let button = div()
            .id(id)
            .size(px(34.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(control_radius(&self.look))
            .bg(control_fill(&self.look))
            .child(icon(glyph, px(18.), solid(self.look.palette.foreground)));
        if enabled {
            let background = solid(self.look.palette.raised);
            let foreground = solid(self.look.palette.foreground);
            self.tappable(button, cx, action).tooltip(move |_, cx| {
                cx.new(|_| ArrangementTooltip {
                    label,
                    background,
                    foreground,
                })
                .into()
            })
        } else {
            button.opacity(0.45)
        }
    }

    fn arrangement_events(
        &self,
        tiles: Rc<Vec<Tile>>,
        view: View,
        viewport: Pair,
        edit: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Canvas<gpui::Hitbox> {
        let entity = cx.weak_entity();
        let dragging = self.output_modes.arrangement.drag.is_some();
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds, hitbox, window, _| {
                window.set_cursor_style(
                    if dragging {
                        CursorStyle::ClosedHand
                    } else {
                        CursorStyle::OpenHand
                    },
                    &hitbox,
                );
                let begin = {
                    let tiles = tiles.clone();
                    move |settings: &mut Settings,
                          device,
                          position: Point<Pixels>,
                          cx: &mut Context<Settings>| {
                        let point = coordinates(position - bounds.origin);
                        let tile = tiles
                            .iter()
                            .rev()
                            .find(|tile| tile.contains(point, view, viewport));
                        settings.begin_arrangement_drag(
                            device,
                            coordinates(position),
                            view,
                            tile,
                            edit,
                            cx,
                        );
                    }
                };
                let touch_begin = begin.clone();
                let touch_entity = entity.clone();
                let touch_tiles = tiles.clone();
                let touch_hitbox = hitbox.clone();
                window.on_mouse_event(move |event: &TouchDragEvent, phase, window, cx| {
                    if !phase.bubble()
                        || (event.phase == TouchPhase::Started && !touch_hitbox.is_hovered(window))
                    {
                        return;
                    }
                    let _ = touch_entity.update(cx, |settings, cx| match event.phase {
                        TouchPhase::Started => {
                            touch_begin(settings, Device::Touch, event.position, cx)
                        }
                        TouchPhase::Moved => settings.move_arrangement_drag(
                            Device::Touch,
                            coordinates(event.position),
                            &touch_tiles,
                            cx,
                        ),
                        TouchPhase::Ended => {
                            settings.finish_arrangement_drag(Device::Touch, true, cx)
                        }
                        TouchPhase::Cancelled => {
                            settings.finish_arrangement_drag(Device::Touch, false, cx)
                        }
                    });
                    if event.phase == TouchPhase::Started {
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                });
                let down_entity = entity.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                    if phase.bubble()
                        && event.button == MouseButton::Left
                        && hitbox.is_hovered(window)
                    {
                        let _ = down_entity.update(cx, |settings, cx| {
                            begin(settings, Device::Mouse, event.position, cx)
                        });
                    }
                });
                let move_entity = entity.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase.bubble() {
                        let _ = move_entity.update(cx, |settings, cx| {
                            if event.pressed_button == Some(MouseButton::Left) {
                                settings.move_arrangement_drag(
                                    Device::Mouse,
                                    coordinates(event.position),
                                    &tiles,
                                    cx,
                                );
                            } else {
                                settings.finish_arrangement_drag(Device::Mouse, false, cx);
                            }
                        });
                    }
                });
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase.bubble() && event.button == MouseButton::Left {
                        let _ = entity.update(cx, |settings, cx| {
                            settings.finish_arrangement_drag(Device::Mouse, true, cx)
                        });
                    }
                });
            },
        )
    }

    pub(super) fn choose_output_position(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(output) = self.output_modes.selected() else {
            return;
        };
        let name = output.name.clone();
        let position = format!("{}, {}", output.x, output.y);
        self.output_field
            .update(cx, |field, cx| field.set_text(position, cx));
        self.show(Dialog::OutputPosition { name }, cx);
        window.focus(&self.output_field.focus_handle(cx), cx);
    }

    pub(in crate::settings) fn output_position(
        &self,
        name: &str,
        cx: &Context<Self>,
    ) -> Option<[i32; 2]> {
        let text = self.output_field.read(cx).text();
        let (x, y) = text.split_once(',')?;
        let position = [x.trim().parse::<i32>().ok()?, y.trim().parse::<i32>().ok()?];
        let outputs = &self.output_modes.configuration()?.outputs;
        let output = outputs
            .iter()
            .find(|output| output.name == name && output.enabled)?;
        let tile = Tile::of(output)?;
        position[0].checked_add(tile.size[0] as i32)?;
        position[1].checked_add(tile.size[1] as i32)?;
        (!outputs
            .iter()
            .filter(|output| output.enabled && output.name != name)
            .filter_map(Tile::of)
            .any(|other| overlaps(position.map(f64::from), tile.size, &other)))
        .then_some(position)
    }

    pub(in crate::settings) fn set_output_position(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some([x, y]) = self.output_position(name, cx) {
            self.dismiss(window, cx);
            self.output_modes.arrangement.view = None;
            self.edit_output(
                name,
                |output| {
                    output.x = x;
                    output.y = y;
                },
                cx,
            );
        }
    }

    fn set_arrangement_view(&mut self, view: View, cx: &mut Context<Self>) {
        self.output_modes.arrangement.view = Some(view);
        self.output_modes.arrangement.drag = None;
        cx.notify();
    }

    fn begin_arrangement_drag(
        &mut self,
        device: Device,
        start: Pair,
        view: View,
        tile: Option<&Tile>,
        edit: bool,
        cx: &mut Context<Self>,
    ) {
        if self.output_modes.arrangement.drag.is_some() {
            return;
        }
        if let Some(tile) = tile {
            self.output_modes.selected = Some(tile.name.clone());
            // An off display is chosen, not moved: it has no place yet.
            if !edit || !tile.enabled {
                cx.notify();
                return;
            }
        }
        self.output_modes.arrangement.view = Some(view);
        self.output_modes.arrangement.drag = Some(Drag {
            device,
            start,
            view,
            monitor: tile.cloned(),
            position: None,
        });
        cx.notify();
    }

    fn move_arrangement_drag(
        &mut self,
        device: Device,
        point: Pair,
        tiles: &[Tile],
        cx: &mut Context<Self>,
    ) {
        // A lit display snaps to and stops at lit displays only.
        let lit = tiles
            .iter()
            .filter(|tile| tile.enabled)
            .cloned()
            .collect::<Vec<_>>();
        let tiles = lit.as_slice();
        let arrangement = &mut self.output_modes.arrangement;
        let Some(drag) = arrangement
            .drag
            .as_mut()
            .filter(|drag| drag.device == device)
        else {
            return;
        };
        let dx = point[0] - drag.start[0];
        let dy = point[1] - drag.start[1];
        if drag.monitor.is_some() && drag.position.is_none() && dx.abs().max(dy.abs()) < 3.0 {
            return;
        }
        let delta = [dx / drag.view.scale, dy / drag.view.scale];
        if let Some(monitor) = &drag.monitor {
            let target = std::array::from_fn(|axis| {
                let edges = std::iter::once(0.0).chain(
                    tiles
                        .iter()
                        .filter(|tile| tile.name != monitor.name)
                        .flat_map(|tile| [tile.origin[axis], tile.origin[axis] + tile.size[axis]]),
                );
                snap_axis(
                    monitor.origin[axis] + delta[axis],
                    monitor.size[axis],
                    edges,
                    drag.view.scale,
                )
            })
            .map(f64::from);
            let current = drag
                .position
                .map_or(monitor.origin, |position| position.map(f64::from));
            let position = constrain_drag(monitor, current, target, tiles);
            if !tiles
                .iter()
                .any(|other| other.name != monitor.name && overlaps(position, monitor.size, other))
            {
                drag.position = Some(position.map(|coordinate| coordinate as i32));
            }
        } else {
            arrangement.view = Some(View {
                centre: [
                    drag.view.centre[0] - delta[0],
                    drag.view.centre[1] - delta[1],
                ],
                ..drag.view
            });
        }
        cx.notify();
    }

    fn finish_arrangement_drag(&mut self, device: Device, commit: bool, cx: &mut Context<Self>) {
        let arrangement = &mut self.output_modes.arrangement;
        if !arrangement
            .drag
            .as_ref()
            .is_some_and(|drag| drag.device == device)
        {
            return;
        }
        if let Some(drag) = arrangement.drag.take()
            && commit
            && let (Some(monitor), Some([x, y])) = (drag.monitor, drag.position)
        {
            self.edit_output(
                &monitor.name,
                |output| {
                    output.x = x;
                    output.y = y;
                },
                cx,
            );
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_compares_both_edges_and_keeps_positions_non_negative() {
        assert_eq!(
            snap_axis(992.0, 400.0, [0.0, 1000.0].into_iter(), 0.2),
            1000
        );
        assert_eq!(snap_axis(592.0, 400.0, [0.0, 1000.0].into_iter(), 0.2), 600);
        assert_eq!(snap_axis(500.0, 400.0, [0.0, 1000.0].into_iter(), 0.2), 500);
        assert_eq!(snap_axis(-55.0, 400.0, [0.0, 1000.0].into_iter(), 0.2), 0);
    }

    #[test]
    fn drag_stops_at_edges_and_can_slide_around_a_display() {
        let moving = Tile {
            name: "DP-1".into(),
            label: "Display 1".into(),
            origin: [0.0, 0.0],
            size: [100.0, 100.0],
            enabled: true,
        };
        let fixed = Tile {
            name: "DP-2".into(),
            label: "Display 2".into(),
            origin: [100.0, 0.0],
            size: [100.0, 100.0],
            enabled: true,
        };
        let tiles = [fixed];
        let edge = constrain_drag(&moving, moving.origin, [300.0, 0.0], &tiles);
        assert_eq!(edge, [0.0, 0.0]);
        assert!(!overlaps(edge, moving.size, &tiles[0]));
        let corner = constrain_drag(&moving, edge, [150.0, 100.0], &tiles);
        assert_eq!(corner, [0.0, 100.0]);
        assert_eq!(
            constrain_drag(&moving, corner, [150.0, 80.0], &tiles),
            [150.0, 100.0]
        );
        assert!(overlaps([150.0, 80.0], moving.size, &tiles[0]));
    }

    #[test]
    fn fit_includes_the_origin_and_outputs_with_negative_coordinates() {
        let tile = Tile {
            name: "DP-1".into(),
            label: "Display".into(),
            origin: [-1000.0, 500.0],
            size: [3000.0, 1000.0],
            enabled: true,
        };
        let view = fit(&[tile], [348.0, 148.0], MAX_SCALE);
        assert_eq!(view.centre, [500.0, 750.0]);
        assert_eq!(view.in_view([0.0, 0.0], [348.0, 148.0])[1], 24.0);
        assert_eq!(view.in_view([2000.0, 1500.0], [348.0, 148.0])[1], 124.0);
    }
}
