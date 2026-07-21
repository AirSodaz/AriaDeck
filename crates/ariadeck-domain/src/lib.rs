//! Business types shared by AriaDeck application services and adapters.
//!
//! This crate intentionally has no dependency on GPUI, Tokio, SQLite, or aria2
//! JSON-RPC wire models.

mod engine;
mod ids;
mod task;
mod transfer;

pub use engine::{
    ConnectionFailure, ConnectionState, EngineSession, EngineSource, ProcessOwnership,
};
pub use ids::{
    CoreInstallationId, CredentialId, EngineSessionId, Gid, GidParseError, ProfileId,
    SessionGeneration, TaskIdentity,
};
pub use task::{
    DownloadFilter, DownloadSort, DownloadStatus, DownloadTask, EnginePath, SortDirection, SortKey,
    TaskConnectionDetails, TaskDetails, TaskError, TaskFields, TaskFile, TaskMetadata,
    TaskNameState, TaskOptionEntry, TaskPeer, TaskServer, TaskSnapshot, TaskSourceKind,
    TaskTracker, TaskUpdateError, TaskUri, TaskUriStatus,
};
pub use transfer::{ByteCount, ByteRate, GlobalStat, SpeedLimitConfig, TaskProgress};
