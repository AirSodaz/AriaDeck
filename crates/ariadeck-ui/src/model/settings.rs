use super::{commands::OperationErrorView, task::RequestId};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ColorSchemeView {
    /// Follow the OS light/dark preference (UI-001).
    #[default]
    System,
    Light,
    Dark,
}

impl ColorSchemeView {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::System => "settings-theme-system",
            Self::Light => "settings-theme-light",
            Self::Dark => "settings-theme-dark",
        }
    }
}

/// Display language preference projected for settings UI.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LanguagePreferenceView {
    #[default]
    System,
    English,
    ChineseSimplified,
}

impl LanguagePreferenceView {
    pub const ALL: [Self; 3] = [Self::System, Self::English, Self::ChineseSimplified];

    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::System => "language-system",
            Self::English => "language-english",
            Self::ChineseSimplified => "language-chinese-simplified",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProxyModeView {
    #[default]
    Disabled,
    /// Follow the operating-system / environment proxy at apply time.
    System,
    Manual,
}

impl ProxyModeView {
    #[must_use]
    pub const fn all() -> [Self; 3] {
        [Self::Disabled, Self::System, Self::Manual]
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::System => "System",
            Self::Manual => "Manual",
        }
    }

    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::Disabled => "settings-proxy-disabled",
            Self::System => "settings-proxy-system",
            Self::Manual => "settings-proxy-manual",
        }
    }

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Disabled => 0,
            Self::System => 1,
            Self::Manual => 2,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadProxySettingsView {
    pub mode: ProxyModeView,
    pub all_proxy: String,
    pub http_proxy: String,
    pub https_proxy: String,
    pub ftp_proxy: String,
    pub no_proxy: Vec<String>,
    pub username: String,
    pub has_password: bool,
    /// Verify peer TLS certificates (`check-certificate`). Default true.
    pub check_certificate: bool,
}

impl Default for DownloadProxySettingsView {
    fn default() -> Self {
        Self {
            mode: ProxyModeView::default(),
            all_proxy: String::new(),
            http_proxy: String::new(),
            https_proxy: String::new(),
            ftp_proxy: String::new(),
            no_proxy: Vec::new(),
            username: String::new(),
            has_password: false,
            check_certificate: true,
        }
    }
}

/// Speed limit values as user-editable text using aria2's `K`/`M` suffix syntax.
///
/// The accepted forms mirror aria2's `--max-*-limit` options: a plain byte
/// count (`1048576`), or a whole number followed by a `K`/`M`/`G` suffix
/// (`512K`, `2M`, case-insensitive), where each unit is a 1024-based multiple
/// exactly as aria2 interprets it. An empty field means "no limit" (`0`). The
/// UI parses these to bytes/second before building a save request.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpeedLimitSettingsView {
    /// Raw text entered by the user for the global download speed limit.
    pub download_limit: String,
    /// Raw text entered by the user for the global upload speed limit.
    pub upload_limit: String,
}

/// File allocation method for transfer-policy settings (RATE-002).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FileAllocationView {
    None,
    #[default]
    Prealloc,
    Trunc,
    Falloc,
}

impl FileAllocationView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Prealloc => "Prealloc",
            Self::Trunc => "Trunc",
            Self::Falloc => "Falloc",
        }
    }

    #[must_use]
    pub const fn all() -> [Self; 4] {
        [Self::None, Self::Prealloc, Self::Trunc, Self::Falloc]
    }
}

/// Transfer-policy settings as user-editable text fields (RATE-002).
///
/// Count fields are plain positive integers. `min_split_size` reuses aria2's
/// `K`/`M`/`G` size syntax (same as speed limits, but 0 is invalid). Scope
/// labels are fixed in the settings page: concurrent downloads affect the live
/// queue; connection/split/allocation/integrity are defaults for new downloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferPolicySettingsView {
    pub max_concurrent_downloads: String,
    pub max_connection_per_server: String,
    pub split: String,
    pub min_split_size: String,
    pub file_allocation: FileAllocationView,
    pub check_integrity: bool,
}

impl Default for TransferPolicySettingsView {
    fn default() -> Self {
        Self {
            max_concurrent_downloads: "5".into(),
            max_connection_per_server: "1".into(),
            split: "5".into(),
            min_split_size: "20M".into(),
            file_allocation: FileAllocationView::Prealloc,
            check_integrity: false,
        }
    }
}

/// Extra BitTorrent trackers applied as aria2 global `bt-tracker` (D1).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TrackerListSourceView {
    #[default]
    Curated,
    Custom,
}

impl TrackerListSourceView {
    #[must_use]
    pub const fn all() -> [Self; 2] {
        [Self::Curated, Self::Custom]
    }

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Curated => 0,
            Self::Custom => 1,
        }
    }

    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::Curated => "settings-tracker-source-curated",
            Self::Custom => "settings-tracker-source-custom",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TrackerListSettingsView {
    pub enabled: bool,
    pub source: TrackerListSourceView,
    pub custom_url: String,
    pub auto_refresh: bool,
    pub last_refreshed_at: Option<i64>,
    pub list_text: String,
}

