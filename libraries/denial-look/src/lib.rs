//! Denial's application look for native Rust clients.
//!
//! Tokens follow Denial's docs/APPLICATION_MATERIALS.md and, on phones,
//! docs/MOBILE_APP_MATERIALS.md: neutral glass for floating groups and chips,
//! warm opaque surfaces for content. The user's appearance, accent, opacity,
//! roundness and motion come from ~/.config/denial/settings.json, the
//! accent taken from the wallpaper from Denial's
//! ~/.local/state/denial/accent-color-v1, and `SettingsWatch` follows both.
//! The language the apps' words are in comes from there too (`words`).

mod watch;
pub mod words;

use std::path::PathBuf;
use std::time::Instant;

use serde_json::Value;

pub use watch::SettingsWatch;

/// The two application appearances, from `appearance.colorSchemePreference`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    Dark,
    Light,
}

pub mod palette {
    /// Every color and alpha that changes with the appearance. Apps select
    /// these roles; they never copy the values.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Palette {
        pub canvas: u32,
        pub raised: u32,
        pub control: u32,
        /// Tint of the window backing, glass groups and chips.
        pub glass: u32,
        pub foreground: u32,
        pub secondary: u32,
        pub separator: u32,
        pub separator_alpha: f32,
        /// Error text, as in a chip that says a message was not sent.
        pub error: u32,
        /// Glass groups never go below this tint alpha.
        pub group_floor: f32,
        /// Chips never go below this tint alpha.
        pub chip_floor: f32,
        /// The window backing and an island's glass together never tint the
        /// frost less than this, so secondary text on an island keeps 4.5:1
        /// even over white frost (black frost in the light appearance).
        pub island_floor: f32,
        /// Black at this alpha under floating groups and islands.
        pub shadow_alpha: f32,
        /// Foreground at this alpha on the edge of groups and chips.
        pub edge_alpha: f32,
        /// Foreground at this alpha on the edge of islands; zero for none.
        pub island_edge_alpha: f32,
    }

    pub const DARK: Palette = Palette {
        canvas: 0x22211F,
        raised: 0x2C2B29,
        control: 0x383734,
        glass: 0x000000,
        foreground: 0xF2F1EF,
        secondary: 0xBBB9B5,
        separator: 0xB5B2AB,
        separator_alpha: 0x24 as f32 / 255.0,
        error: 0xFF8A80,
        group_floor: 0.30,
        chip_floor: 0.50,
        island_floor: 0.72,
        shadow_alpha: 0.18,
        edge_alpha: 0.12,
        island_edge_alpha: 0.0,
    };

    pub const LIGHT: Palette = Palette {
        canvas: 0xF7F6F3,
        raised: 0xFFFFFF,
        control: 0xEEECE7,
        glass: 0xFFFFFF,
        foreground: 0x292825,
        secondary: 0x625F59,
        separator: 0x625F59,
        separator_alpha: 0.16,
        error: 0xB3261E,
        group_floor: 0.50,
        chip_floor: 0.70,
        island_floor: 0.86,
        shadow_alpha: 0.08,
        edge_alpha: 0.10,
        island_edge_alpha: 0.10,
    };

    /// Fill of a confirm button for something that cannot be undone, and of
    /// the end-call button. Nothing else is red.
    pub const DESTRUCTIVE: u32 = 0xD2453B;

    /// Fill of the button that places or answers a call. Nothing else is
    /// green.
    pub const CALL: u32 = 0x3E9B5B;

    /// Avatar fills for people without a photo, mid-tones under light
    /// initials in either appearance.
    const AVATARS: [u32; 8] = [
        0x6E5A8A, 0x4F7A8C, 0x7C6A4E, 0x5B7F62, 0x8A5560, 0x4E6585, 0x6A7B4F, 0x7A5E78,
    ];

    /// The same avatar fill for `key`, a number or a name, every time.
    pub fn avatar(key: &str) -> u32 {
        let hash = key
            .bytes()
            .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b.into()));
        AVATARS[hash as usize % AVATARS.len()]
    }
}

