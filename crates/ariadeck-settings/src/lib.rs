//! Typed, versioned application settings and their persistence boundary.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

pub const CURRENT_SETTINGS_SCHEMA_VERSION: u32 = 7;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorScheme {
    /// Follow the operating-system light/dark preference (UI-001 / D-031).
    #[default]
    System,
    Light,
    Dark,
}

/// Last-used download list filter (UI-001). Not a named-filter library.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ListFilterPreference {
    #[default]
    All,
    Active,
    Waiting,
    Paused,
    Completed,
    Failed,
}

/// Last-used list sort key (UI-001).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ListSortKeyPreference {
    #[default]
    Queue,
    Name,
    Status,
    Progress,
    DownloadSpeed,
    Size,
}

/// Last-used list sort direction (UI-001).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ListSortDirectionPreference {
    #[default]
    Ascending,
    Descending,
}

/// Restored list-query preferences (filter + sort). Search text is never
/// persisted so restarts do not re-hide tasks behind a forgotten query.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UiPreferences {
    pub list_filter: ListFilterPreference,
    pub list_sort_key: ListSortKeyPreference,
    pub list_sort_direction: ListSortDirectionPreference,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadProxyMode {
    #[default]
    Disabled,
    Manual,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ProxyCredentialRef(Uuid);

impl ProxyCredentialRef {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    #[must_use]
    pub fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    #[must_use]
    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for ProxyCredentialRef {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Error)]
#[error("system credential store operation failed while {operation}: {message}")]
pub struct ProxyCredentialError {
    operation: &'static str,
    message: String,
}

pub trait ProxyCredentialStore: Send + Sync {
    fn load(
        &self,
        credential: ProxyCredentialRef,
    ) -> Result<Option<SecretString>, ProxyCredentialError>;
    fn save(
        &self,
        credential: ProxyCredentialRef,
        password: &SecretString,
    ) -> Result<(), ProxyCredentialError>;
    fn delete(&self, credential: ProxyCredentialRef) -> Result<(), ProxyCredentialError>;
}

#[derive(Clone, Debug)]
pub struct SystemProxyCredentialStore {
    service: String,
}

impl Default for SystemProxyCredentialStore {
    fn default() -> Self {
        Self::new("AriaDeck download proxy")
    }
}

impl SystemProxyCredentialStore {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(
        &self,
        credential: ProxyCredentialRef,
    ) -> Result<keyring::Entry, ProxyCredentialError> {
        keyring::Entry::new(&self.service, &credential.as_uuid().to_string()).map_err(|error| {
            ProxyCredentialError {
                operation: "opening an entry",
                message: error.to_string(),
            }
        })
    }
}

impl ProxyCredentialStore for SystemProxyCredentialStore {
    fn load(
        &self,
        credential: ProxyCredentialRef,
    ) -> Result<Option<SecretString>, ProxyCredentialError> {
        match self.entry(credential)?.get_password() {
            Ok(password) => Ok(Some(SecretString::new(password))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(ProxyCredentialError {
                operation: "reading an entry",
                message: error.to_string(),
            }),
        }
    }

    fn save(
        &self,
        credential: ProxyCredentialRef,
        password: &SecretString,
    ) -> Result<(), ProxyCredentialError> {
        use secrecy::ExposeSecret as _;

        self.entry(credential)?
            .set_password(password.expose_secret())
            .map_err(|error| ProxyCredentialError {
                operation: "writing an entry",
                message: error.to_string(),
            })
    }

    fn delete(&self, credential: ProxyCredentialRef) -> Result<(), ProxyCredentialError> {
        match self.entry(credential)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(ProxyCredentialError {
                operation: "deleting an entry",
                message: error.to_string(),
            }),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadProxySettings {
    pub mode: DownloadProxyMode,
    pub all_proxy: Option<String>,
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub ftp_proxy: Option<String>,
    pub no_proxy: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<ProxyCredentialRef>,
}

/// Persisted global speed limits. Zero means unlimited (aria2 convention).
/// Values are stored in bytes per second and applied to aria2 on each connection.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpeedLimitSettings {
    /// Max aggregate download speed in bytes/s (0 = unlimited).
    pub download_limit: u64,
    /// Max aggregate upload speed in bytes/s (0 = unlimited).
    pub upload_limit: u64,
}

/// Persisted aria2 file-allocation method. Values match aria2's option strings.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileAllocationSetting {
    None,
    #[default]
    Prealloc,
    Trunc,
    Falloc,
}

impl FileAllocationSetting {
    #[must_use]
    pub const fn as_aria2(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Prealloc => "prealloc",
            Self::Trunc => "trunc",
            Self::Falloc => "falloc",
        }
    }
}

/// Persisted transfer-policy defaults for connection, split, allocation, and
/// integrity checks. Applied through `aria2.changeGlobalOption` on save and
/// reconnect. Defaults match aria2 1.37.0 documented values.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransferPolicySettings {
    /// Maximum simultaneously active downloads (`max-concurrent-downloads`).
    pub max_concurrent_downloads: u32,
    /// Default max connections per server for new downloads (1–16).
    pub max_connection_per_server: u32,
    /// Default multi-connection split count for new downloads.
    pub split: u32,
    /// Default minimum split size in bytes.
    pub min_split_size: u64,
    /// Default file allocation method for new downloads.
    pub file_allocation: FileAllocationSetting,
    /// Default integrity-check policy for new downloads.
    pub check_integrity: bool,
}

impl Default for TransferPolicySettings {
    fn default() -> Self {
        Self {
            max_concurrent_downloads: 5,
            max_connection_per_server: 1,
            split: 5,
            min_split_size: 20 * 1024 * 1024,
            file_allocation: FileAllocationSetting::Prealloc,
            check_integrity: false,
        }
    }
}