impl TransferPolicySettingsView {
    #[must_use]
    pub fn parse_max_concurrent_downloads(&self) -> Option<u32> {
        parse_positive_u32(&self.max_concurrent_downloads)
    }

    #[must_use]
    pub fn parse_max_connection_per_server(&self) -> Option<u32> {
        parse_positive_u32(&self.max_connection_per_server).filter(|value| (1..=16).contains(value))
    }

    #[must_use]
    pub fn parse_split(&self) -> Option<u32> {
        parse_positive_u32(&self.split)
    }

    /// Parse min-split-size to bytes. Empty is invalid (unlike speed limits).
    #[must_use]
    pub fn parse_min_split_size(&self) -> Option<u64> {
        let value = parse_speed_limit_field(&self.min_split_size)?;
        (value > 0).then_some(value)
    }

    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.parse_max_concurrent_downloads().is_some()
            && self.parse_max_connection_per_server().is_some()
            && self.parse_split().is_some()
            && self.parse_min_split_size().is_some()
    }
}

fn parse_positive_u32(value: &str) -> Option<u32> {
    let trimmed = value.trim();
    if trimmed.is_empty() || !trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    trimmed.parse::<u32>().ok().filter(|value| *value > 0)
}

impl SpeedLimitSettingsView {
    /// Parse `download_limit` to bytes/second. Empty string yields 0 (unlimited).
    #[must_use]
    pub fn parse_download_limit(&self) -> Option<u64> {
        parse_speed_limit_field(&self.download_limit)
    }

    /// Parse `upload_limit` to bytes/second. Empty string yields 0 (unlimited).
    #[must_use]
    pub fn parse_upload_limit(&self) -> Option<u64> {
        parse_speed_limit_field(&self.upload_limit)
    }

    /// Returns true when both fields parse (or are empty).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.parse_download_limit().is_some() && self.parse_upload_limit().is_some()
    }
}

/// Parse an aria2 speed-limit field to bytes/second.
///
/// Accepts an empty string (0/unlimited), a plain integer of bytes, or an
/// integer with a single `K`/`M`/`G` suffix (case-insensitive, 1024-based).
/// Returns `None` when the value is malformed or overflows `u64`.
#[must_use]
pub fn parse_speed_limit_field(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Some(0);
    }
    let (digits, multiplier) = match trimmed.as_bytes().last() {
        Some(b'K' | b'k') => (&trimmed[..trimmed.len() - 1], 1024),
        Some(b'M' | b'm') => (&trimmed[..trimmed.len() - 1], 1024 * 1024),
        Some(b'G' | b'g') => (&trimmed[..trimmed.len() - 1], 1024 * 1024 * 1024),
        _ => (trimmed, 1),
    };
    let digits = digits.trim_end();
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits
        .parse::<u64>()
        .ok()
        .and_then(|amount| amount.checked_mul(multiplier))
}

/// Render a byte/second speed limit back into a compact editable field value.
///
/// Zero becomes an empty string (unlimited). Exact 1024-based multiples use the
/// largest whole `K`/`M`/`G` unit so a saved `2M` round-trips as `2M` instead
/// of `2097152`.
#[must_use]
pub fn format_speed_limit_field(bytes_per_second: u64) -> String {
    if bytes_per_second == 0 {
        return String::new();
    }
    for (suffix, unit) in [
        ('G', 1024u64 * 1024 * 1024),
        ('M', 1024 * 1024),
        ('K', 1024),
    ] {
        if bytes_per_second.is_multiple_of(unit) {
            return format!("{}{suffix}", bytes_per_second / unit);
        }
    }
    bytes_per_second.to_string()
}

#[derive(Clone, Eq, PartialEq)]
pub struct SecretStringView(String);

impl SecretStringView {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Debug for SecretStringView {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretStringView([REDACTED])")
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ProxyPasswordUpdateView {
    #[default]
    Unchanged,
    /// Detach the current keychain credential without deleting it.
    Detach,
    Clear,
    Set(SecretStringView),
}

/// How to update a remote profile's RPC secret on catalog save.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ProfileRpcSecretUpdateView {
    #[default]
    Unchanged,
    Clear,
    Set(SecretStringView),
}

/// Host OS capability flags injected by the desktop composition root so UI
/// controls never present silent no-op switches on unsupported platforms.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlatformCapabilitiesView {
    pub keep_awake_supported: bool,
    pub browser_bridge_supported: bool,
}

impl Default for PlatformCapabilitiesView {
    fn default() -> Self {
        Self {
            keep_awake_supported: true,
            browser_bridge_supported: true,
        }
    }
}

/// How loudly automatic completion/error surfaces appear.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum NotificationVolumeView {
    #[default]
    Normal,
    Quiet,
    Silent,
}

