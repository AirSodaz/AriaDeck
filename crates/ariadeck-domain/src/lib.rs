//! Business types shared by AriaDeck application services and adapters.
//!
//! This crate intentionally has no dependency on GPUI, Tokio, SQLite, or aria2
//! JSON-RPC wire models.

mod category_route;
mod checksum;
mod clipboard;
mod engine;
mod header_preset;
mod ids;
mod mirror_rule;
mod privacy;
mod rpc_endpoint;
mod task;
mod tracker_list;
mod transfer;

pub use category_route::{
    extension_from_filename, filename_hint_from_source, normalize_extension, resolve_category_id,
};
pub use checksum::{
    ChecksumAlgorithm, ChecksumComparison, ParsedExpectedChecksum, compare_checksum,
    parse_expected_checksum,
};
pub use clipboard::{
    MAX_CLIPBOARD_LINKS, MAX_CLIPBOARD_TEXT_BYTES, extract_clipboard_download_uris,
    is_clipboard_download_uri,
};
pub use engine::{
    ConnectionFailure, ConnectionState, EngineSession, EngineSource, ProcessOwnership,
};
pub use header_preset::{
    DomainHeaderPreset, HeaderPresetValidationError, MAX_HEADER_PRESETS, MAX_PRESET_DOMAINS,
    MAX_PRESET_HEADERS, MatchedHeaderPreset, match_header_presets, normalize_domain_pattern,
    validate_header_preset, validate_header_presets, validate_preset_header_line,
};
pub use ids::{
    CoreInstallationId, CredentialId, EngineSessionId, Gid, GidParseError, ProfileId,
    SessionGeneration, TaskIdentity,
};
pub use mirror_rule::{
    MAX_MIRROR_RULES, MirrorMatchKind, MirrorRewriteResult, MirrorRewriteRule,
    MirrorRuleValidationError, builtin_mirror_templates, rewrite_uri_with_rules,
    validate_mirror_rule, validate_mirror_rules,
};
pub use privacy::{
    DiagnosticSnapshot, REDACTED_SOURCE_PLACEHOLDER, magnet_info_hash, redact_endpoint_url,
    redact_source_uri, redact_tracker_uri, task_option_key_is_sensitive,
};
pub use rpc_endpoint::{
    RpcEndpointValidationError, is_loopback_endpoint, validate_rpc_endpoint,
    validate_rpc_endpoint_shape,
};
pub use task::{
    DownloadFilter, DownloadSort, DownloadStatus, DownloadTask, EnginePath, SortDirection, SortKey,
    TaskConnectionDetails, TaskDetails, TaskEditableState, TaskError, TaskFields, TaskFile,
    TaskMetadata, TaskNameState, TaskOptionEntry, TaskPeer, TaskServer, TaskSnapshot,
    TaskSourceKind, TaskTracker, TaskUpdateError, TaskUri, TaskUriStatus,
};
pub use tracker_list::{
    MAX_TRACKER_LIST_BODY_BYTES, MAX_TRACKER_LIST_ENTRIES, MAX_TRACKER_URI_LEN,
    format_bt_tracker_option, format_tracker_list_text, parse_tracker_list,
};
pub use transfer::{
    ByteCount, ByteRate, FileAllocationMethod, GlobalStat, SpeedLimitConfig, TaskConnectionPolicy,
    TaskProgress, TransferPolicyConfig, TransferPolicyError,
};
