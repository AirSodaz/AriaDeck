use std::{
    io,
    path::{Path, PathBuf},
};

use thiserror::Error;
use uuid::Uuid;

use crate::{MAX_CATEGORY_EXTENSIONS, MAX_DOWNLOAD_CATEGORIES};

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("settings store path has no file name: {path}")]
    InvalidStorePath { path: PathBuf },
    #[error("download directory must not be empty")]
    EmptyDownloadDirectory,
    #[error("download category name must not be empty")]
    EmptyCategoryName,
    #[error("download category directory must not be empty")]
    EmptyCategoryDirectory,
    #[error("download category names must be unique (case-insensitive)")]
    DuplicateCategoryName,
    #[error("download category IDs must be unique: {id}")]
    DuplicateCategoryId { id: Uuid },
    #[error("too many download categories (max {MAX_DOWNLOAD_CATEGORIES})")]
    TooManyCategories,
    #[error("download categories must include exactly one fallback category")]
    MissingFallbackCategory,
    #[error("download categories must not include more than one fallback category")]
    MultipleFallbackCategories,
    #[error("download categories must not be empty")]
    EmptyCategories,
    #[error("too many extensions on a download category (max {MAX_CATEGORY_EXTENSIONS})")]
    TooManyCategoryExtensions,
    #[error("invalid download category extension {extension:?}")]
    InvalidCategoryExtension { extension: String },
    #[error("custom tracker list source requires an HTTPS URL")]
    MissingCustomTrackerListUrl,
    #[error("tracker list URL is too long")]
    TrackerListUrlTooLong,
    #[error("tracker list text is too large")]
    TrackerListTextTooLarge,
    #[error("invalid tracker list URL: {reason}")]
    InvalidTrackerListUrl { reason: String },
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
    #[error("unsupported settings export version {found}; this build supports {supported}")]
    UnsupportedExportVersion { found: u32, supported: u32 },
    #[error("malformed settings export: {message}")]
    MalformedExport { message: String },
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

impl SettingsError {
    /// Whether this error indicates a malformed or semantically invalid persisted
    /// `settings.json` that `load_or_initialize` should back up and replace with defaults.
    ///
    /// Uses an exhaustive match so newly added validation variants must be classified explicitly.
    #[must_use]
    pub const fn is_recoverable_document_error(&self) -> bool {
        match self {
            Self::MalformedDocument { .. }
            | Self::EmptyDownloadDirectory
            | Self::EmptyCategoryName
            | Self::EmptyCategoryDirectory
            | Self::DuplicateCategoryName
            | Self::DuplicateCategoryId { .. }
            | Self::TooManyCategories
            | Self::MissingFallbackCategory
            | Self::MultipleFallbackCategories
            | Self::EmptyCategories
            | Self::TooManyCategoryExtensions
            | Self::InvalidCategoryExtension { .. }
            | Self::MissingCustomTrackerListUrl
            | Self::TrackerListUrlTooLong
            | Self::TrackerListTextTooLarge
            | Self::InvalidTrackerListUrl { .. }
            | Self::MissingManualProxyEndpoint
            | Self::ProxyCredentialWithoutUsername
            | Self::InvalidProxyEndpoint { .. }
            | Self::InvalidNoProxyEntry { .. }
            | Self::InvalidTransferPolicy { .. } => true,
            Self::InvalidStorePath { .. }
            | Self::UnsupportedSchemaVersion { .. }
            | Self::UnsupportedExportVersion { .. }
            | Self::MalformedExport { .. }
            | Self::Serialize(_)
            | Self::Io { .. } => false,
        }
    }
}

pub(crate) fn io_error(operation: &'static str, path: &Path, source: io::Error) -> SettingsError {
    SettingsError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}