impl TransferPolicySettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.max_concurrent_downloads == 0 {
            return Err(SettingsError::InvalidTransferPolicy {
                field: "max_concurrent_downloads",
                reason: "must be at least 1".into(),
            });
        }
        if !(1..=16).contains(&self.max_connection_per_server) {
            return Err(SettingsError::InvalidTransferPolicy {
                field: "max_connection_per_server",
                reason: "must be between 1 and 16".into(),
            });
        }
        if self.split == 0 {
            return Err(SettingsError::InvalidTransferPolicy {
                field: "split",
                reason: "must be at least 1".into(),
            });
        }
        if self.min_split_size == 0 {
            return Err(SettingsError::InvalidTransferPolicy {
                field: "min_split_size",
                reason: "must be greater than 0".into(),
            });
        }
        Ok(())
    }
}

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

/// Platform window/tray preferences (schema v6).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformSettings {
    pub close_behavior: CloseBehavior,
    pub show_tray_icon: bool,
    pub start_minimized_to_tray: bool,
}

impl Default for PlatformSettings {
    fn default() -> Self {
        Self {
            close_behavior: CloseBehavior::MinimizeToTray,
            show_tray_icon: true,
            start_minimized_to_tray: false,
        }
    }
}

impl DownloadProxySettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        for (label, endpoint) in [
            ("all", self.all_proxy.as_deref()),
            ("HTTP", self.http_proxy.as_deref()),
            ("HTTPS", self.https_proxy.as_deref()),
            ("FTP", self.ftp_proxy.as_deref()),
        ] {
            if let Some(endpoint) = endpoint {
                validate_proxy_endpoint(label, endpoint)?;
            }
        }
        if self.mode == DownloadProxyMode::Manual
            && self.all_proxy.is_none()
            && self.http_proxy.is_none()
            && self.https_proxy.is_none()
            && self.ftp_proxy.is_none()
        {
            return Err(SettingsError::MissingManualProxyEndpoint);
        }
        if self.credential.is_some() && self.username.as_deref().is_none_or(str::is_empty) {
            return Err(SettingsError::ProxyCredentialWithoutUsername);
        }
        for entry in &self.no_proxy {
            validate_no_proxy_entry(entry)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppSettings {
    pub color_scheme: ColorScheme,
    pub download_directory: PathBuf,
    pub download_proxy: DownloadProxySettings,
    pub speed_limits: SpeedLimitSettings,
    pub transfer_policy: TransferPolicySettings,
    pub notifications: NotificationSettings,
    pub platform: PlatformSettings,
    pub ui: UiPreferences,
}

impl AppSettings {
    #[must_use]
    pub fn new(download_directory: impl Into<PathBuf>) -> Self {
        Self {
            color_scheme: ColorScheme::default(),
            download_directory: download_directory.into(),
            download_proxy: DownloadProxySettings::default(),
            speed_limits: SpeedLimitSettings::default(),
            transfer_policy: TransferPolicySettings::default(),
            notifications: NotificationSettings::default(),
            platform: PlatformSettings::default(),
            ui: UiPreferences::default(),
        }
    }

    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.download_directory.as_os_str().is_empty() {
            return Err(SettingsError::EmptyDownloadDirectory);
        }
        self.download_proxy.validate()?;
        self.transfer_policy.validate()?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsRecovery {
    pub backup_path: PathBuf,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedSettings {
    pub settings: AppSettings,
    pub recovery: Option<SettingsRecovery>,
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("settings store path has no file name: {path}")]
    InvalidStorePath { path: PathBuf },
    #[error("download directory must not be empty")]
    EmptyDownloadDirectory,
    #[error("manual download proxy requires at least one proxy endpoint")]
    MissingManualProxyEndpoint,
    #[error("proxy credential requires a non-empty username")]
    ProxyCredentialWithoutUsername,
    #[error("invalid {label} proxy endpoint: {reason}")]
    InvalidProxyEndpoint { label: &'static str, reason: String },
    #[error("invalid no-proxy entry {entry:?}: {reason}")]
    InvalidNoProxyEntry { entry: String, reason: String },
    #[error("invalid transfer policy field {field}: {reason}")]
    InvalidTransferPolicy { field: &'static str, reason: String },
    #[error("unsupported settings schema version {found}; this build supports {supported}")]
    UnsupportedSchemaVersion { found: u32, supported: u32 },
    #[error("malformed settings document at {path}: {message}")]
    MalformedDocument { path: PathBuf, message: String },
    #[error("failed to serialize settings: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("I/O error while {operation} at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

fn validate_proxy_endpoint(label: &'static str, endpoint: &str) -> Result<(), SettingsError> {
    if endpoint.is_empty() || endpoint.trim() != endpoint {
        return Err(SettingsError::InvalidProxyEndpoint {
            label,
            reason: "value must be non-empty and must not have surrounding whitespace".into(),
        });
    }
    let candidate = if endpoint.contains("://") {
        endpoint.to_owned()
    } else {
        format!("http://{endpoint}")
    };
    let parsed = Url::parse(&candidate).map_err(|error| SettingsError::InvalidProxyEndpoint {
        label,
        reason: error.to_string(),
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(SettingsError::InvalidProxyEndpoint {
            label,
            reason: "only HTTP and HTTPS proxy URLs are supported".into(),
        });
    }
    if parsed.host_str().is_none() {
        return Err(SettingsError::InvalidProxyEndpoint {
            label,
            reason: "host is required".into(),
        });
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(SettingsError::InvalidProxyEndpoint {
            label,
            reason: "credentials must be stored separately from the proxy URL".into(),
        });
    }
    if parsed.path() != "/" || parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(SettingsError::InvalidProxyEndpoint {
            label,
            reason: "path, query, and fragment components are not allowed".into(),
        });
    }
    Ok(())
}

fn validate_no_proxy_entry(entry: &str) -> Result<(), SettingsError> {
    let invalid = |reason: &str| SettingsError::InvalidNoProxyEntry {
        entry: entry.to_owned(),
        reason: reason.into(),
    };
    if entry.is_empty() || entry.trim() != entry {
        return Err(invalid(
            "value must be non-empty and must not have surrounding whitespace",
        ));
    }
    if entry.contains([',', '@']) || entry.contains("://") || entry.chars().any(char::is_whitespace)
    {
        return Err(invalid(
            "expected a host, domain, IP address, or CIDR network",
        ));
    }
    if let Some((address, prefix)) = entry.rsplit_once('/') {
        let address = address
            .parse::<std::net::IpAddr>()
            .map_err(|_| invalid("CIDR base must be an IPv4 or IPv6 address"))?;
        let prefix = prefix
            .parse::<u8>()
            .map_err(|_| invalid("CIDR prefix must be a number"))?;
        let max_prefix = if address.is_ipv4() { 32 } else { 128 };
        if prefix > max_prefix {
            return Err(invalid("CIDR prefix exceeds the address width"));
        }
        return Ok(());
    }
    if !entry
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || ".-:[]".contains(character))
    {
        return Err(invalid("contains unsupported characters"));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct SettingsVersionProbe {
    schema_version: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsDocumentV1 {
    schema_version: u32,
    color_scheme: ColorScheme,
    download_directory: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsDocumentV2 {
    schema_version: u32,
    color_scheme: ColorScheme,
    download_directory: PathBuf,
    download_proxy: DownloadProxySettings,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsDocumentV3 {
    schema_version: u32,
    color_scheme: ColorScheme,
    download_directory: PathBuf,
    download_proxy: DownloadProxySettings,
    speed_limits: SpeedLimitSettings,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsDocumentV4 {
    schema_version: u32,
    color_scheme: ColorScheme,
    download_directory: PathBuf,
    download_proxy: DownloadProxySettings,
    speed_limits: SpeedLimitSettings,
    transfer_policy: TransferPolicySettings,
}

/// Schema v5 notifications lacked OS/low-disk fields; migrate with defaults.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NotificationSettingsV5 {
    volume: NotificationVolume,
    notify_on_completion: bool,
    notify_on_error: bool,
    notify_on_engine_events: bool,
}

impl From<NotificationSettingsV5> for NotificationSettings {
    fn from(value: NotificationSettingsV5) -> Self {
        Self {
            volume: value.volume,
            notify_on_completion: value.notify_on_completion,
            notify_on_error: value.notify_on_error,
            notify_on_engine_events: value.notify_on_engine_events,
            ..Self::default()
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsDocumentV5 {
    schema_version: u32,
    color_scheme: ColorScheme,
    download_directory: PathBuf,
    download_proxy: DownloadProxySettings,
    speed_limits: SpeedLimitSettings,
    transfer_policy: TransferPolicySettings,
    notifications: NotificationSettingsV5,
}

/// Schema v6 platform settings lacked UI list preferences.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsDocumentV6 {
    schema_version: u32,
    color_scheme: ColorScheme,
    download_directory: PathBuf,
    download_proxy: DownloadProxySettings,
    speed_limits: SpeedLimitSettings,
    transfer_policy: TransferPolicySettings,
    notifications: NotificationSettings,
    platform: PlatformSettings,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SettingsDocument {
    schema_version: u32,
    color_scheme: ColorScheme,
    download_directory: PathBuf,
    download_proxy: DownloadProxySettings,
    speed_limits: SpeedLimitSettings,
    transfer_policy: TransferPolicySettings,
    notifications: NotificationSettings,
    platform: PlatformSettings,
    ui: UiPreferences,
}

impl From<&AppSettings> for SettingsDocument {
    fn from(settings: &AppSettings) -> Self {
        Self {
            schema_version: CURRENT_SETTINGS_SCHEMA_VERSION,
            color_scheme: settings.color_scheme,
            download_directory: settings.download_directory.clone(),
            download_proxy: settings.download_proxy.clone(),
            speed_limits: settings.speed_limits,
            transfer_policy: settings.transfer_policy,
            notifications: settings.notifications,
            platform: settings.platform,
            ui: settings.ui,
        }
    }
}

impl TryFrom<SettingsDocument> for AppSettings {
    type Error = SettingsError;

    fn try_from(document: SettingsDocument) -> Result<Self, Self::Error> {
        if document.schema_version != CURRENT_SETTINGS_SCHEMA_VERSION {
            return Err(SettingsError::UnsupportedSchemaVersion {
                found: document.schema_version,
                supported: CURRENT_SETTINGS_SCHEMA_VERSION,
            });
        }
        let settings = Self {
            color_scheme: document.color_scheme,
            download_directory: document.download_directory,
            download_proxy: document.download_proxy,
            speed_limits: document.speed_limits,
            transfer_policy: document.transfer_policy,
            notifications: document.notifications,
            platform: document.platform,
            ui: document.ui,
        };
        settings.validate()?;
        Ok(settings)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonSettingsStore {
    path: PathBuf,
}

impl JsonSettingsStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<AppSettings, SettingsError> {
        self.load_versioned().map(|(settings, _)| settings)
    }

    fn load_versioned(&self) -> Result<(AppSettings, bool), SettingsError> {
        let bytes = fs::read(&self.path)
            .map_err(|source| io_error("read the settings document", &self.path, source))?;
        let malformed = |error: serde_json::Error| SettingsError::MalformedDocument {
            path: self.path.clone(),
            message: error.to_string(),
        };
        let version: SettingsVersionProbe = serde_json::from_slice(&bytes).map_err(malformed)?;
        match version.schema_version {
            1 => {
                let document: SettingsDocumentV1 =
                    serde_json::from_slice(&bytes).map_err(malformed)?;
                if document.schema_version != 1 {
                    return Err(SettingsError::UnsupportedSchemaVersion {
                        found: document.schema_version,
                        supported: CURRENT_SETTINGS_SCHEMA_VERSION,
                    });
                }
                let settings = AppSettings {
                    color_scheme: document.color_scheme,
                    download_directory: document.download_directory,
                    download_proxy: DownloadProxySettings::default(),
                    speed_limits: SpeedLimitSettings::default(),
                    transfer_policy: TransferPolicySettings::default(),
                    notifications: NotificationSettings::default(),
                    platform: PlatformSettings::default(),
                    ui: UiPreferences::default(),
                };
                settings.validate()?;
                Ok((settings, true))
            }
            2 => {
                let document: SettingsDocumentV2 =
                    serde_json::from_slice(&bytes).map_err(malformed)?;
                if document.schema_version != 2 {
                    return Err(SettingsError::UnsupportedSchemaVersion {
                        found: document.schema_version,
                        supported: CURRENT_SETTINGS_SCHEMA_VERSION,
                    });
                }
                let settings = AppSettings {
                    color_scheme: document.color_scheme,
                    download_directory: document.download_directory,
                    download_proxy: document.download_proxy,
                    speed_limits: SpeedLimitSettings::default(),
                    transfer_policy: TransferPolicySettings::default(),
                    notifications: NotificationSettings::default(),
                    platform: PlatformSettings::default(),
                    ui: UiPreferences::default(),
                };
                settings.validate()?;
                Ok((settings, true))
            }
            3 => {
                let document: SettingsDocumentV3 =
                    serde_json::from_slice(&bytes).map_err(malformed)?;
                if document.schema_version != 3 {
                    return Err(SettingsError::UnsupportedSchemaVersion {
                        found: document.schema_version,
                        supported: CURRENT_SETTINGS_SCHEMA_VERSION,
                    });
                }
                let settings = AppSettings {
                    color_scheme: document.color_scheme,
                    download_directory: document.download_directory,
                    download_proxy: document.download_proxy,
                    speed_limits: document.speed_limits,
                    transfer_policy: TransferPolicySettings::default(),
                    notifications: NotificationSettings::default(),
                    platform: PlatformSettings::default(),
                    ui: UiPreferences::default(),
                };
                settings.validate()?;
                Ok((settings, true))
            }
            4 => {
                let document: SettingsDocumentV4 =
                    serde_json::from_slice(&bytes).map_err(malformed)?;
                if document.schema_version != 4 {
                    return Err(SettingsError::UnsupportedSchemaVersion {
                        found: document.schema_version,
                        supported: CURRENT_SETTINGS_SCHEMA_VERSION,
                    });
                }
                let settings = AppSettings {
                    color_scheme: document.color_scheme,
                    download_directory: document.download_directory,
                    download_proxy: document.download_proxy,
                    speed_limits: document.speed_limits,
                    transfer_policy: document.transfer_policy,
                    notifications: NotificationSettings::default(),
                    platform: PlatformSettings::default(),
                    ui: UiPreferences::default(),
                };
                settings.validate()?;
                Ok((settings, true))
            }
            5 => {
                let document: SettingsDocumentV5 =
                    serde_json::from_slice(&bytes).map_err(malformed)?;
                if document.schema_version != 5 {
                    return Err(SettingsError::UnsupportedSchemaVersion {
                        found: document.schema_version,
                        supported: CURRENT_SETTINGS_SCHEMA_VERSION,
                    });
                }
                let settings = AppSettings {
                    color_scheme: document.color_scheme,
                    download_directory: document.download_directory,
                    download_proxy: document.download_proxy,
                    speed_limits: document.speed_limits,
                    transfer_policy: document.transfer_policy,
                    notifications: document.notifications.into(),
                    platform: PlatformSettings::default(),
                    ui: UiPreferences::default(),
                };
                settings.validate()?;
                Ok((settings, true))
            }
            6 => {
                let document: SettingsDocumentV6 =
                    serde_json::from_slice(&bytes).map_err(malformed)?;
                if document.schema_version != 6 {
                    return Err(SettingsError::UnsupportedSchemaVersion {
                        found: document.schema_version,
                        supported: CURRENT_SETTINGS_SCHEMA_VERSION,
                    });
                }
                let settings = AppSettings {
                    color_scheme: document.color_scheme,
                    download_directory: document.download_directory,
                    download_proxy: document.download_proxy,
                    speed_limits: document.speed_limits,
                    transfer_policy: document.transfer_policy,
                    notifications: document.notifications,
                    platform: document.platform,
                    ui: UiPreferences::default(),
                };
                settings.validate()?;
                Ok((settings, true))
            }
            CURRENT_SETTINGS_SCHEMA_VERSION => {
                let document: SettingsDocument =
                    serde_json::from_slice(&bytes).map_err(malformed)?;
                AppSettings::try_from(document).map(|settings| (settings, false))
            }
            found => Err(SettingsError::UnsupportedSchemaVersion {
                found,
                supported: CURRENT_SETTINGS_SCHEMA_VERSION,
            }),
        }
    }

    pub fn load_or_initialize(
        &self,
        defaults: &AppSettings,
    ) -> Result<LoadedSettings, SettingsError> {
        defaults.validate()?;
        if !self.path.exists() {
            self.save(defaults)?;
            return Ok(LoadedSettings {
                settings: defaults.clone(),
                recovery: None,
            });
        }

        match self.load_versioned() {
            Ok((settings, migrated)) => {
                if migrated {
                    self.save(&settings)?;
                }
                Ok(LoadedSettings {
                    settings,
                    recovery: None,
                })
            }
            Err(error @ SettingsError::MalformedDocument { .. })
            | Err(error @ SettingsError::EmptyDownloadDirectory)
            | Err(error @ SettingsError::MissingManualProxyEndpoint)
            | Err(error @ SettingsError::ProxyCredentialWithoutUsername)
            | Err(error @ SettingsError::InvalidProxyEndpoint { .. })
            | Err(error @ SettingsError::InvalidNoProxyEntry { .. }) => {
                let reason = error.to_string();
                let backup_path = self.preserve_corrupt_document()?;
                self.save(defaults)?;
                Ok(LoadedSettings {
                    settings: defaults.clone(),
                    recovery: Some(SettingsRecovery {
                        backup_path,
                        reason,
                    }),
                })
            }
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, settings: &AppSettings) -> Result<(), SettingsError> {
        settings.validate()?;
        let (parent, file_name) = self.parent_and_file_name()?;
        fs::create_dir_all(parent)
            .map_err(|source| io_error("create the settings directory", parent, source))?;
        let payload = serde_json::to_vec_pretty(&SettingsDocument::from(settings))
            .map_err(SettingsError::Serialize)?;
        let temp_path = parent.join(format!(
            ".{}.{}.tmp",
            file_name.to_string_lossy(),
            Uuid::new_v4()
        ));

        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp_path)
                .map_err(|source| {
                    io_error("create the temporary settings file", &temp_path, source)
                })?;
            file.write_all(&payload)
                .map_err(|source| io_error("write the settings document", &temp_path, source))?;
            file.write_all(b"\n")
                .map_err(|source| io_error("finish the settings document", &temp_path, source))?;
            file.flush()
                .map_err(|source| io_error("flush the settings document", &temp_path, source))?;
            file.sync_all()
                .map_err(|source| io_error("sync the settings document", &temp_path, source))?;
            replace_file(&temp_path, &self.path)
                .map_err(|source| io_error("replace the settings document", &self.path, source))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result
    }

    fn preserve_corrupt_document(&self) -> Result<PathBuf, SettingsError> {
        let (parent, file_name) = self.parent_and_file_name()?;
        for suffix in 0_u32.. {
            let marker = if suffix == 0 {
                String::new()
            } else {
                format!(".{suffix}")
            };
            let candidate = parent.join(format!("{}.corrupt{marker}", file_name.to_string_lossy()));
            if !candidate.exists() {
                fs::rename(&self.path, &candidate).map_err(|source| {
                    io_error("preserve the corrupt settings document", &candidate, source)
                })?;
                return Ok(candidate);
            }
        }
        unreachable!("the corruption backup suffix space is finite but unreachable in practice")
    }

    fn parent_and_file_name(&self) -> Result<(&Path, &std::ffi::OsStr), SettingsError> {
        let Some(file_name) = self.path.file_name() else {
            return Err(SettingsError::InvalidStorePath {
                path: self.path.clone(),
            });
        };
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        Ok((parent, file_name))
    }
}

#[cfg(windows)]
fn replace_file(temp_path: &Path, target_path: &Path) -> io::Result<()> {
    match fs::rename(temp_path, target_path) {
        Ok(()) => Ok(()),
        Err(first_error) if target_path.exists() => {
            fs::remove_file(target_path)?;
            fs::rename(temp_path, target_path).or(Err(first_error))
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(windows))]
fn replace_file(temp_path: &Path, target_path: &Path) -> io::Result<()> {
    fs::rename(temp_path, target_path)
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> SettingsError {
    SettingsError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

/// Schema version for the session-local window geometry document.
pub const WINDOW_GEOMETRY_SCHEMA_VERSION: u32 = 1;

/// Minimum restoreable content size in logical pixels (matches desktop min).
pub const WINDOW_MIN_WIDTH: f32 = 960.0;
pub const WINDOW_MIN_HEIGHT: f32 = 620.0;
pub const WINDOW_DEFAULT_WIDTH: f32 = 1180.0;
pub const WINDOW_DEFAULT_HEIGHT: f32 = 760.0;

/// Persisted main-window placement (UI-001 / D-031). Stored separately from
/// settings so resize storms never rewrite the full settings document.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WindowGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

impl Default for WindowGeometry {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: WINDOW_DEFAULT_WIDTH,
            height: WINDOW_DEFAULT_HEIGHT,
            maximized: false,
        }
    }
}

impl WindowGeometry {
    /// Reject NaN/non-finite sizes and clamp below the desktop minimum.
    #[must_use]
    pub fn sanitized(self) -> Self {
        let width = if self.width.is_finite() {
            self.width.max(WINDOW_MIN_WIDTH)
        } else {
            WINDOW_DEFAULT_WIDTH
        };
        let height = if self.height.is_finite() {
            self.height.max(WINDOW_MIN_HEIGHT)
        } else {
            WINDOW_DEFAULT_HEIGHT
        };
        let x = if self.x.is_finite() { self.x } else { 0.0 };
        let y = if self.y.is_finite() { self.y } else { 0.0 };
        Self {
            x,
            y,
            width,
            height,
            maximized: self.maximized,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WindowGeometryDocument {
    schema_version: u32,
    geometry: WindowGeometry,
}

/// Atomic JSON store for main-window geometry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonWindowGeometryStore {
    path: PathBuf,
}

impl JsonWindowGeometryStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load a previously saved geometry, or `None` when missing/corrupt.
    pub fn load(&self) -> Option<WindowGeometry> {
        let bytes = fs::read(&self.path).ok()?;
        let document: WindowGeometryDocument = serde_json::from_slice(&bytes).ok()?;
        if document.schema_version != WINDOW_GEOMETRY_SCHEMA_VERSION {
            return None;
        }
        Some(document.geometry.sanitized())
    }

    pub fn save(&self, geometry: WindowGeometry) -> Result<(), SettingsError> {
        let geometry = geometry.sanitized();
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|source| io_error("create the window-geometry directory", parent, source))?;
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| SettingsError::InvalidStorePath {
                path: self.path.clone(),
            })?;
        let temp_path = parent.join(format!(".{file_name}.tmp"));
        let payload = serde_json::to_vec_pretty(&WindowGeometryDocument {
            schema_version: WINDOW_GEOMETRY_SCHEMA_VERSION,
            geometry,
        })
        .map_err(SettingsError::Serialize)?;
        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&temp_path)
                .map_err(|source| {
                    io_error("write the window-geometry temp file", &temp_path, source)
                })?;
            file.write_all(&payload).map_err(|source| {
                io_error("write the window-geometry temp file", &temp_path, source)
            })?;
            file.write_all(b"\n").map_err(|source| {
                io_error("write the window-geometry temp file", &temp_path, source)
            })?;
            file.sync_all().map_err(|source| {
                io_error("sync the window-geometry temp file", &temp_path, source)
            })?;
        }
        replace_file(&temp_path, &self.path).map_err(|source| {
            io_error("replace the window-geometry document", &self.path, source)
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(root: &Path) -> AppSettings {
        AppSettings {
            color_scheme: ColorScheme::Light,
            download_directory: root.join("downloads"),
            download_proxy: DownloadProxySettings::default(),
            speed_limits: SpeedLimitSettings::default(),
            transfer_policy: TransferPolicySettings::default(),
            notifications: NotificationSettings::default(),
            platform: PlatformSettings::default(),
            ui: UiPreferences::default(),
        }
    }

    #[test]
    fn initializes_and_round_trips_a_versioned_document() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        let expected = settings(root.path());

        let loaded = store
            .load_or_initialize(&expected)
            .expect("initialize settings");
        assert_eq!(loaded.settings, expected);
        assert!(loaded.recovery.is_none());
        assert_eq!(store.load().expect("load settings"), expected);

        let document = fs::read_to_string(store.path()).expect("read settings JSON");
        assert!(document.contains("\"schema_version\": 7"));
        assert!(document.contains("\"transfer_policy\""));
        assert!(document.contains("\"notifications\""));
        assert!(document.contains("\"platform\""));
        assert!(document.contains("\"os_notifications\""));
        assert!(document.contains("\"ui\""));
        assert!(document.ends_with('\n'));
    }

    #[test]
    fn version_one_document_is_migrated_with_proxy_disabled() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        fs::write(
            store.path(),
            r#"{"schema_version":1,"color_scheme":"light","download_directory":"downloads"}"#,
        )
        .expect("seed version one settings");

        let loaded = store
            .load_or_initialize(&settings(root.path()))
            .expect("migrate version one settings");

        assert_eq!(
            loaded.settings.download_proxy,
            DownloadProxySettings::default()
        );
        assert_eq!(loaded.settings.speed_limits, SpeedLimitSettings::default());
        assert_eq!(
            loaded.settings.transfer_policy,
            TransferPolicySettings::default()
        );
        assert_eq!(
            loaded.settings.notifications,
            NotificationSettings::default()
        );
        assert_eq!(loaded.settings.platform, PlatformSettings::default());
        assert_eq!(loaded.settings.ui, UiPreferences::default());
        assert!(loaded.recovery.is_none());
        let migrated = fs::read_to_string(store.path()).expect("read migrated settings");
        assert!(migrated.contains("\"schema_version\": 7"));
        assert!(migrated.contains("\"download_proxy\""));
        assert!(migrated.contains("\"speed_limits\""));
        assert!(migrated.contains("\"transfer_policy\""));
        assert!(migrated.contains("\"notifications\""));
        assert!(migrated.contains("\"platform\""));
        assert!(migrated.contains("\"ui\""));
    }

    #[test]
    fn version_two_document_is_migrated_with_speed_limits_at_zero() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        fs::write(
            store.path(),
            r#"{"schema_version":2,"color_scheme":"dark","download_directory":"downloads","download_proxy":{"mode":"disabled","all_proxy":null,"http_proxy":null,"https_proxy":null,"ftp_proxy":null,"no_proxy":[],"username":null,"credential":null}}"#,
        )
        .expect("seed version two settings");

        let loaded = store
            .load_or_initialize(&settings(root.path()))
            .expect("migrate version two settings");

        assert_eq!(loaded.settings.speed_limits, SpeedLimitSettings::default());
        assert_eq!(
            loaded.settings.transfer_policy,
            TransferPolicySettings::default()
        );
        assert_eq!(
            loaded.settings.notifications,
            NotificationSettings::default()
        );
        assert_eq!(loaded.settings.platform, PlatformSettings::default());
        assert_eq!(loaded.settings.ui, UiPreferences::default());
        assert!(loaded.recovery.is_none());
        let migrated = fs::read_to_string(store.path()).expect("read migrated settings");
        assert!(migrated.contains("\"schema_version\": 7"));
        assert!(migrated.contains("\"speed_limits\""));
        assert!(migrated.contains("\"transfer_policy\""));
        assert!(migrated.contains("\"notifications\""));
        assert!(migrated.contains("\"platform\""));
        assert!(migrated.contains("\"ui\""));
    }

    #[test]
    fn version_three_document_is_migrated_with_default_transfer_policy() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        fs::write(
            store.path(),
            r#"{"schema_version":3,"color_scheme":"dark","download_directory":"downloads","download_proxy":{"mode":"disabled","all_proxy":null,"http_proxy":null,"https_proxy":null,"ftp_proxy":null,"no_proxy":[],"username":null,"credential":null},"speed_limits":{"download_limit":1048576,"upload_limit":0}}"#,
        )
        .expect("seed version three settings");

        let loaded = store
            .load_or_initialize(&settings(root.path()))
            .expect("migrate version three settings");

        assert_eq!(
            loaded.settings.speed_limits,
            SpeedLimitSettings {
                download_limit: 1_048_576,
                upload_limit: 0,
            }
        );
        assert_eq!(
            loaded.settings.transfer_policy,
            TransferPolicySettings::default()
        );
        assert_eq!(
            loaded.settings.notifications,
            NotificationSettings::default()
        );
        assert_eq!(loaded.settings.platform, PlatformSettings::default());
        assert_eq!(loaded.settings.ui, UiPreferences::default());
        assert!(loaded.recovery.is_none());
        let migrated = fs::read_to_string(store.path()).expect("read migrated settings");
        assert!(migrated.contains("\"schema_version\": 7"));
        assert!(migrated.contains("\"transfer_policy\""));
        assert!(migrated.contains("\"max_concurrent_downloads\""));
        assert!(migrated.contains("\"notifications\""));
        assert!(migrated.contains("\"platform\""));
        assert!(migrated.contains("\"ui\""));
    }

    #[test]
    fn version_four_document_is_migrated_with_default_notifications() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        fs::write(
            store.path(),
            r#"{"schema_version":4,"color_scheme":"dark","download_directory":"downloads","download_proxy":{"mode":"disabled","all_proxy":null,"http_proxy":null,"https_proxy":null,"ftp_proxy":null,"no_proxy":[],"username":null,"credential":null},"speed_limits":{"download_limit":0,"upload_limit":0},"transfer_policy":{"max_concurrent_downloads":5,"max_connection_per_server":1,"split":5,"min_split_size":20971520,"file_allocation":"prealloc","check_integrity":false}}"#,
        )
        .expect("seed version four settings");

        let loaded = store
            .load_or_initialize(&settings(root.path()))
            .expect("migrate version four settings");

        assert_eq!(
            loaded.settings.transfer_policy,
            TransferPolicySettings::default()
        );
        assert_eq!(
            loaded.settings.notifications,
            NotificationSettings::default()
        );
        assert_eq!(loaded.settings.platform, PlatformSettings::default());
        assert_eq!(loaded.settings.ui, UiPreferences::default());
        assert!(loaded.recovery.is_none());
        let migrated = fs::read_to_string(store.path()).expect("read migrated settings");
        assert!(migrated.contains("\"schema_version\": 7"));
        assert!(migrated.contains("\"notifications\""));
        assert!(migrated.contains("\"notify_on_completion\""));
        assert!(migrated.contains("\"platform\""));
        assert!(migrated.contains("\"ui\""));
    }

    #[test]
    fn version_five_document_is_migrated_with_default_platform_and_os_notifications() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        fs::write(
            store.path(),
            r#"{"schema_version":5,"color_scheme":"dark","download_directory":"downloads","download_proxy":{"mode":"disabled","all_proxy":null,"http_proxy":null,"https_proxy":null,"ftp_proxy":null,"no_proxy":[],"username":null,"credential":null},"speed_limits":{"download_limit":0,"upload_limit":0},"transfer_policy":{"max_concurrent_downloads":5,"max_connection_per_server":1,"split":5,"min_split_size":20971520,"file_allocation":"prealloc","check_integrity":false},"notifications":{"volume":"quiet","notify_on_completion":false,"notify_on_error":true,"notify_on_engine_events":true}}"#,
        )
        .expect("seed version five settings");

        let loaded = store
            .load_or_initialize(&settings(root.path()))
            .expect("migrate version five settings");

        assert_eq!(
            loaded.settings.notifications,
            NotificationSettings {
                volume: NotificationVolume::Quiet,
                notify_on_completion: false,
                notify_on_error: true,
                notify_on_engine_events: true,
                os_notifications: true,
                notify_on_low_disk: true,
                low_disk_threshold_bytes: 1_073_741_824,
            }
        );
        assert_eq!(loaded.settings.platform, PlatformSettings::default());
        assert_eq!(loaded.settings.ui, UiPreferences::default());
        assert!(loaded.recovery.is_none());
        let migrated = fs::read_to_string(store.path()).expect("read migrated settings");
        assert!(migrated.contains("\"schema_version\": 7"));
        assert!(migrated.contains("\"platform\""));
        assert!(migrated.contains("\"os_notifications\""));
        assert!(migrated.contains("\"close_behavior\""));
        assert!(migrated.contains("\"ui\""));
    }

    #[test]
    fn version_six_document_is_migrated_with_default_ui_preferences() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        fs::write(
            store.path(),
            r#"{"schema_version":6,"color_scheme":"system","download_directory":"downloads","download_proxy":{"mode":"disabled","all_proxy":null,"http_proxy":null,"https_proxy":null,"ftp_proxy":null,"no_proxy":[],"username":null,"credential":null},"speed_limits":{"download_limit":0,"upload_limit":0},"transfer_policy":{"max_concurrent_downloads":5,"max_connection_per_server":1,"split":5,"min_split_size":20971520,"file_allocation":"prealloc","check_integrity":false},"notifications":{"volume":"normal","notify_on_completion":true,"notify_on_error":true,"notify_on_engine_events":true,"os_notifications":true,"notify_on_low_disk":true,"low_disk_threshold_bytes":1073741824},"platform":{"close_behavior":"minimize_to_tray","show_tray_icon":true,"start_minimized_to_tray":false}}"#,
        )
        .expect("seed version six settings");

        let loaded = store
            .load_or_initialize(&settings(root.path()))
            .expect("migrate version six settings");

        assert_eq!(loaded.settings.color_scheme, ColorScheme::System);
        assert_eq!(loaded.settings.ui, UiPreferences::default());
        assert!(loaded.recovery.is_none());
        let migrated = fs::read_to_string(store.path()).expect("read migrated settings");
        assert!(migrated.contains("\"schema_version\": 7"));
        assert!(migrated.contains("\"ui\""));
        assert!(migrated.contains("\"list_filter\""));
    }

    #[test]
    fn ui_preferences_and_system_theme_round_trip() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        let mut expected = settings(root.path());
        expected.color_scheme = ColorScheme::System;
        expected.ui = UiPreferences {
            list_filter: ListFilterPreference::Completed,
            list_sort_key: ListSortKeyPreference::Size,
            list_sort_direction: ListSortDirectionPreference::Descending,
        };

        store.save(&expected).expect("save ui preferences");
        assert_eq!(store.load().expect("load ui preferences"), expected);
    }

    #[test]
    fn window_geometry_round_trips_and_sanitizes() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonWindowGeometryStore::new(root.path().join("window.json"));
        assert!(store.load().is_none());

        let geometry = WindowGeometry {
            x: 120.0,
            y: 80.0,
            width: 1400.0,
            height: 900.0,
            maximized: true,
        };
        store.save(geometry).expect("save window geometry");
        assert_eq!(store.load(), Some(geometry));

        let tiny = WindowGeometry {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            maximized: false,
        };
        store.save(tiny).expect("save tiny geometry");
        let restored = store.load().expect("load sanitized geometry");
        assert_eq!(restored.width, WINDOW_MIN_WIDTH);
        assert_eq!(restored.height, WINDOW_MIN_HEIGHT);
    }

    #[test]
    fn platform_settings_round_trip() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        let mut expected = settings(root.path());
        expected.platform = PlatformSettings {
            close_behavior: CloseBehavior::Quit,
            show_tray_icon: false,
            start_minimized_to_tray: true,
        };
        expected.notifications.os_notifications = false;
        expected.notifications.notify_on_low_disk = false;
        expected.notifications.low_disk_threshold_bytes = 512 * 1024 * 1024;

        store.save(&expected).expect("save platform settings");
        assert_eq!(store.load().expect("load platform settings"), expected);
    }

    #[test]
    fn manual_proxy_round_trips_without_a_password_field() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        let mut expected = settings(root.path());
        expected.download_proxy = DownloadProxySettings {
            mode: DownloadProxyMode::Manual,
            all_proxy: Some("http://proxy.example:8080".into()),
            http_proxy: None,
            https_proxy: Some("secure-proxy.example:8443".into()),
            ftp_proxy: None,
            no_proxy: vec!["localhost".into(), "10.0.0.0/8".into()],
            username: Some("proxy-user".into()),
            credential: Some(ProxyCredentialRef::new()),
        };

        store.save(&expected).expect("save proxy settings");

        assert_eq!(store.load().expect("load proxy settings"), expected);
        let document = fs::read_to_string(store.path()).expect("read proxy settings JSON");
        assert!(!document.to_ascii_lowercase().contains("password"));
        assert!(document.contains("proxy.example:8080"));
        assert!(document.contains("proxy-user"));
    }

    #[test]
    fn proxy_validation_rejects_ambiguous_or_secret_bearing_values() {
        let mut settings = AppSettings::new("downloads");
        settings.download_proxy.mode = DownloadProxyMode::Manual;
        assert!(matches!(
            settings.validate(),
            Err(SettingsError::MissingManualProxyEndpoint)
        ));

        settings.download_proxy.all_proxy = Some("http://user:secret@proxy.example:8080".into());
        assert!(matches!(
            settings.validate(),
            Err(SettingsError::InvalidProxyEndpoint { .. })
        ));

        settings.download_proxy.all_proxy = Some("proxy.example:8080".into());
        settings.download_proxy.no_proxy = vec!["https://localhost".into()];
        assert!(matches!(
            settings.validate(),
            Err(SettingsError::InvalidNoProxyEntry { .. })
        ));
    }

    #[test]
    fn malformed_document_is_preserved_before_defaults_are_restored() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        fs::write(store.path(), b"{not-json").expect("seed malformed settings");
        let defaults = settings(root.path());

        let loaded = store
            .load_or_initialize(&defaults)
            .expect("recover settings");
        let recovery = loaded.recovery.expect("recovery metadata");
        assert_eq!(
            fs::read(recovery.backup_path).expect("read backup"),
            b"{not-json"
        );
        assert_eq!(store.load().expect("load restored defaults"), defaults);
    }

    #[test]
    fn invalid_proxy_document_is_preserved_before_defaults_are_restored() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        let invalid = r#"{
            "schema_version": 2,
            "color_scheme": "dark",
            "download_directory": "downloads",
            "download_proxy": {
                "mode": "manual",
                "all_proxy": null,
                "http_proxy": null,
                "https_proxy": null,
                "ftp_proxy": null,
                "no_proxy": [],
                "username": null,
                "credential": null
            }
        }"#;
        fs::write(store.path(), invalid).expect("seed invalid proxy settings");
        let defaults = settings(root.path());

        let loaded = store
            .load_or_initialize(&defaults)
            .expect("recover invalid proxy settings");
        let recovery = loaded.recovery.expect("recovery metadata");
        assert_eq!(
            fs::read_to_string(recovery.backup_path).expect("read backup"),
            invalid
        );
        assert_eq!(loaded.settings, defaults);
        assert_eq!(store.load().expect("load restored defaults"), defaults);
    }

    #[test]
    fn future_schema_is_rejected_without_replacing_the_document() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        let future = format!(
            "{{\"schema_version\":{},\"color_scheme\":\"dark\",\"download_directory\":\"downloads\"}}",
            CURRENT_SETTINGS_SCHEMA_VERSION + 1
        );
        fs::write(store.path(), &future).expect("seed future settings");

        assert!(matches!(
            store.load_or_initialize(&settings(root.path())),
            Err(SettingsError::UnsupportedSchemaVersion { .. })
        ));
        assert_eq!(
            fs::read_to_string(store.path()).expect("read future JSON"),
            future
        );
    }

    #[test]
    fn empty_download_directory_is_rejected() {
        let store = JsonSettingsStore::new("settings.json");
        let settings = AppSettings::new(PathBuf::new());
        assert!(matches!(
            store.save(&settings),
            Err(SettingsError::EmptyDownloadDirectory)
        ));
    }
}