/// Geometry shared by every floating group, island and chip; the alphas that
/// go with it are in the palette.
pub mod floating {
    pub const EDGE_WIDTH: f32 = 0.75;
    pub const SHADOW_BLUR: f32 = 24.0;
    pub const SHADOW_OFFSET_Y: f32 = 6.0;
    /// The deeper shadow of an island lifted by a long press: black at this
    /// alpha, in either appearance.
    pub const LIFT_SHADOW_ALPHA: f32 = 0.35;
    pub const LIFT_SHADOW_BLUR: f32 = 30.0;
    pub const LIFT_SHADOW_OFFSET_Y: f32 = 10.0;
    /// Radius of the blur under glass groups and text fields: two standard
    /// deviations, so what scrolls under them reads as shapes, not words.
    pub const BACKDROP_BLUR: f32 = 40.0;
}

/// Durations in milliseconds, on Denial's duration scale through
/// `Look::tween`, and the distances that go with them. MOBILE_APP_MATERIALS.md
/// has the table they come from.
pub mod motion {
    /// Launch: islands rise and fade in one after another, with ease-out
    /// cubic.
    pub const LAUNCH_DELAY: u64 = 120;
    pub const LAUNCH_STEP: u64 = 45;
    pub const LAUNCH: u64 = 380;
    /// dp.
    pub const LAUNCH_RISE: f32 = 18.0;
    /// Glass groups fade in at launch.
    pub const GROUP_FADE: u64 = 160;
    /// Between screens: what leaves fades out, then what arrives rises in,
    /// nearest first, on the decelerate curve.
    pub const OUT: u64 = 160;
    pub const IN: u64 = 240;
    pub const IN_STEP: u64 = 18;
    /// lp.
    pub const IN_RISE: f32 = 24.0;
    /// A bottom group drops away by this much, and composer islands rise by
    /// it, lp; they arrive `BOTTOM_DELAY` into the switch.
    pub const BOTTOM_TRAVEL: f32 = 40.0;
    pub const BOTTOM_DELAY: u64 = 120;
    /// Between tabs the screens slide side by side, the way the tabs are
    /// ordered, on the decelerate curve; nothing fades.
    pub const SLIDE: u64 = 300;
    /// Glass group contents cross-fade: out over `GROUP_OUT`, in over
    /// `GROUP_IN` from `GROUP_IN_DELAY`.
    pub const GROUP_OUT: u64 = 100;
    pub const GROUP_IN_DELAY: u64 = 140;
    pub const GROUP_IN: u64 = 140;
    /// A new message rises and fades in, ease-out cubic; dp.
    pub const NEW_MESSAGE: u64 = 300;
    pub const NEW_MESSAGE_RISE: f32 = 16.0;
    /// Long press: the island lifts over `LIFT`, its shadow deepening to
    /// `floating::LIFT_SHADOW_*`; the rest dims to `DIM_ALPHA` over `DIM`,
    /// and its actions fade in meanwhile.
    pub const LIFT: u64 = 120;
    pub const DIM: u64 = 160;
    pub const DIM_ALPHA: f32 = 0.28;
    /// Chips only fade, in this long.
    pub const CHIP_FADE: u64 = 160;
    /// An unread count fades out after its conversation opens.
    pub const READ: u64 = 420;
    /// A short notice stays this long. Not scaled: it is read, not watched.
    pub const NOTICE: u64 = 1800;
}

/// Layout of a phone screen, in logical pixels (lp) unless marked dp.
/// Handhelds run at scale 2 for 225 ppi.
pub mod mobile {
    /// Logical pixels in an Android dp: Denial lays handheld panels out at
    /// 225 logical pixels per inch (deniald's HANDHELD_LOGICAL_PPI) and a dp
    /// is 1/160 inch, so one dp is the same on every Denial phone.
    pub const DP: f32 = 225.0 / 160.0;
    /// Denial's status strip, above the window: the app is configured below
    /// it, and the strip is painted with the window's pixel at (5, 5).
    pub const STATUS_STRIP: f32 = 48.0;
    /// The shell's gesture zone at the bottom of the window: its pill and
    /// home gesture. No controls there.
    pub const GESTURE_INSET: f32 = 52.0;
    /// The shell lifts the whole scene by this much at most while the
    /// on-screen keyboard is up.
    pub const KEYBOARD_LIFT: f32 = 368.0;
    /// Space between the on-screen keyboard and what rides on it: the
    /// gutter, as between any two islands.
    pub const KEYBOARD_GAP: f32 = super::INSET;
    /// Smallest tappable control, dp.
    pub const TOUCH: f32 = 48.0;
    /// Height of the top and of the bottom glass group, dp.
    pub const GROUP_HEIGHT: f32 = 56.0;
    /// Height of a tab bar's tabs, dp. The bar is anchored: opaque, the
    /// window's full width, from the bottom edge up, the gesture zone under
    /// its tabs.
    pub const TAB_BAR: f32 = 52.0;
    /// Height of the composer field and its Send island, dp.
    pub const COMPOSER_HEIGHT: f32 = 48.0;
    /// Avatar in a list row, and in the top group, dp.
    pub const AVATAR: f32 = 46.0;
    pub const AVATAR_SMALL: f32 = 36.0;
    /// Text on a chip, dp.
    pub const CHIP_TEXT: f32 = 12.0;

