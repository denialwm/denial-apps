mod camera;
mod engine;
mod library;
mod words;

use gpui::{
    App, Bounds, Focusable, KeyBinding, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, prelude::*, px, size,
};
use gpui_platform::application;

use crate::camera::{Back, Camera};

const APP_ID: &str = "org.denialwm.Camera";

fn main() {
    denial_look::words::register_chinese(words::CHINESE);
    let (commands, inbox) = engine::commands();
    // One preview frame waits at most; the camera drops frames the window
    // has no time for.
    let (engine_events, events) = async_channel::bounded(1);
    std::thread::Builder::new()
        .name("camera".into())
        .spawn(move || engine::run(inbox, engine_events))
        .expect("camera thread");
    let (library, library_requests) = async_channel::unbounded();
    let (library_loaded, loaded) = async_channel::bounded(4);
    std::thread::Builder::new()
        .name("library".into())
        .spawn(move || library::run(library_requests, library_loaded))
        .expect("library thread");
    application()
        .with_assets(denial_look_gpui::Assets)
        .run(move |cx: &mut App| {
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
                        window_background: WindowBackgroundAppearance::Opaque,
                        titlebar: Some(TitlebarOptions {
                            title: Some("Camera".into()),
                            ..Default::default()
                        }),
                        app_id: Some(APP_ID.into()),
                        ..Default::default()
                    },
                    |window, cx| cx.new(|cx| Camera::new(commands, events, library, loaded, window, cx)),
                )
                .unwrap();
            window
                .update(cx, |view, window, cx| {
                    // Default text and rem spacing follow the dp layout too.
                    window.set_rem_size(denial_look_gpui::dp(16.));
                    window.focus(&view.focus_handle(cx), cx);
                })
                .unwrap();
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            cx.activate(true);
        });
}

