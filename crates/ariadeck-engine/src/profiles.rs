//! Versioned JSON profile catalog persistence and remote endpoint validation.

use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};

use ariadeck_domain::ProfileId;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{
    EngineError, ExternalEngineProfile, LocalEngineConfig, atomic_io::write_atomic_bytes,
    error::io_error,
};

/// Recovery metadata when a corrupt profile document is replaced with defaults.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileStoreRecovery {
    pub backup_path: PathBuf,
    pub reason: String,
}

/// Result of loading or recovering the single-profile store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedProfile {
    pub profile: ExternalEngineProfile,
    pub recovery: Option<ProfileStoreRecovery>,
}

/// How a profile reaches aria2 (PROFILE-001).
///
/// Local managed profiles spawn aria2 under AriaDeck ownership. Remote RPC
/// profiles connect only and never share a managed session file.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileKind {
    LocalManaged,
    RemoteRpc,
}

impl ProfileKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::LocalManaged => "Local managed",
            Self::RemoteRpc => "Remote RPC",
        }
    }

    #[must_use]
    pub const fn is_local(self) -> bool {
        matches!(self, Self::LocalManaged)
    }
}

/// Opaque keyring reference for a remote RPC secret (never persisted as plaintext).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RpcSecretRef(Uuid);

impl RpcSecretRef {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    #[must_use]
    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for RpcSecretRef {
    fn default() -> Self {
        Self::new()
    }
}

/// One catalog entry: local managed engine or remote RPC endpoint.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileEntry {
    pub profile_id: ProfileId,
    pub name: String,
    pub kind: ProfileKind,
    /// Local managed: path to aria2c. Remote: unused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<PathBuf>,
    /// Shared application data root (local managed profile_dir = data_dir/id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_dir: Option<PathBuf>,
    /// Default download directory for this profile.
    pub download_dir: PathBuf,
    /// Remote RPC WebSocket endpoint (`ws`/`wss`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Whether a secret is stored in the system credential store.
    #[serde(default)]
    pub has_secret: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_ref: Option<RpcSecretRef>,
}

impl ProfileEntry {
    #[must_use]
    pub fn local_managed(
        profile_id: ProfileId,
        name: impl Into<String>,
        executable: impl Into<PathBuf>,
        data_dir: impl Into<PathBuf>,
        download_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            profile_id,
            name: name.into(),
            kind: ProfileKind::LocalManaged,
            executable: Some(executable.into()),
            data_dir: Some(data_dir.into()),
            download_dir: download_dir.into(),
            endpoint: None,
            has_secret: false,
            secret_ref: None,
        }
    }

    pub fn remote_rpc(
        profile_id: ProfileId,
        name: impl Into<String>,
        endpoint: impl Into<String>,
        download_dir: impl Into<PathBuf>,
        secret_ref: Option<RpcSecretRef>,
    ) -> Result<Self, EngineError> {
        let endpoint = endpoint.into();
        validate_remote_endpoint(&endpoint)?;
        Ok(Self {
            profile_id,
            name: name.into(),
            kind: ProfileKind::RemoteRpc,
            executable: None,
            data_dir: None,
            download_dir: download_dir.into(),
            endpoint: Some(endpoint),
            has_secret: secret_ref.is_some(),
            secret_ref,
        })
    }

    pub fn validate(&self) -> Result<(), EngineError> {
        if self.name.trim().is_empty() {
            return Err(EngineError::EmptyProfileName);
        }
        if self.download_dir.as_os_str().is_empty() {
            return Err(EngineError::EmptyDownloadDirectory);
        }
        match self.kind {
            ProfileKind::LocalManaged => {
                // Empty executable means "resolve from the active managed core
                // (or PATH discovery) at spawn time" — not a validation error.
                if self
                    .data_dir
                    .as_ref()
                    .is_none_or(|path| path.as_os_str().is_empty())
                {
                    return Err(EngineError::EmptyDataDirectory);
                }
            }
            ProfileKind::RemoteRpc => {
                let endpoint = self
                    .endpoint
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or(EngineError::MissingRemoteEndpoint)?;
                validate_remote_endpoint(endpoint)?;
            }
        }
        Ok(())
    }

    /// Local profiles with no explicit executable path use the active managed
    /// core (or discovery) at process start.
    #[must_use]
    pub fn uses_managed_core(&self) -> bool {
        self.kind == ProfileKind::LocalManaged
            && self
                .executable
                .as_ref()
                .is_none_or(|path| path.as_os_str().is_empty())
    }

    #[must_use]
    pub fn profile_dir(&self) -> Option<PathBuf> {
        match self.kind {
            ProfileKind::LocalManaged => self
                .data_dir
                .as_ref()
                .map(|data_dir| data_dir.join(self.profile_id.to_string())),
            ProfileKind::RemoteRpc => None,
        }
    }

    #[must_use]
    pub fn as_local_config(&self) -> Option<LocalEngineConfig> {
        if self.kind != ProfileKind::LocalManaged {
            return None;
        }
        Some(LocalEngineConfig {
            profile_id: self.profile_id,
            name: self.name.clone(),
            // Empty path is intentional for managed-core resolution.
            executable: self.executable.clone().unwrap_or_default(),
            data_dir: self.data_dir.clone()?,
            download_dir: self.download_dir.clone(),
        })
    }

    #[must_use]
    pub fn as_external_engine_profile(&self) -> Option<ExternalEngineProfile> {
        let config = self.as_local_config()?;
        Some(ExternalEngineProfile {
            profile_id: config.profile_id,
            name: config.name,
            executable: config.executable,
            data_dir: config.data_dir,
            download_dir: config.download_dir,
        })
    }
}

