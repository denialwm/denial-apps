//! The two dialogs, drawn with GPUI: the authentication prompt (`prompt`)
//! and denial-portal's Access request (`access`).
//!
//! Each is a centered, undecorated layer-shell overlay with exclusive
//! keyboard focus, named `dev.denial.*` so the shell treats it as its own.
//! On the desktop it is a transparent canvas the card sits in: Denial
//! frosts the desktop under it wherever its alpha passes the user's
//! opacity threshold, so the card is glass and its shadow is not. On a
//! phone (`DENIAL_SHELL_PROFILE=mobile`) the overlay is the card itself and
//! takes the card's height: the shell centers it above the keyboard, dims
//! the rest and draws the phone's window glass under all of it. There the
//! card is an island of the mobile materials, as Settings' dialogs are.

mod access;
mod link;
mod prompt;

use std::time::Duration;

use denial_look::{Appearance, Look, floating};
use denial_look_gpui::{color, solid};
use gpui::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
use gpui::{
    App, AsyncApp, BoxShadow, Div, ElementId, Font, FontWeight, KeyBinding, Pixels, SharedString,
    Stateful, Styled, TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, actions, canvas, div, point, prelude::*, px, size,
};
use gpui_platform::application;
use serde_json::Value;

actions!(polkit_dialog, [Cancel, Confirm]);

/// The canvas a desktop overlay asks for, and the card inside it.
const CANVAS: (f32, f32) = (600.0, 520.0);
const CARD_WIDTH: f32 = 520.0;
const CANVAS_MARGIN: f32 = 28.0;
/// The gap a phone leaves at each side of the card.
const PHONE_MARGIN: f32 = 8.0;
/// A card that does not show within this long shows anyway.
const REVEAL_DEADLINE: Duration = Duration::from_millis(900);
/// Families for the command line, the first installed one wins.
const MONOSPACE: &[&str] = &[
    "JetBrains Mono",
    "JetBrainsMono Nerd Font",
    "DejaVu Sans Mono",
    "Adwaita Mono",
    "Noto Sans Mono",
    "Liberation Mono",
];
/// The shell's blur threshold when the settings name none.
const DEFAULT_THRESHOLD: f32 = 0.19;

#[derive(Clone, Copy)]
pub enum Kind {
    /// The authentication prompt; the agent started it.
    Prompt,
    /// An Access request; denial-portal started it.
    Access,
}

pub fn run(kind: Kind) {
    let (socket, token, namespace, title) = match kind {
        Kind::Prompt => (
            "DENIAL_POLKIT_SOCKET",
            "DENIAL_POLKIT_TOKEN",
            "dev.denial.Polkit",
            "Authentication",
        ),
        Kind::Access => (
            "DENIAL_ACCESS_SOCKET",
            "DENIAL_ACCESS_TOKEN",
            "dev.denial.Access",
            "Permission",
        ),
    };
    let (link, incoming) = match link::Link::connect(socket, token) {
        Ok(connected) => connected,
        Err(error) => {
            eprintln!("denial-polkit-agent: {error}");
            std::process::exit(1);
        }
    };
    denial_look::words::register_chinese(crate::words::CHINESE);
    let look = Look::load();
    let material = Material::new(&look, &appearance());
    let phone = std::env::var("DENIAL_SHELL_PROFILE").is_ok_and(|profile| profile == "mobile");
    application()
        .with_assets(denial_look_gpui::Assets)
        .run(move |cx: &mut App| {
            // Drawn at the Flutter dialog's sizes, without the apps' zoom.
            denial_look_gpui::input::bind_keys(cx);
            // Denial's Back gesture arrives as KEY_BACK, which GPUI names `back`.
            cx.bind_keys([
                KeyBinding::new("escape", Cancel, None),
                KeyBinding::new("back", Cancel, None),
                KeyBinding::new("enter", Confirm, Some("Dialog")),
            ]);
            let names = cx.text_system().all_font_names();
            let font = gpui::font(look.font_family(&names));
            let mono = MONOSPACE
                .iter()
                .find(|family| names.iter().any(|name| name == *family))
                .map(|family| SharedString::from(*family));
            let frame = Frame {
                look,
                material,
                phone,
                font,
                mono,
            };
            let options = frame.window_options(namespace, title);
            match kind {
                Kind::Prompt => {
                    let window = cx
                        .open_window(options, |window, cx| {
                            cx.new(|cx| prompt::Prompt::new(frame, link, window, cx))
                        })
                        .expect("the authentication dialog's window");
                    follow(window, incoming, cx);
                }
                Kind::Access => {
                    let window = cx
                        .open_window(options, |window, cx| {
                            cx.new(|cx| access::Access::new(frame, link, window, cx))
                        })
                        .expect("the Access dialog's window");
                    follow(window, incoming, cx);
                }
            }
            cx.activate(true);
        });
}