    pub fn dp(value: f32) -> f32 {
        value * DP
    }

    /// Where what rides on the on-screen keyboard sits, lp above the
    /// window's bottom edge while the keyboard is `keyboard` lp tall, 0 when
    /// it is down: on the gesture zone, or a gap above the keys. Only text
    /// fields ride on the keys; everything else stays where it is, under the
    /// keyboard's glass.
    pub fn bottom_inset(keyboard: f32) -> f32 {
        (keyboard + KEYBOARD_GAP).max(GESTURE_INSET)
    }

    /// Padding above the first island of a scroll viewport, which starts at
    /// the top group's upper edge: the group and the gutter under it.
    pub fn content_top() -> f32 {
        dp(GROUP_HEIGHT) + super::INSET
    }

    /// Padding under the last island of a scroll viewport, which runs to the
    /// window's bottom edge, under the gesture zone and the keyboard: the
    /// gutter clear of everything at the bottom. `fixed` lp stay on the
    /// gesture zone (a tab bar, a call bar) and are under the keyboard while
    /// it is up; `riding` lp ride on the keys (a composer).
    pub fn content_bottom(keyboard: f32, fixed: f32, riding: f32) -> f32 {
        let keys = if riding > 0.0 {
            bottom_inset(keyboard) + riding
        } else {
            keyboard
        };
        (GESTURE_INSET + fixed).max(keys) + super::INSET
    }

    /// The top of a scrolling screen that leaves as its content scrolls on
    /// and comes back as soon as it scrolls back, wherever it is: how far it
    /// is up and away, lp, out of `range`.
    #[derive(Clone, Copy, Debug, Default)]
    pub struct ScrollAway {
        /// The content's scroll offset when last followed.
        last: f32,
        away: f32,
    }

    impl ScrollAway {
        /// Starts over shown, the content at `offset`.
        pub fn shown(offset: f32) -> Self {
            Self {
                last: offset,
                away: 0.0,
            }
        }

        /// Follows the content to `offset`, lp scrolled from its start, and
        /// says how far up the top is now. It moves with the content, up to
        /// `range` away, and never further than the content has scrolled, so
        /// at the start it is all there.
        pub fn follow(&mut self, offset: f32, range: f32) -> f32 {
            let offset = offset.max(0.0);
            self.away = (self.away + offset - self.last).clamp(0.0, range.min(offset));
            self.last = offset;
            self.away
        }

        pub fn away(&self) -> f32 {
            self.away
        }
    }

    /// How far the shell lifts a view `height` tall for the keyboard.
    pub fn keyboard_lift(height: f32) -> f32 {
        (0.3 * height).min(KEYBOARD_LIFT)
    }
}

/// Distance of floating groups and islands from the window edge and from
/// each other: the gutter.
pub const INSET: f32 = 8.0;
/// Foreground at this alpha fills a control inside an island or group;
/// controls never get their own glass.
pub const CONTROL_FILL: f32 = 0.08;
/// Foreground at this alpha fills a control while it is pressed.
pub const PRESSED_FILL: f32 = 0.16;
pub const MIN_RADIUS: f32 = 4.0;
const WINDOW_RADIUS: f32 = 28.0;
/// Below this the compositor would leave soft shadows as isolated frost.
const MIN_WINDOW_ALPHA: f32 = 3.0 / 255.0;
const DEFAULT_ACCENT: u32 = 0xD0BCFF;
/// Accent text is shifted toward the foreground until it reaches this
/// contrast against canvas.
const ACCENT_TEXT_CONTRAST: f32 = 4.5;

