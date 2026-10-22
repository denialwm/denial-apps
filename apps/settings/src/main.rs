mod battery;
mod bluetooth;
mod catalog;
mod denial;
mod fingerprint;
mod instance;
mod lock;
mod mobile;
mod props;
mod settings;
mod values;
mod wallpaper;
mod wifi;

use futures_lite::future;
use gpui::{
    App, Bounds, Focusable, KeyBinding, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, prelude::*, px, size,
};
use gpui_platform::application;
use zbus::fdo::RequestNameFlags;

use crate::instance::{InstanceProxy, Open, Opener};
use crate::settings::{Back, Settings};

fn main() {
    // `settings <page>` opens on that page.
    let page = std::env::args().nth(1);
    let (opener, opens) = async_channel::unbounded();
    // The bus name is held for as long as this connection lives.
    let bus = match future::block_on(claim(opener)) {
        Ok(bus) => Some(bus),
        Err(zbus::Error::NameTaken) => return forward(page.unwrap_or_default()),
        Err(error) => {
            eprintln!("settings: session bus: {error}");
            None
        }
    };
    application()
        .with_assets(denial_look_gpui::Assets)
        .run(move |cx: &mut App| {
            denial_look_gpui::input::bind_keys(cx);
            // Denial's Back gesture arrives as KEY_BACK, which GPUI names `back`.
            cx.bind_keys([
                KeyBinding::new("back", Back, None),
                KeyBinding::new("escape", Back, None),
            ]);

            let bounds = Bounds::centered(None, size(px(540.), px(1100.)), cx);
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        window_background: WindowBackgroundAppearance::Transparent,
                        titlebar: Some(TitlebarOptions {
                            title: Some("Settings".into()),
                            ..Default::default()
                        }),
                        app_id: Some(instance::NAME.into()),
                        ..Default::default()
                    },
                    |window, cx| cx.new(|cx| Settings::new(page, window, cx)),
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
                while let Ok((page, token)) = opens.recv().await {
                    let shown = window.update(cx, |view, window, cx| {
                        if !page.is_empty() {
                            view.show_page(&page, window, cx);
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

/// Serves `Opener` and takes Settings' bus name, or fails with NameTaken
/// while another Settings runs.
async fn claim(opener: async_channel::Sender<Open>) -> zbus::Result<zbus::Connection> {
    let bus = zbus::connection::Builder::session()?
        .serve_at(instance::PATH, Opener(opener))?
        .build()
        .await?;
    // Not queued behind the running Settings: refused at once.
    bus.request_name_with_flags(instance::NAME, RequestNameFlags::DoNotQueue.into())
        .await?;
    Ok(bus)
}

/// Hands `page` and this start's activation token to the running Settings.
fn forward(page: String) {
    let token = std::env::var("XDG_ACTIVATION_TOKEN").unwrap_or_default();
    let opened = future::block_on(async {
        let bus = zbus::Connection::session().await?;
        InstanceProxy::new(&bus).await?.open(&page, &token).await
    });
    if let Err(error) = opened {
        eprintln!("settings: opening in the running Settings: {error}");
    }
}
