use std::{
    fs,
    path::{Path, PathBuf},
};

use ariadeck_domain::DomainHeaderPreset;
use serde::{Deserialize, Serialize};

use crate::{
    AppSettings, BrowserBridgeSettings, CURRENT_SETTINGS_SCHEMA_VERSION, ColorScheme,
    CompletionPowerAction, DownloadCategory, DownloadProxyMode, DownloadProxySettings,
    LanguagePreference, MirrorRewriteSettings, NotificationSettings, PlatformSettings,
    SETTINGS_EXPORT_FORMAT_VERSION, SettingsError, SpeedLimitSettings, TrackerListSettings,
    TransferPolicySettings, UiPreferences, atomic_io, error::io_error,
};

/// Portable settings representation used by explicit export/import actions.
///
/// This intentionally does not contain `ProxyCredentialRef`; credentials stay
/// in the OS keychain and are never copied into a transfer document.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SettingsExportDocument {
    export_version: u32,
    settings_schema_version: u32,
    color_scheme: ColorScheme,
    language: LanguagePreference,
    download_directory: PathBuf,
    download_proxy: SettingsExportProxy,
    speed_limits: SpeedLimitSettings,
    transfer_policy: TransferPolicySettings,
    notifications: NotificationSettings,
    platform: PlatformSettings,
    /// Absent from documents written before D-045; `auto_submit` only, because
    /// `allow_cookies` deliberately does not travel — see [`SettingsExportBrowserBridge`].
    #[serde(default)]
    browser_bridge: SettingsExportBrowserBridge,
    ui: UiPreferences,
    categories: Vec<DownloadCategory>,
    tracker_list: TrackerListSettings,
    #[serde(default)]
    header_presets: Vec<DomainHeaderPreset>,
    #[serde(default)]
    mirror_rules: MirrorRewriteSettings,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SettingsExportProxy {
    mode: DownloadProxyMode,
    all_proxy: Option<String>,
    http_proxy: Option<String>,
    https_proxy: Option<String>,
    ftp_proxy: Option<String>,
    no_proxy: Vec<String>,
    username: Option<String>,
    #[serde(default = "crate::proxy::default_true")]
    check_certificate: bool,
}

/// Browser bridge preferences as they appear in a transfer document.
///
/// `allow_cookies` is intentionally missing, for the same reason
/// [`SettingsExportProxy`] omits the credential ref: importing a settings file is
/// an explicit action, but it is not an explicit decision about forwarding
/// cookies. That opt-in is made per machine, and an import always leaves it off.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SettingsExportBrowserBridge {
    auto_submit: bool,
}

impl From<&BrowserBridgeSettings> for SettingsExportBrowserBridge {
    fn from(bridge: &BrowserBridgeSettings) -> Self {
        Self {
            auto_submit: bridge.auto_submit,
        }
    }
}

impl From<&DownloadProxySettings> for SettingsExportProxy {
    fn from(proxy: &DownloadProxySettings) -> Self {
        Self {
            mode: proxy.mode,
            all_proxy: proxy.all_proxy.clone(),
            http_proxy: proxy.http_proxy.clone(),
            https_proxy: proxy.https_proxy.clone(),
            ftp_proxy: proxy.ftp_proxy.clone(),
            no_proxy: proxy.no_proxy.clone(),
            username: proxy.username.clone(),
            check_certificate: proxy.check_certificate,
        }
    }
}

impl SettingsExportDocument {
    fn from_settings(settings: &AppSettings) -> Self {
        Self {
            export_version: SETTINGS_EXPORT_FORMAT_VERSION,
            settings_schema_version: CURRENT_SETTINGS_SCHEMA_VERSION,
            color_scheme: settings.color_scheme,
            language: settings.language,
            download_directory: settings.download_directory.clone(),
            download_proxy: SettingsExportProxy::from(&settings.download_proxy),
            speed_limits: settings.speed_limits,
            transfer_policy: settings.transfer_policy,
            notifications: settings.notifications,
            platform: PlatformSettings {
                action_after_done: CompletionPowerAction::None,
                ..settings.platform
            },
            browser_bridge: SettingsExportBrowserBridge::from(&settings.browser_bridge),
            ui: settings.ui,
            categories: settings.categories.clone(),
            tracker_list: settings.tracker_list.clone(),
            header_presets: settings.header_presets.clone(),
            mirror_rules: settings.mirror_rules.clone(),
        }
    }

