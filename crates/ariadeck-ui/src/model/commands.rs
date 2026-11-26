use super::task::{EngineSessionView, RequestId, TaskIdentity};

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

    #[must_use]
    pub const fn progress_message_key(self) -> &'static str {
        match self {
            Self::PauseAll => "cmd-pause-all-progress",
            Self::ForcePauseAll => "cmd-force-pause-all-progress",
            Self::ResumeAll => "cmd-resume-all-progress",
        }
    }

    #[must_use]
    pub const fn success_message_key(self) -> &'static str {
        match self {
            Self::PauseAll => "cmd-pause-all-success",
            Self::ForcePauseAll => "cmd-force-pause-all-success",
            Self::ResumeAll => "cmd-resume-all-success",
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
    pub const fn progress_message_key(self) -> &'static str {
        match self {
            Self::Pause => "cmd-batch-pause-progress",
            Self::ForcePause => "cmd-batch-force-pause-progress",
            Self::Resume => "cmd-batch-resume-progress",
            Self::Retry => "cmd-batch-retry-progress",
            Self::RemoveTask => "cmd-batch-remove-progress",
            Self::ForceRemoveTask => "cmd-batch-force-remove-progress",
            Self::RemoveTaskAndFiles => "cmd-batch-remove-files-progress",
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
            Self::SetOptions { .. } => "Updating task options...",
            Self::RemoveTask => "Removing task...",
            Self::ForceRemoveTask => "Force-removing task...",
            Self::RemoveTaskAndFiles => {
                "Removing task and moving local files to the Recycle Bin..."
            }
        }
    }

    #[must_use]
    pub const fn progress_message_key(&self) -> &'static str {
        match self {
            Self::Pause => "cmd-pause-progress",
            Self::ForcePause => "cmd-force-pause-progress",
            Self::Resume => "cmd-resume-progress",
            Self::MoveToQueueTop => "cmd-queue-top-progress",
            Self::MoveUpInQueue => "cmd-queue-up-progress",
            Self::MoveDownInQueue => "cmd-queue-down-progress",
            Self::MoveToQueueBottom => "cmd-queue-bottom-progress",
            Self::Retry => "cmd-retry-progress",
            Self::SetOutputName { .. } => "cmd-output-name-progress",
            Self::SetSpeedLimit { .. } => "cmd-speed-limit-progress",
            Self::SetOptions { .. } => "cmd-options-progress",
            Self::RemoveTask => "cmd-remove-progress",
            Self::ForceRemoveTask => "cmd-force-remove-progress",
            Self::RemoveTaskAndFiles => "cmd-remove-files-progress",
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
            Self::SetOptions { .. } => "Task options updated.",
            Self::RemoveTask => "Task removed from aria2; downloaded files were kept.",
            Self::ForceRemoveTask => "Task force-removed from aria2; downloaded files were kept.",
            Self::RemoveTaskAndFiles => "Task removed; local files were moved to the Recycle Bin.",
        }
    }

    #[must_use]
    pub const fn success_message_key(&self) -> &'static str {
        match self {
            Self::Pause => "cmd-pause-success",
            Self::ForcePause => "cmd-force-pause-success",
            Self::Resume => "cmd-resume-success",
            Self::MoveToQueueTop => "cmd-queue-top-success",
            Self::MoveUpInQueue => "cmd-queue-up-success",
            Self::MoveDownInQueue => "cmd-queue-down-success",
            Self::MoveToQueueBottom => "cmd-queue-bottom-success",
            Self::Retry => "cmd-retry-success",
            Self::SetOutputName { .. } => "cmd-output-name-success",
            Self::SetSpeedLimit { .. } => "cmd-speed-limit-success",
            Self::SetOptions { .. } => "cmd-options-success",
            Self::RemoveTask => "cmd-remove-success",
            Self::ForceRemoveTask => "cmd-force-remove-success",
            Self::RemoveTaskAndFiles => "cmd-remove-files-success",
        }
    }
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

    /// Map stable application error codes to Fluent message ids.
    #[must_use]
    pub fn message_key(&self) -> String {
        format!("error-{}", self.code.replace(['.', '_'], "-"))
    }

    /// Localized summary: catalog translation when present, else engine/app English summary.
    #[must_use]
    pub fn localized_summary(&self, translator: &crate::Translator) -> String {
        let key = self.message_key();
        let translated = translator.t(&key);
        if translated == key {
            self.summary.clone()
        } else {
            translated
        }
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TaskEditableStateView {
    pub uris: Vec<String>,
    pub referer: String,
    pub user_agent: String,
    pub headers: String,
    pub proxy: String,
    pub max_download_limit: String,
    pub max_connection_per_server: String,
    pub split: String,
    pub out: String,
    pub dir: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareEditTaskRequestView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub identity: TaskIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareEditTaskResultView {
    pub request_id: RequestId,
    pub identity: TaskIdentity,
    pub outcome: Result<TaskEditableStateView, OperationErrorView>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelinkEditTaskRequestView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub identity: TaskIdentity,
    pub original_primary_uri: Option<String>,
    pub expected_total_bytes: Option<u64>,
    pub expected_filename: Option<String>,
    pub new_url: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub headers: Option<String>,
    pub proxy: Option<String>,
    pub max_download_limit: Option<String>,
    pub max_connection_per_server: Option<String>,
    pub split: Option<String>,
    pub out: Option<String>,
    pub allow_error_relink_resume: bool,
    pub force_mismatch: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RelinkEditOutcomeView {
    Succeeded(CommandOutcomeView),
    NeedsMismatchConfirmation {
        warning_message: String,
        probed_filename: Option<String>,
        probed_total_bytes: Option<u64>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelinkEditTaskResultView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub identity: TaskIdentity,
    pub outcome: RelinkEditOutcomeView,
}
