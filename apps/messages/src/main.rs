mod data;
mod detect;
mod input;
mod local;
mod messages;
mod people;
mod samples;
mod segments;

use ::messages::app::{self, AppProxy};
use async_channel::Sender;
use futures_lite::future;
use gpui::{
    App, Bounds, Focusable, KeyBinding, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, prelude::*, px, size,
};
use gpui_platform::application;
use zbus::fdo::RequestNameFlags;

use crate::messages::{Back, Messages};

/// A conversation the running Messages is asked to show, or none for the
/// window as it is, with the activation token to raise the window with.
type Open = (String, String);

/// `org.denialwm.Messages1` on Messages' bus name.
struct Opener(Sender<Open>);

#[zbus::interface(name = "org.denialwm.Messages1")]
impl Opener {
    async fn open(&self, peer: String, activation_token: String) {
        // Closed only once the app is quitting.
        let _ = self.0.send((peer, activation_token)).await;
    }
}

fn main() {
    // A notification starts Messages with its conversation's peer.
    let peer = std::env::args().nth(1);
    let (opener, opens) = async_channel::unbounded();
    // The bus name is held for as long as this connection lives.
    let bus = match future::block_on(claim(opener)) {
        Ok(bus) => Some(bus),
        Err(zbus::Error::NameTaken) => return forward(peer.unwrap_or_default()),
        Err(error) => {
            eprintln!("messages: session bus: {error}");
            None
        }
    };
    application()
        .with_assets(denial_look_gpui::Assets)
        .run(move |cx: &mut App| {
            input::bind_keys(cx);
            // Denial's Back gesture arrives as KEY_BACK, which GPUI names `back`.
            cx.bind_keys([
                KeyBinding::new("back", Back, None),
                KeyBinding::new("escape", Back, None),
            ]);

            let bounds = Bounds::centered(None, size(px(420.), px(820.)), cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        window_background: WindowBackgroundAppearance::Transparent,
                        titlebar: Some(TitlebarOptions {
                            title: Some("Messages".into()),
                            ..Default::default()
                        }),
                        app_id: Some(app::NAME.into()),
                        ..Default::default()
                    },
                    |window, cx| cx.new(|cx| Messages::new(peer, window, cx)),
                )
                .unwrap();
            window
                .update(cx, |view, window, cx| {
                    // Default text and rem spacing follow the dp layout too.
                    window.set_rem_size(denial_look_gpui::dp(16.));
                    window.focus(&view.focus_handle(cx), cx);
                })
                .unwrap();
            cx.spawn(async move |cx| {
                let _bus = bus;
                while let Ok((peer, token)) = opens.recv().await {
                    let shown = window.update(cx, |view, window, cx| {
                        if !peer.is_empty() {
                            view.open_peer(peer, window, cx);
                        }
                        if token.is_empty() {
                            window.activate_window();
                        } else {
                            window.activate_window_with_token(token);
                        }
                    });
                    if shown.is_err() {
                        return;
                    }
                }
            })
            .detach();
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            cx.activate(true);
        });
}

/// Serves `Opener` and takes Messages' bus name, or fails with NameTaken
/// while another Messages runs.
async fn claim(opener: Sender<Open>) -> zbus::Result<zbus::Connection> {
    let bus = zbus::connection::Builder::session()?
        .serve_at(app::PATH, Opener(opener))?
        .build()
        .await?;
    // Not queued behind the running Messages: refused at once.
    bus.request_name_with_flags(app::NAME, RequestNameFlags::DoNotQueue.into())
        .await?;
    Ok(bus)
}

/// Hands `peer` and this start's activation token to the running Messages.
fn forward(peer: String) {
    let token = std::env::var("XDG_ACTIVATION_TOKEN").unwrap_or_default();
    let opened = future::block_on(async {
        let bus = zbus::Connection::session().await?;
        AppProxy::new(&bus).await?.open(&peer, &token).await
    });
    if let Err(error) = opened {
        eprintln!("messages: opening in the running Messages: {error}");
    }
}