/// What a dialog does with a message from the other end.
trait Receive: 'static {
    fn receive(&mut self, message: Value, window: &mut Window, cx: &mut gpui::Context<Self>)
    where
        Self: Sized;
}

/// Hands the other end's messages to the dialog. The other end ends the
/// dialog by closing the socket.
fn follow<V: Receive + Render>(
    window: WindowHandle<V>,
    incoming: async_channel::Receiver<Value>,
    cx: &mut App,
) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Ok(message) = incoming.recv().await {
            let delivered = window.update(cx, |view, window, cx| view.receive(message, window, cx));
            if delivered.is_err() {
                break;
            }
        }
        std::process::exit(0);
    })
    .detach();
}

/// Denial's appearance settings, for what `Look` does not keep.
fn appearance() -> Value {
    let path = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
        })
        .map(|dir| dir.join("denial/settings.json"));
    path.and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .map(|settings| settings["appearance"].clone())
        .unwrap_or(Value::Null)
}

/// The card's fill and shadow.
#[derive(Clone, Copy)]
struct Material {
    fill: u32,
    alpha: f32,
    /// Black at this alpha under a desktop card; none on a phone.
    shadow: f32,
    /// The card is glass on the phone, or would frost on the desktop.
    glass: bool,
    /// Alpha of the toolbar material under the field and the buttons: the
    /// app-panel opacity, opaque with transparency off.
    toolbar: f32,
}

impl Material {
    /// As the Flutter dialog resolved it. The desktop compositor frosts the
    /// overlay per pixel, only where its alpha passes the user's threshold:
    /// the card stays above it, its shadow below.
    fn new(look: &Look, appearance: &Value) -> Self {
        let dark = look.appearance == denial_look::Appearance::Dark;
        let fallback_shadow = if dark { 0.42 } else { 0.2 };
        let transparent = matches!(
            appearance["transparencyMode"].as_str(),
            Some("glass" | "blur")
        ) && look.window_alpha < 1.0;
        if !transparent {
            return Self {
                fill: look.palette.raised,
                alpha: 1.0,
                shadow: fallback_shadow,
                glass: false,
                toolbar: 1.0,
            };
        }
        let threshold = appearance["backdropBlurOpacityThreshold"]
            .as_f64()
            .map(|value| value as f32)
            .filter(|value| value.is_finite())
            .unwrap_or(DEFAULT_THRESHOLD)
            .clamp(0.0, 1.0);
        let frosted = threshold < 1.0;
        let panel = look.panel_alpha;
        // Above the threshold, with a floor that keeps text legible; on a
        // phone, the island floor the mobile materials keep.
        let alpha = if frosted {
            panel.max(0.34).max((threshold + 0.05).min(1.0))
        } else {
            panel.max(0.92)
        }
        .max(look.palette.island_floor.min(1.0));
        let budget = threshold - 0.015;
        let shadow = if !frosted {
            fallback_shadow
        } else if budget < 0.03 {
            0.0
        } else {
            fallback_shadow.min(budget * 0.7)
        };
        Self {
            fill: look.palette.glass,
            alpha,
            shadow,
            glass: true,
            toolbar: look.panel_alpha,
        }
    }
}

/// What both dialogs share: the look, the card and its controls.
pub struct Frame {
    pub look: Look,
    material: Material,
    /// The overlay is the card, on a phone.
    pub phone: bool,
    pub font: Font,
    /// The command line's family, when one of `MONOSPACE` is installed.
    pub mono: Option<SharedString>,
}

