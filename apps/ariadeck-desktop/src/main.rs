#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod metadata;
mod platform;
mod workspace;

use std::sync::Arc;

use gpui::{
    App, AppContext as _, Bounds, Point, TitlebarOptions, WindowBounds, WindowDecorations,
    WindowOptions, px, size,
};
use gpui_platform::application;
use tokio::runtime::Builder;

use ariadeck_settings::{JsonWindowGeometryStore, WINDOW_DEFAULT_HEIGHT, WINDOW_DEFAULT_WIDTH};

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

    application()
        .with_assets(ariadeck_ui::Assets)
        .run(move |cx: &mut App| {
            ariadeck_ui::init(cx);
            // ACCESS-001: locale-shaped size/rate formatting for the process lifetime.
            ariadeck_ui::set_active_format_options(ariadeck_ui::FormatOptions::from_env());
            // Only quit when every window is gone *and* the app is not
            // intentionally hidden to the tray (PLAT-001).
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    // Tray-hidden sessions keep a zero-window app alive until
                    // Quit is chosen from the tray menu.
                    if !DesktopRoot::tray_session_active() {
                        cx.quit();
                    }
                }
            })
            .detach();

            let window_bounds = restored_window_bounds(cx);
            let open_result = cx.open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    titlebar: Some(platform_titlebar()),
                    window_decorations: platform_window_decorations(),
                    window_min_size: Some(size(px(960.0), px(620.0))),
                    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
                    icon: window_icon(),
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

fn restored_window_bounds(cx: &App) -> WindowBounds {
    let path = DesktopRoot::default_data_dir().join("window.json");
    let store = JsonWindowGeometryStore::new(path);
    if let Some(geometry) = store.load() {
        let bounds = Bounds {
            origin: Point {
                x: px(geometry.x),
                y: px(geometry.y),
            },
            size: size(px(geometry.width), px(geometry.height)),
        };
        if geometry.maximized {
            return WindowBounds::Maximized(bounds);
        }
        return WindowBounds::Windowed(bounds);
    }
    WindowBounds::Windowed(Bounds::centered(
        None,
        size(px(WINDOW_DEFAULT_WIDTH), px(WINDOW_DEFAULT_HEIGHT)),
        cx,
    ))
}

#[cfg(target_os = "windows")]
fn platform_titlebar() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("AriaDeck".into()),
        appears_transparent: true,
        ..TitlebarOptions::default()
    }
}

#[cfg(target_os = "macos")]
fn platform_titlebar() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("AriaDeck".into()),
        appears_transparent: true,
        traffic_light_position: Some(gpui::point(px(12.0), px(15.0))),
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn platform_titlebar() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("AriaDeck".into()),
        ..TitlebarOptions::default()
    }
}

/// Return the application icon for X11/Wayland window managers (Linux + FreeBSD).
///
/// GPUI reads this via `WindowOptions::icon` and forwards it to the platform
/// window so the WM can show it in the taskbar or window list.
/// On Windows the icon is embedded as a Win32 resource (handled in `build.rs`
/// via `winres`); on macOS it lives in the `.app` bundle. Neither platform has
/// a `WindowOptions::icon` field, so this function is cfg-gated accordingly.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn window_icon() -> Option<Arc<image::RgbaImage>> {
    // 128×128 RGBA rendered from assets/icon.svg at build time.
    const SIZE: u32 = 128;
    let rgba = include_bytes!(concat!(env!("OUT_DIR"), "/window_icon.rgba"));
    image::RgbaImage::from_raw(SIZE, SIZE, rgba.to_vec()).map(Arc::new)
}

#[cfg(target_os = "linux")]
fn platform_window_decorations() -> Option<WindowDecorations> {
    Some(WindowDecorations::Server)
}

#[cfg(not(target_os = "linux"))]
fn platform_window_decorations() -> Option<WindowDecorations> {
    None
}