impl NotificationVolumeView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Quiet => "Quiet",
            Self::Silent => "Silent",
        }
    }

    #[must_use]
    pub const fn all() -> [Self; 3] {
        [Self::Normal, Self::Quiet, Self::Silent]
    }
}

/// Notification preferences for automatic task/engine surfaces (OBS-001 / PLAT-001).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotificationSettingsView {
    pub volume: NotificationVolumeView,
    pub notify_on_completion: bool,
    pub notify_on_error: bool,
    pub notify_on_engine_events: bool,
    pub os_notifications: bool,
    pub notify_on_low_disk: bool,
    pub low_disk_threshold_bytes: u64,
}

impl Default for NotificationSettingsView {
    fn default() -> Self {
        Self {
            volume: NotificationVolumeView::Normal,
            notify_on_completion: true,
            notify_on_error: true,
            notify_on_engine_events: true,
            os_notifications: true,
            notify_on_low_disk: true,
            low_disk_threshold_bytes: 1_073_741_824,
        }
    }
}

/// What the window close control does while the process stays alive (PLAT-001).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum CloseBehaviorView {
    #[default]
    MinimizeToTray,
    Quit,
}

impl CloseBehaviorView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::MinimizeToTray => "Minimize to tray",
            Self::Quit => "Quit AriaDeck",
        }
    }

    #[must_use]
    pub const fn all() -> [Self; 2] {
        [Self::MinimizeToTray, Self::Quit]
    }
}

/// Action to perform when all active downloads complete.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CompletionPowerActionView {
    #[default]
    None,
    Sleep,
    Hibernate,
    Shutdown,
}

impl CompletionPowerActionView {
    pub const ALL: [Self; 4] = [Self::None, Self::Sleep, Self::Hibernate, Self::Shutdown];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Sleep => "sleep",
            Self::Hibernate => "hibernate",
            Self::Shutdown => "shutdown",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "Do nothing",
            Self::Sleep => "Sleep",
            Self::Hibernate => "Hibernate",
            Self::Shutdown => "Shut down",
        }
    }

    pub const fn message_key(self) -> &'static str {
        match self {
            Self::None => "settings-action-none",
            Self::Sleep => "settings-action-sleep",
            Self::Hibernate => "settings-action-hibernate",
            Self::Shutdown => "settings-action-shutdown",
        }
    }

    pub const fn is_armed(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Platform window/tray and power preferences (PLAT-001).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlatformSettingsView {
    pub close_behavior: CloseBehaviorView,
    pub show_tray_icon: bool,
    pub start_minimized_to_tray: bool,
    pub keep_awake_while_active: bool,
    pub action_after_done: CompletionPowerActionView,
}

impl Default for PlatformSettingsView {
    fn default() -> Self {
        Self {
            close_behavior: CloseBehaviorView::MinimizeToTray,
            show_tray_icon: true,
            start_minimized_to_tray: false,
            keep_awake_while_active: true,
            action_after_done: CompletionPowerActionView::None,
        }
    }
}

/// Browser bridge preferences (D-045). Both off by default.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BrowserBridgeSettingsView {
    pub allow_cookies: bool,
    pub auto_submit: bool,
}

/// Output folder + extension rules (C1 / D-040 / D-042).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DownloadCategoryView {
    pub id: String,
    pub name: String,
    pub directory: String,
    /// Comma-separated extensions for the settings editor (e.g. `mp4, mkv`).
    pub extensions: String,
    pub is_fallback: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsView {
    pub color_scheme: ColorSchemeView,
    pub language: LanguagePreferenceView,
    /// Mirror of the fallback category directory (engine/profile default root).
    pub download_directory: String,
    pub download_proxy: DownloadProxySettingsView,
    pub speed_limits: SpeedLimitSettingsView,
    pub transfer_policy: TransferPolicySettingsView,
    pub notifications: NotificationSettingsView,
    pub platform: PlatformSettingsView,
    pub browser_bridge: BrowserBridgeSettingsView,
    pub categories: Vec<DownloadCategoryView>,
    pub tracker_list: TrackerListSettingsView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsSaveRequestView {
    pub request_id: RequestId,
    pub settings: SettingsView,
    pub proxy_password: ProxyPasswordUpdateView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsSaveOutcomeView {
    Success,
    Failure(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsSaveResultView {
    pub request_id: RequestId,
    pub settings: SettingsView,
    pub outcome: SettingsSaveOutcomeView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsExportRequestView {
    pub path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsExportOutcomeView {
    Success,
    Failure(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsExportResultView {
    pub path: String,
    pub outcome: SettingsExportOutcomeView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsImportRequestView {
    pub path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsImportOutcomeView {
    Ready(Box<SettingsView>),
    Failure(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsImportResultView {
    pub path: String,
    pub outcome: SettingsImportOutcomeView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticExportRequestView {
    pub path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiagnosticExportOutcomeView {
    Success,
    Failure(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticExportResultView {
    pub path: String,
    pub outcome: DiagnosticExportOutcomeView,
}
