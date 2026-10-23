//! The apps' icons: 24-unit stroked outlines in one color. An app serves them
//! by passing `Assets` to `Application::with_assets`.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString, Svg, prelude::*, svg};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Accessibility,
    Airplane,
    Alert,
    Android,
    Apps,
    Archive,
    Back,
    Backspace,
    Battery,
    Bell,
    Block,
    Bluetooth,
    Chevron,
    Close,
    Computer,
    Copy,
    Delete,
    Delivered,
    Display,
    Fingerprint,
    Flip,
    Gamepad,
    Gridlines,
    HangUp,
    Headphones,
    Hold,
    Hotspot,
    Incoming,
    Info,
    Keyboard,
    Keypad,
    Language,
    Location,
    Lock,
    Message,
    Missed,
    Moon,
    More,
    Mouse,
    Mute,
    Network,
    New,
    Outgoing,
    Palette,
    Person,
    Phone,
    Photos,
    Pin,
    Progress,
    Read,
    Recent,
    Retry,
    Search,
    Send,
    Settings,
    Share,
    Shield,
    Sim,
    Speaker,
    Star,
    Starred,
    Storage,
    Timer,
    Unarchive,
    Unread,
    Update,
    Vibrate,
    Wifi,
    WifiFair,
    WifiGood,
    WifiWeak,
}

const ALL: [Icon; 71] = [
    Icon::Accessibility,
    Icon::Airplane,
    Icon::Alert,
    Icon::Android,
    Icon::Apps,
    Icon::Archive,
    Icon::Back,
    Icon::Backspace,
    Icon::Battery,
    Icon::Bell,
    Icon::Block,
    Icon::Bluetooth,
    Icon::Chevron,
    Icon::Close,
    Icon::Computer,
    Icon::Copy,
    Icon::Delete,
    Icon::Delivered,
    Icon::Display,
    Icon::Fingerprint,
    Icon::Flip,
    Icon::Gamepad,
    Icon::Gridlines,
    Icon::HangUp,
    Icon::Headphones,
    Icon::Hold,
    Icon::Hotspot,
    Icon::Incoming,
    Icon::Info,
    Icon::Keyboard,
    Icon::Keypad,
    Icon::Language,
    Icon::Location,
    Icon::Lock,
    Icon::Message,
    Icon::Missed,
    Icon::Moon,
    Icon::More,
    Icon::Mouse,
    Icon::Mute,
    Icon::Network,
    Icon::New,
    Icon::Outgoing,
    Icon::Palette,
    Icon::Person,
    Icon::Phone,
    Icon::Photos,
    Icon::Pin,
    Icon::Progress,
    Icon::Read,
    Icon::Recent,
    Icon::Retry,
    Icon::Search,
    Icon::Send,
    Icon::Settings,
    Icon::Share,
    Icon::Shield,
    Icon::Sim,
    Icon::Speaker,
    Icon::Star,
    Icon::Starred,
    Icon::Storage,
    Icon::Timer,
    Icon::Unarchive,
    Icon::Unread,
    Icon::Update,
    Icon::Vibrate,
    Icon::Wifi,
    Icon::WifiFair,
    Icon::WifiGood,
    Icon::WifiWeak,
];

