use super::format::format_rate;

/// Stable presentation identity for a task within one profile.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TaskIdentity {
    pub profile_id: String,
    pub gid: String,
}

impl TaskIdentity {
    /// Construct from a strongly-typed domain task identity.
    #[must_use]
    pub fn from_domain(identity: ariadeck_domain::TaskIdentity) -> Self {
        Self {
            profile_id: identity.profile_id.to_string(),
            gid: identity.gid.to_string(),
        }
    }

    /// Stable process-local `u64` key for GPUI element identity.
    #[must_use]
    pub fn element_key(&self) -> u64 {
        use std::hash::{DefaultHasher, Hash as _, Hasher as _};
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum TaskStatusView {
    Active,
    Seeding,
    Waiting,
    Paused,
    Complete,
    Failed,
    Verifying,
    Removed,
    #[default]
    Unknown,
}

/// Reuses [`ariadeck_domain::TaskNameState`] directly.
pub type TaskNameStateView = ariadeck_domain::TaskNameState;

pub trait TaskNameStateExt {
    #[must_use]
    fn label(self) -> &'static str;
}

impl TaskNameStateExt for TaskNameStateView {
    fn label(self) -> &'static str {
        match self {
            Self::Resolving => "Resolving filename",
            Self::Resolved => "Filename resolved",
            Self::Custom => "Custom filename",
        }
    }
}

/// Reuses [`ariadeck_domain::TaskSourceKind`] directly.
pub type TaskSourceKindView = ariadeck_domain::TaskSourceKind;

pub trait TaskSourceKindExt {
    #[must_use]
    fn label(self) -> &'static str;

    #[must_use]
    fn message_key(self) -> &'static str;
}

impl TaskSourceKindExt for TaskSourceKindView {
    fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::DirectUri => "Direct URL",
            Self::Magnet => "Magnet",
            Self::BitTorrent => "BitTorrent",
            Self::Metalink => "Metalink",
        }
    }

    fn message_key(self) -> &'static str {
        match self {
            Self::Unknown => "task-source-unknown",
            Self::DirectUri => "task-source-direct-uri",
            Self::Magnet => "task-source-magnet",
            Self::BitTorrent => "task-source-bittorrent",
            Self::Metalink => "task-source-metalink",
        }
    }
}

