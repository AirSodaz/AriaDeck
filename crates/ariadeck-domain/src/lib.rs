//! Business types shared by AriaDeck application services and adapters.
//!
//! This crate intentionally has no dependency on GPUI, Tokio, SQLite, or aria2
//! JSON-RPC wire models.

mod category_route;
mod engine;
mod ids;
mod privacy;
mod task;
mod tracker_list;
mod transfer;

pub use category_route::{
    extension_from_filename, filename_hint_from_source, normalize_extension, resolve_category_id,
};
pub use engine::{
    ConnectionFailure, ConnectionState, EngineSession, EngineSource, ProcessOwnership,
};
pub use ids::{
    CoreInstallationId, CredentialId, EngineSessionId, Gid, GidParseError, ProfileId,
    SessionGeneration, TaskIdentity,
};
pub use privacy::{
    DiagnosticSnapshot, REDACTED_SOURCE_PLACEHOLDER, magnet_info_hash, redact_endpoint_url,
    redact_source_uri, redact_tracker_uri, task_option_key_is_sensitive,
};
pub use task::{
    DownloadFilter, DownloadSort, DownloadStatus, DownloadTask, EnginePath, SortDirection, SortKey,
    TaskConnectionDetails, TaskDetails, TaskError, TaskFields, TaskFile, TaskMetadata,
    TaskNameState, TaskOptionEntry, TaskPeer, TaskServer, TaskSnapshot, TaskSourceKind,
    TaskTracker, TaskUpdateError, TaskUri, TaskUriStatus,
};
pub use tracker_list::{
    MAX_TRACKER_LIST_BODY_BYTES, MAX_TRACKER_LIST_ENTRIES, MAX_TRACKER_URI_LEN,
    format_bt_tracker_option, format_tracker_list_text, parse_tracker_list,
};
pub use transfer::{
    ByteCount, ByteRate, FileAllocationMethod, GlobalStat, SpeedLimitConfig, TaskConnectionPolicy,
    TaskProgress, TransferPolicyConfig, TransferPolicyError,
};
