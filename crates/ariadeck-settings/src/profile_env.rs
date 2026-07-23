//! Per-profile aria2 environment bags (download dirs, proxy, limits, trackers).
//!
//! # Storage layout (D-043)
//!
//! ```text
//! <data_dir>/
//!   settings.json                 # client-wide prefs + active env mirror
//!   profiles.json                 # catalog (identity / kind / endpoint)
//!   profiles/<profile_id>/
//!     environment.json            # this module — schema v1 only
//! ```
//!
//! Client-wide prefs (theme, language, notifications, platform, UI list prefs)
//! stay in `settings.json`. Values that must match the active engine host live
//! here and swap when the user activates another profile.
//!
//! Schema policy matches settings / profiles catalog: only the current version
//! is accepted in-tree; add a previous-tag migrator only when a release needs it.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    AppSettings, DownloadCategory, DownloadProxySettings, SettingsError, SpeedLimitSettings,
    TrackerListSettings, TransferPolicySettings, default_download_categories, validate_categories,
};

/// On-disk schema for `profiles/<id>/environment.json`.
pub const PROFILE_ENVIRONMENT_SCHEMA_VERSION: u32 = 1;

/// Aria2-host-related settings scoped to one profile.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileEnvironment {
    pub schema_version: u32,
    pub download_directory: PathBuf,
    pub download_proxy: DownloadProxySettings,
    pub speed_limits: SpeedLimitSettings,
    pub transfer_policy: TransferPolicySettings,
    pub categories: Vec<DownloadCategory>,
    pub tracker_list: TrackerListSettings,
}

impl ProfileEnvironment {
    #[must_use]
    pub fn from_settings(settings: &AppSettings) -> Self {
        let mut env = Self {
            schema_version: PROFILE_ENVIRONMENT_SCHEMA_VERSION,
            download_directory: settings.download_directory.clone(),
            download_proxy: settings.download_proxy.clone(),
            speed_limits: settings.speed_limits,
            transfer_policy: settings.transfer_policy,
            categories: settings.categories.clone(),
            tracker_list: settings.tracker_list.clone(),
        };
        sync_env_download_directory(&mut env);
        env
    }

    #[must_use]
    pub fn defaults_for_root(download_directory: impl Into<PathBuf>) -> Self {
        let download_directory = download_directory.into();
        Self {
            schema_version: PROFILE_ENVIRONMENT_SCHEMA_VERSION,
            download_directory: download_directory.clone(),
            download_proxy: DownloadProxySettings::default(),
            speed_limits: SpeedLimitSettings::default(),
            transfer_policy: TransferPolicySettings::default(),
            categories: default_download_categories(&download_directory),
            tracker_list: TrackerListSettings::default(),
        }
    }

    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.schema_version != PROFILE_ENVIRONMENT_SCHEMA_VERSION {
            return Err(SettingsError::UnsupportedSchemaVersion {
                found: self.schema_version,
                supported: PROFILE_ENVIRONMENT_SCHEMA_VERSION,
            });
        }
        if self.download_directory.as_os_str().is_empty() {
            return Err(SettingsError::EmptyDownloadDirectory);
        }
        self.download_proxy.validate()?;
        self.transfer_policy.validate()?;
        validate_categories(&self.categories)?;
        self.tracker_list.validate()?;
        Ok(())
    }

    /// Overlay engine-env fields onto a full `AppSettings` (preserves UI prefs).
    pub fn apply_to(&self, settings: &mut AppSettings) {
        settings.download_directory = self.download_directory.clone();
        settings.download_proxy = self.download_proxy.clone();
        settings.speed_limits = self.speed_limits;
        settings.transfer_policy = self.transfer_policy;
        settings.categories = self.categories.clone();
        settings.tracker_list = self.tracker_list.clone();
        crate::sync_download_directory_from_fallback(settings);
    }
}

/// Filesystem layout: `<data_dir>/profiles/<profile_id>/environment.json`.
#[derive(Clone, Debug)]
pub struct ProfileEnvironmentStore {
    root: PathBuf,
}