impl TaskStatusView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Seeding => "Seeding",
            Self::Waiting => "Waiting",
            Self::Paused => "Paused",
            Self::Complete => "Complete",
            Self::Failed => "Failed",
            Self::Verifying => "Verifying",
            Self::Removed => "Removed",
            Self::Unknown => "Unknown",
        }
    }

    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::Active => "task-status-active",
            Self::Seeding => "task-status-seeding",
            Self::Waiting => "task-status-waiting",
            Self::Paused => "task-status-paused",
            Self::Complete => "task-status-complete",
            Self::Failed => "task-status-failed",
            Self::Verifying => "task-status-verifying",
            Self::Removed => "task-status-removed",
            Self::Unknown => "task-status-unknown",
        }
    }

    #[must_use]
    pub const fn can_pause(self) -> bool {
        matches!(
            self,
            Self::Active | Self::Seeding | Self::Waiting | Self::Verifying
        )
    }

    #[must_use]
    pub const fn can_resume(self) -> bool {
        matches!(self, Self::Paused)
    }

    #[must_use]
    pub const fn can_retry(self) -> bool {
        matches!(self, Self::Failed)
    }

    #[must_use]
    pub const fn can_remove(self) -> bool {
        !matches!(self, Self::Unknown)
    }

    #[must_use]
    pub const fn can_move_in_queue(self) -> bool {
        matches!(
            self,
            Self::Active | Self::Seeding | Self::Waiting | Self::Paused | Self::Verifying
        )
    }

    /// Per-task speed limits use aria2's `changeOption`, which targets a live
    /// download. A completed/failed/removed task cannot change its limits, and
    /// an unknown status has no addressable task.
    #[must_use]
    pub const fn can_set_speed_limit(self) -> bool {
        matches!(
            self,
            Self::Active | Self::Seeding | Self::Waiting | Self::Paused | Self::Verifying
        )
    }

    #[must_use]
    pub const fn uses_active_connections(self) -> bool {
        matches!(self, Self::Active | Self::Seeding)
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Complete | Self::Failed | Self::Removed)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskErrorView {
    pub code: Option<u32>,
    pub summary: String,
    pub details: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadRowView {
    pub identity: TaskIdentity,
    pub display_name: String,
    pub name_state: TaskNameStateView,
    pub source_kind: TaskSourceKindView,
    pub primary_source: Option<String>,
    pub directory: Option<String>,
    pub category_id: Option<String>,
    pub followed_by: Vec<String>,
    pub belongs_to: Option<String>,
    pub status: TaskStatusView,
    pub error: Option<TaskErrorView>,
    pub total_bytes: u64,
    pub completed_bytes: u64,
    pub uploaded_bytes: u64,
    pub download_rate: u64,
    pub upload_rate: u64,
    pub eta_seconds: Option<u64>,
    pub observed_seeding_seconds: Option<u64>,
    pub revision: u64,
}

impl DownloadRowView {
    #[must_use]
    pub fn progress_basis_points(&self) -> Option<u16> {
        if self.total_bytes == 0 {
            return None;
        }
        let completed = u128::from(self.completed_bytes.min(self.total_bytes));
        Some(((completed * 10_000) / u128::from(self.total_bytes)) as u16)
    }

    #[must_use]
    pub const fn can_set_output_name(&self) -> bool {
        matches!(self.source_kind, TaskSourceKindView::DirectUri)
            && matches!(
                self.status,
                TaskStatusView::Active
                    | TaskStatusView::Waiting
                    | TaskStatusView::Paused
                    | TaskStatusView::Verifying
            )
    }

    #[must_use]
    pub const fn can_relink_url(&self) -> bool {
        matches!(self.source_kind, TaskSourceKindView::DirectUri)
            && (!self.status.is_terminal() || matches!(self.status, TaskStatusView::Failed))
    }

    #[must_use]
    pub const fn can_edit_or_relink(&self) -> bool {
        self.status.can_set_speed_limit() || self.can_relink_url()
    }

    /// Share ratio as fixed thousandths (1.000 == 1000), without floating
    /// point rounding in the underlying byte calculation.
    #[must_use]
    pub fn share_ratio_milli(&self) -> Option<u64> {
        if self.total_bytes == 0 {
            return None;
        }
        let value = (u128::from(self.uploaded_bytes) * 1_000) / u128::from(self.total_bytes);
        Some(u64::try_from(value).unwrap_or(u64::MAX))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TaskCountsView {
    pub all: usize,
    pub active: usize,
    pub waiting: usize,
    pub paused: usize,
    pub completed: usize,
    pub failed: usize,
}

/// Stopped-result page progress for completed/failed history (HISTORY-001 / B6).
///
/// `total` is aria2's in-memory result count and may be lower than lifetime
/// history once `--max-download-result` is exceeded. `local_saved` counts
/// durable rows held only in SQLite (survives engine restarts and result purge).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StoppedHistoryView {
    pub loaded: usize,
    pub total: Option<usize>,
    pub can_load_more: bool,
    pub local_saved: usize,
}

impl StoppedHistoryView {
    #[must_use]
    pub fn summary_label(self) -> Option<String> {
        let engine = self.total.map(|total| {
            if total == 0 {
                None
            } else {
                Some(format!("Engine {}/{total}", self.loaded.min(total)))
            }
        });
        match (engine.flatten(), self.local_saved > 0) {
            (Some(engine), true) => Some(format!("{engine} · Saved {}", self.local_saved)),
            (Some(engine), false) => Some(engine),
            (None, true) => Some(format!("Saved {}", self.local_saved)),
            (None, false) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SpeedSampleView {
    pub download_rate: u64,
    pub upload_rate: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ConnectionView {
    #[default]
    Disconnected,
    Connecting,
    Authenticating,
    Synchronizing,
    Connected,
    Reconnecting {
        attempt: u32,
    },
    Failed {
        summary: String,
        retryable: bool,
    },
}

impl ConnectionView {
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Disconnected => "Offline",
            Self::Connecting => "Connecting",
            Self::Authenticating => "Authenticating",
            Self::Synchronizing => "Synchronizing",
            Self::Connected => "Connected",
            Self::Reconnecting { .. } => "Reconnecting",
            Self::Failed { .. } => "Connection failed",
        }
    }

    #[must_use]
    pub const fn message_key(&self) -> &'static str {
        match self {
            Self::Disconnected => "connection-offline",
            Self::Connecting => "connection-connecting",
            Self::Authenticating => "connection-authenticating",
            Self::Synchronizing => "connection-synchronizing",
            Self::Connected => "connection-connected",
            Self::Reconnecting { .. } => "connection-reconnecting",
            Self::Failed { .. } => "connection-failed",
        }
    }

    #[must_use]
    pub const fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }

    #[must_use]
    pub const fn can_retry(&self) -> bool {
        matches!(
            self,
            Self::Disconnected
                | Self::Reconnecting { .. }
                | Self::Failed {
                    retryable: true,
                    ..
                }
        )
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum EngineHealthView {
    #[default]
    External,
    Running {
        restarts: u32,
    },
    Restarting {
        attempt: u32,
    },
    Failed {
        summary: String,
    },
}

impl EngineHealthView {
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::External => "External RPC",
            Self::Running { restarts: 0 } => "Local engine running",
            Self::Running { .. } => "Local engine recovered",
            Self::Restarting { .. } => "Local engine restarting",
            Self::Failed { .. } => "Local engine stopped",
        }
    }

    #[must_use]
    pub const fn message_key(&self) -> &'static str {
        match self {
            Self::External => "engine-external",
            Self::Running { restarts: 0 } => "engine-running",
            Self::Running { .. } => "engine-recovered",
            Self::Restarting { .. } => "engine-restarting",
            Self::Failed { .. } => "engine-stopped",
        }
    }
}

/// UI projection of aria2 capabilities from system.listMethods (RPC-002).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EngineCapabilitiesView {
    pub version: String,
    pub methods_probed: bool,
    pub force_pause: bool,
    pub force_pause_all: bool,
    pub force_remove: bool,
    pub queue_positioning: bool,
    pub change_option: bool,
    pub change_uri: bool,
    pub change_global_option: bool,
    pub get_peers: bool,
    pub get_servers: bool,
    pub multicall: bool,
}

