use ariadeck_domain::{
    ByteCount, DownloadStatus, EnginePath, Gid, ProfileId, SpeedLimitConfig, TaskConnectionDetails,
    TaskDetails, TaskError, TaskSourceKind, TransferPolicyConfig,
};
use async_trait::async_trait;
use thiserror::Error;

use crate::{AddDownloadRequest, DownloadProxyConfig, QueueMove};

/// UI-independent port implemented by the aria2 RPC adapter.
#[async_trait]
pub trait DownloadEngineGateway: Send + Sync {
    async fn add_download(&self, request: &AddDownloadRequest) -> Result<Vec<Gid>, GatewayError>;
    async fn retry_download(
        &self,
        gid: Gid,
        fallback: &AddDownloadRequest,
    ) -> Result<Gid, GatewayError>;
    async fn pause(&self, gid: Gid) -> Result<(), GatewayError>;
    /// Force-pause skips graceful peer/server teardown. Prefer ordinary pause.
    async fn force_pause(&self, _gid: Gid) -> Result<(), GatewayError> {
        Err(GatewayError::new(
            GatewayErrorKind::Unsupported,
            "The connected engine does not expose force-pause.",
            false,
        ))
    }
    async fn resume(&self, gid: Gid) -> Result<(), GatewayError>;
    async fn pause_all(&self) -> Result<(), GatewayError> {
        Err(GatewayError::new(
            GatewayErrorKind::Unsupported,
            "The connected engine does not expose pause-all.",
            false,
        ))
    }
    async fn force_pause_all(&self) -> Result<(), GatewayError> {
        Err(GatewayError::new(
            GatewayErrorKind::Unsupported,
            "The connected engine does not expose force-pause-all.",
            false,
        ))
    }
    async fn resume_all(&self) -> Result<(), GatewayError> {
        Err(GatewayError::new(
            GatewayErrorKind::Unsupported,
            "The connected engine does not expose resume-all.",
            false,
        ))
    }
    async fn move_in_queue(&self, _gid: Gid, _movement: QueueMove) -> Result<(), GatewayError> {
        Err(GatewayError::new(
            GatewayErrorKind::Unsupported,
            "The connected engine does not expose queue positioning.",
            false,
        ))
    }
    async fn change_options(
        &self,
        gid: Gid,
        options: &[(String, String)],
    ) -> Result<(), GatewayError>;
    async fn apply_download_proxy(
        &self,
        _config: &DownloadProxyConfig,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::new(
            GatewayErrorKind::Unsupported,
            "The connected engine does not expose global proxy settings.",
            false,
        ))
    }
    async fn apply_speed_limit(&self, _config: &SpeedLimitConfig) -> Result<(), GatewayError> {
        Err(GatewayError::new(
            GatewayErrorKind::Unsupported,
            "The connected engine does not expose global speed limits.",
            false,
        ))
    }
    async fn apply_transfer_policy(
        &self,
        _config: &TransferPolicyConfig,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::new(
            GatewayErrorKind::Unsupported,
            "The connected engine does not expose transfer-policy settings.",
            false,
        ))
    }
    async fn remove(&self, gid: Gid, target: TaskRemovalTarget) -> Result<(), GatewayError>;
    /// Force-remove a live task without graceful peer/server teardown.
    /// Stopped-result removal still uses the ordinary result path.
    async fn force_remove(
        &self,
        _gid: Gid,
        _target: TaskRemovalTarget,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::new(
            GatewayErrorKind::Unsupported,
            "The connected engine does not expose force-remove.",
            false,
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskRemovalTarget {
    LiveTask,
    DownloadResult,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadDestinationRequest {
    pub directory: EnginePath,
    pub required_bytes: Option<u64>,
    pub files: Vec<DownloadDestinationFile>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadDestinationFile {
    pub relative_path: EnginePath,
    pub reject_existing: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DownloadDestinationReport {
    pub available_bytes: u64,
}

/// Local filesystem capability supplied only for a locally managed engine.
pub trait DownloadDestinationGateway: Send + Sync {
    fn preflight(
        &self,
        request: &DownloadDestinationRequest,
    ) -> Result<DownloadDestinationReport, GatewayError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskFileRemovalRequest {
    pub directory: EnginePath,
    pub files: Vec<EnginePath>,
    pub include_control_files: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TaskFileRemovalPreview {
    pub content_files: usize,
    pub control_files: usize,
    pub missing_paths: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TaskFileRemovalReport {
    pub moved_to_trash: usize,
    pub missing_paths: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskOpenTarget {
    Download,
    Folder,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskOpenRequest {
    pub directory: EnginePath,
    pub files: Vec<EnginePath>,
    pub target: TaskOpenTarget,
}

/// Local filesystem capability supplied only for a locally managed engine.
#[async_trait]
pub trait TaskFileGateway: Send + Sync {
    fn preflight(
        &self,
        request: &TaskFileRemovalRequest,
    ) -> Result<TaskFileRemovalPreview, GatewayError>;

    async fn move_to_trash(
        &self,
        request: &TaskFileRemovalRequest,
    ) -> Result<TaskFileRemovalReport, GatewayError>;

    async fn open(&self, _request: &TaskOpenRequest) -> Result<(), GatewayError> {
        Err(GatewayError::new(
            GatewayErrorKind::Unsupported,
            "Opening task paths is unavailable for this engine profile.",
            false,
        ))
    }
}

/// On-demand, potentially expensive task data kept outside list refreshes.
#[async_trait]
pub trait TaskDetailsGateway: Send + Sync {
    async fn task_details(&self, gid: Gid) -> Result<TaskDetails, GatewayError>;
}

/// On-demand URI/mirror, server, peer, and read-only option projections.
///
/// `active` and `is_bittorrent` let the adapter skip projections that only
/// exist for active tasks or only for the matching source kind (D-017).
#[async_trait]
pub trait TaskConnectionDetailsGateway: Send + Sync {
    async fn connection_details(
        &self,
        _gid: Gid,
        _active: bool,
        _is_bittorrent: bool,
    ) -> Result<TaskConnectionDetails, GatewayError> {
        Err(GatewayError::new(
            GatewayErrorKind::Unsupported,
            "The connected engine does not expose connection detail projections.",
            false,
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GatewayErrorKind {
    Disconnected,
    OutcomeUnknown,
    Authentication,
    Timeout,
    Rejected,
    Unsupported,
    UnsafePath,
    Filesystem,
    Internal,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{message}")]
pub struct GatewayError {
    pub kind: GatewayErrorKind,
    pub message: String,
    pub retryable: bool,
}

impl GatewayError {
    #[must_use]
    pub fn new(kind: GatewayErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable,
        }
    }
}

/// Durable completed/failed task summary (B6). Stored outside aria2 memory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryRecord {
    pub profile_id: ProfileId,
    pub gid: Gid,
    pub status: DownloadStatus,
    pub display_name: String,
    pub directory: Option<EnginePath>,
    pub info_hash: Option<String>,
    pub source_kind: TaskSourceKind,
    pub total_length: ByteCount,
    pub completed_length: ByteCount,
    pub error: Option<TaskError>,
    /// Source URI already redacted for storage (D-032).
    pub primary_uri_redacted: Option<String>,
    pub recorded_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{message}")]
pub struct HistoryStoreError {
    pub message: String,
}

impl HistoryStoreError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Local history persistence port. Implementations live outside this crate (ADR-001).
pub trait TaskHistoryStore: Send + Sync {
    fn upsert(&self, record: &HistoryRecord) -> Result<(), HistoryStoreError>;
    fn remove(&self, profile_id: ProfileId, gid: Gid) -> Result<(), HistoryStoreError>;
    fn list(
        &self,
        profile_id: ProfileId,
        limit: usize,
    ) -> Result<Vec<HistoryRecord>, HistoryStoreError>;
    fn count(&self, profile_id: ProfileId) -> Result<usize, HistoryStoreError>;

    /// Associate a task with a download category id (C1). Empty id clears affiliation.
    fn set_task_category(
        &self,
        profile_id: ProfileId,
        gid: Gid,
        category_id: Option<&str>,
    ) -> Result<(), HistoryStoreError> {
        let _ = (profile_id, gid, category_id);
        Ok(())
    }

    fn task_category(
        &self,
        profile_id: ProfileId,
        gid: Gid,
    ) -> Result<Option<String>, HistoryStoreError> {
        let _ = (profile_id, gid);
        Ok(None)
    }

    /// All category affiliations for a profile: (gid hex, category_id).
    fn list_task_categories(
        &self,
        profile_id: ProfileId,
    ) -> Result<Vec<(Gid, String)>, HistoryStoreError> {
        let _ = profile_id;
        Ok(Vec::new())
    }

    fn remove_task_category(
        &self,
        profile_id: ProfileId,
        gid: Gid,
    ) -> Result<(), HistoryStoreError> {
        self.set_task_category(profile_id, gid, None)
    }
}

/// No-op history store used when SQLite is unavailable or in unit tests.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullHistoryStore;

impl TaskHistoryStore for NullHistoryStore {
    fn upsert(&self, _record: &HistoryRecord) -> Result<(), HistoryStoreError> {
        Ok(())
    }

    fn remove(&self, _profile_id: ProfileId, _gid: Gid) -> Result<(), HistoryStoreError> {
        Ok(())
    }

    fn list(
        &self,
        _profile_id: ProfileId,
        _limit: usize,
    ) -> Result<Vec<HistoryRecord>, HistoryStoreError> {
        Ok(Vec::new())
    }

    fn count(&self, _profile_id: ProfileId) -> Result<usize, HistoryStoreError> {
        Ok(0)
    }
}
