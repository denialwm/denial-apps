//! The apps' icons: 24-unit stroked outlines in one color. An app serves them
//! by passing `Assets` to `Application::with_assets`.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString, Svg, prelude::*, svg};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Alert,
    Archive,
    Back,
    Backspace,
    Block,
    Chevron,
    Close,
    Copy,
    Delete,
    Delivered,
    Flip,
    Gridlines,
    HangUp,
    Hold,
    Incoming,
    Info,
    Keypad,
    Lock,
    Message,
    Missed,
    More,
    Mute,
    Network,
    New,
    Outgoing,
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
    Sim,
    Speaker,
    Star,
    Starred,
    Timer,
    Unarchive,
    Unread,
}

const ALL: [Icon; 44] = [
    Icon::Alert,
    Icon::Archive,
    Icon::Back,
    Icon::Backspace,
    Icon::Block,
    Icon::Chevron,
    Icon::Close,
    Icon::Copy,
    Icon::Delete,
    Icon::Delivered,
    Icon::Flip,
    Icon::Gridlines,
    Icon::HangUp,
    Icon::Hold,
    Icon::Incoming,
    Icon::Info,
    Icon::Keypad,
    Icon::Lock,
    Icon::Message,
    Icon::Missed,
    Icon::More,
    Icon::Mute,
    Icon::Network,
    Icon::New,
    Icon::Outgoing,
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
    Icon::Sim,
    Icon::Speaker,
    Icon::Star,
    Icon::Starred,
    Icon::Timer,
    Icon::Unarchive,
    Icon::Unread,
];

impl Icon {
    fn name(self) -> &'static str {
        match self {
            Icon::Alert => "alert",
            Icon::Archive => "archive",
            Icon::Back => "back",
            Icon::Backspace => "backspace",
            Icon::Block => "block",
            Icon::Chevron => "chevron",
            Icon::Close => "close",
            Icon::Copy => "copy",
            Icon::Delete => "delete",
            Icon::Delivered => "delivered",
            Icon::Flip => "flip",
            Icon::Gridlines => "gridlines",
            Icon::HangUp => "hang-up",
            Icon::Hold => "hold",
            Icon::Incoming => "incoming",
            Icon::Info => "info",
            Icon::Keypad => "keypad",
            Icon::Lock => "lock",
            Icon::Message => "message",
            Icon::Missed => "missed",
            Icon::More => "more",
            Icon::Mute => "mute",
            Icon::Network => "network",
            Icon::New => "new",
            Icon::Outgoing => "outgoing",
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
            Icon::Sim => "sim",
            Icon::Speaker => "speaker",
            Icon::Star => "star",
            Icon::Starred => "starred",
            Icon::Timer => "timer",
            Icon::Unarchive => "unarchive",
            Icon::Unread => "unread",
        }
    }

    /// The shapes inside the 24-unit square.
    fn shapes(self) -> &'static str {
        match self {
            Icon::Alert => r#"<circle cx="12" cy="12" r="9"/><path d="M12 7v6M12 16.5v.5"/>"#,
            Icon::Archive => {
                r#"<rect x="3" y="4" width="18" height="5" rx="1"/><path d="M5 9v10h14V9M10 13h4"/>"#
            }
            Icon::Back => r#"<path d="M15 5l-7 7 7 7"/>"#,
            Icon::Backspace => r#"<path d="M9 5h11v14H9l-6-7z"/><path d="M12 9l5 6M17 9l-5 6"/>"#,
            Icon::Block => r#"<circle cx="12" cy="12" r="8.5"/><path d="M6 6l12 12"/>"#,
            Icon::Chevron => r#"<path d="M9 5l7 7-7 7"/>"#,
            Icon::Close => r#"<path d="M6 6l12 12M18 6L6 18"/>"#,
            Icon::Copy => {
                r#"<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M4 16V4h12"/>"#
            }
            Icon::Delete => r#"<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/>"#,
            Icon::Delivered => r#"<circle cx="12" cy="12" r="9"/><path d="M7.5 12.5l3 3 6-6.5"/>"#,
            Icon::Flip => {
                r#"<path d="M19.5 10A8 8 0 0 0 5.7 7M4.5 14a8 8 0 0 0 13.8 3"/><path d="M5 3v4h4M19 21v-4h-4"/>"#
            }
            Icon::Gridlines => r#"<rect x="3.5" y="3.5" width="17" height="17" rx="2"/><path d="M9.2 3.5v17M14.8 3.5v17M3.5 9.2h17M3.5 14.8h17"/>"#,
            Icon::HangUp => {
                r#"<path transform="rotate(135 12 12)" d="M6 3h3l2 5-2.5 1.5a11 11 0 006 6L16 13l5 2v3a2 2 0 01-2 2A16 16 0 014 5a2 2 0 012-2z"/>"#
            }
            Icon::Hold => r#"<path d="M9 5v14M15 5v14"/>"#,
            Icon::Incoming => r#"<path d="M17 7L7 17M7 9v8h8"/>"#,
            Icon::Info => r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.5"/>"#,
            Icon::Keypad => {
                r#"<circle cx="6" cy="5" r="1"/><circle cx="12" cy="5" r="1"/><circle cx="18" cy="5" r="1"/><circle cx="6" cy="11" r="1"/><circle cx="12" cy="11" r="1"/><circle cx="18" cy="11" r="1"/><circle cx="6" cy="17" r="1"/><circle cx="12" cy="17" r="1"/><circle cx="18" cy="17" r="1"/><circle cx="12" cy="22" r="1"/>"#
            }
            Icon::Lock => {
                r#"<rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 018 0v3"/>"#
            }
            Icon::Message => r#"<path d="M4 5h16v11H9l-5 4z"/>"#,
            Icon::Missed => r#"<path d="M4 8l7 7 7-7M13 8h5v5"/>"#,
            Icon::More => {
                r#"<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>"#
            }
            Icon::Mute => {
                r#"<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0014 0M12 18v3M4 4l16 16"/>"#
            }
            Icon::Network => r#"<path d="M5 19v-3M10 19v-7M15 19v-10M20 19V5"/>"#,
            Icon::New => r#"<path d="M4 20h4L19 9l-4-4L4 16z"/><path d="M13.5 6.5l4 4"/>"#,
            Icon::Outgoing => r#"<path d="M7 17L17 7M9 7h8v8"/>"#,
            Icon::Person => r#"<circle cx="12" cy="8" r="4"/><path d="M4 20a8 8 0 0116 0"/>"#,
            Icon::Phone => {
                r#"<path d="M6 3h3l2 5-2.5 1.5a11 11 0 006 6L16 13l5 2v3a2 2 0 01-2 2A16 16 0 014 5a2 2 0 012-2z"/>"#
            }
            Icon::Photos => r#"<rect x="3.5" y="3.5" width="7" height="7" rx="1.5"/><rect x="13.5" y="3.5" width="7" height="7" rx="1.5"/><rect x="3.5" y="13.5" width="7" height="7" rx="1.5"/><rect x="13.5" y="13.5" width="7" height="7" rx="1.5"/>"#,
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
            Icon::Timer => r#"<circle cx="12" cy="13" r="8"/><path d="M12 9v4l2.5 2M9.5 2.5h5"/>"#,
            Icon::Unarchive => {
                r#"<rect x="3" y="4" width="18" height="5" rx="1"/><path d="M5 9v10h14V9M12 17v-5M9.5 14.5L12 12l2.5 2.5"/>"#
            }
            Icon::Unread => {
                r#"<path d="M4 5h9M4 5v11h5l-4 4v-4M20 11v5H9"/><circle cx="18" cy="6" r="2.5"/>"#
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