impl EngineCapabilitiesView {
    /// Open-handed defaults used before the first successful capability probe.
    #[must_use]
    pub fn unknown() -> Self {
        Self {
            version: String::new(),
            methods_probed: false,
            force_pause: true,
            force_pause_all: true,
            force_remove: true,
            queue_positioning: true,
            change_option: true,
            change_uri: true,
            change_global_option: true,
            get_peers: true,
            get_servers: true,
            multicall: true,
        }
    }

    #[must_use]
    pub fn unsupported_force_pause_message(&self) -> &'static str {
        "This aria2 build does not expose force-pause."
    }

    #[must_use]
    pub fn unsupported_force_pause_all_message(&self) -> &'static str {
        "This aria2 build does not expose force-pause-all."
    }

    #[must_use]
    pub fn unsupported_force_remove_message(&self) -> &'static str {
        "This aria2 build does not expose force-remove."
    }

    #[must_use]
    pub fn unsupported_queue_message(&self) -> &'static str {
        "This aria2 build does not expose queue positioning."
    }

    #[must_use]
    pub fn unsupported_change_option_message(&self) -> &'static str {
        "This aria2 build does not expose per-task option changes."
    }

    #[must_use]
    pub fn unsupported_change_uri_message(&self) -> &'static str {
        "This aria2 build does not expose per-task URI replacement."
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSnapshot {
    pub profile_id: String,
    pub session_id: String,
    pub generation: u64,
    pub source_revision: u64,
    pub connection: ConnectionView,
    pub stale: bool,
    pub local_path_actions_available: bool,
    pub download_rate: u64,
    pub upload_rate: u64,
    pub speed_history: Vec<SpeedSampleView>,
    pub counts: TaskCountsView,
    pub stopped_history: StoppedHistoryView,
    pub tasks: Vec<DownloadRowView>,
    pub capabilities: EngineCapabilitiesView,
}