    fn into_settings(self, current: &AppSettings) -> Result<AppSettings, SettingsError> {
        if self.export_version != SETTINGS_EXPORT_FORMAT_VERSION {
            return Err(SettingsError::UnsupportedExportVersion {
                found: self.export_version,
                supported: SETTINGS_EXPORT_FORMAT_VERSION,
            });
        }
        if self.settings_schema_version != CURRENT_SETTINGS_SCHEMA_VERSION {
            return Err(SettingsError::UnsupportedSchemaVersion {
                found: self.settings_schema_version,
                supported: CURRENT_SETTINGS_SCHEMA_VERSION,
            });
        }
        let same_proxy_identity = self.download_proxy.mode == current.download_proxy.mode
            && self.download_proxy.all_proxy == current.download_proxy.all_proxy
            && self.download_proxy.http_proxy == current.download_proxy.http_proxy
            && self.download_proxy.https_proxy == current.download_proxy.https_proxy
            && self.download_proxy.ftp_proxy == current.download_proxy.ftp_proxy
            && self.download_proxy.no_proxy == current.download_proxy.no_proxy
            && self.download_proxy.username == current.download_proxy.username;
        let credential = same_proxy_identity
            .then_some(current.download_proxy.credential)
            .flatten();
        let mut settings = AppSettings {
            color_scheme: self.color_scheme,
            language: self.language,
            download_directory: self.download_directory,
            download_proxy: DownloadProxySettings {
                mode: self.download_proxy.mode,
                all_proxy: self.download_proxy.all_proxy,
                http_proxy: self.download_proxy.http_proxy,
                https_proxy: self.download_proxy.https_proxy,
                ftp_proxy: self.download_proxy.ftp_proxy,
                no_proxy: self.download_proxy.no_proxy,
                username: self.download_proxy.username,
                credential,
                check_certificate: self.download_proxy.check_certificate,
            },
            speed_limits: self.speed_limits,
            transfer_policy: self.transfer_policy,
            notifications: self.notifications,
            platform: PlatformSettings {
                action_after_done: CompletionPowerAction::None,
                ..self.platform
            },
            browser_bridge: BrowserBridgeSettings {
                // An import never turns cookie forwarding on; see
                // SettingsExportBrowserBridge.
                allow_cookies: false,
                auto_submit: self.browser_bridge.auto_submit,
            },
            // First-run history belongs to this installation, so it survives an
            // import untouched rather than travelling inside the document.
            onboarding: current.onboarding,
            ui: self.ui,
            categories: self.categories,
            tracker_list: self.tracker_list,
            header_presets: self.header_presets,
            mirror_rules: self.mirror_rules,
        };
        crate::sync_download_directory_from_fallback(&mut settings);
        settings.validate()?;
        Ok(settings)
    }
}

/// Serialize settings to the portable, credential-free transfer format.
pub fn export_settings_json(settings: &AppSettings) -> Result<String, SettingsError> {
    settings.validate()?;
    serde_json::to_string_pretty(&SettingsExportDocument::from_settings(settings))
        .map_err(SettingsError::Serialize)
}

#[derive(Debug, Deserialize)]
struct ExportVersionProbe {
    export_version: Option<u32>,
    settings_schema_version: Option<u32>,
}

/// Parse a portable settings document while preserving the current keychain credential.
pub fn import_settings_json(
    payload: &str,
    current: &AppSettings,
) -> Result<AppSettings, SettingsError> {
    if let Ok(probe) = serde_json::from_str::<ExportVersionProbe>(payload) {
        if let Some(found) = probe.export_version
            && found != SETTINGS_EXPORT_FORMAT_VERSION
        {
            return Err(SettingsError::UnsupportedExportVersion {
                found,
                supported: SETTINGS_EXPORT_FORMAT_VERSION,
            });
        }
        if let Some(found) = probe.settings_schema_version
            && found != CURRENT_SETTINGS_SCHEMA_VERSION
        {
            return Err(SettingsError::UnsupportedSchemaVersion {
                found,
                supported: CURRENT_SETTINGS_SCHEMA_VERSION,
            });
        }
    }
    let document: SettingsExportDocument =
        serde_json::from_str(payload).map_err(|error| SettingsError::MalformedExport {
            message: error.to_string(),
        })?;
    document.into_settings(current)
}

/// Write a portable settings document to a user-selected path.
pub fn export_settings_to_path(
    path: impl AsRef<Path>,
    settings: &AppSettings,
) -> Result<(), SettingsError> {
    let path = path.as_ref();
    let payload = export_settings_json(settings)?;
    atomic_io::write_atomic_json(
        path,
        payload.as_bytes(),
        "create the export directory",
        "create the temporary export file",
        "write the settings export",
        "flush the settings export",
        "replace the settings export",
    )
}

/// Read and validate a portable settings document from a user-selected path.
pub fn import_settings_from_path(
    path: impl AsRef<Path>,
    current: &AppSettings,
) -> Result<AppSettings, SettingsError> {
    let path = path.as_ref();
    let payload = fs::read_to_string(path)
        .map_err(|source| io_error("read the settings export", path, source))?;
    import_settings_json(&payload, current)
}
