//! Versioned JSON profile catalog persistence and remote endpoint validation.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use ariadeck_domain::ProfileId;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{EngineError, ExternalEngineProfile, LocalEngineConfig, error::io_error};

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
    if !matches!(parsed.scheme(), "ws" | "wss") {
        return Err(EngineError::InvalidRemoteEndpoint {
            reason: "endpoint must use ws or wss".into(),
        });
    }
    if parsed.host_str().is_none() {
        return Err(EngineError::InvalidRemoteEndpoint {
            reason: "endpoint must include a host".into(),
        });
    }
    Ok(())
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
        for profile in &self.profiles {
            profile.validate()?;
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
        if let Some(existing) = self
            .profiles
            .iter_mut()
            .find(|profile| profile.profile_id == entry.profile_id)
        {
            *existing = entry;
        } else {
            self.profiles.push(entry);
        }
        self.validate()
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
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if self.path.file_name().is_none() {
            return Err(EngineError::InvalidStorePath {
                path: self.path.clone(),
            });
        }
        fs::create_dir_all(parent)
            .map_err(|error| io_error("create the profile store directory", parent, error))?;

        let payload = serde_json::to_vec_pretty(profile)
            .map_err(|source| EngineError::Serialize { source })?;
        let temp_path = parent.join(format!(
            ".{}.{}.tmp",
            self.path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("profile"),
            Uuid::new_v4()
        ));

        let write_result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp_path)
                .map_err(|error| {
                    io_error("create the temporary profile file", &temp_path, error)
                })?;
            file.write_all(&payload)
                .map_err(|error| io_error("write the profile metadata", &temp_path, error))?;
            file.write_all(b"\n")
                .map_err(|error| io_error("finish the profile metadata", &temp_path, error))?;
            file.flush()
                .map_err(|error| io_error("flush the profile metadata", &temp_path, error))?;
            file.sync_all()
                .map_err(|error| io_error("sync the profile metadata", &temp_path, error))?;
            replace_file(&temp_path, &self.path).map_err(|error| {
                io_error("atomically replace the profile metadata", &self.path, error)
            })
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        write_result
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

        match serde_json::from_slice::<ProfileCatalog>(&bytes) {
            Ok(catalog) => {
                if catalog.schema_version != PROFILE_CATALOG_SCHEMA_VERSION {
                    return Err(EngineError::UnsupportedSchemaVersion {
                        found: catalog.schema_version,
                        supported: PROFILE_CATALOG_SCHEMA_VERSION,
                    });
                }
                catalog.validate()?;
                Ok(LoadedCatalog {
                    catalog,
                    migrated: false,
                    recovery: None,
                })
            }
            Err(error) => {
                // Only recover when the probe looked like the current schema or
                // schema_version was absent; wrong versions already returned above.
                let reason = format!("profile catalog JSON is malformed: {error}");
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
        }
    }

    pub fn save_catalog(&self, catalog: &ProfileCatalog) -> Result<(), EngineError> {
        // Always persist as the current schema; callers may still carry an older
        // in-memory version during a transition.
        let mut catalog = catalog.clone();
        catalog.schema_version = PROFILE_CATALOG_SCHEMA_VERSION;
        catalog.validate()?;
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if self.path.file_name().is_none() {
            return Err(EngineError::InvalidStorePath {
                path: self.path.clone(),
            });
        }
        fs::create_dir_all(parent)
            .map_err(|error| io_error("create the profile store directory", parent, error))?;

        let payload = serde_json::to_vec_pretty(&catalog)
            .map_err(|source| EngineError::Serialize { source })?;
        let temp_path = parent.join(format!(
            ".{}.{}.tmp",
            self.path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("profiles"),
            Uuid::new_v4()
        ));

        let write_result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp_path)
                .map_err(|error| {
                    io_error("create the temporary profile catalog", &temp_path, error)
                })?;
            file.write_all(&payload)
                .map_err(|error| io_error("write the profile catalog", &temp_path, error))?;
            file.write_all(b"\n")
                .map_err(|error| io_error("finish the profile catalog", &temp_path, error))?;
            file.flush()
                .map_err(|error| io_error("flush the profile catalog", &temp_path, error))?;
            file.sync_all()
                .map_err(|error| io_error("sync the profile catalog", &temp_path, error))?;
            replace_file(&temp_path, &self.path).map_err(|error| {
                io_error("atomically replace the profile catalog", &self.path, error)
            })
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        write_result
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

#[cfg(windows)]
pub(crate) fn replace_file(temp_path: &Path, target_path: &Path) -> io::Result<()> {
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
pub(crate) fn replace_file(temp_path: &Path, target_path: &Path) -> io::Result<()> {
    fs::rename(temp_path, target_path)
}