pub(crate) fn validate_remote_endpoint(endpoint: &str) -> Result<(), EngineError> {
    if endpoint.trim() != endpoint || endpoint.is_empty() {
        return Err(EngineError::InvalidRemoteEndpoint {
            reason: "value must be non-empty and must not have surrounding whitespace".into(),
        });
    }
    let parsed = Url::parse(endpoint).map_err(|error| EngineError::InvalidRemoteEndpoint {
        reason: error.to_string(),
    })?;
    ariadeck_domain::validate_rpc_endpoint_shape(&parsed).map_err(|error| {
        EngineError::InvalidRemoteEndpoint {
            reason: error.to_string(),
        }
    })
}

/// Versioned multi-profile catalog (PROFILE-001).
///
/// Only the current schema is accepted in-tree. When a release tag needs a
/// migration, add a single previous-tag path; dev configs are maintained manually.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileCatalog {
    pub schema_version: u32,
    pub active_profile_id: ProfileId,
    pub profiles: Vec<ProfileEntry>,
}

pub const PROFILE_CATALOG_SCHEMA_VERSION: u32 = 1;

impl ProfileCatalog {
    #[must_use]
    pub fn from_single(profile: ExternalEngineProfile) -> Self {
        let entry = ProfileEntry::local_managed(
            profile.profile_id,
            profile.name,
            profile.executable,
            profile.data_dir,
            profile.download_dir,
        );
        Self {
            schema_version: PROFILE_CATALOG_SCHEMA_VERSION,
            active_profile_id: entry.profile_id,
            profiles: vec![entry],
        }
    }

    pub fn validate(&self) -> Result<(), EngineError> {
        if self.schema_version != PROFILE_CATALOG_SCHEMA_VERSION {
            return Err(EngineError::UnsupportedSchemaVersion {
                found: self.schema_version,
                supported: PROFILE_CATALOG_SCHEMA_VERSION,
            });
        }
        if self.profiles.is_empty() {
            return Err(EngineError::EmptyProfileCatalog);
        }
        let mut seen = std::collections::HashSet::with_capacity(self.profiles.len());
        for profile in &self.profiles {
            profile.validate()?;
            if !seen.insert(profile.profile_id) {
                return Err(EngineError::DuplicateProfileId {
                    profile_id: profile.profile_id,
                });
            }
        }
        if !self
            .profiles
            .iter()
            .any(|profile| profile.profile_id == self.active_profile_id)
        {
            return Err(EngineError::ActiveProfileMissing {
                profile_id: self.active_profile_id,
            });
        }
        Ok(())
    }