impl Default for WorkspaceSnapshot {
    fn default() -> Self {
        Self {
            profile_id: String::new(),
            session_id: String::new(),
            generation: 0,
            source_revision: 0,
            connection: ConnectionView::Disconnected,
            stale: false,
            local_path_actions_available: false,
            download_rate: 0,
            upload_rate: 0,
            speed_history: Vec::new(),
            counts: TaskCountsView::default(),
            stopped_history: StoppedHistoryView::default(),
            tasks: Vec::new(),
            capabilities: EngineCapabilitiesView::unknown(),
        }
    }
}

impl WorkspaceSnapshot {
    #[must_use]
    pub fn has_active_downloads(&self) -> bool {
        self.counts.active > 0
    }

    /// Compact transfer summary for the system-tray tooltip (PLAT-001).
    #[must_use]
    pub fn tray_tooltip(&self) -> String {
        format!(
            "AriaDeck · {} · ↓ {} · ↑ {}",
            self.connection.label(),
            format_rate(self.download_rate),
            format_rate(self.upload_rate),
        )
    }

    #[must_use]
    pub fn engine_session(&self) -> Option<EngineSessionView> {
        if self.profile_id.is_empty() || self.session_id.is_empty() || self.generation == 0 {
            return None;
        }
        Some(EngineSessionView {
            profile_id: self.profile_id.clone(),
            session_id: self.session_id.clone(),
            generation: self.generation,
        })
    }

    #[must_use]
    pub fn commands_available(&self) -> bool {
        self.connection.is_connected() && !self.stale && self.engine_session().is_some()
    }
}
/// Opaque UI representation of one exact engine connection session.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EngineSessionView {
    pub profile_id: String,
    pub session_id: String,
    pub generation: u64,
}

/// Monotonic identifier used to reject out-of-order asynchronous UI results.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RequestId(u64);

impl RequestId {
    #[must_use]
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum UpdateStatusView {
    #[default]
    Disabled,
    Checking,
    Current,
    Available {
        version: String,
        title: String,
        notes: String,
        release_url: String,
        installer_available: bool,
    },
    Downloading {
        version: String,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
    Downloaded {
        version: String,
    },
    Failed(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateOfferView {
    pub version: String,
    pub title: String,
    pub notes: String,
    pub release_url: String,
    pub installer_available: bool,
}

/// Kind of in-app activity/history entry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ActivityKindView {
    Completion,
    Error,
    Engine,
    Command,
    Info,
}

impl ActivityKindView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Completion => "Completed",
            Self::Error => "Error",
            Self::Engine => "Engine",
            Self::Command => "Command",
            Self::Info => "Info",
        }
    }

    #[must_use]
    pub const fn is_error(self) -> bool {
        matches!(self, Self::Error | Self::Engine)
    }
}

/// One in-session activity/history row (not persisted across restarts).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityEntryView {
    pub id: u64,
    pub kind: ActivityKindView,
    pub summary: String,
    pub detail: Option<String>,
    pub task: Option<TaskIdentity>,
    pub count: u32,
}
