//! Typed, versioned application settings and their persistence boundary.

mod atomic_io;
mod categories;
mod environment;
mod error;
mod export;
mod platform;
mod profile_env;
mod proxy;
mod store;
mod system_proxy;
mod tracker;
mod transfer;
mod ui;
mod window_geometry;

#[cfg(test)]
mod tests;

pub use categories::{
    DownloadCategory, MAX_CATEGORY_EXTENSION_LEN, MAX_CATEGORY_EXTENSIONS, MAX_DOWNLOAD_CATEGORIES,
    default_download_categories, format_category_extensions, normalize_category_extensions,
    parse_category_extensions_text, resolve_download_category, validate_categories,
};
pub use error::SettingsError;
pub use export::{
    export_settings_json, export_settings_to_path, import_settings_from_path, import_settings_json,
};
pub use platform::{
    BrowserBridgeSettings, CloseBehavior, CompletionPowerAction, NotificationSettings,
    NotificationVolume, PlatformSettings,
};
pub use profile_env::{
    PROFILE_ENVIRONMENT_SCHEMA_VERSION, ProfileEnvironment, ProfileEnvironmentStore,
};
pub use proxy::{
    DownloadProxyMode, DownloadProxySettings, ProxyCredentialError, ProxyCredentialRef,
    ProxyCredentialStore, SystemProxyCredentialStore,
};
pub use store::{
    AppSettings, JsonSettingsStore, LoadedSettings, SettingsRecovery,
    sync_download_directory_from_fallback,
};
pub use system_proxy::{
    ResolvedSystemProxy, SystemProxyError, parse_windows_proxy_server, resolve_from_env_map,
    resolve_system_proxy,
};
pub use tracker::{
    CURATED_TRACKER_LIST_URL, MAX_TRACKER_LIST_TEXT_CHARS, MAX_TRACKER_LIST_URL_CHARS,
    TrackerListSettings, TrackerListSource,
};
pub use transfer::{FileAllocationSetting, SpeedLimitSettings, TransferPolicySettings};
pub use ui::{
    ColorScheme, LanguagePreference, ListFilterPreference, ListSortDirectionPreference,
    ListSortKeyPreference, OnboardingSettings, UiPreferences,
};
pub use uuid::Uuid;
pub use window_geometry::{
    JsonWindowGeometryStore, WINDOW_DEFAULT_HEIGHT, WINDOW_DEFAULT_WIDTH,
    WINDOW_GEOMETRY_SCHEMA_VERSION, WINDOW_MIN_HEIGHT, WINDOW_MIN_WIDTH, WindowGeometry,
};

pub const CURRENT_SETTINGS_SCHEMA_VERSION: u32 = 1;
/// Version of the user-facing settings transfer document.
pub const SETTINGS_EXPORT_FORMAT_VERSION: u32 = 1;
