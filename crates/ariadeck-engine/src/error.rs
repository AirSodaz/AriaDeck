//! Error types for local engine supervision, core registry, and profile storage.

use std::{
    io,
    path::{Path, PathBuf},
};

use ariadeck_domain::ProfileId;
use thiserror::Error;

/// Errors returned by local engine lifecycle and profile storage operations.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("profile name cannot be empty")]
    EmptyProfileName,
    #[error("profile data directory cannot be empty")]
    EmptyDataDirectory,
    #[error("profile download directory cannot be empty")]
    EmptyDownloadDirectory,
    #[error("executable does not exist: {path}")]
    ExecutableNotFound { path: PathBuf },
    #[error("executable path is a directory: {path}")]
    ExecutableIsDirectory { path: PathBuf },
    #[error("executable path is not a regular file: {path}")]
    ExecutableIsNotFile { path: PathBuf },
    #[error("executable validation failed for {path}: {reason}")]
    ExecutableValidation { path: PathBuf, reason: String },
    #[error("failed to spawn aria2 for {path}: {source}")]
    Spawn {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to start the local engine supervisor: {0}")]
    SpawnSupervisor(io::Error),
    #[error("I/O error while {operation} at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid profile store path: {path}")]
    InvalidStorePath { path: PathBuf },
    #[error("failed to serialize profile metadata: {source}")]
    Serialize {
        #[source]
        source: serde_json::Error,
    },
    #[error("malformed profile JSON at {path}: {message}")]
    MalformedProfile { path: PathBuf, message: String },
    #[error("malformed managed core manifest at {path}: {message}")]
    MalformedCoreManifest { path: PathBuf, message: String },
    /// Another AriaDeck instance still owns this profile directory.
    #[error(
        "profile {profile_id} is already owned by process {owner_pid}; close that AriaDeck instance or stop its managed aria2 before starting again"
    )]
    ProfileAlreadyOwned {
        profile_id: ProfileId,
        owner_pid: u32,
        lock_path: PathBuf,
    },
    #[error("profile catalog has no profiles")]
    EmptyProfileCatalog,
    #[error("active profile {profile_id} is missing from the catalog")]
    ActiveProfileMissing { profile_id: ProfileId },
    #[error("profile {profile_id} was not found")]
    ProfileNotFound { profile_id: ProfileId },
    #[error("invalid remote profile endpoint: {reason}")]
    InvalidRemoteEndpoint { reason: String },
    #[error("remote profile requires a non-empty endpoint")]
    MissingRemoteEndpoint,
    #[error("managed core {id} was not found")]
    CoreNotFound {
        id: ariadeck_domain::CoreInstallationId,
    },
    #[error("aria2 version {version} ({target}) is already installed")]
    CoreAlreadyInstalled { version: String, target: String },
    #[error("invalid managed core version label: {version}")]
    InvalidCoreVersion { version: String },
    #[error("cannot remove the active managed core {id}; activate another version first")]
    CannotRemoveActiveCore {
        id: ariadeck_domain::CoreInstallationId,
    },
    #[error("no last-working managed core is recorded for rollback")]
    NoLastWorkingCore,
    #[error("already running the last-working managed core {id}")]
    AlreadyOnLastWorkingCore {
        id: ariadeck_domain::CoreInstallationId,
    },
    #[error("cannot remove the last remaining profile")]
    CannotRemoveLastProfile,
    #[error("unsupported profile catalog schema version {found}; this build supports {supported}")]
    UnsupportedSchemaVersion { found: u32, supported: u32 },
}

pub(crate) fn io_error(operation: &'static str, path: &Path, source: io::Error) -> EngineError {
    EngineError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}
