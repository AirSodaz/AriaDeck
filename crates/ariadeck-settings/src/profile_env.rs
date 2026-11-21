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

use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    AppSettings, DownloadCategory, DownloadProxySettings, SettingsError, SpeedLimitSettings,
    TrackerListSettings, TransferPolicySettings, default_download_categories,
    environment::{sync_fallback_directory, validate_engine_environment},
    error::io_error,
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
        sync_fallback_directory(&mut env.download_directory, &env.categories);
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
        validate_engine_environment(
            &self.download_directory,
            &self.download_proxy,
            &self.transfer_policy,
            &self.categories,
            &self.tracker_list,
        )
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
        match self.load(profile_id) {
            Ok(env) => Ok(env),
            Err(error) if error.is_recoverable_document_error() => {
                let backup =
                    path.with_file_name(format!("environment.corrupt.{}.json", Uuid::new_v4()));
                match fs::rename(&path, &backup) {
                    Ok(()) => {}
                    Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
                    Err(source) => {
                        return Err(io_error(
                            "preserve the corrupt profile environment document",
                            &backup,
                            source,
                        ));
                    }
                }
                let env = ProfileEnvironment::from_settings(seed);
                env.validate()?;
                self.save(profile_id, &env)?;
                Ok(env)
            }
            Err(error) => Err(error),
        }
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
        sync_fallback_directory(&mut env.download_directory, &env.categories);
        env.validate()?;
        Ok(env)
    }

    pub fn save(&self, profile_id: Uuid, env: &ProfileEnvironment) -> Result<(), SettingsError> {
        let mut env = env.clone();
        env.schema_version = PROFILE_ENVIRONMENT_SCHEMA_VERSION;
        sync_fallback_directory(&mut env.download_directory, &env.categories);
        env.validate()?;
        let path = self.path_for(profile_id);
        let payload = serde_json::to_vec_pretty(&env).map_err(SettingsError::Serialize)?;
        crate::atomic_io::write_atomic_json(
            &path,
            &payload,
            "creating the profile environment directory",
            "creating the temporary profile environment file",
            "writing the profile environment",
            "flushing the profile environment",
            "replacing the profile environment",
        )
    }

    /// Best-effort remove bag when a profile is deleted from the catalog.
    pub fn remove(&self, profile_id: Uuid) {
        let dir = self.root.join(profile_id.to_string());
        let _ = fs::remove_dir_all(dir);
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

    #[test]
    fn repeated_atomic_saves_leave_no_temp_files_behind() {
        let root = tempfile::tempdir().expect("temp");
        let store = ProfileEnvironmentStore::new(root.path());
        let id = Uuid::new_v4();
        let env = ProfileEnvironment::defaults_for_root(root.path().join("dl"));
        for _ in 0..8 {
            store.save(id, &env).expect("atomic save");
        }
        let dir = store.path_for(id).parent().expect("parent").to_path_buf();
        let entries = fs::read_dir(&dir)
            .expect("readdir")
            .map(|entry| entry.expect("entry").file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec![std::ffi::OsString::from("environment.json")]);
    }

    #[test]
    fn load_or_initialize_recovers_corrupt_document_and_keeps_unsupported_version_fatal() {
        let root = tempfile::tempdir().expect("temp");
        let store = ProfileEnvironmentStore::new(root.path());
        let id = Uuid::new_v4();
        let seed = AppSettings::new(root.path().join("Downloads"));
        let path = store.path_for(id);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, b"{corrupt").expect("seed corrupt");

        let recovered = store
            .load_or_initialize(id, &seed)
            .expect("corrupt environment should recover");
        assert_eq!(recovered, ProfileEnvironment::from_settings(&seed));
        let dir = path.parent().expect("parent");
        let backups = fs::read_dir(dir)
            .expect("readdir")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("environment.corrupt.")
            })
            .count();
        assert_eq!(backups, 1);

        fs::write(
            &path,
            format!(
                r#"{{"schema_version":{}}}"#,
                PROFILE_ENVIRONMENT_SCHEMA_VERSION + 1
            ),
        )
        .expect("seed future");
        assert!(matches!(
            store.load_or_initialize(id, &seed),
            Err(SettingsError::UnsupportedSchemaVersion { .. })
        ));
    }
}
