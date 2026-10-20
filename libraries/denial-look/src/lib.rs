//! Denial's application look for native Rust clients.
//!
//! Tokens follow Denial's docs/APPLICATION_MATERIALS.md: neutral black glass
//! for floating groups, warm charcoal for opaque content. The user's accent,
//! opacity, roundness and motion come from ~/.config/denial/settings.json.

use std::f32::consts::PI;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::Value;

pub mod palette {
    pub const CANVAS: u32 = 0x22211F;
    pub const RAISED: u32 = 0x2C2B29;
    pub const CONTROL: u32 = 0x383734;
    pub const GLASS: u32 = 0x000000;
    pub const FOREGROUND: u32 = 0xF2F1EF;
    pub const SECONDARY: u32 = 0xBBB9B5;
    pub const SEPARATOR: u32 = 0xB5B2AB;
    pub const SEPARATOR_ALPHA: f32 = 0x24 as f32 / 255.0;
    /// Foreground used on light fills such as a pale accent.
    pub const INK: u32 = 0x292825;
}

/// Edge and shadow shared by every floating glass group.
pub mod floating {
    pub const EDGE_WIDTH: f32 = 0.75;
    /// Foreground at this alpha.
    pub const EDGE_ALPHA: f32 = 0.12;
    /// Black at this alpha.
    pub const SHADOW_ALPHA: f32 = 0.18;
    pub const SHADOW_BLUR: f32 = 24.0;
    pub const SHADOW_OFFSET_Y: f32 = 6.0;
}

/// Distance of floating groups from the window edge.
pub const INSET: f32 = 8.0;
pub const MIN_RADIUS: f32 = 4.0;
const WINDOW_RADIUS: f32 = 28.0;
/// Below this the compositor would leave soft shadows as isolated frost.
const MIN_WINDOW_ALPHA: f32 = 3.0 / 255.0;
const DEFAULT_ACCENT: u32 = 0xD0BCFF;

/// `R_child = max(R_min, R_container - d)`.
pub fn nested_radius(container: f32, inset: f32) -> f32 {
    (container - inset).max(MIN_RADIUS)
}

pub struct Look {
    pub accent: u32,
    pub on_accent: u32,
    /// Alpha of the glass tint behind the whole window. At 1.0 the window is
    /// opaque canvas; below it Denial frosts the desktop behind the window.
    pub window_alpha: f32,
    /// Alpha of floating glass groups.
    pub panel_alpha: f32,
    pub window_radius: f32,
    /// Denial's animation duration scale; zero means reduced motion.
    pub duration_scale: f32,
    /// The user's font family, if they chose one.
    pub font: Option<String>,
}

impl Look {
    pub fn load() -> Self {
        let settings = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .and_then(|dir| std::fs::read(dir.join("denial/settings.json")).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(Value::Null);
        Self::from_settings(&settings)
    }

    pub fn from_settings(settings: &Value) -> Self {
        let appearance = &settings["appearance"];
        let number = |value: &Value, default: f32| {
            value
                .as_f64()
                .map(|n| n as f32)
                .filter(|n| n.is_finite())
                .unwrap_or(default)
        };
        let accent = match (
            appearance["accentSource"].as_str(),
            appearance["customAccentColor"].as_u64(),
        ) {
            (Some("custom"), Some(argb)) => (argb & 0xFF_FFFF) as u32,
            _ => DEFAULT_ACCENT,
        };
        let glass = &appearance["glass"];
        let (window_alpha, panel_alpha) = match appearance["transparencyMode"].as_str() {
            Some("glass") => (
                number(&glass["windowOpacity"], 1.0),
                number(&glass["appPanelOpacity"], 1.0),
            ),
            Some("blur") => (
                MIN_WINDOW_ALPHA,
                number(&appearance["panelOpacity"], 0.5).clamp(0.05, 1.0),
            ),
            _ => (1.0, 1.0),
        };
        let font = appearance["fontFamily"]
            .as_str()
            .filter(|family| !family.is_empty())
            .map(str::to_owned);
        Self {
            accent,
            on_accent: readable_on(accent),
            window_alpha: window_alpha.clamp(MIN_WINDOW_ALPHA, 1.0),
            panel_alpha: panel_alpha.clamp(0.0, 1.0),
            window_radius: WINDOW_RADIUS * number(&appearance["cornerRadiusScale"], 1.0).max(0.0),
            duration_scale: number(&settings["animations"]["durationScale"], 1.0).clamp(0.0, 4.0),
            font,
        }
    }

    /// Radius of a floating group placed `INSET` from the window edge.
    pub fn group_radius(&self) -> f32 {
        nested_radius(self.window_radius, INSET)
    }

    pub fn tween(&self, millis: u64) -> Tween {
        Tween {
            start: Instant::now(),
            duration: Duration::from_millis(millis).mul_f32(self.duration_scale),
        }
    }

    pub fn reduced_motion(&self) -> bool {
        self.duration_scale == 0.0
    }

    /// The user's font if installed, otherwise the first installed fallback.
    pub fn font_family(&self, installed: &[String]) -> String {
        self.font
            .iter()
            .map(String::as_str)
            .chain(FALLBACK_FONTS)
            .find(|family| installed.iter().any(|name| name == family))
            .unwrap_or("DejaVu Sans")
            .to_owned()
    }
}

const FALLBACK_FONTS: [&str; 5] = ["Noto Sans", "Adwaita Sans", "Inter", "Cantarell", "DejaVu Sans"];

/// A one-shot animation. With reduced motion it is finished on creation.
#[derive(Clone, Copy)]
pub struct Tween {
    start: Instant,
    duration: Duration,
}

impl Tween {
    pub fn progress(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        (now.saturating_duration_since(self.start).as_secs_f32() / self.duration.as_secs_f32())
            .min(1.0)
    }

    pub fn running(&self, now: Instant) -> bool {
        self.progress(now) < 1.0
    }
}

pub fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

pub fn ease_in_out_cubic(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Rises and falls back to zero over the tween: a tap pulse.
pub fn pulse(t: f32) -> f32 {
    (PI * t).sin() * (1.0 - t)
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn lerp_rgb(a: u32, b: u32, t: f32) -> u32 {
    let channel = |shift: u32| {
        let from = ((a >> shift) & 0xFF) as f32;
        let to = ((b >> shift) & 0xFF) as f32;
        (lerp(from, to, t).round() as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

fn readable_on(fill: u32) -> u32 {
    let light = contrast(fill, palette::FOREGROUND);
    let dark = contrast(fill, palette::INK);
    if light >= dark {
        palette::FOREGROUND
    } else {
        palette::INK
    }
}

fn contrast(a: u32, b: u32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn luminance(rgb: u32) -> f32 {
    let linear = |shift: u32| {
        let c = ((rgb >> shift) & 0xFF) as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(16) + 0.7152 * linear(8) + 0.0722 * linear(0)
}