impl Frame {
    fn window_options(&self, namespace: &str, title: &str) -> WindowOptions {
        // A phone stretches the card across its screen; the dialog sets its
        // height once it is laid out.
        let (canvas, anchor, margin) = if self.phone {
            let margin = px(PHONE_MARGIN);
            (
                size(px(0.), px(360.)),
                Anchor::LEFT | Anchor::RIGHT,
                Some((px(0.), margin, px(0.), margin)),
            )
        } else {
            (size(px(CANVAS.0), px(CANVAS.1)), Anchor::empty(), None)
        };
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(gpui::Bounds {
                origin: point(px(0.), px(0.)),
                size: canvas,
            })),
            window_background: WindowBackgroundAppearance::Transparent,
            titlebar: Some(TitlebarOptions {
                title: Some(denial_look_gpui::say(title.to_owned())),
                ..Default::default()
            }),
            app_id: Some(namespace.to_owned()),
            kind: WindowKind::LayerShell(LayerShellOptions {
                namespace: namespace.to_owned(),
                layer: Layer::Overlay,
                anchor,
                exclusive_zone: None,
                exclusive_edge: None,
                margin,
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
            }),
            focus: true,
            show: true,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            ..Default::default()
        }
    }

    /// The shell's panel radius, which the phone's glass under the card
    /// takes too.
    fn card_radius(&self) -> f32 {
        let roundness = self.look.window_radius / 20.0;
        (28.0 * roundness).clamp(8.0, 44.0)
    }

    /// The window radius less the toolbar's 8 lp inset, at least 4.
    fn toolbar_radius(&self) -> Pixels {
        px((self.look.window_radius - 8.0).max(4.0))
    }

    /// The floating toolbar material the reply field and the buttons sit
    /// on, as application search fields and toolbar actions have it.
    fn toolbar<E: Styled>(&self, element: E) -> E {
        let look = &self.look;
        let dark = look.appearance == Appearance::Dark;
        element
            .rounded(self.toolbar_radius())
            .bg(color(look.palette.glass, self.material.toolbar))
            .border(px(0.75))
            .border_color(color(
                look.palette.foreground,
                if dark { 0.12 } else { 0.10 },
            ))
            .shadow(vec![BoxShadow {
                color: color(0x000000, if dark { 0.18 } else { 0.08 }),
                offset: point(px(0.), px(6.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
    }

    /// Foreground at this alpha over the toolbar material while pressed or
    /// hovered.
    const PRESSED: f32 = 0.1;
    const HOVERED: f32 = 0.06;

    /// The card, holding `content`, in the canvas: centered on the desktop;
    /// the whole overlay on a phone, which is fitted to its height.
    /// `revealed` false keeps the canvas empty.
    fn canvas(&self, revealed: bool, content: Div, window: &mut Window) -> Div {
        let look = &self.look;
        let material = self.material;
        let mut card = div()
            .flex()
            .flex_col()
            .font(self.font.clone())
            .text_size(px(15.))
            .text_color(solid(look.palette.foreground))
            .rounded(px(self.card_radius()))
            .bg(color(material.fill, material.alpha))
            .child(content);
        if material.glass || look.appearance == denial_look::Appearance::Light {
            card = card
                .border(px(floating::EDGE_WIDTH))
                .border_color(color(look.palette.foreground, look.palette.edge_alpha));
        }
        if !revealed {
            return div().size_full();
        }
        if self.phone {
            let viewport = window.viewport_size();
            // Fit the overlay to the card, once the card is laid out.
            let fit = canvas(
                move |bounds, window, _| {
                    let height = bounds.size.height.ceil();
                    if (height - viewport.height).abs() > px(0.5) && height > px(0.) {
                        window.resize(size(viewport.width, height));
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full();
            return div()
                .w_full()
                .flex()
                .flex_col()
                .child(card.relative().w_full().p(px(22.)).pb(px(20.)).child(fit));
        }
        let viewport = window.viewport_size();
        let width = px(CARD_WIDTH)
            .min(viewport.width - px(CANVAS_MARGIN * 2.))
            .max(px(0.));
        let shadow = (material.shadow > 0.0).then(|| BoxShadow {
            color: color(0x000000, material.shadow),
            offset: point(px(0.), px(8.)),
            blur_radius: px(28.),
            spread_radius: px(0.),
            inset: false,
        });
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                card.w(width)
                    .max_h(viewport.height - px(CANVAS_MARGIN * 2.))
                    .p(px(22.))
                    .pb(px(20.))
                    .shadow(shadow.into_iter().collect()),
            )
    }

    /// A button on the toolbar material, 44 high. The one that goes ahead
    /// is told apart by an accent tint alone. `live` false shows its label
    /// faint; the caller adds what a click does.
    fn button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        primary: bool,
        live: bool,
    ) -> Stateful<Div> {
        let look = &self.look;
        let foreground = look.palette.foreground;
        let radius = self.toolbar_radius();
        let face = div()
            .id("face")
            .size_full()
            .px(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(radius)
            .whitespace_nowrap()
            .text_size(px(14.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(color(foreground, if live { 1.0 } else { 0.45 }))
            .child(denial_look_gpui::say(label));
        let face = if live {
            face.hover(move |s| s.bg(color(foreground, Self::HOVERED)))
                .active(move |s| s.bg(color(foreground, Self::PRESSED)))
        } else {
            face
        };
        let tint = div()
            .size_full()
            .rounded(radius)
            .when(primary, |tint| tint.bg(color(look.accent, 0.28)))
            .child(face);
        self.toolbar(div().id(id))
            .h(px(44.))
            .min_w(px(84.))
            .flex_none()
            .when(live, |button| button.cursor_pointer())
            .child(tint)
    }

    /// A row of buttons at the end.
    fn buttons(&self, buttons: impl IntoIterator<Item = Stateful<Div>>) -> Div {
        div()
            .flex()
            .gap(px(10.))
            .pt(px(8.))
            .justify_end()
            .children(buttons)
    }
}

/// A monogram in a circle, for an account or app without a picture.
fn monogram(name: &str, diameter: f32) -> Div {
    let fill = denial_look::palette::avatar(name);
    let letter: String = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".to_owned());
    div()
        .size(px(diameter))
        .flex_none()
        .rounded_full()
        .bg(solid(fill))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(diameter * 0.42))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(solid(denial_look::on(fill)))
        .child(letter)
}
