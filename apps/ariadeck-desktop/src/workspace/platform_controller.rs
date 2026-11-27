//! System tray, window visibility, keep-awake, and debounced window geometry
//! persistence for [`super::DesktopRoot`].

use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use ariadeck_i18n::{LocaleId, Translator};
use ariadeck_settings::{AppSettings, LanguagePreference, WindowGeometry};
use ariadeck_ui::{ConnectionView, TrayStatusView};
use gpui::{Context, Window};
#[cfg(target_os = "windows")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::TRAY_SESSION_ACTIVE;
use crate::platform::{self, SystemTray, TrayAction, TrayBadgeState, TrayIconAppearance};

const TRAY_THEME_PROBE_INTERVAL: Duration = Duration::from_secs(2);

pub(crate) type TrayStatusKey = (u64, u64, ConnectionView, usize, u64, u64, LocaleId);

pub(crate) struct PlatformController {
    tray: Option<SystemTray>,
    last_tray_status_key: Option<TrayStatusKey>,
    last_tray_tooltip: String,
    last_tray_title: Option<String>,
    last_tray_appearance: Option<(TrayIconAppearance, TrayBadgeState)>,
    cached_appearance: TrayIconAppearance,
    last_theme_probe: Instant,
    window_hidden_to_tray: bool,
    is_keep_awake: bool,
    last_saved_geometry: Option<WindowGeometry>,
    pending_geometry: Option<WindowGeometry>,
    geometry_save_generation: u64,
}

impl PlatformController {
    pub(crate) fn new(settings: &AppSettings, initial_geometry: Option<WindowGeometry>) -> Self {
        let initial_appearance = platform::detect_tray_appearance();
        let tray = if settings.platform.show_tray_icon {
            match tray_from_settings(settings, initial_appearance) {
                Ok(tray) => Some(tray),
                Err(error) => {
                    tracing::warn!(%error, "system tray icon could not be created");
                    None
                }
            }
        } else {
            None
        };
        Self {
            tray,
            last_tray_status_key: None,
            last_tray_tooltip: String::new(),
            last_tray_title: None,
            last_tray_appearance: Some((initial_appearance, TrayBadgeState::Idle)),
            cached_appearance: initial_appearance,
            last_theme_probe: Instant::now(),
            window_hidden_to_tray: false,
            is_keep_awake: false,
            last_saved_geometry: initial_geometry,
            pending_geometry: None,
            geometry_save_generation: 0,
        }
    }

    pub(crate) fn has_tray(&self) -> bool {
        self.tray.is_some()
    }

    pub(crate) fn is_hidden_to_tray(&self) -> bool {
        self.window_hidden_to_tray
    }

    pub(crate) fn poll_tray_actions(&self) -> Vec<TrayAction> {
        self.tray
            .as_ref()
            .map(SystemTray::poll_actions)
            .unwrap_or_default()
    }

    pub(crate) fn sync_tray_if_needed(
        &mut self,
        key: TrayStatusKey,
        build_status: impl FnOnce() -> TrayStatusView,
    ) {
        if self.tray.is_none() {
            return;
        }
        if self.last_tray_status_key.as_ref() != Some(&key) {
            let status = build_status();
            self.last_tray_status_key = Some(key);
            self.sync_tray_status(status);
        } else {
            self.refresh_tray_appearance(None);
        }
    }

    pub(crate) fn sync_tray_status(&mut self, status: TrayStatusView) {
        let Some(tray) = self.tray.as_ref() else {
            return;
        };
        if status.tooltip != self.last_tray_tooltip {
            tray.set_tooltip(&status.tooltip);
            self.last_tray_tooltip = status.tooltip;
        }
        if status.title != self.last_tray_title {
            tray.set_title(status.title.as_deref());
            self.last_tray_title = status.title;
        }
        self.refresh_tray_appearance(Some(status.active_transfers));
    }

    fn refresh_tray_appearance(&mut self, active_transfers: Option<usize>) {
        let Some(tray) = self.tray.as_ref() else {
            return;
        };
        if self.last_theme_probe.elapsed() >= TRAY_THEME_PROBE_INTERVAL {
            self.cached_appearance = platform::detect_tray_appearance();
            self.last_theme_probe = Instant::now();
        }
        let badge = if matches!(self.cached_appearance, TrayIconAppearance::Template) {
            TrayBadgeState::Idle
        } else if let Some(active) = active_transfers {
            if active > 0 {
                TrayBadgeState::Active
            } else {
                TrayBadgeState::Idle
            }
        } else {
            self.last_tray_appearance
                .map(|(_, prev)| prev)
                .unwrap_or(TrayBadgeState::Idle)
        };
        let next_appearance = (self.cached_appearance, badge);
        if self.last_tray_appearance != Some(next_appearance) {
            tray.set_appearance(next_appearance.0, next_appearance.1);
            self.last_tray_appearance = Some(next_appearance);
        }
    }

    pub(crate) fn sync_keep_awake(
        &mut self,
        enabled_by_settings: bool,
        has_active_downloads: bool,
    ) {
        let should_keep_awake = enabled_by_settings && has_active_downloads;
        if should_keep_awake != self.is_keep_awake {
            platform::set_keep_awake(should_keep_awake);
            self.is_keep_awake = should_keep_awake;
        }
    }

