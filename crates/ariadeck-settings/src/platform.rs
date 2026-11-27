use serde::{Deserialize, Serialize};

use crate::proxy::default_true;

/// How loudly AriaDeck surfaces completion/error events.
///
/// Quiet still records activity history and keeps command-feedback toasts, but
/// suppresses automatic completion/error toast surfaces. Defaults favor visible
/// completions and errors. OS-native toasts follow the same volume/category
/// gates when enabled (PLAT-001).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationVolume {
    #[default]
    Normal,
    Quiet,
    Silent,
}

/// Persisted notification preferences for grouped completion/error surfaces.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationSettings {
    pub volume: NotificationVolume,
    pub notify_on_completion: bool,
    pub notify_on_error: bool,
    pub notify_on_engine_events: bool,
    /// Also emit OS-native desktop notifications for gated automatic events.
    pub os_notifications: bool,
    /// Surface a low-disk warning when free space falls below the threshold.
    pub notify_on_low_disk: bool,
    /// Free-space threshold in bytes (default 1 GiB).
    pub low_disk_threshold_bytes: u64,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            volume: NotificationVolume::Normal,
            notify_on_completion: true,
            notify_on_error: true,
            notify_on_engine_events: true,
            os_notifications: true,
            notify_on_low_disk: true,
            low_disk_threshold_bytes: 1_073_741_824,
        }
    }
}

/// What the window close control does while the app stays running.
///
/// Quit is always available from the tray menu and File/Exit. Managed aria2 is
/// always stopped when AriaDeck exits; remote engines are never stopped by the
/// desktop process (D-030 / PLAT-001).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior {
    /// Hide the main window and keep the process + managed engine running.
    #[default]
    MinimizeToTray,
    /// Fully quit AriaDeck (and stop a managed engine).
    Quit,
}

/// What power management action to take when all active downloads finish.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionPowerAction {
    #[default]
    None,
    Sleep,
    Hibernate,
    Shutdown,
}

impl CompletionPowerAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Sleep => "sleep",
            Self::Hibernate => "hibernate",
            Self::Shutdown => "shutdown",
        }
    }

    pub const fn is_armed(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Platform window/tray and power preferences (schema v1).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformSettings {
    pub close_behavior: CloseBehavior,
    pub show_tray_icon: bool,
    pub start_minimized_to_tray: bool,
    #[serde(default = "default_true")]
    pub keep_awake_while_active: bool,
    #[serde(default)]
    pub action_after_done: CompletionPowerAction,
    /// Watch the system clipboard for downloadable links while AriaDeck is running (Opt-in).
    #[serde(default)]
    pub watch_clipboard: bool,
}

impl Default for PlatformSettings {
    fn default() -> Self {
        Self {
            close_behavior: CloseBehavior::MinimizeToTray,
            show_tray_icon: true,
            start_minimized_to_tray: false,
            keep_awake_while_active: true,
            action_after_done: CompletionPowerAction::None,
            watch_clipboard: false,
        }
    }
}

/// Browser bridge preferences (D-045).
///
/// Client-wide UI behavior rather than engine state, so it belongs here and not
/// in the per-profile engine environment bag (D-043). Both fields default off:
/// the bridge's safe configuration must be the one you get without deciding
/// anything.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserBridgeSettings {
    /// Forward the extension's cookie for the download's origin. A cookie is the
    /// highest-impact field the bridge can carry, so it needs this **and** the
    /// extension's own host-permission grant (browser-bridge.md §5).
    #[serde(default)]
    pub allow_cookies: bool,
    /// Add forwarded downloads without opening the confirmation dialog. Never
    /// silent: every add still leaves a toast and an activity entry (§6, D-025).
    #[serde(default)]
    pub auto_submit: bool,
}
