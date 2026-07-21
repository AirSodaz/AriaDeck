use std::path::PathBuf;

/// Stable presentation identity for a task within one profile.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TaskIdentity {
    pub profile_id: String,
    pub gid: String,
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

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum TaskNameStateView {
    #[default]
    Resolving,
    Resolved,
    Custom,
}

impl TaskNameStateView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Resolving => "Resolving filename",
            Self::Resolved => "Filename resolved",
            Self::Custom => "Custom filename",
        }
    }

    #[must_use]
    pub const fn is_resolving(self) -> bool {
        matches!(self, Self::Resolving)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum TaskSourceKindView {
    #[default]
    Unknown,
    DirectUri,
    Magnet,
    BitTorrent,
    Metalink,
}

impl TaskSourceKindView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::DirectUri => "Direct URL",
            Self::Magnet => "Magnet",
            Self::BitTorrent => "BitTorrent",
            Self::Metalink => "Metalink",
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

    /// Per-task connection policy uses the same live-task changeOption surface.
    #[must_use]
    pub const fn can_set_connection_policy(self) -> bool {
        self.can_set_speed_limit()
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

/// Stopped-result page progress for completed/failed history (HISTORY-001).
///
/// `total` is aria2's in-memory result count and may be lower than lifetime
/// history once `--max-download-result` is exceeded. Before SQLite history
/// exists, only engine-held results and the managed session file survive a
/// restart.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StoppedHistoryView {
    pub loaded: usize,
    pub total: Option<usize>,
    pub can_load_more: bool,
}

impl StoppedHistoryView {
    #[must_use]
    pub fn summary_label(self) -> Option<String> {
        let total = self.total?;
        if total == 0 {
            return None;
        }
        Some(format!("History {}/{total}", self.loaded.min(total)))
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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum TaskCommandView {
    Pause,
    ForcePause,
    Resume,
    MoveToQueueTop,
    MoveUpInQueue,
    MoveDownInQueue,
    MoveToQueueBottom,
    Retry,
    SetOutputName {
        output_name: String,
    },
    SetSpeedLimit {
        download_limit: u64,
        upload_limit: u64,
    },
    SetConnectionPolicy {
        max_connection_per_server: u32,
        split: u32,
        min_split_size: u64,
    },
    SetOptions {
        seed_ratio: Option<String>,
        seed_time_minutes: Option<String>,
        selected_file_indices: Option<Vec<u32>>,
    },
    RemoveTask,
    ForceRemoveTask,
    RemoveTaskAndFiles,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GlobalTaskCommandView {
    PauseAll,
    ForcePauseAll,
    ResumeAll,
}

impl GlobalTaskCommandView {
    #[must_use]
    pub const fn progress_label(self) -> &'static str {
        match self {
            Self::PauseAll => "Pausing all tasks...",
            Self::ForcePauseAll => "Force-pausing all tasks...",
            Self::ResumeAll => "Resuming all tasks...",
        }
    }

    #[must_use]
    pub const fn success_label(self) -> &'static str {
        match self {
            Self::PauseAll => "All eligible tasks paused.",
            Self::ForcePauseAll => "All eligible tasks force-paused.",
            Self::ResumeAll => "All paused tasks resumed.",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BatchTaskCommandView {
    Pause,
    ForcePause,
    Resume,
    Retry,
    RemoveTask,
    ForceRemoveTask,
    RemoveTaskAndFiles,
}

impl BatchTaskCommandView {
    #[must_use]
    pub const fn progress_label(self) -> &'static str {
        match self {
            Self::Pause => "Pausing selected tasks...",
            Self::ForcePause => "Force-pausing selected tasks...",
            Self::Resume => "Resuming selected tasks...",
            Self::Retry => "Creating new tasks from selected failed tasks...",
            Self::RemoveTask => "Removing selected tasks...",
            Self::ForceRemoveTask => "Force-removing selected tasks...",
            Self::RemoveTaskAndFiles => {
                "Removing selected tasks and moving local files to the Recycle Bin..."
            }
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pause => "Pause",
            Self::ForcePause => "Force pause",
            Self::Resume => "Resume",
            Self::Retry => "Retry",
            Self::RemoveTask => "Remove",
            Self::ForceRemoveTask => "Force remove",
            Self::RemoveTaskAndFiles => "Remove with files",
        }
    }
}

impl TaskCommandView {
    #[must_use]
    pub const fn progress_label(&self) -> &'static str {
        match self {
            Self::Pause => "Pausing task...",
            Self::ForcePause => "Force-pausing task...",
            Self::Resume => "Resuming task...",
            Self::MoveToQueueTop => "Moving task to the top of the queue...",
            Self::MoveUpInQueue => "Moving task up in the queue...",
            Self::MoveDownInQueue => "Moving task down in the queue...",
            Self::MoveToQueueBottom => "Moving task to the bottom of the queue...",
            Self::Retry => "Creating a new task from the failed source...",
            Self::SetOutputName { .. } => "Updating output name...",
            Self::SetSpeedLimit { .. } => "Updating speed limits...",
            Self::SetConnectionPolicy { .. } => "Updating connection policy...",
            Self::SetOptions { .. } => "Updating task options...",
            Self::RemoveTask => "Removing task...",
            Self::ForceRemoveTask => "Force-removing task...",
            Self::RemoveTaskAndFiles => {
                "Removing task and moving local files to the Recycle Bin..."
            }
        }
    }

    #[must_use]
    pub const fn success_label(&self) -> &'static str {
        match self {
            Self::Pause => "Task paused.",
            Self::ForcePause => "Task force-paused.",
            Self::Resume => "Task resumed.",
            Self::MoveToQueueTop => "Task moved to the top of the queue.",
            Self::MoveUpInQueue => "Task moved up in the queue.",
            Self::MoveDownInQueue => "Task moved down in the queue.",
            Self::MoveToQueueBottom => "Task moved to the bottom of the queue.",
            Self::Retry => "New retry task created; the failed result was kept.",
            Self::SetOutputName { .. } => "Output name updated.",
            Self::SetSpeedLimit { .. } => "Speed limits updated for this task.",
            Self::SetConnectionPolicy { .. } => "Connection policy updated for this task.",
            Self::SetOptions { .. } => "Task options updated.",
            Self::RemoveTask => "Task removed from aria2; downloaded files were kept.",
            Self::ForceRemoveTask => "Task force-removed from aria2; downloaded files were kept.",
            Self::RemoveTaskAndFiles => "Task removed; local files were moved to the Recycle Bin.",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AddDownloadInputModeView {
    #[default]
    Links,
    MetadataFiles,
}

impl AddDownloadInputModeView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Links => "Links",
            Self::MetadataFiles => "Torrent / Metalink",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddDownloadMetadataKindView {
    Torrent,
    Metalink,
}

impl AddDownloadMetadataKindView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Torrent => "Torrent",
            Self::Metalink => "Metalink",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AddDownloadModeView {
    #[default]
    SeparateTasks,
    Mirrors,
}

impl AddDownloadModeView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::SeparateTasks => "Separate tasks",
            Self::Mirrors => "Mirrors (one task)",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FileConflictPolicyView {
    #[default]
    AutoRename,
    Reject,
    Overwrite,
}

impl FileConflictPolicyView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::AutoRename => "Keep both",
            Self::Reject => "Reject",
            Self::Overwrite => "Overwrite",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AddDownloadSourceView {
    Uri {
        line: usize,
        uri: String,
    },
    MetadataFile {
        path: PathBuf,
        kind: AddDownloadMetadataKindView,
        content_sha256: String,
        info_hash: Option<String>,
        selected_file_indices: Vec<u32>,
    },
}

impl AddDownloadSourceView {
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Uri { line, uri } => format!("Line {line} - {uri}"),
            Self::MetadataFile { path, kind, .. } => {
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into(),
                );
                format!("{} - {name}", kind.label())
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadMetadataFileView {
    pub index: u32,
    pub path: String,
    pub length: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadMetadataPreviewView {
    pub path: PathBuf,
    pub kind: AddDownloadMetadataKindView,
    pub content_sha256: String,
    pub info_hash: Option<String>,
    pub files: Vec<AddDownloadMetadataFileView>,
    pub selected_file_indices: Vec<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadMetadataPreviewRequestView {
    pub request_id: RequestId,
    pub paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AddDownloadMetadataPreviewOutcomeView {
    Ready(AddDownloadMetadataPreviewView),
    Failed(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadMetadataPreviewItemView {
    pub path: PathBuf,
    pub outcome: AddDownloadMetadataPreviewOutcomeView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadMetadataPreviewResultView {
    pub request_id: RequestId,
    pub items: Vec<AddDownloadMetadataPreviewItemView>,
}

/// Advanced source controls for a new direct-URI download (ADD-005).
///
/// Secrets use `SecretStringView` so Debug and notices never echo them. Cookie
/// and HTTP password are separate from free-form headers so the application
/// layer can keep redaction and validation consistent.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AddDownloadAdvancedOptionsView {
    pub referer: String,
    pub user_agent: String,
    /// Multi-line `Name: value` headers. Cookie/Authorization belong in the
    /// dedicated secret fields below.
    pub headers: String,
    pub cookie: Option<SecretStringView>,
    pub http_user: String,
    pub http_passwd: Option<SecretStringView>,
    /// aria2 `type=digest` form, for example `sha-256=…`.
    pub checksum: String,
}

impl AddDownloadAdvancedOptionsView {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.referer.trim().is_empty()
            && self.user_agent.trim().is_empty()
            && self.headers.trim().is_empty()
            && self.cookie.is_none()
            && self.http_user.trim().is_empty()
            && self.http_passwd.is_none()
            && self.checksum.trim().is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadRequestView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub sources: Vec<AddDownloadSourceView>,
    pub mode: AddDownloadModeView,
    pub destination: Option<String>,
    pub required_bytes: Option<u64>,
    pub file_conflict: FileConflictPolicyView,
    pub advanced: AddDownloadAdvancedOptionsView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskCommandRequestView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub identity: TaskIdentity,
    pub command: TaskCommandView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlobalTaskCommandRequestView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub command: GlobalTaskCommandView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchTaskCommandRequestView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub identities: Vec<TaskIdentity>,
    pub command: BatchTaskCommandView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskDetailsRequestView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub identity: TaskIdentity,
    pub active: bool,
    pub is_bittorrent: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskOpenTargetView {
    Download,
    Folder,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskOpenRequestView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub identity: TaskIdentity,
    pub target: TaskOpenTargetView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaskOpenOutcomeView {
    Success,
    Failure(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskOpenResultView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub identity: TaskIdentity,
    pub target: TaskOpenTargetView,
    pub outcome: TaskOpenOutcomeView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationErrorView {
    pub code: String,
    pub summary: String,
    pub retryable: bool,
}

impl OperationErrorView {
    #[must_use]
    pub fn outcome_unknown(&self) -> bool {
        self.code == "rpc.command_outcome_unknown"
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandOutcomeView {
    Success { tasks: Vec<TaskIdentity> },
    Failure(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchTaskFailureView {
    pub identity: Option<TaskIdentity>,
    pub error: OperationErrorView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BatchCommandOutcomeView {
    Success {
        succeeded: Vec<TaskIdentity>,
    },
    PartialSuccess {
        succeeded: Vec<TaskIdentity>,
        failed: Vec<BatchTaskFailureView>,
    },
    Failure {
        failed: Vec<BatchTaskFailureView>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadItemResultView {
    pub sources: Vec<AddDownloadSourceView>,
    pub existing_task: Option<TaskIdentity>,
    pub outcome: CommandOutcomeView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadResultView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub items: Vec<AddDownloadItemResultView>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskCommandResultView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub identity: TaskIdentity,
    pub command: TaskCommandView,
    pub outcome: CommandOutcomeView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlobalTaskCommandResultView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub command: GlobalTaskCommandView,
    pub outcome: CommandOutcomeView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchTaskCommandResultView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub identities: Vec<TaskIdentity>,
    pub command: BatchTaskCommandView,
    pub outcome: BatchCommandOutcomeView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskFileView {
    pub index: u32,
    pub path: String,
    pub length: u64,
    pub completed_length: u64,
    pub selected: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TaskUriStatusView {
    Used,
    Waiting,
    #[default]
    Unknown,
}

impl TaskUriStatusView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Used => "In use",
            Self::Waiting => "Mirror",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskUriView {
    pub uri: String,
    pub status: TaskUriStatusView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskTrackerView {
    pub tier: u32,
    pub uri: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskServerView {
    pub file_index: u32,
    pub uri: String,
    pub current_uri: String,
    pub download_rate: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskPeerView {
    pub address: String,
    pub port: u16,
    pub download_rate: u64,
    pub upload_rate: u64,
    pub seeder: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskOptionView {
    pub key: String,
    pub value: String,
    pub redacted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskDetailsView {
    pub directory: Option<String>,
    pub primary_source: Option<String>,
    pub output_path: Option<String>,
    pub path_validation: TaskPathValidationView,
    pub info_hash: Option<String>,
    pub piece_length: Option<u64>,
    pub piece_count: Option<u32>,
    pub trackers: Vec<TaskTrackerView>,
    pub uris: Vec<TaskUriView>,
    pub servers: Vec<TaskServerView>,
    pub peers: Vec<TaskPeerView>,
    pub options: Vec<TaskOptionView>,
    pub files: Vec<TaskFileView>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum TaskPathValidationView {
    #[default]
    Unavailable,
    Valid {
        existing_files: usize,
        missing_paths: usize,
    },
    Warning(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaskDetailsOutcomeView {
    Ready(Box<TaskDetailsView>),
    Failed(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskDetailsResultView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub identity: TaskIdentity,
    pub outcome: TaskDetailsOutcomeView,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ColorSchemeView {
    Light,
    #[default]
    Dark,
}

impl ColorSchemeView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProxyModeView {
    #[default]
    Disabled,
    Manual,
}

impl ProxyModeView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::Manual => "Manual",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DownloadProxySettingsView {
    pub mode: ProxyModeView,
    pub all_proxy: String,
    pub http_proxy: String,
    pub https_proxy: String,
    pub ftp_proxy: String,
    pub no_proxy: Vec<String>,
    pub username: String,
    pub has_password: bool,
}

/// Speed limit values as user-editable text using aria2's `K`/`M` suffix syntax.
///
/// The accepted forms mirror aria2's `--max-*-limit` options: a plain byte
/// count (`1048576`), or a whole number followed by a `K`/`M`/`G` suffix
/// (`512K`, `2M`, case-insensitive), where each unit is a 1024-based multiple
/// exactly as aria2 interprets it. An empty field means "no limit" (`0`). The
/// UI parses these to bytes/second before building a save request.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpeedLimitSettingsView {
    /// Raw text entered by the user for the global download speed limit.
    pub download_limit: String,
    /// Raw text entered by the user for the global upload speed limit.
    pub upload_limit: String,
}

/// File allocation method for transfer-policy settings (RATE-002).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FileAllocationView {
    None,
    #[default]
    Prealloc,
    Trunc,
    Falloc,
}

impl FileAllocationView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Prealloc => "Prealloc",
            Self::Trunc => "Trunc",
            Self::Falloc => "Falloc",
        }
    }

    #[must_use]
    pub const fn all() -> [Self; 4] {
        [Self::None, Self::Prealloc, Self::Trunc, Self::Falloc]
    }
}

/// Transfer-policy settings as user-editable text fields (RATE-002).
///
/// Count fields are plain positive integers. `min_split_size` reuses aria2's
/// `K`/`M`/`G` size syntax (same as speed limits, but 0 is invalid). Scope
/// labels are fixed in the settings page: concurrent downloads affect the live
/// queue; connection/split/allocation/integrity are defaults for new downloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferPolicySettingsView {
    pub max_concurrent_downloads: String,
    pub max_connection_per_server: String,
    pub split: String,
    pub min_split_size: String,
    pub file_allocation: FileAllocationView,
    pub check_integrity: bool,
}

impl Default for TransferPolicySettingsView {
    fn default() -> Self {
        Self {
            max_concurrent_downloads: "5".into(),
            max_connection_per_server: "1".into(),
            split: "5".into(),
            min_split_size: "20M".into(),
            file_allocation: FileAllocationView::Prealloc,
            check_integrity: false,
        }
    }
}

impl TransferPolicySettingsView {
    #[must_use]
    pub fn parse_max_concurrent_downloads(&self) -> Option<u32> {
        parse_positive_u32(&self.max_concurrent_downloads)
    }

    #[must_use]
    pub fn parse_max_connection_per_server(&self) -> Option<u32> {
        parse_positive_u32(&self.max_connection_per_server).filter(|value| (1..=16).contains(value))
    }

    #[must_use]
    pub fn parse_split(&self) -> Option<u32> {
        parse_positive_u32(&self.split)
    }

    /// Parse min-split-size to bytes. Empty is invalid (unlike speed limits).
    #[must_use]
    pub fn parse_min_split_size(&self) -> Option<u64> {
        let value = parse_speed_limit_field(&self.min_split_size)?;
        (value > 0).then_some(value)
    }

    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.parse_max_concurrent_downloads().is_some()
            && self.parse_max_connection_per_server().is_some()
            && self.parse_split().is_some()
            && self.parse_min_split_size().is_some()
    }
}

fn parse_positive_u32(value: &str) -> Option<u32> {
    let trimmed = value.trim();
    if trimmed.is_empty() || !trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    trimmed.parse::<u32>().ok().filter(|value| *value > 0)
}

impl SpeedLimitSettingsView {
    /// Parse `download_limit` to bytes/second. Empty string yields 0 (unlimited).
    #[must_use]
    pub fn parse_download_limit(&self) -> Option<u64> {
        parse_speed_limit_field(&self.download_limit)
    }

    /// Parse `upload_limit` to bytes/second. Empty string yields 0 (unlimited).
    #[must_use]
    pub fn parse_upload_limit(&self) -> Option<u64> {
        parse_speed_limit_field(&self.upload_limit)
    }

    /// Returns true when both fields parse (or are empty).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.parse_download_limit().is_some() && self.parse_upload_limit().is_some()
    }
}

/// Parse an aria2 speed-limit field to bytes/second.
///
/// Accepts an empty string (0/unlimited), a plain integer of bytes, or an
/// integer with a single `K`/`M`/`G` suffix (case-insensitive, 1024-based).
/// Returns `None` when the value is malformed or overflows `u64`.
#[must_use]
pub fn parse_speed_limit_field(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Some(0);
    }
    let (digits, multiplier) = match trimmed.as_bytes().last() {
        Some(b'K' | b'k') => (&trimmed[..trimmed.len() - 1], 1024),
        Some(b'M' | b'm') => (&trimmed[..trimmed.len() - 1], 1024 * 1024),
        Some(b'G' | b'g') => (&trimmed[..trimmed.len() - 1], 1024 * 1024 * 1024),
        _ => (trimmed, 1),
    };
    let digits = digits.trim_end();
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits
        .parse::<u64>()
        .ok()
        .and_then(|amount| amount.checked_mul(multiplier))
}

/// Render a byte/second speed limit back into a compact editable field value.
///
/// Zero becomes an empty string (unlimited). Exact 1024-based multiples use the
/// largest whole `K`/`M`/`G` unit so a saved `2M` round-trips as `2M` instead
/// of `2097152`.
#[must_use]
pub fn format_speed_limit_field(bytes_per_second: u64) -> String {
    if bytes_per_second == 0 {
        return String::new();
    }
    for (suffix, unit) in [
        ('G', 1024u64 * 1024 * 1024),
        ('M', 1024 * 1024),
        ('K', 1024),
    ] {
        if bytes_per_second.is_multiple_of(unit) {
            return format!("{}{suffix}", bytes_per_second / unit);
        }
    }
    bytes_per_second.to_string()
}

#[derive(Clone, Eq, PartialEq)]
pub struct SecretStringView(String);

impl SecretStringView {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Debug for SecretStringView {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretStringView([REDACTED])")
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum ProxyPasswordUpdateView {
    #[default]
    Unchanged,
    Clear,
    Set(SecretStringView),
}

/// How loudly automatic completion/error surfaces appear.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum NotificationVolumeView {
    #[default]
    Normal,
    Quiet,
    Silent,
}

impl NotificationVolumeView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Quiet => "Quiet",
            Self::Silent => "Silent",
        }
    }

    #[must_use]
    pub const fn all() -> [Self; 3] {
        [Self::Normal, Self::Quiet, Self::Silent]
    }
}

/// Notification preferences for automatic task/engine surfaces (OBS-001).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotificationSettingsView {
    pub volume: NotificationVolumeView,
    pub notify_on_completion: bool,
    pub notify_on_error: bool,
    pub notify_on_engine_events: bool,
}

impl Default for NotificationSettingsView {
    fn default() -> Self {
        Self {
            volume: NotificationVolumeView::Normal,
            notify_on_completion: true,
            notify_on_error: true,
            notify_on_engine_events: true,
        }
    }
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

/// Local managed vs remote RPC profile (PROFILE-001).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum ProfileKindView {
    #[default]
    LocalManaged,
    RemoteRpc,
}

impl ProfileKindView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::LocalManaged => "Local managed",
            Self::RemoteRpc => "Remote RPC",
        }
    }

    #[must_use]
    pub const fn all() -> [Self; 2] {
        [Self::LocalManaged, Self::RemoteRpc]
    }
}

/// One profile catalog entry for settings/switch UI.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileEntryView {
    pub profile_id: String,
    pub name: String,
    pub kind: ProfileKindView,
    pub executable: String,
    pub download_dir: String,
    pub endpoint: String,
    pub has_secret: bool,
}

impl Default for ProfileEntryView {
    fn default() -> Self {
        Self {
            profile_id: String::new(),
            name: "Local aria2".into(),
            kind: ProfileKindView::LocalManaged,
            executable: String::new(),
            download_dir: String::new(),
            endpoint: String::new(),
            has_secret: false,
        }
    }
}

/// Multi-profile catalog presented to the settings page.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProfileCatalogView {
    pub active_profile_id: String,
    pub profiles: Vec<ProfileEntryView>,
}

impl ProfileCatalogView {
    #[must_use]
    pub fn active(&self) -> Option<&ProfileEntryView> {
        self.profiles
            .iter()
            .find(|profile| profile.profile_id == self.active_profile_id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwitchProfileRequestView {
    pub request_id: RequestId,
    pub profile_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SwitchProfileOutcomeView {
    Success,
    Failure(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwitchProfileResultView {
    pub request_id: RequestId,
    pub profile_id: String,
    pub catalog: ProfileCatalogView,
    pub outcome: SwitchProfileOutcomeView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveProfileCatalogRequestView {
    pub request_id: RequestId,
    pub catalog: ProfileCatalogView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SaveProfileCatalogOutcomeView {
    Success,
    Failure(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveProfileCatalogResultView {
    pub request_id: RequestId,
    pub catalog: ProfileCatalogView,
    pub outcome: SaveProfileCatalogOutcomeView,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsView {
    pub color_scheme: ColorSchemeView,
    pub download_directory: String,
    pub download_proxy: DownloadProxySettingsView,
    pub speed_limits: SpeedLimitSettingsView,
    pub transfer_policy: TransferPolicySettingsView,
    pub notifications: NotificationSettingsView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsSaveRequestView {
    pub request_id: RequestId,
    pub settings: SettingsView,
    pub proxy_password: ProxyPasswordUpdateView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsSaveOutcomeView {
    Success,
    Failure(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsSaveResultView {
    pub request_id: RequestId,
    pub settings: SettingsView,
    pub outcome: SettingsSaveOutcomeView,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum WorkspaceFilter {
    #[default]
    All,
    Active,
    Waiting,
    Paused,
    Completed,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum WorkspaceSortKey {
    #[default]
    Queue,
    Name,
    Status,
    Progress,
    DownloadSpeed,
    Size,
}

impl WorkspaceSortKey {
    pub const ALL: [Self; 6] = [
        Self::Queue,
        Self::Name,
        Self::Status,
        Self::Progress,
        Self::DownloadSpeed,
        Self::Size,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Queue => "Queue",
            Self::Name => "Name",
            Self::Status => "Status",
            Self::Progress => "Progress",
            Self::DownloadSpeed => "Download speed",
            Self::Size => "Size",
        }
    }

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Name => "name",
            Self::Status => "status",
            Self::Progress => "progress",
            Self::DownloadSpeed => "download-speed",
            Self::Size => "size",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum WorkspaceSortDirection {
    #[default]
    Ascending,
    Descending,
}

impl WorkspaceSortDirection {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ascending => "Ascending",
            Self::Descending => "Descending",
        }
    }

    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

impl WorkspaceFilter {
    pub const ALL: [Self; 6] = [
        Self::All,
        Self::Active,
        Self::Waiting,
        Self::Paused,
        Self::Completed,
        Self::Failed,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All tasks",
            Self::Active => "Active",
            Self::Waiting => "Waiting",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }

    #[must_use]
    pub const fn short_label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Active => "Active",
            Self::Waiting => "Waiting",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }

    #[must_use]
    pub const fn count(self, counts: TaskCountsView) -> usize {
        match self {
            Self::All => counts.all,
            Self::Active => counts.active,
            Self::Waiting => counts.waiting,
            Self::Paused => counts.paused,
            Self::Completed => counts.completed,
            Self::Failed => counts.failed,
        }
    }

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Active => "active",
            Self::Waiting => "waiting",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceQuery {
    pub filter: WorkspaceFilter,
    pub search: String,
    pub sort_key: WorkspaceSortKey,
    pub sort_direction: WorkspaceSortDirection,
}

#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

#[must_use]
pub fn format_rate(bytes_per_second: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_second))
}

#[must_use]
pub fn format_eta(seconds: Option<u64>) -> String {
    let Some(seconds) = seconds else {
        return "—".into();
    };
    if seconds < 60 {
        return format!("{seconds}s");
    }
    if seconds < 3_600 {
        return format!("{}m {}s", seconds / 60, seconds % 60);
    }
    format!("{}h {}m", seconds / 3_600, (seconds % 3_600) / 60)
}

#[must_use]
pub fn format_percent(basis_points: Option<u16>) -> String {
    basis_points.map_or_else(
        || "—".into(),
        |value| format!("{:.1}%", f64::from(value) / 100.0),
    )
}

#[must_use]
pub fn format_share_ratio(milli: Option<u64>) -> String {
    milli.map_or_else(
        || "—".into(),
        |value| {
            let hundredths = value.saturating_add(5) / 10;
            format!("{}.{:02}", hundredths / 100, hundredths % 100)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_request_debug_output_redacts_proxy_password() {
        let request = SettingsSaveRequestView {
            request_id: RequestId::from_u64(1),
            settings: SettingsView::default(),
            proxy_password: ProxyPasswordUpdateView::Set(SecretStringView::new("never-log-this")),
        };

        let debug = format!("{request:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("never-log-this"));
    }

    #[test]
    fn compact_transfer_formatting_is_stable_at_unit_boundaries() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1_536), "1.5 KiB");
        assert_eq!(format_rate(1_048_576), "1.0 MiB/s");
        assert_eq!(format_eta(Some(3_661)), "1h 1m");
        assert_eq!(format_percent(Some(5_050)), "50.5%");
    }

    #[test]
    fn file_conflict_policy_defaults_to_keep_both() {
        assert_eq!(
            FileConflictPolicyView::default(),
            FileConflictPolicyView::AutoRename
        );
        assert_eq!(FileConflictPolicyView::AutoRename.label(), "Keep both");
        assert_eq!(FileConflictPolicyView::Reject.label(), "Reject");
        assert_eq!(FileConflictPolicyView::Overwrite.label(), "Overwrite");
    }

    #[test]
    fn progress_clamps_overreported_completion() {
        let mut row = DownloadRowView {
            identity: TaskIdentity {
                profile_id: "profile".into(),
                gid: "gid".into(),
            },
            display_name: "archive".into(),
            name_state: TaskNameStateView::Resolved,
            source_kind: TaskSourceKindView::DirectUri,
            primary_source: None,
            directory: None,
            followed_by: Vec::new(),
            belongs_to: None,
            status: TaskStatusView::Active,
            error: None,
            total_bytes: 100,
            completed_bytes: 120,
            uploaded_bytes: 250,
            download_rate: 0,
            upload_rate: 0,
            eta_seconds: None,
            observed_seeding_seconds: None,
            revision: 1,
        };

        assert_eq!(row.progress_basis_points(), Some(10_000));
        assert_eq!(row.share_ratio_milli(), Some(2_500));
        assert_eq!(format_share_ratio(row.share_ratio_milli()), "2.50");
        assert!(row.can_set_output_name());

        row.source_kind = TaskSourceKindView::Magnet;
        assert!(!row.can_set_output_name());
        row.source_kind = TaskSourceKindView::DirectUri;
        row.status = TaskStatusView::Complete;
        assert!(!row.can_set_output_name());
    }

    #[test]
    fn retry_is_available_only_for_failed_tasks() {
        assert!(TaskStatusView::Failed.can_retry());
        assert!(!TaskStatusView::Paused.can_retry());
        assert!(!TaskStatusView::Complete.can_retry());
    }

    #[test]
    fn seeding_keeps_live_task_controls_and_active_connections() {
        assert!(TaskStatusView::Seeding.can_pause());
        assert!(TaskStatusView::Seeding.can_move_in_queue());
        assert!(TaskStatusView::Seeding.can_set_speed_limit());
        assert!(TaskStatusView::Seeding.uses_active_connections());
        assert!(!TaskStatusView::Seeding.is_terminal());
    }

    #[test]
    fn remove_is_available_for_live_tasks_and_stopped_results() {
        assert!(TaskStatusView::Active.can_remove());
        assert!(TaskStatusView::Complete.can_remove());
        assert!(TaskStatusView::Failed.can_remove());
        assert!(TaskStatusView::Removed.can_remove());
        assert!(!TaskStatusView::Unknown.can_remove());
    }

    #[test]
    fn speed_limit_field_parses_bytes_and_ki_mi_gi_suffixes() {
        assert_eq!(parse_speed_limit_field(""), Some(0));
        assert_eq!(parse_speed_limit_field("   "), Some(0));
        assert_eq!(parse_speed_limit_field("0"), Some(0));
        assert_eq!(parse_speed_limit_field("1048576"), Some(1_048_576));
        assert_eq!(parse_speed_limit_field("512K"), Some(512 * 1024));
        assert_eq!(parse_speed_limit_field("512k"), Some(512 * 1024));
        assert_eq!(parse_speed_limit_field("2M"), Some(2 * 1024 * 1024));
        assert_eq!(
            parse_speed_limit_field(" 3g "),
            Some(3 * 1024 * 1024 * 1024)
        );
        assert_eq!(parse_speed_limit_field("10 M"), Some(10 * 1024 * 1024));
    }

    #[test]
    fn speed_limit_field_rejects_malformed_and_overflowing_values() {
        assert_eq!(parse_speed_limit_field("abc"), None);
        assert_eq!(parse_speed_limit_field("K"), None);
        assert_eq!(parse_speed_limit_field("1.5M"), None);
        assert_eq!(parse_speed_limit_field("-5"), None);
        assert_eq!(parse_speed_limit_field("5MB"), None);
        // 18 EiB * 1024 overflows u64.
        assert_eq!(parse_speed_limit_field("18446744073709551615K"), None);
    }

    #[test]
    fn speed_limit_field_round_trips_through_the_compact_formatter() {
        assert_eq!(format_speed_limit_field(0), "");
        assert_eq!(format_speed_limit_field(2 * 1024 * 1024), "2M");
        assert_eq!(format_speed_limit_field(512 * 1024), "512K");
        assert_eq!(format_speed_limit_field(3 * 1024 * 1024 * 1024), "3G");
        // Not a clean unit multiple stays in raw bytes.
        assert_eq!(format_speed_limit_field(1_500_000), "1500000");
        for value in [0, 100, 1024, 2 * 1024 * 1024, 5 * 1024 * 1024 * 1024] {
            assert_eq!(
                parse_speed_limit_field(&format_speed_limit_field(value)),
                Some(value)
            );
        }
    }

    #[test]
    fn transfer_policy_view_parses_counts_and_size_with_aria2_ranges() {
        let mut view = TransferPolicySettingsView::default();
        assert!(view.is_valid());
        assert_eq!(view.parse_max_concurrent_downloads(), Some(5));
        assert_eq!(view.parse_max_connection_per_server(), Some(1));
        assert_eq!(view.parse_split(), Some(5));
        assert_eq!(view.parse_min_split_size(), Some(20 * 1024 * 1024));

        view.max_connection_per_server = "16".into();
        assert_eq!(view.parse_max_connection_per_server(), Some(16));
        view.max_connection_per_server = "17".into();
        assert_eq!(view.parse_max_connection_per_server(), None);
        view.max_connection_per_server = "0".into();
        assert_eq!(view.parse_max_connection_per_server(), None);

        view.min_split_size = "1M".into();
        assert_eq!(view.parse_min_split_size(), Some(1024 * 1024));
        view.min_split_size = "".into();
        assert_eq!(view.parse_min_split_size(), None);
        view.min_split_size = "0".into();
        assert_eq!(view.parse_min_split_size(), None);
    }
}