    pub(crate) fn hide_to_tray(&mut self, window: &mut Window) -> bool {
        if self.tray.is_none() {
            return false;
        }
        if hide_native_window(window) {
            self.window_hidden_to_tray = true;
            TRAY_SESSION_ACTIVE.store(true, Ordering::SeqCst);
            if let Some(tray) = self.tray.as_ref() {
                tray.set_visible(true);
            }
            return true;
        }
        false
    }

    pub(crate) fn show_from_tray<V: 'static>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> bool {
        if show_native_window(window) {
            self.window_hidden_to_tray = false;
            TRAY_SESSION_ACTIVE.store(false, Ordering::SeqCst);
            window.activate_window();
            cx.activate(true);
            return true;
        }
        false
    }

    pub(crate) fn sync_tray_with_settings(&mut self, settings: &AppSettings) {
        let want_tray = settings.platform.show_tray_icon;
        match (want_tray, self.tray.is_some()) {
            (true, false) => {
                let appearance = platform::detect_tray_appearance();
                self.cached_appearance = appearance;
                self.last_theme_probe = Instant::now();
                match tray_from_settings(settings, appearance) {
                    Ok(tray) => {
                        self.tray = Some(tray);
                        self.last_tray_status_key = None;
                        self.last_tray_tooltip.clear();
                        self.last_tray_title = None;
                        self.last_tray_appearance = Some((appearance, TrayBadgeState::Idle));
                    }
                    Err(error) => tracing::warn!(%error, "system tray icon could not be created"),
                }
            }
            (false, true) => {
                self.tray.take();
                self.last_tray_status_key = None;
                self.last_tray_tooltip.clear();
                self.last_tray_title = None;
                self.last_tray_appearance = None;
                if self.window_hidden_to_tray {
                    // Cannot stay hidden without a tray affordance.
                    TRAY_SESSION_ACTIVE.store(false, Ordering::SeqCst);
                    self.window_hidden_to_tray = false;
                }
            }
            (true, true) => {
                if let Some(tray) = self.tray.as_ref() {
                    tray.set_visible(true);
                }
            }
            (false, false) => {}
        }
    }

    pub(crate) fn record_saved_geometry(&mut self, geometry: WindowGeometry) {
        self.last_saved_geometry = Some(geometry);
    }

    pub(crate) fn prepare_geometry_save(&mut self, geometry: WindowGeometry) -> Option<u64> {
        let geometry = geometry.sanitized();
        if self.last_saved_geometry == Some(geometry) {
            self.pending_geometry = None;
            return None;
        }
        self.pending_geometry = Some(geometry);
        self.geometry_save_generation = self.geometry_save_generation.wrapping_add(1);
        Some(self.geometry_save_generation)
    }

    pub(crate) fn take_ready_geometry(&mut self, generation: u64) -> Option<WindowGeometry> {
        if self.geometry_save_generation != generation {
            return None;
        }
        let geometry = self.pending_geometry.take()?;
        if self.last_saved_geometry == Some(geometry) {
            return None;
        }
        Some(geometry)
    }

    pub(crate) fn cleanup_on_quit(&mut self) {
        TRAY_SESSION_ACTIVE.store(false, Ordering::SeqCst);
        self.window_hidden_to_tray = false;
        // Drop tray first so Windows removes the icon immediately.
        self.tray.take();
    }

    pub(crate) fn cleanup_on_drop(&mut self) -> Option<WindowGeometry> {
        TRAY_SESSION_ACTIVE.store(false, Ordering::SeqCst);
        self.window_hidden_to_tray = false;
        if self.is_keep_awake {
            platform::set_keep_awake(false);
            self.is_keep_awake = false;
        }
        self.tray.take();
        self.pending_geometry.take()
    }
}

pub(crate) fn hide_native_window(window: &Window) -> bool {
    #[cfg(target_os = "windows")]
    {
        if let Ok(handle) = HasWindowHandle::window_handle(window)
            && let RawWindowHandle::Win32(win32) = handle.as_raw()
        {
            return platform::hide_show_win32(win32.hwnd.get(), false);
        }
    }
    window.minimize_window();
    true
}

pub(crate) fn show_native_window(window: &Window) -> bool {
    #[cfg(target_os = "windows")]
    {
        if let Ok(handle) = HasWindowHandle::window_handle(window)
            && let RawWindowHandle::Win32(win32) = handle.as_raw()
        {
            let restored = platform::hide_show_win32(win32.hwnd.get(), true);
            window.activate_window();
            return restored;
        }
    }
    window.activate_window();
    true
}

fn tray_from_settings(
    settings: &AppSettings,
    appearance: TrayIconAppearance,
) -> Result<SystemTray, String> {
    let locale = match settings.language {
        LanguagePreference::System => LocaleId::from_system_env(),
        LanguagePreference::En => LocaleId::En,
        LanguagePreference::ZhCn => LocaleId::ZhCn,
    };
    let translator = Translator::new(locale);
    SystemTray::try_new_with_labels(
        &translator.t("tray-show"),
        &translator.t("tray-pause-all"),
        &translator.t("tray-resume-all"),
        &translator.t("tray-quit"),
        &translator.t("app-name"),
        appearance,
    )
}
