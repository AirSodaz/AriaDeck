use std::{collections::HashMap, sync::Arc, time::Duration};

use ariadeck_domain::{
    ConnectionState, DownloadTask, EngineSession, Gid, GlobalStat, ProfileId, TaskSnapshot,
};
use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::mpsc;

use crate::{
    DownloadEngineGateway, NullHistoryStore, StoppedHistoryState, StoreError, StorePatch,
    TaskConnectionDetailsGateway, TaskCounts, TaskDetailsGateway, TaskHistoryStore, TaskListView,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EngineCapabilities {
    pub version: String,
    pub enabled_features: Vec<String>,
    /// Method names published by `system.listMethods` when available.
    /// An empty list means the probe was unavailable; callers must treat
    /// unknown methods as capability-gated rather than assuming support.
    pub methods: Vec<String>,
}

impl EngineCapabilities {
    /// True when `system.listMethods` returned a non-empty method list.
    #[must_use]
    pub fn methods_probed(&self) -> bool {
        !self.methods.is_empty()
    }

    /// Strict probe: only true when listMethods explicitly published `method`.
    ///
    /// Returns `false` when the probe is unavailable. Use this for advanced
    /// controls that should not offer a write path until capability is known.
    #[must_use]
    pub fn supports_method_strict(&self, method: &str) -> bool {
        self.methods_probed() && self.methods.iter().any(|name| name == method)
    }

    /// True when support is unknown (probe missing) or explicitly published.
    #[must_use]
    pub fn allows_method(&self, method: &str) -> bool {
        !self.methods_probed() || self.supports_method_strict(method)
    }

    #[must_use]
    pub fn supports_force_pause(&self) -> bool {
        self.allows_method("aria2.forcePause")
    }

    #[must_use]
    pub fn supports_force_pause_all(&self) -> bool {
        self.allows_method("aria2.forcePauseAll")
    }

    #[must_use]
    pub fn supports_force_remove(&self) -> bool {
        self.allows_method("aria2.forceRemove")
    }

    #[must_use]
    pub fn supports_queue_positioning(&self) -> bool {
        self.allows_method("aria2.changePosition")
    }

    #[must_use]
    pub fn supports_change_option(&self) -> bool {
        self.allows_method("aria2.changeOption")
    }

    #[must_use]
    pub fn supports_change_uri(&self) -> bool {
        self.allows_method("aria2.changeUri")
    }

    #[must_use]
    pub fn supports_change_global_option(&self) -> bool {
        self.allows_method("aria2.changeGlobalOption")
    }

    #[must_use]
    pub fn supports_get_peers(&self) -> bool {
        self.allows_method("aria2.getPeers")
    }

    #[must_use]
    pub fn supports_get_servers(&self) -> bool {
        self.allows_method("aria2.getServers")
    }

    #[must_use]
    pub fn supports_multicall(&self) -> bool {
        self.allows_method("system.multicall")
    }

    /// User-facing explanation when a method is missing from the probe.
    #[must_use]
    pub fn unsupported_method_message(method: &str) -> String {
        match method {
            "aria2.forcePause" => {
                "This aria2 build does not expose force-pause (aria2.forcePause).".into()
            }
            "aria2.forcePauseAll" => {
                "This aria2 build does not expose force-pause-all (aria2.forcePauseAll).".into()
            }
            "aria2.forceRemove" => {
                "This aria2 build does not expose force-remove (aria2.forceRemove).".into()
            }
            "aria2.changePosition" => {
                "This aria2 build does not expose queue positioning (aria2.changePosition).".into()
            }
            "aria2.changeOption" => {
                "This aria2 build does not expose per-task option changes (aria2.changeOption)."
                    .into()
            }
            "aria2.changeUri" => {
                "This aria2 build does not expose per-task URI replacement (aria2.changeUri)."
                    .into()
            }
            "aria2.changeGlobalOption" => {
                "This aria2 build does not expose global option changes (aria2.changeGlobalOption)."
                    .into()
            }
            "aria2.getPeers" => {
                "This aria2 build does not expose peer details (aria2.getPeers).".into()
            }
            "aria2.getServers" => {
                "This aria2 build does not expose server details (aria2.getServers).".into()
            }
            "system.multicall" => {
                "This aria2 build does not expose multicall (system.multicall).".into()
            }
            other => format!("This aria2 build does not expose {other}."),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveSyncSnapshot {
    pub active: Vec<TaskSnapshot>,
    pub waiting: Vec<TaskSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoppedPage {
    pub offset: usize,
    pub total: usize,
    pub tasks: Vec<TaskSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitialSyncSnapshot {
    pub capabilities: EngineCapabilities,
    pub global_stat: GlobalStat,
    pub live: LiveSyncSnapshot,
    pub stopped: StoppedPage,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RefreshHint {
    Task(Gid),
    Full,
}

#[async_trait]
pub trait DownloadSyncSession: Send + Sync {
    async fn initial_snapshot(&self, stopped_count: u32) -> Result<InitialSyncSnapshot, SyncError>;
    async fn refresh_global_stat(&self) -> Result<GlobalStat, SyncError>;
    async fn refresh_live(&self) -> Result<LiveSyncSnapshot, SyncError>;
    async fn refresh_stopped_page(
        &self,
        offset: usize,
        count: u32,
    ) -> Result<StoppedPage, SyncError>;
    async fn refresh_tasks(
        &self,
        gids: &[Gid],
    ) -> Result<Vec<(Gid, Option<TaskSnapshot>)>, SyncError>;
    async fn close(&self);
}

pub struct ConnectedSyncSession {
    pub(super) session: Box<dyn DownloadSyncSession>,
    pub(super) command_gateway: Option<Arc<dyn DownloadEngineGateway>>,
    pub(super) details_gateway: Option<Arc<dyn TaskDetailsGateway>>,
    pub(super) connection_gateway: Option<Arc<dyn TaskConnectionDetailsGateway>>,
    pub(super) notifications: mpsc::Receiver<RefreshHint>,
}

pub(super) struct ConnectedSessionParts {
    pub(super) session: Box<dyn DownloadSyncSession>,
    pub(super) command_gateway: Option<Arc<dyn DownloadEngineGateway>>,
    pub(super) details_gateway: Option<Arc<dyn TaskDetailsGateway>>,
    pub(super) connection_gateway: Option<Arc<dyn TaskConnectionDetailsGateway>>,
    pub(super) notifications: mpsc::Receiver<RefreshHint>,
}

impl ConnectedSyncSession {
    #[must_use]
    pub fn new(
        session: Box<dyn DownloadSyncSession>,
        notifications: mpsc::Receiver<RefreshHint>,
    ) -> Self {
        Self {
            session,
            command_gateway: None,
            details_gateway: None,
            connection_gateway: None,
            notifications,
        }
    }

    #[must_use]
    pub fn new_with_gateways(
        session: Box<dyn DownloadSyncSession>,
        command_gateway: Arc<dyn DownloadEngineGateway>,
        details_gateway: Arc<dyn TaskDetailsGateway>,
        connection_gateway: Arc<dyn TaskConnectionDetailsGateway>,
        notifications: mpsc::Receiver<RefreshHint>,
    ) -> Self {
        Self {
            session,
            command_gateway: Some(command_gateway),
            details_gateway: Some(details_gateway),
            connection_gateway: Some(connection_gateway),
            notifications,
        }
    }

    pub(super) fn into_parts(self) -> ConnectedSessionParts {
        ConnectedSessionParts {
            session: self.session,
            command_gateway: self.command_gateway,
            details_gateway: self.details_gateway,
            connection_gateway: self.connection_gateway,
            notifications: self.notifications,
        }
    }
}

#[async_trait]
pub trait DownloadSyncConnector: Send + Sync {
    async fn connect(&self) -> Result<ConnectedSyncSession, SyncError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncErrorKind {
    Configuration,
    Disconnected,
    Tls,
    Authentication,
    Timeout,
    Protocol,
    Internal,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{message}")]
pub struct SyncError {
    pub kind: SyncErrorKind,
    pub message: String,
    pub retryable: bool,
}

impl SyncError {
    #[must_use]
    pub fn new(kind: SyncErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable,
        }
    }

    pub(super) fn store(error: StoreError) -> Self {
        Self::new(SyncErrorKind::Internal, error.to_string(), false)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ActivityMode {
    #[default]
    Foreground,
    Background,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PollIntervals {
    pub global_stat: Duration,
    pub live_tasks: Duration,
    pub stopped_tasks: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RefreshPolicy {
    pub foreground: PollIntervals,
    pub background: PollIntervals,
    pub notification_debounce: Duration,
}

impl Default for RefreshPolicy {
    fn default() -> Self {
        Self {
            foreground: PollIntervals {
                global_stat: Duration::from_millis(500),
                live_tasks: Duration::from_millis(500),
                stopped_tasks: Duration::from_secs(5),
            },
            background: PollIntervals {
                global_stat: Duration::from_secs(3),
                live_tasks: Duration::from_secs(3),
                stopped_tasks: Duration::from_secs(10),
            },
            notification_debounce: Duration::from_millis(100),
        }
    }
}

impl RefreshPolicy {
    pub(super) const fn intervals(self, mode: ActivityMode) -> PollIntervals {
        match mode {
            ActivityMode::Foreground => self.foreground,
            ActivityMode::Background => self.background,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReconnectPolicy {
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub jitter_percent: u8,
    pub max_attempts: Option<u32>,
    pub reset_after: Duration,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            base_delay: Duration::from_millis(250),
            max_delay: Duration::from_secs(30),
            jitter_percent: 20,
            max_attempts: None,
            reset_after: Duration::from_secs(10),
        }
    }
}

#[derive(Clone)]
pub struct CoordinatorConfig {
    pub profile_id: ProfileId,
    pub refresh: RefreshPolicy,
    pub reconnect: ReconnectPolicy,
    pub stopped_page_size: u32,
    /// Optional durable history (B6). Failures never break sync.
    pub history: Arc<dyn TaskHistoryStore>,
}

impl CoordinatorConfig {
    #[must_use]
    pub fn new(profile_id: ProfileId) -> Self {
        Self {
            profile_id,
            refresh: RefreshPolicy::default(),
            reconnect: ReconnectPolicy::default(),
            stopped_page_size: 100,
            history: Arc::new(NullHistoryStore),
        }
    }
}

impl std::fmt::Debug for CoordinatorConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoordinatorConfig")
            .field("profile_id", &self.profile_id)
            .field("refresh", &self.refresh)
            .field("reconnect", &self.reconnect)
            .field("stopped_page_size", &self.stopped_page_size)
            .field("history", &"TaskHistoryStore")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyncEvent {
    ConnectionStateChanged(ConnectionState),
    CapabilitiesChanged(EngineCapabilities),
    StorePatched(StorePatch),
    SpeedHistoryChanged,
    Error(SyncError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreSnapshot {
    pub session: EngineSession,
    pub connection_state: ConnectionState,
    pub stale: bool,
    pub global_stat: GlobalStat,
    pub speed_history: crate::SpeedHistory,
    pub counts: TaskCounts,
    /// How many stopped results are loaded locally versus aria2's total.
    pub stopped_history: StoppedHistoryState,
    pub view: TaskListView,
    pub tasks: Vec<DownloadTask>,
    pub observed_seeding_seconds: HashMap<Gid, u64>,
    /// Last probed engine capabilities for this session (empty before connect).
    pub capabilities: EngineCapabilities,
    /// Task category affiliations for the current profile (C1).
    pub category_by_gid: HashMap<Gid, String>,
}

pub(super) fn validate_capabilities(capabilities: &EngineCapabilities) -> Result<(), SyncError> {
    if capabilities.version.trim().is_empty() {
        return Err(SyncError::new(
            SyncErrorKind::Protocol,
            "aria2 returned an empty version during capability verification",
            false,
        ));
    }
    Ok(())
}