    #[must_use]
    pub fn active(&self) -> Option<&ProfileEntry> {
        self.profiles
            .iter()
            .find(|profile| profile.profile_id == self.active_profile_id)
    }

    pub fn active_mut(&mut self) -> Option<&mut ProfileEntry> {
        let id = self.active_profile_id;
        self.profiles
            .iter_mut()
            .find(|profile| profile.profile_id == id)
    }

    pub fn set_active(&mut self, profile_id: ProfileId) -> Result<(), EngineError> {
        if !self
            .profiles
            .iter()
            .any(|profile| profile.profile_id == profile_id)
        {
            return Err(EngineError::ProfileNotFound { profile_id });
        }
        self.active_profile_id = profile_id;
        Ok(())
    }

    pub fn upsert(&mut self, entry: ProfileEntry) -> Result<(), EngineError> {
        entry.validate()?;
        let mut next = self.clone();
        if let Some(existing) = next
            .profiles
            .iter_mut()
            .find(|profile| profile.profile_id == entry.profile_id)
        {
            *existing = entry;
        } else {
            next.profiles.push(entry);
        }
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn remove(&mut self, profile_id: ProfileId) -> Result<ProfileEntry, EngineError> {
        let index = self
            .profiles
            .iter()
            .position(|profile| profile.profile_id == profile_id)
            .ok_or(EngineError::ProfileNotFound { profile_id })?;
        if self.profiles.len() == 1 {
            return Err(EngineError::CannotRemoveLastProfile);
        }
        let removed = self.profiles.remove(index);
        if self.active_profile_id == profile_id {
            self.active_profile_id = self.profiles[0].profile_id;
        }
        Ok(removed)
    }

    #[must_use]
    pub fn get(&self, profile_id: ProfileId) -> Option<&ProfileEntry> {
        self.profiles
            .iter()
            .find(|profile| profile.profile_id == profile_id)
    }

    pub fn get_mut(&mut self, profile_id: ProfileId) -> Option<&mut ProfileEntry> {
        self.profiles
            .iter_mut()
            .find(|profile| profile.profile_id == profile_id)
    }
}

/// Result of loading a multi-profile catalog (with optional migration/recovery).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedCatalog {
    pub catalog: ProfileCatalog,
    pub migrated: bool,
    pub recovery: Option<ProfileStoreRecovery>,
}

pub(crate) fn create_runtime_file(path: &Path) -> Result<(), EngineError> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map(|_| ())
        .map_err(|error| io_error("create a runtime file", path, error))
}

/// A JSON-backed store for one external profile's non-secret metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonProfileStore {
    path: PathBuf,
}

