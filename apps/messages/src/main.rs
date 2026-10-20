mod data;
mod input;
mod messages;

use gpui::{
    App, Bounds, Focusable, KeyBinding, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, prelude::*, px, size,
};
use gpui_platform::application;

use messages::{Back, Messages};

fn main() {
    application().run(|cx: &mut App| {
        input::bind_keys(cx);
        cx.bind_keys([KeyBinding::new("escape", Back, None)]);

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
                    app_id: Some("org.denialwm.MessagesSample".into()),
                    ..Default::default()
                },
                |_, cx| cx.new(Messages::new),
            )
            .unwrap();
        window
            .update(cx, |view, window, cx| {
                // Default text and rem spacing follow the dp layout too.
                window.set_rem_size(messages::dp(16.));
                window.focus(&view.focus_handle(cx), cx);
            })
            .unwrap();
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}
