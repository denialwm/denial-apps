//! The two dialogs, drawn with GPUI: the authentication prompt (`prompt`)
//! and denial-portal's Access request (`access`).
//!
//! Each is named `dev.denial.Polkit` or `dev.denial.Access` and takes the
//! keyboard only when clicked or tapped: typing meant for another window
//! never lands in it. Its window opens once the other end's first message
//! is in, so it never shows empty, and takes the card's height.
//!
//! On the desktop it is an app window, as Settings is: Denial frames,
//! rounds and frosts it, and opens it pinned above every window, centered
//! and without the keyboard (its `own_dialogs`). On a phone
//! (`DENIAL_SHELL_PROFILE=mobile`) it is a layer-shell overlay across the
//! screen: the shell centers it above the keyboard, dims the rest and draws
//! the phone's window glass under all of it. There the card is an island of
//! the mobile materials, as Settings' dialogs are.

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
    WindowKind, WindowOptions, actions, canvas, div, point, prelude::*, px, size,
};
use gpui_platform::application;
use serde_json::Value;

actions!(polkit_dialog, [Cancel, Confirm]);

/// The desktop window's width, and its height until the card is laid out.
const DESKTOP_SIZE: (f32, f32) = (520.0, 320.0);
/// The gap a phone leaves at each side of the card.
const PHONE_MARGIN: f32 = 8.0;
/// A window whose first message does not come within this long opens
/// anyway.
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
    let phone = std::env::var("DENIAL_SHELL_PROFILE").is_ok_and(|profile| profile == "mobile");
    let material = Material::new(&look, phone);
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
            // The window never asks to be activated: it waits to be clicked.
            match kind {
                Kind::Prompt => open(
                    options,
                    |window, cx| prompt::Prompt::new(frame, link, window, cx),
                    incoming,
                    cx,
                ),
                Kind::Access => open(
                    options,
                    |window, cx| access::Access::new(frame, link, window, cx),
                    incoming,
                    cx,
                ),
            }
        });
}

/// What a dialog does with a message from the other end.
trait Receive: 'static {
    fn receive(&mut self, message: Value, window: &mut Window, cx: &mut gpui::Context<Self>)
    where
        Self: Sized;
}

/// Opens the dialog's window once the other end's first message is in, or
/// after `REVEAL_DEADLINE`, then hands the dialog every message. The other
/// end ends the dialog by closing the socket.
fn open<V: Receive + Render>(
    options: WindowOptions,
    build: impl FnOnce(&mut Window, &mut gpui::Context<V>) -> V + 'static,
    incoming: async_channel::Receiver<Value>,
    cx: &mut App,
) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        let deadline = cx.background_executor().timer(REVEAL_DEADLINE);
        let first =
            futures_lite::future::or(async { incoming.recv().await.ok().map(Some) }, async {
                deadline.await;
                Some(None)
            })
            .await;
        // Closed before saying anything.
        let Some(first) = first else {
            std::process::exit(0);
        };
        let window = cx
            .open_window(options, |window, cx| cx.new(|cx| build(window, cx)))
            .expect("the dialog's window");
        let mut next = first;
        loop {
            let message = match next.take() {
                Some(message) => message,
                None => match incoming.recv().await {
                    Ok(message) => message,
                    Err(_) => break,
                },
            };
            let delivered = window.update(cx, |view, window, cx| view.receive(message, window, cx));
            if delivered.is_err() {
                break;
            }
        }
        std::process::exit(0);
    })
    .detach();
}

/// The card's fill.
#[derive(Clone, Copy)]
struct Material {
    fill: u32,
    alpha: f32,
    /// Alpha of the toolbar material under the field and the buttons: a
    /// glass group's.
    toolbar: f32,
}

impl Material {
    /// As Settings' window and dialogs have it. On the desktop the card is
    /// the window: its backing at the window opacity, which Denial frosts,
    /// with glass groups on it. On a phone the shell draws the window glass
    /// under the card, which is an island over it.
    fn new(look: &Look, phone: bool) -> Self {
        let (fill, alpha) = if phone {
            look.island_fill()
        } else {
            look.backing()
        };
        Self {
            fill,
            alpha,
            toolbar: look.group_alpha(),
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
        // Both take the card's height once it is laid out.
        let (bounds, kind) = if self.phone {
            // Across the screen less the gutters.
            let margin = px(PHONE_MARGIN);
            (
                size(px(0.), px(360.)),
                WindowKind::LayerShell(LayerShellOptions {
                    namespace: namespace.to_owned(),
                    layer: Layer::Overlay,
                    anchor: Anchor::LEFT | Anchor::RIGHT,
                    exclusive_zone: None,
                    exclusive_edge: None,
                    margin: Some((px(0.), margin, px(0.), margin)),
                    // On top, but never a keyboard grab: an exclusive layer
                    // takes every key, wherever the user is typing.
                    keyboard_interactivity: KeyboardInteractivity::OnDemand,
                }),
            )
        } else {
            (
                size(px(DESKTOP_SIZE.0), px(DESKTOP_SIZE.1)),
                WindowKind::Normal,
            )
        };
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(gpui::Bounds {
                origin: point(px(0.), px(0.)),
                size: bounds,
            })),
            window_background: WindowBackgroundAppearance::Transparent,
            titlebar: Some(TitlebarOptions {
                title: Some(denial_look_gpui::say(title.to_owned())),
                ..Default::default()
            }),
            app_id: Some(namespace.to_owned()),
            kind,
            focus: false,
            show: true,
            is_movable: true,
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

    /// The card, holding `content`: the whole window, which takes its
    /// height. A phone's card is an island with the panel radius, as the
    /// shell's glass under it; Denial frames, rounds and frosts the desktop
    /// window itself. `shake` moves the content sideways within the card.
    fn canvas(&self, shake: f32, content: Div, window: &mut Window) -> Div {
        let look = &self.look;
        let material = self.material;
        let viewport = window.viewport_size();
        // Fit the window to the card, once the card is laid out.
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
        let mut card = div()
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .p(px(22.))
            .pb(px(20.))
            .font(self.font.clone())
            .text_size(px(15.))
            .text_color(solid(look.palette.foreground))
            .bg(color(material.fill, material.alpha))
            .child(content.relative().left(px(shake)))
            .child(fit);
        if self.phone {
            card = card.rounded(px(self.card_radius()));
            if material.alpha < 1.0 || look.appearance == Appearance::Light {
                card = card
                    .border(px(floating::EDGE_WIDTH))
                    .border_color(color(look.palette.foreground, look.palette.edge_alpha));
            }
        }
        div().w_full().flex().flex_col().child(card)
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
