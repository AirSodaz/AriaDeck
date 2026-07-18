use ariadeck_ui::{AppShell, Theme};
use gpui::{App, AppContext as _, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_platform::application;

fn main() {
    ariadeck_telemetry::init("ariadeck=info");
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting AriaDeck");

    application().run(|cx: &mut App| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1180.0), px(760.0)), cx);

        let open_result = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..WindowOptions::default()
            },
            |_, cx| cx.new(|_| AppShell::new(Theme::dark())),
        );

        if let Err(error) = open_result {
            tracing::error!(?error, "failed to open the AriaDeck window");
            cx.quit();
            return;
        }

        cx.activate(true);
    });
}
