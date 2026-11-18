use super::{
    commands::OperationErrorView,
    task::{EngineSessionView, RequestId, TaskIdentity},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskDetailsRequestView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub identity: TaskIdentity,
    pub active: bool,
    pub is_bittorrent: bool,
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

    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::Used => "dialog-details-uri-used",
            Self::Waiting => "dialog-details-uri-waiting",
            Self::Unknown => "dialog-details-uri-unknown",
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
