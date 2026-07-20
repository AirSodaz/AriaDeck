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

impl TaskStatusView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
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
        matches!(self, Self::Active | Self::Waiting | Self::Verifying)
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
            Self::Active | Self::Waiting | Self::Paused | Self::Verifying
        )
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadRowView {
    pub identity: TaskIdentity,
    pub display_name: String,
    pub name_state: TaskNameStateView,
    pub source_kind: TaskSourceKindView,
    pub followed_by: Vec<String>,
    pub belongs_to: Option<String>,
    pub status: TaskStatusView,
    pub error: Option<TaskErrorView>,
    pub total_bytes: u64,
    pub completed_bytes: u64,
    pub download_rate: u64,
    pub upload_rate: u64,
    pub eta_seconds: Option<u64>,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSnapshot {
    pub profile_id: String,
    pub session_id: String,
    pub generation: u64,
    pub source_revision: u64,
    pub connection: ConnectionView,
    pub stale: bool,
    pub download_rate: u64,
    pub upload_rate: u64,
    pub speed_history: Vec<SpeedSampleView>,
    pub counts: TaskCountsView,
    pub tasks: Vec<DownloadRowView>,
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
            download_rate: 0,
            upload_rate: 0,
            speed_history: Vec::new(),
            counts: TaskCountsView::default(),
            tasks: Vec::new(),
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
    Resume,
    MoveToQueueTop,
    MoveUpInQueue,
    MoveDownInQueue,
    MoveToQueueBottom,
    Retry,
    SetOutputName { output_name: String },
    RemoveTask,
    RemoveTaskAndFiles,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GlobalTaskCommandView {
    PauseAll,
    ResumeAll,
}

impl GlobalTaskCommandView {
    #[must_use]
    pub const fn progress_label(self) -> &'static str {
        match self {
            Self::PauseAll => "Pausing all tasks...",
            Self::ResumeAll => "Resuming all tasks...",
        }
    }

    #[must_use]
    pub const fn success_label(self) -> &'static str {
        match self {
            Self::PauseAll => "All eligible tasks paused.",
            Self::ResumeAll => "All paused tasks resumed.",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BatchTaskCommandView {
    Pause,
    Resume,
    Retry,
    RemoveTask,
    RemoveTaskAndFiles,
}

impl BatchTaskCommandView {
    #[must_use]
    pub const fn progress_label(self) -> &'static str {
        match self {
            Self::Pause => "Pausing selected tasks...",
            Self::Resume => "Resuming selected tasks...",
            Self::Retry => "Creating new tasks from selected failed tasks...",
            Self::RemoveTask => "Removing selected tasks...",
            Self::RemoveTaskAndFiles => {
                "Removing selected tasks and moving local files to the Recycle Bin..."
            }
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pause => "Pause",
            Self::Resume => "Resume",
            Self::Retry => "Retry",
            Self::RemoveTask => "Remove",
            Self::RemoveTaskAndFiles => "Remove with files",
        }
    }
}

impl TaskCommandView {
    #[must_use]
    pub const fn progress_label(&self) -> &'static str {
        match self {
            Self::Pause => "Pausing task...",
            Self::Resume => "Resuming task...",
            Self::MoveToQueueTop => "Moving task to the top of the queue...",
            Self::MoveUpInQueue => "Moving task up in the queue...",
            Self::MoveDownInQueue => "Moving task down in the queue...",
            Self::MoveToQueueBottom => "Moving task to the bottom of the queue...",
            Self::Retry => "Creating a new task from the failed source...",
            Self::SetOutputName { .. } => "Updating output name...",
            Self::RemoveTask => "Removing task...",
            Self::RemoveTaskAndFiles => {
                "Removing task and moving local files to the Recycle Bin..."
            }
        }
    }

    #[must_use]
    pub const fn success_label(&self) -> &'static str {
        match self {
            Self::Pause => "Task paused.",
            Self::Resume => "Task resumed.",
            Self::MoveToQueueTop => "Task moved to the top of the queue.",
            Self::MoveUpInQueue => "Task moved up in the queue.",
            Self::MoveDownInQueue => "Task moved down in the queue.",
            Self::MoveToQueueBottom => "Task moved to the bottom of the queue.",
            Self::Retry => "New retry task created; the failed result was kept.",
            Self::SetOutputName { .. } => "Output name updated.",
            Self::RemoveTask => "Task removed from aria2; downloaded files were kept.",
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadRequestView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub sources: Vec<AddDownloadSourceView>,
    pub mode: AddDownloadModeView,
    pub destination: Option<String>,
    pub required_bytes: Option<u64>,
    pub file_conflict: FileConflictPolicyView,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskDetailsView {
    pub directory: Option<String>,
    pub info_hash: Option<String>,
    pub piece_length: Option<u64>,
    pub piece_count: Option<u32>,
    pub files: Vec<TaskFileView>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaskDetailsOutcomeView {
    Ready(TaskDetailsView),
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsView {
    pub color_scheme: ColorSchemeView,
    pub download_directory: String,
    pub download_proxy: DownloadProxySettingsView,
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
            followed_by: Vec::new(),
            belongs_to: None,
            status: TaskStatusView::Active,
            error: None,
            total_bytes: 100,
            completed_bytes: 120,
            download_rate: 0,
            upload_rate: 0,
            eta_seconds: None,
            revision: 1,
        };

        assert_eq!(row.progress_basis_points(), Some(10_000));
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
    fn remove_is_available_for_live_tasks_and_stopped_results() {
        assert!(TaskStatusView::Active.can_remove());
        assert!(TaskStatusView::Complete.can_remove());
        assert!(TaskStatusView::Failed.can_remove());
        assert!(TaskStatusView::Removed.can_remove());
        assert!(!TaskStatusView::Unknown.can_remove());
    }
}
