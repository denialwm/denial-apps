//! The device's own lights: multicolor LEDs under `/sys/class/leds` that
//! nothing in the system drives, such as a phone's logo or trigger lights.
//! Settings keeps the user's choice in Denial's settings document, under
//! `lights`, and Denial sets the lights from it:
//!
//! ```json
//! "lights": {
//!   "offWithScreen": true,
//!   "leds": { "rgb:logo": { "on": true, "color": "#ff2a00" } }
//! }
//! ```

use std::fs;
use std::path::Path;

const LEDS: &str = "/sys/class/leds";

/// Functions the system or other apps drive: a camera flash, a keyboard or
/// screen backlight, a charging light.
const SYSTEM: &[&str] = &[
    "backlight",
    "charging",
    "flash",
    "kbd_backlight",
    "lcd-backlight",
    "torch",
];

/// The colours a light offers, as `0xrrggbb`.
pub const COLORS: &[u32] = &[
    0xff1a1a, 0xff7a00, 0xffd000, 0x2bd94a, 0x00d5e0, 0x2a5cff, 0xa03cff, 0xff3ca0, 0xffffff,
];

#[derive(Clone, Debug, PartialEq)]
pub struct Light {
    /// The LED's name, `color:function`, which the document keys by.
    pub name: String,
    pub title: String,
}

/// The device's own lights, in name order.
pub fn lights() -> Vec<Light> {
    lights_in(Path::new(LEDS))
}

fn lights_in(leds: &Path) -> Vec<Light> {
    let Ok(entries) = fs::read_dir(leds) else {
        return Vec::new();
    };
    let mut lights: Vec<Light> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| leds.join(name).join("multi_intensity").exists())
        .filter_map(|name| {
            let function = name
                .split_once(':')
                .map_or(name.as_str(), |(_, function)| function);
            if SYSTEM.iter().any(|system| function.starts_with(system)) {
                return None;
            }
            Some(Light {
                title: title(function),
                name,
            })
        })
        .collect();
    lights.sort_by(|a, b| a.name.cmp(&b.name));
    lights
}

/// What a light is called, from its function: `trigger-left` is the left
/// trigger, `fan-2` the second fan light. An unknown function is shown as
/// it is written.
fn title(function: &str) -> String {
    let (base, number) = match function.rsplit_once('-') {
        Some((base, number)) if number.parse::<u32>().is_ok() => (base, Some(number)),
        _ => (function, None),
    };
    let named = match base {
        "trigger-left" => "Left trigger".to_owned(),
        "trigger-right" => "Right trigger".to_owned(),
        "logo" => "Logo".to_owned(),
        "gamemode" => "Game mode".to_owned(),
        "fan" => "Fan".to_owned(),
        "status" => "Status".to_owned(),
        "indicator" => "Indicator".to_owned(),
        other => {
            let spaced = other.replace(['-', '_'], " ");
            let mut letters = spaced.chars();
            letters
                .next()
                .map(|first| first.to_uppercase().chain(letters).collect())
                .unwrap_or_default()
        }
    };
    let named = denial_look::words::translate(&named)
        .map(str::to_owned)
        .unwrap_or(named);
    match number {
        Some(number) => format!("{named} {number}"),
        None => named,
    }
}

/// `0xrrggbb` as the document writes it, `#rrggbb`.
pub fn hex(color: u32) -> String {
    format!("#{color:06x}")
}

/// `#rrggbb` as `0xrrggbb`.
pub fn parse(hex: &str) -> Option<u32> {
    let digits = hex.strip_prefix('#')?;
    (digits.len() == 6)
        .then(|| u32::from_str_radix(digits, 16).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_the_multicolor_lights_the_system_leaves_alone() {
        let root = std::env::temp_dir().join(format!("settings-lights-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for name in [
            "rgb:logo",
            "rgb:fan-2",
            "rgb:kbd_backlight",
            "rgb:trigger-left",
        ] {
            fs::create_dir_all(root.join(name)).unwrap();
            fs::write(root.join(name).join("multi_intensity"), "0 0 0").unwrap();
        }
        fs::create_dir_all(root.join("white:flash")).unwrap();
        fs::create_dir_all(root.join("input3::capslock")).unwrap();
        let lights = lights_in(&root);
        let names: Vec<_> = lights.iter().map(|light| light.name.as_str()).collect();
        assert_eq!(names, ["rgb:fan-2", "rgb:logo", "rgb:trigger-left"]);
        let titles: Vec<_> = lights.iter().map(|light| light.title.as_str()).collect();
        assert_eq!(titles, ["Fan 2", "Logo", "Left trigger"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn colors_round_trip() {
        assert_eq!(hex(0xff2a00), "#ff2a00");
        assert_eq!(parse("#ff2a00"), Some(0xff2a00));
        assert_eq!(parse("ff2a00"), None);
        assert_eq!(parse("#ff2a0"), None);
    }
}
