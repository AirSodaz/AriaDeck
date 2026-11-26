use std::{
    fs,
    path::{Path, PathBuf},
};

use ariadeck_domain::DomainHeaderPreset;
use serde::{Deserialize, Serialize};

use crate::{
    BrowserBridgeSettings, CURRENT_SETTINGS_SCHEMA_VERSION, ColorScheme, DownloadCategory,
    DownloadProxySettings, LanguagePreference, MirrorRewriteSettings, NotificationSettings,
    OnboardingSettings, PlatformSettings, SettingsError, SpeedLimitSettings, TrackerListSettings,
    TransferPolicySettings, UiPreferences, atomic_io, default_download_categories,
    environment::{sync_fallback_directory, validate_engine_environment},
    error::io_error,
    rules::validate_header_preset_list,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppSettings {
    pub color_scheme: ColorScheme,
    pub language: LanguagePreference,
    pub download_directory: PathBuf,
    pub download_proxy: DownloadProxySettings,
    pub speed_limits: SpeedLimitSettings,
    pub transfer_policy: TransferPolicySettings,
    pub notifications: NotificationSettings,
    pub platform: PlatformSettings,
    pub browser_bridge: BrowserBridgeSettings,
    /// First-run guidance state (B4). Deliberately absent from the export
    /// document: importing settings is not a claim about first-run history.
    pub onboarding: OnboardingSettings,
    pub ui: UiPreferences,
    /// Output folders + extension rules (C1 / D-042). Exactly one is_fallback.
    pub categories: Vec<DownloadCategory>,
    /// Extra BitTorrent trackers for aria2 `bt-tracker` (D1).
    pub tracker_list: TrackerListSettings,
    /// Client-wide domain-matched request header presets (cookie-free).
    pub header_presets: Vec<DomainHeaderPreset>,
    /// Client-wide URL mirror rewrite rules.
    pub mirror_rules: MirrorRewriteSettings,
}

impl AppSettings {
    #[must_use]
    pub fn new(download_directory: impl Into<PathBuf>) -> Self {
        let download_directory = download_directory.into();
        Self {
            color_scheme: ColorScheme::default(),
            language: LanguagePreference::default(),
            download_directory: download_directory.clone(),
            download_proxy: DownloadProxySettings::default(),
            speed_limits: SpeedLimitSettings::default(),
            transfer_policy: TransferPolicySettings::default(),
            notifications: NotificationSettings::default(),
            platform: PlatformSettings::default(),
            browser_bridge: BrowserBridgeSettings::default(),
            onboarding: OnboardingSettings::default(),
            ui: UiPreferences::default(),
            categories: default_download_categories(&download_directory),
            tracker_list: TrackerListSettings::default(),
            header_presets: Vec::new(),
            mirror_rules: MirrorRewriteSettings::default(),
        }
    }

    pub fn validate(&self) -> Result<(), SettingsError> {
        validate_engine_environment(
            &self.download_directory,
            &self.download_proxy,
            &self.transfer_policy,
            &self.categories,
            &self.tracker_list,
        )?;
        validate_header_preset_list(&self.header_presets)?;
        self.mirror_rules.validate()?;
        Ok(())
    }
}

/// Keep `download_directory` aligned with the fallback category path.
pub fn sync_download_directory_from_fallback(settings: &mut AppSettings) {
    sync_fallback_directory(&mut settings.download_directory, &settings.categories);
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

#[derive(Debug, Deserialize)]
struct SettingsVersionProbe {
    schema_version: u32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SettingsDocument {
    schema_version: u32,
    color_scheme: ColorScheme,
    language: LanguagePreference,
    download_directory: PathBuf,
    download_proxy: DownloadProxySettings,
    speed_limits: SpeedLimitSettings,
    transfer_policy: TransferPolicySettings,
    notifications: NotificationSettings,
    platform: PlatformSettings,
    /// Added by D-045 and additive on purpose: schema stays v1 and a settings
    /// file written before the bridge existed still loads, taking the
    /// all-off default instead of tripping the corrupt-document recovery path.
    #[serde(default)]
    browser_bridge: BrowserBridgeSettings,
    /// Added by B4, additive for the same reason as `browser_bridge` above: a
    /// settings file written before the first-run guide existed loads with the
    /// not-yet-dismissed default, which is exactly right for an upgrade.
    #[serde(default)]
    onboarding: OnboardingSettings,
    ui: UiPreferences,
    categories: Vec<DownloadCategory>,
    tracker_list: TrackerListSettings,
    #[serde(default)]
    header_presets: Vec<DomainHeaderPreset>,
    #[serde(default)]
    mirror_rules: MirrorRewriteSettings,
}

impl From<&AppSettings> for SettingsDocument {
    fn from(settings: &AppSettings) -> Self {
        Self {
            schema_version: CURRENT_SETTINGS_SCHEMA_VERSION,
            color_scheme: settings.color_scheme,
            language: settings.language,
            download_directory: settings.download_directory.clone(),
            download_proxy: settings.download_proxy.clone(),
            speed_limits: settings.speed_limits,
            transfer_policy: settings.transfer_policy,
            notifications: settings.notifications,
            platform: settings.platform,
            browser_bridge: settings.browser_bridge,
            onboarding: settings.onboarding,
            ui: settings.ui,
            categories: settings.categories.clone(),
            tracker_list: settings.tracker_list.clone(),
            header_presets: settings.header_presets.clone(),
            mirror_rules: settings.mirror_rules.clone(),
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
        let mut settings = Self {
            color_scheme: document.color_scheme,
            language: document.language,
            download_directory: document.download_directory,
            download_proxy: document.download_proxy,
            speed_limits: document.speed_limits,
            transfer_policy: document.transfer_policy,
            notifications: document.notifications,
            platform: document.platform,
            browser_bridge: document.browser_bridge,
            onboarding: document.onboarding,
            ui: document.ui,
            categories: document.categories,
            tracker_list: document.tracker_list,
            header_presets: document.header_presets,
            mirror_rules: document.mirror_rules,
        };
        sync_download_directory_from_fallback(&mut settings);
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
        let bytes = fs::read(&self.path)
            .map_err(|source| io_error("read the settings document", &self.path, source))?;
        let malformed = |error: serde_json::Error| SettingsError::MalformedDocument {
            path: self.path.clone(),
            message: error.to_string(),
        };
        // Probe version before full decode so unsupported schemas are not mistaken
        // for malformed documents (which load_or_initialize would silently reset).
        // Only the current schema is accepted in-tree. When a release tag needs a
        // migration, add a single previous-tag path here. Dev configs are maintained
        // manually (reset settings.json as needed).
        let probe: SettingsVersionProbe = serde_json::from_slice(&bytes).map_err(malformed)?;
        if probe.schema_version != CURRENT_SETTINGS_SCHEMA_VERSION {
            return Err(SettingsError::UnsupportedSchemaVersion {
                found: probe.schema_version,
                supported: CURRENT_SETTINGS_SCHEMA_VERSION,
            });
        }
        let document: SettingsDocument = serde_json::from_slice(&bytes).map_err(malformed)?;
        AppSettings::try_from(document)
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

        match self.load() {
            Ok(settings) => Ok(LoadedSettings {
                settings,
                recovery: None,
            }),
            Err(error) if error.is_recoverable_document_error() => {
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
        let mut settings = settings.clone();
        sync_download_directory_from_fallback(&mut settings);
        settings.validate()?;
        let payload = serde_json::to_vec_pretty(&SettingsDocument::from(&settings))
            .map_err(SettingsError::Serialize)?;
        atomic_io::write_atomic_json(
            &self.path,
            &payload,
            "create the settings directory",
            "create the temporary settings file",
            "write the settings document",
            "sync the settings document",
            "replace the settings document",
        )
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