impl Icon {
    fn name(self) -> &'static str {
        match self {
            Icon::Accessibility => "accessibility",
            Icon::Airplane => "airplane",
            Icon::Alert => "alert",
            Icon::Android => "android",
            Icon::Apps => "apps",
            Icon::Archive => "archive",
            Icon::Back => "back",
            Icon::Backspace => "backspace",
            Icon::Battery => "battery",
            Icon::Bell => "bell",
            Icon::Block => "block",
            Icon::Bluetooth => "bluetooth",
            Icon::Chevron => "chevron",
            Icon::Close => "close",
            Icon::Computer => "computer",
            Icon::Copy => "copy",
            Icon::Delete => "delete",
            Icon::Delivered => "delivered",
            Icon::Display => "display",
            Icon::Fingerprint => "fingerprint",
            Icon::Flip => "flip",
            Icon::Gamepad => "gamepad",
            Icon::Gridlines => "gridlines",
            Icon::HangUp => "hang-up",
            Icon::Headphones => "headphones",
            Icon::Hold => "hold",
            Icon::Hotspot => "hotspot",
            Icon::Incoming => "incoming",
            Icon::Info => "info",
            Icon::Keyboard => "keyboard",
            Icon::Keypad => "keypad",
            Icon::Language => "language",
            Icon::Location => "location",
            Icon::Lock => "lock",
            Icon::Message => "message",
            Icon::Missed => "missed",
            Icon::Moon => "moon",
            Icon::More => "more",
            Icon::Mouse => "mouse",
            Icon::Mute => "mute",
            Icon::Network => "network",
            Icon::New => "new",
            Icon::Outgoing => "outgoing",
            Icon::Palette => "palette",
            Icon::Person => "person",
            Icon::Phone => "phone",
            Icon::Photos => "photos",
            Icon::Pin => "pin",
            Icon::Progress => "progress",
            Icon::Read => "read",
            Icon::Recent => "recent",
            Icon::Retry => "retry",
            Icon::Search => "search",
            Icon::Send => "send",
            Icon::Settings => "settings",
            Icon::Share => "share",
            Icon::Shield => "shield",
            Icon::Sim => "sim",
            Icon::Speaker => "speaker",
            Icon::Star => "star",
            Icon::Starred => "starred",
            Icon::Storage => "storage",
            Icon::Timer => "timer",
            Icon::Unarchive => "unarchive",
            Icon::Unread => "unread",
            Icon::Update => "update",
            Icon::Vibrate => "vibrate",
            Icon::Wifi => "wifi",
            Icon::WifiFair => "wifi-fair",
            Icon::WifiGood => "wifi-good",
            Icon::WifiWeak => "wifi-weak",
        }
    }

    /// The shapes inside the 24-unit square.
    fn shapes(self) -> &'static str {
        match self {
            Icon::Accessibility => {
                r#"<circle cx="12" cy="4.5" r="1.5"/><path d="M5 8.5l7 1.5 7-1.5M12 10v4.5M12 14.5l-3.5 6.5M12 14.5l3.5 6.5"/>"#
            }
            Icon::Airplane => {
                r#"<path d="M10.5 3.5a1.5 1.5 0 013 0V9l7.5 4.5v2l-7.5-2.3V18l2 1.5V21L12 20l-3.5 1v-1.5l2-1.5v-4.8L3 15.5v-2L10.5 9z"/>"#
            }
            Icon::Alert => r#"<circle cx="12" cy="12" r="9"/><path d="M12 7v6M12 16.5v.5"/>"#,
            Icon::Android => {
                r#"<path d="M4.5 18a7.5 7.5 0 0115 0z"/><path d="M8 11.5L6 8.5M16 11.5l2-3M9.5 14.5v.5M14.5 14.5v.5"/>"#
            }
            Icon::Apps => {
                r#"<circle cx="7" cy="7" r="2.5"/><circle cx="17" cy="7" r="2.5"/><circle cx="7" cy="17" r="2.5"/><circle cx="17" cy="17" r="2.5"/>"#
            }
            Icon::Archive => {
                r#"<rect x="3" y="4" width="18" height="5" rx="1"/><path d="M5 9v10h14V9M10 13h4"/>"#
            }
            Icon::Back => r#"<path d="M15 5l-7 7 7 7"/>"#,
            Icon::Backspace => r#"<path d="M9 5h11v14H9l-6-7z"/><path d="M12 9l5 6M17 9l-5 6"/>"#,
            Icon::Battery => {
                r#"<rect x="7" y="4" width="10" height="17" rx="2"/><path d="M10 2.5h4M10 13h4M10 17h4"/>"#
            }
            Icon::Bell => {
                r#"<path d="M6 16v-5a6 6 0 0112 0v5l1.5 2h-15z"/><path d="M10 20.5a2 2 0 004 0"/>"#
            }
            Icon::Block => r#"<circle cx="12" cy="12" r="8.5"/><path d="M6 6l12 12"/>"#,
            Icon::Bluetooth => r#"<path d="M7 7l10 10-5 4V3l5 4L7 17"/>"#,
            Icon::Chevron => r#"<path d="M9 5l7 7-7 7"/>"#,
            Icon::Close => r#"<path d="M6 6l12 12M18 6L6 18"/>"#,
            Icon::Computer => {
                r#"<rect x="3" y="4.5" width="18" height="12" rx="1.5"/><path d="M8.5 20h7M12 16.5V20"/>"#
            }
            Icon::Copy => {
                r#"<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M4 16V4h12"/>"#
            }
            Icon::Delete => r#"<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/>"#,
            Icon::Delivered => r#"<circle cx="12" cy="12" r="9"/><path d="M7.5 12.5l3 3 6-6.5"/>"#,
            Icon::Display => {
                r#"<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4"/>"#
            }
            Icon::Fingerprint => {
                r#"<path d="M4.5 9a8.5 8.5 0 0115 0M6 16.5c.6-1.5 1-3 1-4.5a5 5 0 0110 0c0 1.5-.1 3-.4 4.4M9.5 20.5c1-2 1.5-4.5 1.5-8.5a1 1 0 012 0c0 2-.2 3.8-.6 5.5"/>"#
            }
            Icon::Flip => {
                r#"<path d="M19.5 10A8 8 0 0 0 5.7 7M4.5 14a8 8 0 0 0 13.8 3"/><path d="M5 3v4h4M19 21v-4h-4"/>"#
            }
            Icon::Gamepad => {
                r#"<path d="M7 7h10a4.5 4.5 0 014.4 5.5l-1 4.3a2.5 2.5 0 01-4.2 1.2L14 16h-4l-2.2 2a2.5 2.5 0 01-4.2-1.2l-1-4.3A4.5 4.5 0 017 7z"/><path d="M8 10v4M6 12h4M16 11v.5M17.5 13v.5"/>"#
            }
            Icon::Gridlines => {
                r#"<rect x="3.5" y="3.5" width="17" height="17" rx="2"/><path d="M9.2 3.5v17M14.8 3.5v17M3.5 9.2h17M3.5 14.8h17"/>"#
            }
            Icon::HangUp => {
                r#"<path transform="rotate(135 12 12)" d="M6 3h3l2 5-2.5 1.5a11 11 0 006 6L16 13l5 2v3a2 2 0 01-2 2A16 16 0 014 5a2 2 0 012-2z"/>"#
            }
            Icon::Headphones => {
                r#"<path d="M4 17v-4a8 8 0 0116 0v4"/><rect x="3.5" y="14" width="4" height="6.5" rx="1.5"/><rect x="16.5" y="14" width="4" height="6.5" rx="1.5"/>"#
            }
            Icon::Hold => r#"<path d="M9 5v14M15 5v14"/>"#,
            Icon::Hotspot => {
                r#"<circle cx="12" cy="12" r="1.5"/><path d="M8.5 15.5a5 5 0 010-7M15.5 8.5a5 5 0 010 7M5.6 18.4a9 9 0 010-12.8M18.4 5.6a9 9 0 010 12.8"/>"#
            }
            Icon::Incoming => r#"<path d="M17 7L7 17M7 9v8h8"/>"#,
            Icon::Info => r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.5"/>"#,
            Icon::Keyboard => {
                r#"<rect x="2.5" y="6" width="19" height="12" rx="1.5"/><path d="M6 9.5h.5M9.5 9.5h.5M13.5 9.5h.5M17.5 9.5h.5M6 12h.5M9.5 12h.5M13.5 12h.5M17.5 12h.5M8 15h8"/>"#
            }
            Icon::Keypad => {
                r#"<circle cx="6" cy="5" r="1"/><circle cx="12" cy="5" r="1"/><circle cx="18" cy="5" r="1"/><circle cx="6" cy="11" r="1"/><circle cx="12" cy="11" r="1"/><circle cx="18" cy="11" r="1"/><circle cx="6" cy="17" r="1"/><circle cx="12" cy="17" r="1"/><circle cx="18" cy="17" r="1"/><circle cx="12" cy="22" r="1"/>"#
            }
            Icon::Language => {
                r#"<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 2.6 3.8 5.6 3.8 9s-1.3 6.4-3.8 9c-2.5-2.6-3.8-5.6-3.8-9S9.5 5.6 12 3z"/>"#
            }
            Icon::Location => {
                r#"<path d="M12 21s-6.5-6-6.5-11a6.5 6.5 0 0113 0c0 5-6.5 11-6.5 11z"/><circle cx="12" cy="10" r="2.5"/>"#
            }
            Icon::Lock => {
                r#"<rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 018 0v3"/>"#
            }
            Icon::Message => r#"<path d="M4 5h16v11H9l-5 4z"/>"#,
            Icon::Missed => r#"<path d="M4 8l7 7 7-7M13 8h5v5"/>"#,
            Icon::Moon => r#"<path d="M20 14.5A8 8 0 119.5 4 6.5 6.5 0 0020 14.5z"/>"#,
            Icon::More => {
                r#"<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>"#
            }
            Icon::Mouse => {
                r#"<rect x="6" y="3" width="12" height="18" rx="6"/><path d="M12 6.5v3.5"/>"#
            }
            Icon::Mute => {
                r#"<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0014 0M12 18v3M4 4l16 16"/>"#
            }
            Icon::Network => r#"<path d="M5 19v-3M10 19v-7M15 19v-10M20 19V5"/>"#,
            Icon::New => r#"<path d="M4 20h4L19 9l-4-4L4 16z"/><path d="M13.5 6.5l4 4"/>"#,
            Icon::Outgoing => r#"<path d="M7 17L17 7M9 7h8v8"/>"#,
            Icon::Palette => {
                r#"<path d="M12 3a9 9 0 100 18c1.2 0 1.8-.9 1.4-2l-.4-1a1.7 1.7 0 011.6-2.3H17a4 4 0 004-4c0-4.8-4-8.7-9-8.7z"/><path d="M7.5 11v.5M10.5 7v.5M15 7.5v.5"/>"#
            }
            Icon::Person => r#"<circle cx="12" cy="8" r="4"/><path d="M4 20a8 8 0 0116 0"/>"#,
            Icon::Phone => {
                r#"<path d="M6 3h3l2 5-2.5 1.5a11 11 0 006 6L16 13l5 2v3a2 2 0 01-2 2A16 16 0 014 5a2 2 0 012-2z"/>"#
            }
            Icon::Photos => {
                r#"<rect x="3.5" y="3.5" width="7" height="7" rx="1.5"/><rect x="13.5" y="3.5" width="7" height="7" rx="1.5"/><rect x="3.5" y="13.5" width="7" height="7" rx="1.5"/><rect x="13.5" y="13.5" width="7" height="7" rx="1.5"/>"#
            }
            Icon::Pin => r#"<path d="M9 4h6l-1 6 4 4H6l4-4z"/><path d="M12 14v6"/>"#,
            Icon::Progress => r#"<path d="M12 3a9 9 0 1 1-9 9"/>"#,
            Icon::Read => r#"<path d="M4.5 12.5l5 5L20 7"/>"#,
            Icon::Recent => r#"<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/>"#,
            Icon::Retry => r#"<path d="M4 12a8 8 0 1 0 2.5-5.8M4 4v5h5"/>"#,
            Icon::Search => r#"<circle cx="11" cy="11" r="6.5"/><path d="M16 16l4.5 4.5"/>"#,
            Icon::Send => r#"<path d="M5 12h13M13 6l6 6-6 6"/>"#,
            Icon::Settings => {
                r#"<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/>"#
            }
            Icon::Share => {
                r#"<circle cx="6" cy="12" r="2.5"/><circle cx="18" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M8.2 10.8l7.6-3.6M8.2 13.2l7.6 3.6"/>"#
            }
            Icon::Shield => {
                r#"<path d="M12 3l7.5 3v5.5c0 4.5-3.2 8.2-7.5 9.5-4.3-1.3-7.5-5-7.5-9.5V6z"/>"#
            }
            Icon::Sim => r#"<path d="M7 3h7l4 4v14H7z"/><path d="M10 12h5v5h-5z"/>"#,
            Icon::Speaker => {
                r#"<path d="M4 9h4l5-4v14l-5-4H4z"/><path d="M16.5 8.5a5 5 0 010 7M19 6a8.5 8.5 0 010 12"/>"#
            }
            Icon::Star => {
                r#"<path d="M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z"/>"#
            }
            Icon::Starred => {
                r#"<path fill="black" d="M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z"/>"#
            }
            Icon::Storage => {
                r#"<ellipse cx="12" cy="6" rx="7.5" ry="2.5"/><path d="M4.5 6v12c0 1.4 3.4 2.5 7.5 2.5s7.5-1.1 7.5-2.5V6M4.5 12c0 1.4 3.4 2.5 7.5 2.5s7.5-1.1 7.5-2.5"/>"#
            }
            Icon::Timer => r#"<circle cx="12" cy="13" r="8"/><path d="M12 9v4l2.5 2M9.5 2.5h5"/>"#,
            Icon::Unarchive => {
                r#"<rect x="3" y="4" width="18" height="5" rx="1"/><path d="M5 9v10h14V9M12 17v-5M9.5 14.5L12 12l2.5 2.5"/>"#
            }
            Icon::Unread => {
                r#"<path d="M4 5h9M4 5v11h5l-4 4v-4M20 11v5H9"/><circle cx="18" cy="6" r="2.5"/>"#
            }
            Icon::Update => r#"<path d="M12 3.5v11M7.5 10l4.5 4.5 4.5-4.5M5 20h14"/>"#,
            Icon::Vibrate => {
                r#"<rect x="8" y="4" width="8" height="16" rx="1.5"/><path d="M4.5 8v8M19.5 8v8M2 10v4M22 10v4"/>"#
            }
            Icon::Wifi => {
                r#"<path d="M2.5 9a14 14 0 0119 0M5.5 12.5a9.5 9.5 0 0113 0M8.5 16a5 5 0 017 0M12 19.5v.5"/>"#
            }
            Icon::WifiFair => {
                r#"<path d="M12 19.5v.5 M8.5 16a5 5 0 017 0"/><path stroke-opacity=".3" d="M5.5 12.5a9.5 9.5 0 0113 0 M2.5 9a14 14 0 0119 0"/>"#
            }
            Icon::WifiGood => {
                r#"<path d="M12 19.5v.5 M8.5 16a5 5 0 017 0 M5.5 12.5a9.5 9.5 0 0113 0"/><path stroke-opacity=".3" d="M2.5 9a14 14 0 0119 0"/>"#
            }
            Icon::WifiWeak => {
                r#"<path d="M12 19.5v.5"/><path stroke-opacity=".3" d="M8.5 16a5 5 0 017 0 M5.5 12.5a9.5 9.5 0 0113 0 M2.5 9a14 14 0 0119 0"/>"#
            }
        }
    }

    fn path(self) -> String {
        format!("icons/{}.svg", self.name())
    }
}

/// `icon`, `size` lp square, in `color`. GPUI paints an svg only in its own
/// text color, never the one it sits in, so the color is not optional.
pub fn icon(icon: Icon, size: gpui::Pixels, color: gpui::Hsla) -> Svg {
    svg()
        .path(icon.path())
        .flex_none()
        .size(size)
        .text_color(color)
}

/// Serves the icons to GPUI under `icons/`.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let Some(name) = path
            .strip_prefix("icons/")
            .and_then(|file| file.strip_suffix(".svg"))
        else {
            return Ok(None);
        };
        Ok(ALL.iter().find(|icon| icon.name() == name).map(|icon| {
            Cow::Owned(
                format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
                    icon.shapes()
                )
                .into_bytes(),
            )
        }))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ALL
            .iter()
            .map(|icon| SharedString::from(icon.path()))
            .filter(|icon_path| icon_path.starts_with(path))
            .collect())
    }
}