impl ProfileEnvironmentStore {
    #[must_use]
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            root: data_dir.into().join("profiles"),
        }
    }

    #[must_use]
    pub fn path_for(&self, profile_id: Uuid) -> PathBuf {
        self.root
            .join(profile_id.to_string())
            .join("environment.json")
    }

    /// Load bag or seed defaults from `seed` and persist.
    pub fn load_or_initialize(
        &self,
        profile_id: Uuid,
        seed: &AppSettings,
    ) -> Result<ProfileEnvironment, SettingsError> {
        let path = self.path_for(profile_id);
        if !path.exists() {
            let env = ProfileEnvironment::from_settings(seed);
            env.validate()?;
            self.save(profile_id, &env)?;
            return Ok(env);
        }
        self.load(profile_id)
    }

    pub fn load(&self, profile_id: Uuid) -> Result<ProfileEnvironment, SettingsError> {
        let path = self.path_for(profile_id);
        let bytes = fs::read(&path)
            .map_err(|source| io_error("reading profile environment", &path, source))?;

        #[derive(Deserialize)]
        struct SchemaProbe {
            schema_version: u32,
        }

        // Probe version before full decode so unsupported schemas are not
        // mistaken for malformed documents.
        let probe: SchemaProbe =
            serde_json::from_slice(&bytes).map_err(|source| SettingsError::MalformedDocument {
                path: path.clone(),
                message: source.to_string(),
            })?;
        if probe.schema_version != PROFILE_ENVIRONMENT_SCHEMA_VERSION {
            return Err(SettingsError::UnsupportedSchemaVersion {
                found: probe.schema_version,
                supported: PROFILE_ENVIRONMENT_SCHEMA_VERSION,
            });
        }

        let mut env: ProfileEnvironment =
            serde_json::from_slice(&bytes).map_err(|source| SettingsError::MalformedDocument {
                path: path.clone(),
                message: source.to_string(),
            })?;
        env.schema_version = PROFILE_ENVIRONMENT_SCHEMA_VERSION;
        sync_env_download_directory(&mut env);
        env.validate()?;
        Ok(env)
    }

    pub fn save(&self, profile_id: Uuid, env: &ProfileEnvironment) -> Result<(), SettingsError> {
        let mut env = env.clone();
        env.schema_version = PROFILE_ENVIRONMENT_SCHEMA_VERSION;
        sync_env_download_directory(&mut env);
        env.validate()?;
        let path = self.path_for(profile_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                io_error("creating the profile environment directory", parent, source)
            })?;
        }
        let payload = serde_json::to_string_pretty(&env).map_err(SettingsError::Serialize)?;
        let temp_path = path.with_extension("json.tmp");
        let result = (|| -> Result<(), SettingsError> {
            let mut file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&temp_path)
                .map_err(|source| {
                    io_error(
                        "creating the temporary profile environment file",
                        &temp_path,
                        source,
                    )
                })?;
            file.write_all(payload.as_bytes())
                .and_then(|_| file.write_all(b"\n"))
                .map_err(|source| {
                    io_error("writing the profile environment", &temp_path, source)
                })?;
            file.flush()
                .and_then(|_| file.sync_all())
                .map_err(|source| {
                    io_error("flushing the profile environment", &temp_path, source)
                })?;
            fs::rename(&temp_path, &path)
                .map_err(|source| io_error("replacing the profile environment", &path, source))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result
    }

    /// Best-effort remove bag when a profile is deleted from the catalog.
    pub fn remove(&self, profile_id: Uuid) {
        let dir = self.root.join(profile_id.to_string());
        let _ = fs::remove_dir_all(dir);
    }
}

fn sync_env_download_directory(env: &mut ProfileEnvironment) {
    if let Some(fallback) = env.categories.iter().find(|c| c.is_fallback) {
        env.download_directory = fallback.directory.clone();
    } else if let Some(first) = env.categories.first() {
        env.download_directory = first.directory.clone();
    }
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> SettingsError {
    SettingsError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppSettings;

    #[test]
    fn round_trip_and_apply_preserves_ui() {
        let root = tempfile::tempdir().expect("temp");
        let store = ProfileEnvironmentStore::new(root.path());
        let id = Uuid::new_v4();
        let mut seed = AppSettings::new(root.path().join("downloads"));
        seed.color_scheme = crate::ColorScheme::Dark;
        seed.speed_limits.download_limit = 1_024;
        let env = store.load_or_initialize(id, &seed).expect("init");
        assert_eq!(env.speed_limits.download_limit, 1_024);
        assert_eq!(env.schema_version, PROFILE_ENVIRONMENT_SCHEMA_VERSION);

        let mut next = AppSettings::new(root.path().join("other"));
        next.color_scheme = crate::ColorScheme::Light;
        next.language = crate::LanguagePreference::ZhCn;
        env.apply_to(&mut next);
        assert_eq!(next.color_scheme, crate::ColorScheme::Light);
        assert_eq!(next.language, crate::LanguagePreference::ZhCn);
        assert_eq!(next.speed_limits.download_limit, 1_024);
        assert_eq!(next.download_directory, seed.download_directory);
    }

    #[test]
    fn save_reload_syncs_fallback_directory() {
        let root = tempfile::tempdir().expect("temp");
        let store = ProfileEnvironmentStore::new(root.path());
        let id = Uuid::new_v4();
        let mut env = ProfileEnvironment::defaults_for_root(root.path().join("dl"));
        if let Some(general) = env.categories.iter_mut().find(|c| c.is_fallback) {
            general.directory = root.path().join("remote-dl");
        }
        store.save(id, &env).expect("save");
        let loaded = store.load(id).expect("load");
        assert_eq!(loaded.download_directory, root.path().join("remote-dl"));
    }

    #[test]
    fn unsupported_environment_schema_is_rejected_without_replace() {
        let root = tempfile::tempdir().expect("temp");
        let store = ProfileEnvironmentStore::new(root.path());
        let id = Uuid::new_v4();
        let path = store.path_for(id);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(
            &path,
            r#"{"schema_version":9,"download_directory":"x","download_proxy":{"mode":"disabled","all_proxy":null,"http_proxy":null,"https_proxy":null,"ftp_proxy":null,"no_proxy":[],"username":null,"credential":null,"check_certificate":true},"speed_limits":{"download_limit":0,"upload_limit":0},"transfer_policy":{"max_concurrent_downloads":5,"max_connection_per_server":1,"split":5,"min_split_size":20971520,"file_allocation":"prealloc","check_integrity":false},"categories":[{"id":"00000000-0000-4000-8000-000000000001","name":"General","directory":"x","extensions":[],"is_fallback":true}],"tracker_list":{"enabled":false,"source":"curated","custom_url":null,"auto_refresh":false,"last_refreshed_at":null,"list_text":""}}"#,
        )
        .expect("seed");
        let err = store.load(id).expect_err("unsupported");
        assert!(matches!(
            err,
            SettingsError::UnsupportedSchemaVersion {
                found: 9,
                supported: PROFILE_ENVIRONMENT_SCHEMA_VERSION
            }
        ));
        let on_disk = fs::read_to_string(&path).expect("read");
        assert!(on_disk.contains(r#""schema_version":9"#));
    }
}
