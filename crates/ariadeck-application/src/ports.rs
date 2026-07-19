use ariadeck_domain::{Gid, TaskDetails};
use async_trait::async_trait;
use thiserror::Error;

use crate::AddDownloadRequest;

/// UI-independent port implemented by the aria2 RPC adapter.
#[async_trait]
pub trait DownloadEngineGateway: Send + Sync {
    async fn add_download(&self, request: &AddDownloadRequest) -> Result<Gid, GatewayError>;
    async fn pause(&self, gid: Gid) -> Result<(), GatewayError>;
    async fn resume(&self, gid: Gid) -> Result<(), GatewayError>;
    async fn remove(&self, gid: Gid, target: TaskRemovalTarget) -> Result<(), GatewayError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskRemovalTarget {
    LiveTask,
    DownloadResult,
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
