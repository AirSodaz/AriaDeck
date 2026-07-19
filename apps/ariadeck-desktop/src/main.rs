#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod workspace;

use std::sync::Arc;

use gpui::{App, AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use gpui_platform::application;
use tokio::runtime::Builder;

use crate::workspace::DesktopRoot;

fn main() {
    ariadeck_telemetry::init("ariadeck=info");
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting AriaDeck");

    let runtime = match Builder::new_multi_thread()
        .enable_all()
        .thread_name("ariadeck-async")
        .build()
    {
        Ok(runtime) => Arc::new(runtime),
        Err(error) => {
            tracing::error!(%error, "failed to initialize the asynchronous runtime");
            return;
        }
    };

    application().run(move |cx: &mut App| {
        ariadeck_ui::init(cx);
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
                titlebar: Some(TitlebarOptions {
                    title: Some("AriaDeck".into()),
                    ..TitlebarOptions::default()
                }),
                ..WindowOptions::default()
            },
            {
                let runtime = runtime.clone();
                move |window, cx| cx.new(|cx| DesktopRoot::new(runtime.clone(), window, cx))
            },
        );

        if let Err(error) = open_result {
            tracing::error!(?error, "failed to open the AriaDeck window");
            cx.quit();
            return;
        }

        cx.activate(true);
    });
}