impl JsonProfileStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Atomically replace the profile file with a fully written temporary file.
    pub fn save(&self, profile: &ExternalEngineProfile) -> Result<(), EngineError> {
        let payload = serde_json::to_vec_pretty(profile)
            .map_err(|source| EngineError::Serialize { source })?;
        write_atomic_bytes(
            &self.path,
            &payload,
            true,
            "create the profile store directory",
            "write the profile metadata",
            "atomically replace the profile metadata",
        )
    }

    pub fn load(&self) -> Result<ExternalEngineProfile, EngineError> {
        let bytes = fs::read(&self.path)
            .map_err(|error| io_error("read the profile metadata", &self.path, error))?;
        serde_json::from_slice(&bytes).map_err(|error| EngineError::MalformedProfile {
            path: self.path.clone(),
            message: error.to_string(),
        })
    }

    pub fn save_profile(&self, profile: &ExternalEngineProfile) -> Result<(), EngineError> {
        self.save(profile)
    }

    pub fn load_profile(&self) -> Result<ExternalEngineProfile, EngineError> {
        self.load()
    }

    /// Load the stored profile, or preserve a corrupt document and install defaults.
    ///
    /// When the corrupt document still has a readable profile_id, that identity
    /// is preserved so existing profile directories remain addressable. Otherwise a
    /// new id from defaults is used.
    pub fn load_or_recover(
        &self,
        defaults: &ExternalEngineProfile,
    ) -> Result<LoadedProfile, EngineError> {
        if !self.path.exists() {
            self.save(defaults)?;
            return Ok(LoadedProfile {
                profile: defaults.clone(),
                recovery: None,
            });
        }

        // Read raw bytes first so identity can be recovered before rename.
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) => {
                return Err(io_error("read the profile metadata", &self.path, error));
            }
        };
        match serde_json::from_slice::<ExternalEngineProfile>(&bytes) {
            Ok(profile) => Ok(LoadedProfile {
                profile,
                recovery: None,
            }),
            Err(error) => {
                let reason = error.to_string();
                let recovered_id = serde_json::from_slice::<serde_json::Value>(&bytes)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("profile_id")
                            .and_then(|id| id.as_str())
                            .and_then(|id| id.parse::<ProfileId>().ok())
                    });
                let backup_path = self.preserve_corrupt_document_bytes(&bytes)?;
                let mut profile = defaults.clone();
                if let Some(profile_id) = recovered_id {
                    profile.profile_id = profile_id;
                }
                self.save(&profile)?;
                Ok(LoadedProfile {
                    profile,
                    recovery: Some(ProfileStoreRecovery {
                        backup_path,
                        reason,
                    }),
                })
            }
        }
    }

    /// Load the multi-profile catalog for the current schema only.
    ///
    /// Unsupported schema versions are rejected without replacing the file
    /// (same policy as settings). Malformed current-schema documents recover
    /// with a backup. Historical single-profile documents are not migrated
    /// in-tree—rewrite to schema 1 manually or reset.
    pub fn load_catalog_or_recover(
        &self,
        defaults: &ExternalEngineProfile,
    ) -> Result<LoadedCatalog, EngineError> {
        if !self.path.exists() {
            let catalog = ProfileCatalog::from_single(defaults.clone());
            self.save_catalog(&catalog)?;
            return Ok(LoadedCatalog {
                catalog,
                migrated: false,
                recovery: None,
            });
        }

        let bytes = fs::read(&self.path)
            .map_err(|error| io_error("read the profile catalog", &self.path, error))?;

        #[derive(Deserialize)]
        struct SchemaProbe {
            schema_version: u32,
        }

        // Probe version before full decode so unsupported schemas are not
        // mistaken for malformed documents (which would silently reset).
        match serde_json::from_slice::<SchemaProbe>(&bytes) {
            Ok(probe) if probe.schema_version != PROFILE_CATALOG_SCHEMA_VERSION => {
                return Err(EngineError::UnsupportedSchemaVersion {
                    found: probe.schema_version,
                    supported: PROFILE_CATALOG_SCHEMA_VERSION,
                });
            }
            Ok(_) => {}
            Err(_) => {
                // Missing/unreadable schema_version → treat as malformed recovery.
            }
        }

        let validation_error = match serde_json::from_slice::<ProfileCatalog>(&bytes) {
            Ok(catalog) => {
                if catalog.schema_version != PROFILE_CATALOG_SCHEMA_VERSION {
                    return Err(EngineError::UnsupportedSchemaVersion {
                        found: catalog.schema_version,
                        supported: PROFILE_CATALOG_SCHEMA_VERSION,
                    });
                }
                match catalog.validate() {
                    Ok(()) => {
                        return Ok(LoadedCatalog {
                            catalog,
                            migrated: false,
                            recovery: None,
                        });
                    }
                    Err(error) => format!("profile catalog validation failed: {error}"),
                }
            }
            Err(error) => format!("profile catalog JSON is malformed: {error}"),
        };

        // Only recover when the probe looked like the current schema or
        // schema_version was absent; wrong versions already returned above.
        let reason = validation_error;
        let recovered_id = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|value| {
                value
                    .get("active_profile_id")
                    .or_else(|| value.get("profile_id"))
                    .and_then(|id| id.as_str())
                    .and_then(|id| id.parse::<ProfileId>().ok())
            });
        let backup_path = self.preserve_corrupt_document_bytes(&bytes)?;
        let mut defaults = defaults.clone();
        if let Some(profile_id) = recovered_id {
            defaults.profile_id = profile_id;
        }
        let catalog = ProfileCatalog::from_single(defaults);
        self.save_catalog(&catalog)?;
        Ok(LoadedCatalog {
            catalog,
            migrated: true,
            recovery: Some(ProfileStoreRecovery {
                backup_path,
                reason,
            }),
        })
    }

    pub fn save_catalog(&self, catalog: &ProfileCatalog) -> Result<(), EngineError> {
        // Always persist as the current schema; callers may still carry an older
        // in-memory version during a transition.
        let mut catalog = catalog.clone();
        catalog.schema_version = PROFILE_CATALOG_SCHEMA_VERSION;
        catalog.validate()?;
        let payload = serde_json::to_vec_pretty(&catalog)
            .map_err(|source| EngineError::Serialize { source })?;
        write_atomic_bytes(
            &self.path,
            &payload,
            true,
            "create the profile store directory",
            "write the profile catalog",
            "atomically replace the profile catalog",
        )
    }

    fn preserve_corrupt_document_bytes(&self, bytes: &[u8]) -> Result<PathBuf, EngineError> {
        let parent = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|error| io_error("create the profile store directory", parent, error))?;
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("profiles.json");
        let backup_path = parent.join(format!("{file_name}.corrupt-{}.bak", Uuid::new_v4()));
        // Prefer rename of the live path; fall back to writing the captured bytes.
        match fs::rename(&self.path, &backup_path) {
            Ok(()) => Ok(backup_path),
            Err(_) => {
                fs::write(&backup_path, bytes).map_err(|error| {
                    io_error("preserve the corrupt profile document", &backup_path, error)
                })?;
                let _ = fs::remove_file(&self.path);
                Ok(backup_path)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EngineError, ExternalEngineProfile};
    use ariadeck_domain::ProfileId;
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    fn temporary_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!("ariadeck-engine-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create test dir");
        path
    }

    fn sample_profile(root: &Path) -> ExternalEngineProfile {
        ExternalEngineProfile::new(
            ProfileId::new(),
            "Test profile",
            std::env::current_exe().expect("current exe"),
            root.join("data"),
            root.join("downloads"),
        )
    }

    #[test]
    fn profile_store_round_trips_atomically_and_handles_malformed_json() {
        let root = temporary_directory();
        let profile = sample_profile(&root);
        let store = JsonProfileStore::new(root.join("profiles").join("profile.json"));
        assert!(store.save(&profile).is_ok());
        assert_eq!(
            store
                .load()
                .unwrap_or_else(|error| panic!("load failed: {error}")),
            profile
        );

        let replacement = ExternalEngineProfile::new(
            profile.profile_id,
            "Replacement",
            profile.executable.clone(),
            profile.data_dir.clone(),
            profile.download_dir.clone(),
        );
        assert!(store.save(&replacement).is_ok());
        assert_eq!(
            store
                .load()
                .unwrap_or_else(|error| panic!("reload failed: {error}")),
            replacement
        );
        let raw = fs::read_to_string(store.path())
            .unwrap_or_else(|error| panic!("profile file read failed: {error}"));
        assert!(!raw.contains("rpc-secret"));
        let temporary_files = fs::read_dir(store.path().parent().unwrap_or_else(|| Path::new(".")))
            .unwrap_or_else(|error| panic!("profile directory read failed: {error}"))
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(temporary_files, 0);

        fs::write(store.path(), b"{ definitely not json")
            .unwrap_or_else(|error| panic!("malformed profile write failed: {error}"));

        assert!(matches!(
            store.load(),
            Err(EngineError::MalformedProfile { .. })
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn profile_store_load_or_recover_preserves_identity_from_corrupt_document() {
        let root = temporary_directory();
        let profile_id = ProfileId::new();
        let store = JsonProfileStore::new(root.join("profiles.json"));
        let defaults = ExternalEngineProfile::new(
            ProfileId::new(),
            "Recovered",
            std::env::current_exe().expect("exe"),
            root.join("data"),
            root.join("downloads"),
        );
        fs::write(
            store.path(),
            format!(
                "{{\"profile_id\":\"{profile_id}\",\"name\":true,\"executable\":\"x\",\"data_dir\":\"y\",\"download_dir\":\"z\"}}"
            ),
        )
        .expect("seed corrupt profile");

        let loaded = store
            .load_or_recover(&defaults)
            .unwrap_or_else(|error| panic!("recover failed: {error}"));
        assert_eq!(loaded.profile.profile_id, profile_id);
        assert_eq!(loaded.profile.name, "Recovered");
        let recovery = loaded.recovery.expect("recovery metadata");
        assert!(recovery.backup_path.is_file());
        assert_eq!(
            store
                .load()
                .unwrap_or_else(|error| panic!("load recovered failed: {error}"))
                .profile_id,
            profile_id
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn profile_catalog_round_trips_current_schema() {
        let root = temporary_directory();
        let defaults = sample_profile(&root);
        let store = JsonProfileStore::new(root.join("profiles.json"));
        let mut catalog = ProfileCatalog::from_single(defaults.clone());
        assert_eq!(catalog.schema_version, PROFILE_CATALOG_SCHEMA_VERSION);

        let remote = ProfileEntry::remote_rpc(
            ProfileId::new(),
            "NAS",
            "wss://aria2.example.invalid/jsonrpc",
            root.join("nas-downloads"),
            None,
        )
        .expect("remote entry");
        catalog.upsert(remote.clone()).expect("upsert remote");
        catalog
            .set_active(remote.profile_id)
            .expect("activate remote");
        store.save_catalog(&catalog).expect("save catalog");

        let reloaded = store
            .load_catalog_or_recover(&defaults)
            .expect("reload catalog");
        assert!(!reloaded.migrated);
        assert_eq!(
            reloaded.catalog.schema_version,
            PROFILE_CATALOG_SCHEMA_VERSION
        );
        assert_eq!(reloaded.catalog.profiles.len(), 2);
        assert_eq!(reloaded.catalog.active_profile_id, remote.profile_id);
        assert_eq!(
            reloaded.catalog.active().map(|p| p.kind),
            Some(ProfileKind::RemoteRpc)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unsupported_profile_catalog_schema_is_rejected_without_replace() {
        let root = temporary_directory();
        let defaults = sample_profile(&root);
        let path = root.join("profiles.json");
        let store = JsonProfileStore::new(&path);
        fs::write(
            &path,
            r#"{"schema_version":99,"active_profile_id":"00000000-0000-4000-8000-000000000001","profiles":[]}"#,
        )
        .expect("seed unsupported schema");
        let err = store
            .load_catalog_or_recover(&defaults)
            .expect_err("unsupported schema");
        assert!(matches!(
            err,
            EngineError::UnsupportedSchemaVersion {
                found: 99,
                supported: PROFILE_CATALOG_SCHEMA_VERSION
            }
        ));
        let on_disk = fs::read_to_string(&path).expect("read");
        assert!(on_disk.contains("\"schema_version\":99"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remote_profile_endpoint_validation_rejects_http_and_empty() {
        let root = temporary_directory();
        assert!(matches!(
            ProfileEntry::remote_rpc(
                ProfileId::new(),
                "Bad",
                "http://example.invalid/jsonrpc",
                root.join("d"),
                None,
            ),
            Err(EngineError::InvalidRemoteEndpoint { .. })
        ));
        assert!(matches!(
            ProfileEntry::remote_rpc(ProfileId::new(), "Bad", "", root.join("d"), None),
            Err(EngineError::InvalidRemoteEndpoint { .. })
                | Err(EngineError::MissingRemoteEndpoint)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_profile_allows_empty_executable_for_managed_core() {
        let root = temporary_directory();
        let mut entry = ProfileEntry::local_managed(
            ProfileId::new(),
            "Managed",
            PathBuf::new(),
            root.clone(),
            root.join("downloads"),
        );
        entry.executable = None;
        entry
            .validate()
            .expect("empty executable is managed-core opt-in");
        assert!(entry.uses_managed_core());
        let config = entry.as_local_config().expect("local config");
        assert!(config.executable.as_os_str().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn catalog_refuses_to_remove_the_last_profile() {
        let root = temporary_directory();
        let profile = sample_profile(&root);
        let mut catalog = ProfileCatalog::from_single(profile.clone());
        assert!(matches!(
            catalog.remove(profile.profile_id),
            Err(EngineError::CannotRemoveLastProfile)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remote_profile_rejects_embedded_credentials_query_and_non_jsonrpc_path() {
        let root = temporary_directory();
        for bad in [
            "wss://user:pass@nas.example:6800/jsonrpc",
            "wss://nas.example:6800/jsonrpc?token=secret",
            "wss://nas.example:6800/jsonrpc#frag",
            "wss://nas.example:6800/wrong",
        ] {
            assert!(
                matches!(
                    ProfileEntry::remote_rpc(ProfileId::new(), "Bad", bad, root.join("d"), None),
                    Err(EngineError::InvalidRemoteEndpoint { .. })
                ),
                "expected {bad} to be rejected"
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn catalog_rejects_duplicate_profile_ids_and_upsert_is_atomic() {
        let root = temporary_directory();
        let profile = sample_profile(&root);
        let mut catalog = ProfileCatalog::from_single(profile.clone());
        let mut duplicate = catalog.profiles[0].clone();
        duplicate.name = "Duplicate".into();
        catalog.profiles.push(duplicate);

        let err = catalog.validate().expect_err("duplicate id must fail");
        assert!(matches!(err, EngineError::DuplicateProfileId { .. }));

        // Atomic upsert: failed validation must not mutate existing catalog.
        let mut valid_catalog = ProfileCatalog::from_single(profile);
        let mut bad_entry = valid_catalog.profiles[0].clone();
        bad_entry.name = "   ".into();
        assert!(valid_catalog.upsert(bad_entry).is_err());
        assert_eq!(valid_catalog.profiles[0].name, "Test profile");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn schema_v1_catalog_failing_validation_recovers_with_backup() {
        let root = temporary_directory();
        let defaults = sample_profile(&root);
        let path = root.join("profiles.json");
        let store = JsonProfileStore::new(&path);
        // Valid JSON for schema_version 1, but `profiles` is empty -> validation failure.
        fs::write(
            &path,
            format!(
                r#"{{"schema_version":1,"active_profile_id":"{}","profiles":[]}}"#,
                defaults.profile_id
            ),
        )
        .expect("write invalid v1 catalog");

        let loaded = store
            .load_catalog_or_recover(&defaults)
            .expect("invalid schema-v1 catalog should recover");
        let recovery = loaded.recovery.expect("recovery metadata");
        assert!(recovery.backup_path.exists());
        assert_eq!(loaded.catalog.profiles.len(), 1);
        let _ = fs::remove_dir_all(root);
    }
}