/// `R_child = max(R_min, R_container - d)`.
pub fn nested_radius(container: f32, inset: f32) -> f32 {
    (container - inset).max(MIN_RADIUS)
}

pub struct Look {
    pub appearance: Appearance,
    pub palette: palette::Palette,
    pub accent: u32,
    /// Text and icons on an accent fill.
    pub on_accent: u32,
    /// The accent for text on opaque surfaces: readable against canvas.
    pub accent_text: u32,
    /// Alpha of the glass tint behind the whole window. At 1.0 the window is
    /// opaque canvas; below it Denial frosts the desktop behind the window.
    pub window_alpha: f32,
    /// The user's app-panel opacity, before the group and chip floors.
    pub panel_alpha: f32,
    pub window_radius: f32,
    /// Denial's animation duration scale; zero means reduced motion.
    pub duration_scale: f32,
    /// The user's font family, if they chose one.
    pub font: Option<String>,
}

/// `$XDG_CONFIG_HOME/denial/settings.json`, else `~/.config/denial/settings.json`.
fn settings_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|dir| dir.join("denial/settings.json"))
}

/// Denial's settings document; Null when there is none to read.
fn settings() -> Value {
    settings_path()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Value::Null)
}

/// Where Denial keeps the accent it took from the wallpaper:
/// `$XDG_STATE_HOME/denial/accent-color-v1`, else under `~/.local/state`.
fn wallpaper_accent_path() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .map(|dir| dir.join("denial/accent-color-v1"))
}

/// The wallpaper's accent as Denial keeps it: the magic `DENACNT\0`, the
/// format 1, then red, green and blue.
fn wallpaper_accent(bytes: &[u8]) -> Option<u32> {
    match bytes {
        [b'D', b'E', b'N', b'A', b'C', b'N', b'T', 0, 1, r, g, b] => {
            Some(u32::from_be_bytes([0, *r, *g, *b]))
        }
        _ => None,
    }
}

