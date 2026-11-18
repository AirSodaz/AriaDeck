use ariadeck_domain::{
    EngineSession, Gid, SpeedLimitConfig, TaskConnectionDetails, TaskDetails, TaskIdentity,
    TransferPolicyConfig,
};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use crate::{
    AppCommand, ApplicationError, ApplicationErrorCode, CommandOutcome, DownloadProxyConfig,
    StoppedHistoryState, TaskListQuery,
};

use super::types::{ActivityMode, StoreSnapshot, SyncError, SyncEvent};

#[derive(Clone)]
pub struct SyncHandle {
    pub(super) commands: mpsc::Sender<Control>,
    pub(super) events: broadcast::Sender<SyncEvent>,
    pub(super) cancellation: watch::Sender<bool>,
    pub(super) completion: watch::Receiver<bool>,
}

impl SyncHandle {
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<SyncEvent> {
        self.events.subscribe()
    }

    pub async fn set_activity(&self, mode: ActivityMode) {
        let _ = self.commands.send(Control::SetActivity(mode)).await;
    }

    pub async fn force_refresh(&self) {
        let _ = self.commands.send(Control::ForceRefresh).await;
    }

    async fn request<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<Result<T, ApplicationError>>) -> Control,
    ) -> Result<T, ApplicationError> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(build(sender))
            .await
            .map_err(|_| unavailable_error("The synchronization coordinator is unavailable."))?;
        receiver.await.map_err(|_| {
            unavailable_error("The synchronization coordinator stopped unexpectedly.")
        })?
    }

    /// Append the next stopped-result page when the local cache is incomplete.
    ///
    /// Returns the post-request history state. A no-op when every currently
    /// reported stopped result is already loaded or the engine is unavailable.
    pub async fn load_more_stopped(&self) -> Option<StoppedHistoryState> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(Control::LoadMoreStopped { sender })
            .await
            .ok()?;
        receiver.await.ok()
    }

    /// Persist and apply a task category affiliation (C1).
    pub async fn set_task_category(&self, gid: Gid, category_id: Option<String>) {
        let (sender, receiver) = oneshot::channel();
        if self
            .commands
            .send(Control::SetTaskCategory {
                gid,
                category_id,
                sender,
            })
            .await
            .is_ok()
        {
            let _ = receiver.await;
        }
    }

    pub async fn snapshot(&self, query: TaskListQuery) -> Option<StoreSnapshot> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(Control::Snapshot { query, sender })
            .await
            .ok()?;
        receiver.await.ok()
    }

    pub async fn execute(&self, session: EngineSession, command: AppCommand) -> CommandOutcome {
        let (sender, receiver) = oneshot::channel();
        if self
            .commands
            .send(Control::Execute {
                session,
                command,
                sender,
            })
            .await
            .is_err()
        {
            return unavailable_command_outcome("The synchronization coordinator is unavailable.");
        }
        receiver.await.unwrap_or_else(|_| {
            unavailable_command_outcome("The synchronization coordinator stopped unexpectedly.")
        })
    }

    pub async fn apply_download_proxy(
        &self,
        session: EngineSession,
        config: DownloadProxyConfig,
    ) -> Result<(), ApplicationError> {
        self.request(|sender| Control::ApplyDownloadProxy {
            session,
            config,
            sender,
        })
        .await
    }

    pub async fn apply_speed_limit(
        &self,
        session: EngineSession,
        config: SpeedLimitConfig,
    ) -> Result<(), ApplicationError> {
        self.request(|sender| Control::ApplySpeedLimit {
            session,
            config,
            sender,
        })
        .await
    }

    pub async fn apply_transfer_policy(
        &self,
        session: EngineSession,
        config: TransferPolicyConfig,
    ) -> Result<(), ApplicationError> {
        self.request(|sender| Control::ApplyTransferPolicy {
            session,
            config,
            sender,
        })
        .await
    }

    pub async fn apply_bt_tracker(
        &self,
        session: EngineSession,
        trackers: Vec<String>,
    ) -> Result<(), ApplicationError> {
        self.request(|sender| Control::ApplyBtTracker {
            session,
            trackers,
            sender,
        })
        .await
    }

    pub async fn task_details(
        &self,
        session: EngineSession,
        task: TaskIdentity,
    ) -> Result<TaskDetails, ApplicationError> {
        self.request(|sender| Control::TaskDetails {
            session,
            task,
            sender,
        })
        .await
    }

    /// Request the on-demand URI/server/peer/option projections for a task.
    ///
    /// `active` and `is_bittorrent` reflect the caller's current view of the
    /// task so the adapter can skip active-only or source-specific projections.
    pub async fn connection_details(
        &self,
        session: EngineSession,
        task: TaskIdentity,
        active: bool,
        is_bittorrent: bool,
    ) -> Result<TaskConnectionDetails, ApplicationError> {
        self.request(|sender| Control::ConnectionDetails {
            session,
            task,
            active,
            is_bittorrent,
            sender,
        })
        .await
    }

    pub async fn stop(&self) {
        let _ = self.cancellation.send(true);
        let mut completion = self.completion.clone();
        if *completion.borrow() {
            return;
        }
        let _ = completion.changed().await;
    }
}

