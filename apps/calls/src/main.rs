mod calls;
mod data;
mod instance;
mod local;
mod people;
mod samples;

use futures_lite::future;
use gpui::{
    App, Bounds, Focusable, KeyBinding, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, prelude::*, px, size,
};
use gpui_platform::application;
use zbus::fdo::RequestNameFlags;

use crate::calls::{Back, Calls};
use crate::instance::{InstanceProxy, Open, Opener};

fn main() {
    // `calls <number>` opens the keypad with the number on it.
    let number = std::env::args().nth(1);
    let (opener, opens) = async_channel::unbounded();
    // The bus name is held for as long as this connection lives.
    let bus = match future::block_on(claim(opener)) {
        Ok(bus) => Some(bus),
        Err(zbus::Error::NameTaken) => return forward(number.unwrap_or_default()),
        Err(error) => {
            eprintln!("calls: session bus: {error}");
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
                            title: Some("Calls".into()),
                            ..Default::default()
                        }),
                        app_id: Some(instance::NAME.into()),
                        ..Default::default()
                    },
                    |window, cx| cx.new(|cx| Calls::new(number, window, cx)),
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
                while let Ok((number, token)) = opens.recv().await {
                    let shown = window.update(cx, |view, window, cx| {
                        if !number.is_empty() {
                            view.show_number(number, window, cx);
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

/// Serves `Opener` and takes Calls' bus name, or fails with NameTaken while
/// another Calls runs.
async fn claim(opener: async_channel::Sender<Open>) -> zbus::Result<zbus::Connection> {
    let bus = zbus::connection::Builder::session()?
        .serve_at(instance::PATH, Opener(opener))?
        .build()
        .await?;
    // Not queued behind the running Calls: refused at once.
    bus.request_name_with_flags(instance::NAME, RequestNameFlags::DoNotQueue.into())
        .await?;
    Ok(bus)
}

/// Hands `number` and this start's activation token to the running Calls.
fn forward(number: String) {
    let token = std::env::var("XDG_ACTIVATION_TOKEN").unwrap_or_default();
    let opened = future::block_on(async {
        let bus = zbus::Connection::session().await?;
        InstanceProxy::new(&bus).await?.open(&number, &token).await
    });
    if let Err(error) = opened {
        eprintln!("calls: opening in the running Calls: {error}");
    }
}
