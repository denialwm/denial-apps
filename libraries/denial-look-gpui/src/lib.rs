//! Denial's mobile materials as GPUI elements: islands, glass groups and
//! chips with the fills, edges and shadows of docs/MOBILE_APP_MATERIALS.md,
//! the text field and the icons the apps share. The values come from
//! `denial-look`; this crate only builds elements from them.

mod icons;
pub mod input;

use denial_look::{CONTROL_FILL, INSET, Look, floating, mobile, nested_radius};
use gpui::{BoxShadow, Div, Hsla, Pixels, Styled, div, point, prelude::*, px, relative, rgba};

pub use icons::{Assets, Icon, icon};

/// `value` dp in logical pixels.
pub fn dp(value: f32) -> Pixels {
    px(mobile::dp(value))
}

pub fn color(rgb: u32, alpha: f32) -> Hsla {
    rgba((rgb << 8) | (alpha.clamp(0.0, 1.0) * 255.0).round() as u32).into()
}

pub fn solid(rgb: u32) -> Hsla {
    color(rgb, 1.0)
}

/// The window backing everything else floats on.
pub fn backing(look: &Look) -> Hsla {
    let (rgb, alpha) = look.backing();
    color(rgb, alpha)
}

/// The floating shadow of groups and lone islands.
pub fn shadow(look: &Look) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: color(0x000000, look.palette.shadow_alpha),
        offset: point(px(0.), px(floating::SHADOW_OFFSET_Y)),
        blur_radius: px(floating::SHADOW_BLUR),
        spread_radius: px(0.),
        inset: false,
    }]
}

/// The shadow of an island lifted by a long press, `t` of the way up from no
/// shadow, as in a run.
pub fn lifted(t: f32) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: color(0x000000, floating::LIFT_SHADOW_ALPHA * t),
        offset: point(px(0.), px(floating::LIFT_SHADOW_OFFSET_Y * t)),
        blur_radius: px(floating::LIFT_SHADOW_BLUR * t),
        spread_radius: px(0.),
        inset: false,
    }]
}

/// An island's fill: the smoke glass of `Look::island_fill`, or opaque
/// raised in an opaque window.
pub fn island_fill(look: &Look) -> Hsla {
    let (rgb, alpha) = look.island_fill();
    color(rgb, alpha)
}

/// An island's edge: the floating edge on smoke glass, as groups have it;
/// on an opaque island only in the light appearance.
pub fn island_edge(look: &Look) -> Option<Hsla> {
    let alpha = if look.island_fill().1 < 1.0 {
        look.palette.edge_alpha
    } else {
        look.palette.island_edge_alpha
    };
    (alpha > 0.0).then(|| color(look.palette.foreground, alpha))
}

/// An island in a run of three or more, such as a bubble or a conversation:
/// smoke, the group radius and the island edge. Runs drop the shadow;
/// twenty of them would read as dirt on the frost.
pub fn island_run(look: &Look) -> Div {
    let island = div().bg(island_fill(look)).rounded(px(look.group_radius()));
    match island_edge(look) {
        Some(edge) => island.border(px(floating::EDGE_WIDTH)).border_color(edge),
        None => island,
    }
}

/// An island on its own: a run's island with the floating shadow.
pub fn island(look: &Look) -> Div {
    island_run(look).shadow(shadow(look))
}

/// A run's island over the backdrop blur, for what stays in place while
/// content scrolls under it: a text field, its Send, a bar.
pub fn glass_island(look: &Look) -> Div {
    island_run(look).backdrop_blur(px(floating::BACKDROP_BLUR))
}

/// A glass group for navigation and commands that stay in place: the glass
/// tint, never below the group floor, over the backdrop blur, with the
/// floating edge and shadow. It takes the touches over content scrolling
/// under it.
pub fn glass_group(look: &Look) -> Div {
    div()
        .occlude()
        .backdrop_blur(px(floating::BACKDROP_BLUR))
        .bg(color(look.palette.glass, look.group_alpha()))
        .rounded(px(look.group_radius()))
        .border(px(floating::EDGE_WIDTH))
        .border_color(color(look.palette.foreground, look.palette.edge_alpha))
        .shadow(shadow(look))
}

/// A chip: one short line about what is around it, on a fully round pill of
/// glass tint never below the chip floor. Its text is the foreground, or the
/// error color with an icon; never the secondary color.
pub fn chip(look: &Look) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(dp(6.))
        .px(dp(11.))
        .py(dp(3.))
        .rounded_full()
        .bg(color(look.palette.glass, look.chip_alpha()))
        .border(px(floating::EDGE_WIDTH))
        .border_color(color(look.palette.foreground, look.palette.edge_alpha))
        .text_size(dp(mobile::CHIP_TEXT))
        .line_height(relative(1.3))
        .text_color(solid(look.palette.foreground))
        .whitespace_nowrap()
}

/// The fill of a control inside an island or group: the foreground at
/// `CONTROL_FILL`, so it takes the tint of what it sits on.
pub fn control_fill(look: &Look) -> Hsla {
    color(look.palette.foreground, CONTROL_FILL)
}

/// Radius of a control inset by `INSET` inside an island or group.
pub fn control_radius(look: &Look) -> Pixels {
    px(nested_radius(look.group_radius(), INSET))
}