pub(super) enum Control {
    SetActivity(ActivityMode),
    ForceRefresh,
    LoadMoreStopped {
        sender: oneshot::Sender<StoppedHistoryState>,
    },
    SetTaskCategory {
        gid: Gid,
        category_id: Option<String>,
        sender: oneshot::Sender<()>,
    },
    Snapshot {
        query: TaskListQuery,
        sender: oneshot::Sender<StoreSnapshot>,
    },
    Execute {
        session: EngineSession,
        command: AppCommand,
        sender: oneshot::Sender<CommandOutcome>,
    },
    ApplyDownloadProxy {
        session: EngineSession,
        config: DownloadProxyConfig,
        sender: oneshot::Sender<Result<(), ApplicationError>>,
    },
    ApplySpeedLimit {
        session: EngineSession,
        config: SpeedLimitConfig,
        sender: oneshot::Sender<Result<(), ApplicationError>>,
    },
    ApplyTransferPolicy {
        session: EngineSession,
        config: TransferPolicyConfig,
        sender: oneshot::Sender<Result<(), ApplicationError>>,
    },
    ApplyBtTracker {
        session: EngineSession,
        trackers: Vec<String>,
        sender: oneshot::Sender<Result<(), ApplicationError>>,
    },
    TaskDetails {
        session: EngineSession,
        task: TaskIdentity,
        sender: oneshot::Sender<Result<TaskDetails, ApplicationError>>,
    },
    ConnectionDetails {
        session: EngineSession,
        task: TaskIdentity,
        active: bool,
        is_bittorrent: bool,
        sender: oneshot::Sender<Result<TaskConnectionDetails, ApplicationError>>,
    },
}

pub(super) fn unavailable_command_outcome(summary: &str) -> CommandOutcome {
    CommandOutcome::failure(unavailable_error(summary))
}

pub(super) fn unavailable_error(summary: &str) -> ApplicationError {
    ApplicationError::new(ApplicationErrorCode::Disconnected, summary, true)
}

pub(super) fn stale_session_error() -> ApplicationError {
    ApplicationError::new(
        ApplicationErrorCode::StaleSession,
        "The task view belongs to an obsolete engine session. Refresh and try again.",
        true,
    )
}

pub(super) fn wrong_profile_error() -> ApplicationError {
    ApplicationError::new(
        ApplicationErrorCode::WrongProfile,
        "The task belongs to a different engine profile.",
        false,
    )
}

pub(super) fn unsupported_error(summary: &str) -> ApplicationError {
    ApplicationError::new(ApplicationErrorCode::Unsupported, summary, false)
}

pub(super) fn connection_failure(error: &SyncError) -> ariadeck_domain::ConnectionFailure {
    ariadeck_domain::ConnectionFailure {
        code: format!("sync.{:?}", error.kind).to_ascii_lowercase(),
        summary: error.message.clone(),
        retryable: error.retryable,
    }
}
