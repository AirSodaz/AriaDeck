use ariadeck_domain::{EnginePath, Gid, TaskDetails};
use async_trait::async_trait;
use thiserror::Error;

use crate::{AddDownloadRequest, DownloadProxyConfig};

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
    async fn resume(&self, gid: Gid) -> Result<(), GatewayError>;
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
    async fn remove(&self, gid: Gid, target: TaskRemovalTarget) -> Result<(), GatewayError>;
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
}

/// On-demand, potentially expensive task data kept outside list refreshes.
#[async_trait]
pub trait TaskDetailsGateway: Send + Sync {
    async fn task_details(&self, gid: Gid) -> Result<TaskDetails, GatewayError>;
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
