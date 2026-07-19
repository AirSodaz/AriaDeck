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
        !matches!(self, Self::Removed)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadRowView {
    pub identity: TaskIdentity,
    pub display_name: String,
    pub status: TaskStatusView,
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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TaskCommandView {
    Pause,
    Resume,
    Retry,
    RemoveTask,
}

impl TaskCommandView {
    #[must_use]
    pub const fn progress_label(self) -> &'static str {
        match self {
            Self::Pause => "Pausing task...",
            Self::Resume => "Resuming task...",
            Self::Retry => "Creating a new task from the failed source...",
            Self::RemoveTask => "Removing task...",
        }
    }

    #[must_use]
    pub const fn success_label(self) -> &'static str {
        match self {
            Self::Pause => "Task paused.",
            Self::Resume => "Task resumed.",
            Self::Retry => "Retry task accepted by aria2.",
            Self::RemoveTask => "Task removed from aria2.",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadRequestView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub uri: String,
    pub destination: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskCommandRequestView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub identity: TaskIdentity,
    pub command: TaskCommandView,
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
    Success { task: Option<TaskIdentity> },
    Failure(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadResultView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub outcome: CommandOutcomeView,
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SettingsView {
    pub color_scheme: ColorSchemeView,
    pub download_directory: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsSaveRequestView {
    pub request_id: RequestId,
    pub settings: SettingsView,
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
    fn compact_transfer_formatting_is_stable_at_unit_boundaries() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1_536), "1.5 KiB");
        assert_eq!(format_rate(1_048_576), "1.0 MiB/s");
        assert_eq!(format_eta(Some(3_661)), "1h 1m");
        assert_eq!(format_percent(Some(5_050)), "50.5%");
    }

    #[test]
    fn progress_clamps_overreported_completion() {
        let row = DownloadRowView {
            identity: TaskIdentity {
                profile_id: "profile".into(),
                gid: "gid".into(),
            },
            display_name: "archive".into(),
            status: TaskStatusView::Active,
            total_bytes: 100,
            completed_bytes: 120,
            download_rate: 0,
            upload_rate: 0,
            eta_seconds: None,
            revision: 1,
        };

        assert_eq!(row.progress_basis_points(), Some(10_000));
    }

    #[test]
    fn retry_is_available_only_for_failed_tasks() {
        assert!(TaskStatusView::Failed.can_retry());
        assert!(!TaskStatusView::Paused.can_retry());
        assert!(!TaskStatusView::Complete.can_retry());
    }
}
