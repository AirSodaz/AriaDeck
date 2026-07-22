#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod instance;
mod metadata;
mod platform;
mod workspace;

use std::{env, ffi::OsString, path::PathBuf, sync::Arc};

use gpui::{
    App, AppContext as _, Bounds, Point, TitlebarOptions, WindowBounds, WindowDecorations,
    WindowOptions, px, size,
};
use gpui_platform::application;
use tokio::runtime::Builder;

use ariadeck_settings::{JsonWindowGeometryStore, WINDOW_DEFAULT_HEIGHT, WINDOW_DEFAULT_WIDTH};

use crate::{
    instance::{
        InstanceRole, LaunchRequest, MAX_LAUNCH_ITEMS, coordinate_instance, is_supported_magnet_uri,
    },
    workspace::DesktopRoot,
};

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

    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let initial_request = launch_request_from_args(env::args_os().skip(1), &current_dir);
    let instance_requests = match coordinate_instance(
        runtime.as_ref(),
        &DesktopRoot::default_data_dir(),
        &initial_request,
    ) {
        Ok(InstanceRole::Primary(receiver)) => Some(receiver),
        Ok(InstanceRole::Forwarded) => return,
        Err(error) => {
            tracing::warn!(%error, "single-instance coordination is unavailable");
            None
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
                    let initial_request = initial_request.clone();
                    let instance_requests = instance_requests;
                    move |window, cx| {
                        cx.new(|cx| {
                            DesktopRoot::new(
                                runtime.clone(),
                                initial_request.clone(),
                                instance_requests,
                                window,
                                cx,
                            )
                        })
                    }
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

fn launch_request_from_args(
    args: impl IntoIterator<Item = OsString>,
    current_dir: &std::path::Path,
) -> LaunchRequest {
    let mut args = args.into_iter();
    let mut request = LaunchRequest::default();
    while let Some(argument) = args.next() {
        match argument.to_str() {
            Some("--open-metadata") => {
                let Some(path) = args.next().map(PathBuf::from) else {
                    break;
                };
                let supported = path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| {
                        extension.eq_ignore_ascii_case("torrent")
                            || extension.eq_ignore_ascii_case("metalink")
                            || extension.eq_ignore_ascii_case("meta4")
                    });
                if supported {
                    request.metadata_paths.push(if path.is_absolute() {
                        path
                    } else {
                        current_dir.join(path)
                    });
                }
            }
            Some("--open-magnet") => {
                let Some(uri) = args.next().and_then(|value| value.into_string().ok()) else {
                    break;
                };
                let uri = uri.trim();
                if is_supported_magnet_uri(uri) {
                    request.magnet_uris.push(uri.to_owned());
                }
            }
            _ => {}
        }
        if request.len() == MAX_LAUNCH_ITEMS {
            break;
        }
    }
    request
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

#[cfg(test)]
mod tests {
    use super::launch_request_from_args;
    use std::{ffi::OsString, path::PathBuf};

    #[test]
    fn launch_request_from_args_accepts_supported_metadata_extensions_only() {
        let current_dir = PathBuf::from("launch-directory");
        let request = launch_request_from_args(
            [
                OsString::from("ignored.torrent"),
                OsString::from("--open-metadata"),
                OsString::from("sample.TORRENT"),
                OsString::from("--open-metadata"),
                OsString::from("sample.metalink"),
                OsString::from("--open-metadata"),
                OsString::from("sample.meta4"),
                OsString::from("--open-metadata"),
                OsString::from("sample.txt"),
            ],
            &current_dir,
        );
        assert_eq!(
            request.metadata_paths,
            vec![
                current_dir.join("sample.TORRENT"),
                current_dir.join("sample.metalink"),
                current_dir.join("sample.meta4"),
            ]
        );
        assert!(request.magnet_uris.is_empty());
    }

    #[test]
    fn launch_request_from_args_preserves_paths_and_accepts_only_flagged_magnets() {
        let current_dir = PathBuf::from("launch-directory");
        let request = launch_request_from_args(
            [
                OsString::from("bare.torrent"),
                OsString::from("magnet:?xt=urn:btih:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                OsString::from("--open-metadata"),
                OsString::from("示例 file.TORRENT"),
                OsString::from("--open-metadata"),
                OsString::from("--leading-dash.meta4"),
                OsString::from("--open-magnet"),
                OsString::from(
                    "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example file",
                ),
                OsString::from("--open-magnet"),
                OsString::from("https://example.test/not-a-magnet"),
            ],
            &current_dir,
        );

        assert_eq!(
            request.metadata_paths,
            vec![
                current_dir.join("示例 file.TORRENT"),
                current_dir.join("--leading-dash.meta4"),
            ]
        );
        assert_eq!(
            request.magnet_uris,
            vec!["magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example file"]
        );
    }
}