impl Look {
    pub fn load() -> Self {
        let settings = settings();
        words::follow(&settings);
        let wallpaper = wallpaper_accent_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| wallpaper_accent(&bytes));
        Self::from_settings_and_wallpaper(&settings, wallpaper)
    }

    pub fn from_settings(settings: &Value) -> Self {
        Self::from_settings_and_wallpaper(settings, None)
    }

    /// The look `settings` give, with the accent Denial took from the
    /// wallpaper when they ask for that one.
    pub fn from_settings_and_wallpaper(settings: &Value, wallpaper: Option<u32>) -> Self {
        let appearance = &settings["appearance"];
        let number = |value: &Value, default: f32| {
            value
                .as_f64()
                .map(|n| n as f32)
                .filter(|n| n.is_finite())
                .unwrap_or(default)
        };
        // noPreference keeps Denial's dark fallback, as the shell does.
        let (scheme, palette) = match appearance["colorSchemePreference"].as_str() {
            Some("preferLight") => (Appearance::Light, palette::LIGHT),
            _ => (Appearance::Dark, palette::DARK),
        };
        let accent = match (
            appearance["accentSource"].as_str(),
            appearance["customAccentColor"].as_u64(),
        ) {
            (Some("custom"), Some(argb)) => (argb & 0xFF_FFFF) as u32,
            _ => wallpaper.unwrap_or(DEFAULT_ACCENT),
        };
        let glass = &appearance["glass"];
        let (window_alpha, panel_alpha) = match appearance["transparencyMode"].as_str() {
            Some("glass") => (
                number(&glass["windowOpacity"], 1.0),
                number(&glass["appPanelOpacity"], 1.0),
            ),
            // Blur keeps the glass's window opacity, the one key for the
            // backing behind apps.
            Some("blur") => (
                number(&glass["windowOpacity"], MIN_WINDOW_ALPHA),
                number(&appearance["panelOpacity"], 0.5).clamp(0.05, 1.0),
            ),
            _ => (1.0, 1.0),
        };
        let font = appearance["fontFamily"]
            .as_str()
            .filter(|family| !family.is_empty())
            .map(str::to_owned);
        Self {
            appearance: scheme,
            palette,
            accent,
            on_accent: on(accent),
            accent_text: readable_toward(accent, palette.foreground, palette.canvas),
            window_alpha: window_alpha.clamp(MIN_WINDOW_ALPHA, 1.0),
            panel_alpha: panel_alpha.clamp(0.0, 1.0),
            window_radius: WINDOW_RADIUS * number(&appearance["cornerRadiusScale"], 1.0).max(0.0),
            duration_scale: number(&settings["animations"]["durationScale"], 1.0).clamp(0.0, 4.0),
            font,
        }
    }

    /// Radius of a floating group or island placed `INSET` from the window
    /// edge.
    pub fn group_radius(&self) -> f32 {
        nested_radius(self.window_radius, INSET)
    }

    /// Radius of message bubbles and of the conversation islands shaped like
    /// them. The last bubble of a run turns its corner toward the sender down
    /// to `MIN_RADIUS`.
    pub fn bubble_radius(&self) -> f32 {
        self.window_radius.max(MIN_RADIUS)
    }

    /// The window backing: the glass tint at window opacity, or opaque canvas
    /// at 1.0. Color and alpha.
    pub fn backing(&self) -> (u32, f32) {
        if self.window_alpha >= 1.0 {
            (self.palette.canvas, 1.0)
        } else {
            (self.palette.glass, self.window_alpha)
        }
    }

    /// Tint alpha of a glass group: app-panel opacity, never below the
    /// group floor.
    pub fn group_alpha(&self) -> f32 {
        self.panel_alpha.max(self.palette.group_floor)
    }

    /// An island's fill, color and alpha: the glass tint at app-panel
    /// opacity, deep enough that window and island together reach the island
    /// floor; opaque raised in an opaque window.
    pub fn island_fill(&self) -> (u32, f32) {
        if self.window_alpha >= 1.0 {
            return (self.palette.raised, 1.0);
        }
        let floor = 1.0 - (1.0 - self.palette.island_floor) / (1.0 - self.window_alpha);
        (
            self.palette.glass,
            self.panel_alpha.max(floor).clamp(0.0, 1.0),
        )
    }

    /// Tint alpha of a chip: app-panel opacity, never below the chip floor.
    pub fn chip_alpha(&self) -> f32 {
        self.panel_alpha.max(self.palette.chip_floor)
    }

    pub fn tween(&self, millis: u64) -> Tween {
        self.tween_at(Instant::now(), 0, millis)
    }

    /// A tween that starts `delay` milliseconds after `start`, as item i of
    /// a stagger does.
    fn tween_at(&self, start: Instant, delay: u64, millis: u64) -> Tween {
        Tween {
            start,
            scale: self.duration_scale,
            delay: delay as f32,
            millis: millis as f32,
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

const FALLBACK_FONTS: [&str; 5] = [
    "Noto Sans",
    "Adwaita Sans",
    "Inter",
    "Cantarell",
    "DejaVu Sans",
];

/// A one-shot animation on Denial's duration scale. With reduced motion it
/// is finished on creation, delay included.
#[derive(Clone, Copy)]
pub struct Tween {
    start: Instant,
    scale: f32,
    /// Unscaled milliseconds after `start`.
    delay: f32,
    millis: f32,
}

impl Tween {
    pub fn progress(&self, now: Instant) -> f32 {
        let span = self.millis * self.scale;
        let elapsed = now.saturating_duration_since(self.start).as_secs_f32() * 1000.0;
        if span <= 0.0 {
            // No length: done once its delay is over, at once without motion.
            return if elapsed >= self.delay * self.scale {
                1.0
            } else {
                0.0
            };
        }
        ((elapsed - self.delay * self.scale) / span).clamp(0.0, 1.0)
    }

    pub fn running(&self, now: Instant) -> bool {
        self.progress(now) < 1.0
    }

    /// A tween on this one's clock that starts `delay` milliseconds after
    /// this one does and lasts `millis`.
    pub fn segment(&self, delay: u64, millis: u64) -> Tween {
        Tween {
            delay: self.delay + delay as f32,
            millis: millis as f32,
            ..*self
        }
    }
}

/// The delay of item `index` in a stagger that starts after `first`
/// milliseconds and adds `step` per item.
pub fn stagger(first: u64, step: u64, index: usize) -> u64 {
    first + step * index as u64
}

pub fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

pub fn ease_in_cubic(t: f32) -> f32 {
    t * t * t
}

/// cubic-bezier(0.2, 0.8, 0.2, 1): quick to leave, long to settle. Islands
/// arriving on another screen move on it.
pub fn decelerate(t: f32) -> f32 {
    const X1: f32 = 0.2;
    const Y1: f32 = 0.8;
    const X2: f32 = 0.2;
    const Y2: f32 = 1.0;
    let bezier = |a: f32, b: f32, s: f32| {
        let r = 1.0 - s;
        3.0 * r * r * s * a + 3.0 * r * s * s * b + s * s * s
    };
    let t = t.clamp(0.0, 1.0);
    // x(s) rises monotonically, so halving finds the s with x(s) = t.
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..24 {
        let mid = (low + high) / 2.0;
        if bezier(X1, X2, mid) < t {
            low = mid;
        } else {
            high = mid;
        }
    }
    bezier(Y1, Y2, (low + high) / 2.0)
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

/// The text color, dark or light foreground, that reads best on `fill`.
pub fn on(fill: u32) -> u32 {
    let (light, dark) = (palette::DARK.foreground, palette::LIGHT.foreground);
    if contrast(fill, light) >= contrast(fill, dark) {
        light
    } else {
        dark
    }
}

/// `color` moved toward `toward` until it reaches `ACCENT_TEXT_CONTRAST`
/// against `ground`.
fn readable_toward(color: u32, toward: u32, ground: u32) -> u32 {
    let mut shifted = color;
    for _ in 0..8 {
        if contrast(shifted, ground) >= ACCENT_TEXT_CONTRAST {
            break;
        }
        shifted = lerp_rgb(shifted, toward, 0.18);
    }
    shifted
}

pub fn contrast(a: u32, b: u32) -> f32 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// `fill` at `alpha` over `under`.
    fn over(fill: u32, alpha: f32, under: u32) -> u32 {
        lerp_rgb(under, fill, alpha)
    }

    #[test]
    fn foregrounds_read_on_surfaces_and_floors() {
        for palette in [palette::DARK, palette::LIGHT] {
            for surface in [palette.canvas, palette.raised, palette.control] {
                assert!(contrast(palette.foreground, surface) >= 7.0);
                assert!(contrast(palette.secondary, surface) >= 4.5);
            }
            assert!(contrast(palette.error, palette.raised) >= 4.5);
            // Chips and groups at their floors over black and white frost.
            // The worst case is dark glass over white frost: chips 3.5:1,
            // groups 1.9:1. The floors were chosen on real wallpapers, whose
            // frost is never white.
            for frost in [0x000000, 0xFFFFFF] {
                let chip = over(palette.glass, palette.chip_floor, frost);
                assert!(
                    contrast(palette.foreground, chip) >= 3.4,
                    "chip on {frost:06x}"
                );
                let group = over(palette.glass, palette.group_floor, frost);
                assert!(
                    contrast(palette.foreground, group) >= 1.8,
                    "group on {frost:06x}"
                );
            }
        }
        assert!(contrast(on(palette::DESTRUCTIVE), palette::DESTRUCTIVE) >= 3.0);
        assert!(contrast(on(palette::CALL), palette::CALL) >= 3.0);
    }

    #[test]
    fn islands_keep_secondary_text_over_the_worst_frost() {
        for (scheme, frost) in [("preferDark", 0xFFFFFF), ("preferLight", 0x000000)] {
            for window in [0.0, 0.25, 0.5, 0.9] {
                let look = Look::from_settings(&serde_json::json!({
                    "appearance": {
                        "colorSchemePreference": scheme,
                        "transparencyMode": "glass",
                        "glass": { "appPanelOpacity": 0.0, "windowOpacity": window },
                    }
                }));
                let (backing, backing_alpha) = look.backing();
                let (fill, alpha) = look.island_fill();
                let island = over(fill, alpha, over(backing, backing_alpha, frost));
                let palette = look.palette;
                assert!(
                    contrast(palette.secondary, island) >= 4.5,
                    "{scheme} at window {window}"
                );
            }
        }
    }

    #[test]
    fn accent_comes_from_the_wallpaper_unless_custom() {
        let kept = *b"DENACNT\0\x01\x3b\x4a\xa6";
        assert_eq!(wallpaper_accent(&kept), Some(0x3B4AA6));
        assert_eq!(wallpaper_accent(&kept[..11]), None);
        let wallpaper = serde_json::json!({"appearance": {"accentSource": "wallpaper"}});
        let custom = serde_json::json!({
            "appearance": {"accentSource": "custom", "customAccentColor": 0xFF11_2233u32}
        });
        assert_eq!(
            Look::from_settings_and_wallpaper(&wallpaper, Some(0x3B4AA6)).accent,
            0x3B4AA6
        );
        assert_eq!(
            Look::from_settings_and_wallpaper(&custom, Some(0x3B4AA6)).accent,
            0x112233
        );
        assert_eq!(Look::from_settings(&wallpaper).accent, DEFAULT_ACCENT);
    }

    #[test]
    fn appearance_follows_the_color_scheme() {
        let look = |scheme: &str| {
            Look::from_settings(&serde_json::json!({
                "appearance": {
                    "colorSchemePreference": scheme,
                    "transparencyMode": "glass",
                    "glass": { "appPanelOpacity": 0.0, "windowOpacity": 0.4 },
                }
            }))
        };
        let (light, dark) = (look("preferLight"), look("noPreference"));
        assert_eq!(light.appearance, Appearance::Light);
        assert_eq!(dark.appearance, Appearance::Dark);
        assert_eq!(light.group_alpha(), 0.50);
        assert_eq!(dark.chip_alpha(), 0.50);
        assert_eq!(dark.backing(), (0x000000, 0.4));
        for look in [light, dark] {
            assert!(contrast(look.accent_text, look.palette.canvas) >= ACCENT_TEXT_CONTRAST);
        }
    }

    #[test]
    fn content_clears_what_is_at_the_bottom() {
        use mobile::{GESTURE_INSET, content_bottom};
        // Nothing at the bottom: a gutter over the gesture zone, or over the
        // keys while they are up.
        assert_eq!(content_bottom(0.0, 0.0, 0.0), GESTURE_INSET + INSET);
        assert_eq!(content_bottom(600.0, 0.0, 0.0), 600.0 + INSET);
        // A tab bar stays under the keyboard; a composer rides on it.
        assert_eq!(content_bottom(0.0, 80.0, 0.0), GESTURE_INSET + 80.0 + INSET);
        assert_eq!(content_bottom(600.0, 80.0, 0.0), 600.0 + INSET);
        assert_eq!(content_bottom(600.0, 0.0, 70.0), 600.0 + 2.0 * INSET + 70.0);
    }

    #[test]
    fn the_top_scrolls_away_and_back_from_anywhere() {
        let mut top = mobile::ScrollAway::shown(0.0);
        assert_eq!(top.follow(30.0, 100.0), 30.0);
        assert_eq!(top.follow(900.0, 100.0), 100.0);
        // Back a little, far from the start: it comes back as much.
        assert_eq!(top.follow(860.0, 100.0), 60.0);
        assert_eq!(top.follow(700.0, 100.0), 0.0);
        // Down again, then back to the start, where it is all there.
        assert_eq!(top.follow(780.0, 100.0), 80.0);
        assert_eq!(top.follow(20.0, 100.0), 0.0);
    }

    #[test]
    fn decelerate_leaves_fast_and_lands_exactly() {
        assert!(decelerate(0.0).abs() < 1e-4);
        assert!((decelerate(1.0) - 1.0).abs() < 1e-4);
        assert!(decelerate(0.25) > 0.6);
        assert!(decelerate(0.5) < decelerate(0.75));
    }

    #[test]
    fn reduced_motion_finishes_every_tween_at_once() {
        let look = Look::from_settings(&serde_json::json!({
            "animations": { "durationScale": 0.0 }
        }));
        let start = Instant::now();
        let tween = look.tween_at(start, stagger(120, 45, 9), 380);
        assert_eq!(tween.progress(start), 1.0);
        assert_eq!(tween.segment(160, 240).progress(start), 1.0);
        let moving = Look::from_settings(&Value::Null).tween_at(start, 100, 200);
        assert_eq!(moving.progress(start), 0.0);
        assert_eq!(moving.progress(start + Duration::from_millis(200)), 0.5);
    }
}
