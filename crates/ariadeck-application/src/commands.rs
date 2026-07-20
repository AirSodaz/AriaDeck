use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
};

use ariadeck_domain::{
    DownloadStatus, EnginePath, ProfileId, TaskIdentity, TaskMetadata, TaskSourceKind,
};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::{DownloadEngineGateway, GatewayError, GatewayErrorKind, TaskRemovalTarget};

#[derive(Clone, Eq, PartialEq)]
pub enum AddDownloadSource {
    Uris(Vec<String>),
    Torrent(Arc<[u8]>),
    Metalink(Arc<[u8]>),
}

impl Default for AddDownloadSource {
    fn default() -> Self {
        Self::Uris(Vec::new())
    }
}

impl fmt::Debug for AddDownloadSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uris(uris) => formatter.debug_tuple("Uris").field(uris).finish(),
            Self::Torrent(content) => formatter
                .debug_struct("Torrent")
                .field("content_bytes", &content.len())
                .finish(),
            Self::Metalink(content) => formatter
                .debug_struct("Metalink")
                .field("content_bytes", &content.len())
                .finish(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AddDownloadRequest {
    pub source: AddDownloadSource,
    pub destination: Option<EnginePath>,
    pub file_conflict: FileConflictPolicy,
    pub selected_file_indices: Option<Vec<u32>>,
    pub options: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FileConflictPolicy {
    #[default]
    AutoRename,
    Reject,
    Overwrite,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DownloadProxyMode {
    #[default]
    Disabled,
    Manual,
}

#[derive(Clone)]
pub struct DownloadProxyConfig {
    pub mode: DownloadProxyMode,
    pub all_proxy: Option<String>,
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub ftp_proxy: Option<String>,
    pub no_proxy: Vec<String>,
    pub username: Option<String>,
    pub password: Option<SecretString>,
}

impl Default for DownloadProxyConfig {
    fn default() -> Self {
        Self {
            mode: DownloadProxyMode::Disabled,
            all_proxy: None,
            http_proxy: None,
            https_proxy: None,
            ftp_proxy: None,
            no_proxy: Vec::new(),
            username: None,
            password: None,
        }
    }
}

impl std::fmt::Debug for DownloadProxyConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DownloadProxyConfig")
            .field("mode", &self.mode)
            .field("all_proxy", &self.all_proxy)
            .field("http_proxy", &self.http_proxy)
            .field("https_proxy", &self.https_proxy)
            .field("ftp_proxy", &self.ftp_proxy)
            .field("no_proxy", &self.no_proxy)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

impl PartialEq for DownloadProxyConfig {
    fn eq(&self, other: &Self) -> bool {
        self.mode == other.mode
            && self.all_proxy == other.all_proxy
            && self.http_proxy == other.http_proxy
            && self.https_proxy == other.https_proxy
            && self.ftp_proxy == other.ftp_proxy
            && self.no_proxy == other.no_proxy
            && self.username == other.username
            && self.password.as_ref().map(ExposeSecret::expose_secret)
                == other.password.as_ref().map(ExposeSecret::expose_secret)
    }
}

impl Eq for DownloadProxyConfig {}

impl DownloadProxyConfig {
    fn validate(&self) -> Result<(), ApplicationError> {
        if self.mode == DownloadProxyMode::Manual
            && self.all_proxy.is_none()
            && self.http_proxy.is_none()
            && self.https_proxy.is_none()
            && self.ftp_proxy.is_none()
        {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "Manual download proxy requires at least one proxy endpoint.",
                false,
            ));
        }
        if self.password.is_some() && self.username.as_deref().is_none_or(str::is_empty) {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "A proxy password requires a non-empty username.",
                false,
            ));
        }
        Ok(())
    }
}

impl AddDownloadRequest {
    fn validate(&self) -> Result<(), ApplicationError> {
        let AddDownloadSource::Uris(uris) = &self.source else {
            let content = match &self.source {
                AddDownloadSource::Torrent(content) | AddDownloadSource::Metalink(content) => {
                    content
                }
                AddDownloadSource::Uris(_) => unreachable!(),
            };
            if content.is_empty() {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "Torrent or Metalink metadata must not be empty.",
                    false,
                ));
            }
            if let Some(indices) = &self.selected_file_indices
                && (indices.is_empty()
                    || indices.first() == Some(&0)
                    || indices.windows(2).any(|pair| pair[0] >= pair[1]))
            {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "Selected metadata file indexes must be non-empty, 1-based, unique, and sorted.",
                    false,
                ));
            }
            return Ok(());
        };
        if self.selected_file_indices.is_some() {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "File selection is supported only for Torrent or Metalink metadata.",
                false,
            ));
        }
        if uris.is_empty() || uris.iter().any(|uri| uri.trim().is_empty()) {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "At least one non-empty URL or magnet link is required.",
                false,
            ));
        }
        let mut unique = HashSet::new();
        for uri in uris {
            let uri = uri.trim();
            if !unique.insert(uri) {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    format!("Duplicate download URI: {uri}"),
                    false,
                ));
            }
            let parsed = Url::parse(uri).map_err(|error| {
                ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    format!("Invalid download URI: {error}"),
                    false,
                )
            })?;
            if !matches!(
                parsed.scheme(),
                "http" | "https" | "ftp" | "sftp" | "magnet"
            ) {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    format!("Unsupported download URI scheme: {}", parsed.scheme()),
                    false,
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoveTasksRequest {
    pub tasks: Vec<TaskIdentity>,
    pub scope: TaskRemovalScope,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetTaskOutputNameRequest {
    pub task: TaskIdentity,
    pub output_name: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum QueueMove {
    Top,
    Up,
    Down,
    Bottom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MoveTaskInQueueRequest {
    pub task: TaskIdentity,
    pub movement: QueueMove,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskRemovalScope {
    TaskOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    AddDownload(AddDownloadRequest),
    PauseAll,
    ResumeAll,
    PauseTasks(Vec<TaskIdentity>),
    ResumeTasks(Vec<TaskIdentity>),
    MoveTaskInQueue(MoveTaskInQueueRequest),
    RetryTasks(Vec<TaskIdentity>),
    SetTaskOutputName(SetTaskOutputNameRequest),
    RemoveTasks(RemoveTasksRequest),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskCommandContext {
    pub status: DownloadStatus,
    pub metadata: TaskMetadata,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CommandItem {
    Task(TaskIdentity),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemFailure {
    pub item: Option<CommandItem>,
    pub error: ApplicationError,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandOutcome {
    Success {
        succeeded: Vec<CommandItem>,
    },
    PartialSuccess {
        succeeded: Vec<CommandItem>,
        failed: Vec<ItemFailure>,
    },
    Failure {
        failed: Vec<ItemFailure>,
    },
}

impl CommandOutcome {
    #[must_use]
    pub fn failure(error: ApplicationError) -> Self {
        Self::Failure {
            failed: vec![ItemFailure { item: None, error }],
        }
    }

    #[must_use]
    pub fn has_successes(&self) -> bool {
        match self {
            Self::Success { .. } => true,
            Self::PartialSuccess { succeeded, .. } => !succeeded.is_empty(),
            Self::Failure { .. } => false,
        }
    }

    #[must_use]
    pub fn has_unknown_outcome(&self) -> bool {
        let failures = match self {
            Self::Success { .. } => return false,
            Self::PartialSuccess { failed, .. } | Self::Failure { failed } => failed,
        };
        failures
            .iter()
            .any(|failure| failure.error.code == ApplicationErrorCode::OutcomeUnknown)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationErrorCode {
    Validation,
    WrongProfile,
    StaleSession,
    Disconnected,
    OutcomeUnknown,
    NotObserved,
    RetryNotObserved,
    RemovalNotObserved,
    Authentication,
    Timeout,
    Rejected,
    Unsupported,
    UnsafePath,
    Filesystem,
    Internal,
}

impl ApplicationErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Validation => "validation.invalid_request",
            Self::WrongProfile => "command.wrong_profile",
            Self::StaleSession => "command.stale_session",
            Self::Disconnected => "rpc.disconnected",
            Self::OutcomeUnknown => "rpc.command_outcome_unknown",
            Self::NotObserved => "rpc.add_not_observed",
            Self::RetryNotObserved => "rpc.retry_not_observed",
            Self::RemovalNotObserved => "rpc.remove_not_observed",
            Self::Authentication => "rpc.authentication_failed",
            Self::Timeout => "rpc.timeout",
            Self::Rejected => "rpc.command_rejected",
            Self::Unsupported => "command.unsupported",
            Self::UnsafePath => "filesystem.unsafe_path",
            Self::Filesystem => "filesystem.operation_failed",
            Self::Internal => "application.internal",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationError {
    pub code: ApplicationErrorCode,
    pub summary: String,
    pub retryable: bool,
}

impl ApplicationError {
    #[must_use]
    pub fn new(code: ApplicationErrorCode, summary: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            summary: summary.into(),
            retryable,
        }
    }
}

impl From<GatewayError> for ApplicationError {
    fn from(error: GatewayError) -> Self {
        let code = match error.kind {
            GatewayErrorKind::Disconnected => ApplicationErrorCode::Disconnected,
            GatewayErrorKind::OutcomeUnknown => ApplicationErrorCode::OutcomeUnknown,
            GatewayErrorKind::Authentication => ApplicationErrorCode::Authentication,
            GatewayErrorKind::Timeout => ApplicationErrorCode::Timeout,
            GatewayErrorKind::Rejected => ApplicationErrorCode::Rejected,
            GatewayErrorKind::Unsupported => ApplicationErrorCode::Unsupported,
            GatewayErrorKind::UnsafePath => ApplicationErrorCode::UnsafePath,
            GatewayErrorKind::Filesystem => ApplicationErrorCode::Filesystem,
            GatewayErrorKind::Internal => ApplicationErrorCode::Internal,
        };
        Self::new(code, error.message, error.retryable)
    }
}

pub struct CommandService {
    profile_id: ProfileId,
    gateway: Arc<dyn DownloadEngineGateway>,
}

impl CommandService {
    #[must_use]
    pub fn new(profile_id: ProfileId, gateway: Arc<dyn DownloadEngineGateway>) -> Self {
        Self {
            profile_id,
            gateway,
        }
    }

    pub async fn execute(
        &self,
        command: AppCommand,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        match command {
            AppCommand::AddDownload(request) => self.add_download(request).await,
            AppCommand::PauseAll => self.execute_global(GlobalTaskOperation::PauseAll).await,
            AppCommand::ResumeAll => self.execute_global(GlobalTaskOperation::ResumeAll).await,
            AppCommand::PauseTasks(tasks) => {
                self.execute_batch(tasks, TaskOperation::Pause, task_contexts)
                    .await
            }
            AppCommand::ResumeTasks(tasks) => {
                self.execute_batch(tasks, TaskOperation::Resume, task_contexts)
                    .await
            }
            AppCommand::MoveTaskInQueue(request) => {
                self.execute_batch(
                    vec![request.task],
                    TaskOperation::MoveInQueue(request.movement),
                    task_contexts,
                )
                .await
            }
            AppCommand::RetryTasks(tasks) => self.retry_tasks(tasks, task_contexts).await,
            AppCommand::SetTaskOutputName(request) => {
                self.set_task_output_name(request, task_contexts).await
            }
            AppCommand::RemoveTasks(request) => {
                self.execute_batch(
                    request.tasks,
                    TaskOperation::Remove(request.scope),
                    task_contexts,
                )
                .await
            }
        }
    }

    pub async fn apply_download_proxy(
        &self,
        config: &DownloadProxyConfig,
    ) -> Result<(), ApplicationError> {
        config.validate()?;
        self.gateway
            .apply_download_proxy(config)
            .await
            .map_err(Into::into)
    }

    async fn add_download(&self, request: AddDownloadRequest) -> CommandOutcome {
        if let Err(error) = request.validate() {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure { item: None, error }],
            };
        }

        match self.gateway.add_download(&request).await {
            Ok(gids) if gids.is_empty() => CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: None,
                    error: ApplicationError::new(
                        ApplicationErrorCode::Internal,
                        "Download engine accepted the request without returning a task ID.",
                        true,
                    ),
                }],
            },
            Ok(gids) => CommandOutcome::Success {
                succeeded: gids
                    .into_iter()
                    .map(|gid| CommandItem::Task(TaskIdentity::new(self.profile_id, gid)))
                    .collect(),
            },
            Err(error) => CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: None,
                    error: error.into(),
                }],
            },
        }
    }

    async fn execute_global(&self, operation: GlobalTaskOperation) -> CommandOutcome {
        let result = match operation {
            GlobalTaskOperation::PauseAll => self.gateway.pause_all().await,
            GlobalTaskOperation::ResumeAll => self.gateway.resume_all().await,
        };
        match result {
            Ok(()) => CommandOutcome::Success {
                succeeded: Vec::new(),
            },
            Err(error) => CommandOutcome::failure(error.into()),
        }
    }

    async fn set_task_output_name(
        &self,
        request: SetTaskOutputNameRequest,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        let item = CommandItem::Task(request.task);
        if request.task.profile_id != self.profile_id {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::WrongProfile,
                        "The task belongs to a different engine profile.",
                        false,
                    ),
                }],
            };
        }
        let Some(context) = task_contexts.get(&request.task) else {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "The task is no longer present in the current engine session.",
                        false,
                    ),
                }],
            };
        };
        let output_name = request.output_name.trim();
        if output_name.is_empty()
            || output_name == "."
            || output_name == ".."
            || output_name.contains(['/', '\\', '\0'])
        {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        "Output name must be a non-empty file name without path separators.",
                        false,
                    ),
                }],
            };
        }
        if context.metadata.source_kind != TaskSourceKind::DirectUri {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Unsupported,
                        "A custom output name is currently supported only for direct URI tasks.",
                        false,
                    ),
                }],
            };
        }
        if context.status.is_terminal() {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "A completed or failed task cannot change its output name.",
                        false,
                    ),
                }],
            };
        }

        let options = [("out".to_owned(), output_name.to_owned())];
        match self
            .gateway
            .change_options(request.task.gid, &options)
            .await
        {
            Ok(()) => CommandOutcome::Success {
                succeeded: vec![item],
            },
            Err(error) => CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: error.into(),
                }],
            },
        }
    }

    async fn execute_batch(
        &self,
        tasks: Vec<TaskIdentity>,
        operation: TaskOperation,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        if tasks.is_empty() {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: None,
                    error: ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        "At least one task must be selected.",
                        false,
                    ),
                }],
            };
        }

        let mut seen = HashSet::new();
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();
        for identity in tasks.into_iter().filter(|identity| seen.insert(*identity)) {
            let item = CommandItem::Task(identity);
            if identity.profile_id != self.profile_id {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::WrongProfile,
                        "The task belongs to a different engine profile.",
                        false,
                    ),
                });
                continue;
            }

            let Some(context) = task_contexts.get(&identity) else {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "The task is no longer present in the current engine session.",
                        false,
                    ),
                });
                continue;
            };
            let allowed = match operation {
                TaskOperation::Pause => matches!(
                    context.status,
                    DownloadStatus::Active | DownloadStatus::Waiting | DownloadStatus::Verifying
                ),
                TaskOperation::Resume => matches!(context.status, DownloadStatus::Paused),
                TaskOperation::MoveInQueue(_) => !matches!(
                    context.status,
                    DownloadStatus::Complete
                        | DownloadStatus::Error
                        | DownloadStatus::Removed
                        | DownloadStatus::Unknown
                ),
                TaskOperation::Remove(_) => !matches!(context.status, DownloadStatus::Unknown),
            };
            if !allowed {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        format!(
                            "{} is not available while the task is {:?}.",
                            task_operation_label(operation),
                            context.status
                        ),
                        false,
                    ),
                });
                continue;
            }

            let result = match operation {
                TaskOperation::Pause => self.gateway.pause(identity.gid).await,
                TaskOperation::Resume => self.gateway.resume(identity.gid).await,
                TaskOperation::MoveInQueue(movement) => {
                    self.gateway.move_in_queue(identity.gid, movement).await
                }
                TaskOperation::Remove(TaskRemovalScope::TaskOnly) => {
                    let target = if context.status.is_terminal() {
                        TaskRemovalTarget::DownloadResult
                    } else {
                        TaskRemovalTarget::LiveTask
                    };
                    self.gateway.remove(identity.gid, target).await
                }
            };
            match result {
                Ok(()) => succeeded.push(item),
                Err(error) => failed.push(ItemFailure {
                    item: Some(item),
                    error: error.into(),
                }),
            }
        }

        finish_batch(succeeded, failed)
    }

    async fn retry_tasks(
        &self,
        tasks: Vec<TaskIdentity>,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        if tasks.is_empty() {
            return CommandOutcome::failure(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "At least one failed task must be selected.",
                false,
            ));
        }

        let mut seen = HashSet::new();
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();
        for identity in tasks.into_iter().filter(|identity| seen.insert(*identity)) {
            let item = CommandItem::Task(identity);
            if identity.profile_id != self.profile_id {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::WrongProfile,
                        "The task belongs to a different engine profile.",
                        false,
                    ),
                });
                continue;
            }
            let Some(context) = task_contexts.get(&identity) else {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "The failed task is no longer present in the current engine session.",
                        false,
                    ),
                });
                continue;
            };
            if context.status != DownloadStatus::Error {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "Only failed tasks can be retried.",
                        false,
                    ),
                });
                continue;
            }
            let source = context.metadata.primary_uri.clone().or_else(|| {
                context
                    .metadata
                    .info_hash
                    .as_ref()
                    .map(|hash| format!("magnet:?xt=urn:btih:{hash}"))
            });
            let Some(source) = source else {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Unsupported,
                        "The task has no replayable URL, magnet link, or info hash.",
                        false,
                    ),
                });
                continue;
            };
            let request = AddDownloadRequest {
                source: AddDownloadSource::Uris(vec![source]),
                destination: context.metadata.directory.clone(),
                file_conflict: FileConflictPolicy::default(),
                selected_file_indices: None,
                options: Vec::new(),
            };
            match self.gateway.retry_download(identity.gid, &request).await {
                Ok(gid) => {
                    succeeded.push(CommandItem::Task(TaskIdentity::new(self.profile_id, gid)))
                }
                Err(error) => failed.push(ItemFailure {
                    item: Some(item),
                    error: error.into(),
                }),
            }
        }

        finish_batch(succeeded, failed)
    }
}

fn task_operation_label(operation: TaskOperation) -> &'static str {
    match operation {
        TaskOperation::Pause => "Pause",
        TaskOperation::Resume => "Resume",
        TaskOperation::MoveInQueue(_) => "Change queue priority",
        TaskOperation::Remove(_) => "Remove",
    }
}

fn finish_batch(succeeded: Vec<CommandItem>, failed: Vec<ItemFailure>) -> CommandOutcome {
    match (succeeded.is_empty(), failed.is_empty()) {
        (false, true) => CommandOutcome::Success { succeeded },
        (false, false) => CommandOutcome::PartialSuccess { succeeded, failed },
        (true, false) => CommandOutcome::Failure { failed },
        (true, true) => CommandOutcome::failure(ApplicationError::new(
            ApplicationErrorCode::Internal,
            "The command produced no result.",
            false,
        )),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TaskOperation {
    Pause,
    Resume,
    MoveInQueue(QueueMove),
    Remove(TaskRemovalScope),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GlobalTaskOperation {
    PauseAll,
    ResumeAll,
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use ariadeck_domain::Gid;
    use async_trait::async_trait;
    use futures::executor::block_on;

    use super::*;

    #[test]
    fn proxy_config_debug_output_redacts_the_password() {
        let config = DownloadProxyConfig {
            mode: DownloadProxyMode::Manual,
            all_proxy: Some("proxy.example:8080".into()),
            username: Some("proxy-user".into()),
            password: Some(SecretString::new("never-log-this".into())),
            ..DownloadProxyConfig::default()
        };

        let debug = format!("{config:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("never-log-this"));
    }

    #[test]
    fn command_outcome_detects_unknown_failures_in_full_and_partial_results() {
        let unknown = ItemFailure {
            item: None,
            error: ApplicationError::new(
                ApplicationErrorCode::OutcomeUnknown,
                "response lost",
                false,
            ),
        };
        assert!(
            CommandOutcome::Failure {
                failed: vec![unknown.clone()]
            }
            .has_unknown_outcome()
        );
        assert!(
            CommandOutcome::PartialSuccess {
                succeeded: vec![CommandItem::Task(TaskIdentity::new(
                    ProfileId::new(),
                    Gid::from_u64(1),
                ))],
                failed: vec![unknown],
            }
            .has_unknown_outcome()
        );
        assert!(
            !CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: None,
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "rejected",
                        false,
                    ),
                }],
            }
            .has_unknown_outcome()
        );
    }

    type ChangedOptionCall = (Gid, Vec<(String, String)>);

    #[derive(Default)]
    struct FakeGateway {
        adds: Mutex<Vec<AddDownloadRequest>>,
        add_gids: Mutex<Option<Vec<Gid>>>,
        retries: Mutex<Vec<(Gid, AddDownloadRequest)>>,
        calls: Mutex<Vec<(TaskOperation, Gid)>>,
        removals: Mutex<Vec<(Gid, TaskRemovalTarget)>>,
        changed_options: Mutex<Vec<ChangedOptionCall>>,
        queue_moves: Mutex<Vec<(Gid, QueueMove)>>,
        fail_gid: Option<Gid>,
    }

    #[async_trait]
    impl DownloadEngineGateway for FakeGateway {
        async fn add_download(
            &self,
            request: &AddDownloadRequest,
        ) -> Result<Vec<Gid>, GatewayError> {
            self.adds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(request.clone());
            let gids = self
                .add_gids
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            Ok(gids.unwrap_or_else(|| vec![Gid::from_u64(99)]))
        }

        async fn retry_download(
            &self,
            gid: Gid,
            fallback: &AddDownloadRequest,
        ) -> Result<Gid, GatewayError> {
            self.retries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((gid, fallback.clone()));
            Ok(Gid::from_u64(99))
        }

        async fn pause(&self, gid: Gid) -> Result<(), GatewayError> {
            self.record(TaskOperation::Pause, gid)
        }

        async fn resume(&self, gid: Gid) -> Result<(), GatewayError> {
            self.record(TaskOperation::Resume, gid)
        }

        async fn move_in_queue(&self, gid: Gid, movement: QueueMove) -> Result<(), GatewayError> {
            self.queue_moves
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((gid, movement));
            self.record(TaskOperation::MoveInQueue(movement), gid)
        }

        async fn change_options(
            &self,
            gid: Gid,
            options: &[(String, String)],
        ) -> Result<(), GatewayError> {
            self.changed_options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((gid, options.to_vec()));
            Ok(())
        }

        async fn remove(&self, gid: Gid, target: TaskRemovalTarget) -> Result<(), GatewayError> {
            self.removals
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((gid, target));
            self.record(TaskOperation::Remove(TaskRemovalScope::TaskOnly), gid)
        }
    }

    impl FakeGateway {
        fn record(&self, operation: TaskOperation, gid: Gid) -> Result<(), GatewayError> {
            self.calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((operation, gid));
            if self.fail_gid == Some(gid) {
                Err(GatewayError::new(
                    GatewayErrorKind::Rejected,
                    "aria2 rejected the command",
                    false,
                ))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn batch_command_reports_partial_success_and_deduplicates() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway {
            adds: Mutex::default(),
            add_gids: Mutex::default(),
            retries: Mutex::default(),
            calls: Mutex::default(),
            removals: Mutex::default(),
            changed_options: Mutex::default(),
            queue_moves: Mutex::default(),
            fail_gid: Some(Gid::from_u64(2)),
        });
        let service = CommandService::new(profile_id, gateway.clone());
        let one = TaskIdentity::new(profile_id, Gid::from_u64(1));
        let two = TaskIdentity::new(profile_id, Gid::from_u64(2));
        let contexts = HashMap::from([
            (
                one,
                TaskCommandContext {
                    status: DownloadStatus::Active,
                    metadata: TaskMetadata::default(),
                },
            ),
            (
                two,
                TaskCommandContext {
                    status: DownloadStatus::Active,
                    metadata: TaskMetadata::default(),
                },
            ),
        ]);

        let outcome =
            block_on(service.execute(AppCommand::PauseTasks(vec![one, one, two]), &contexts));

        let CommandOutcome::PartialSuccess { succeeded, failed } = outcome else {
            panic!("expected partial success");
        };
        assert_eq!(succeeded, vec![CommandItem::Task(one)]);
        assert_eq!(failed.len(), 1);
        assert_eq!(
            gateway
                .calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            2
        );
    }

    #[test]
    fn batch_command_skips_ineligible_tasks_without_calling_the_gateway() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new(profile_id, gateway.clone());
        let active = TaskIdentity::new(profile_id, Gid::from_u64(1));
        let complete = TaskIdentity::new(profile_id, Gid::from_u64(2));
        let contexts = HashMap::from([
            (
                active,
                TaskCommandContext {
                    status: DownloadStatus::Active,
                    metadata: TaskMetadata::default(),
                },
            ),
            (
                complete,
                TaskCommandContext {
                    status: DownloadStatus::Complete,
                    metadata: TaskMetadata::default(),
                },
            ),
        ]);

        let outcome =
            block_on(service.execute(AppCommand::PauseTasks(vec![active, complete]), &contexts));

        let CommandOutcome::PartialSuccess { succeeded, failed } = outcome else {
            panic!("expected partial success");
        };
        assert_eq!(succeeded, vec![CommandItem::Task(active)]);
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].item, Some(CommandItem::Task(complete)));
        assert_eq!(failed[0].error.code, ApplicationErrorCode::Rejected);
        assert_eq!(
            gateway
                .calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_slice(),
            &[(TaskOperation::Pause, active.gid)]
        );
    }

    #[test]
    fn wrong_profile_is_rejected_before_gateway_call() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new(profile_id, gateway.clone());
        let foreign = TaskIdentity::new(ProfileId::new(), Gid::from_u64(1));

        let outcome = block_on(service.execute(
            AppCommand::RemoveTasks(RemoveTasksRequest {
                tasks: vec![foreign],
                scope: TaskRemovalScope::TaskOnly,
            }),
            &HashMap::new(),
        ));

        let CommandOutcome::Failure { failed } = outcome else {
            panic!("expected command failure");
        };
        assert_eq!(failed[0].error.code, ApplicationErrorCode::WrongProfile);
        assert!(
            gateway
                .calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn add_download_rejects_unsupported_or_malformed_uris() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new(profile_id, gateway);

        for uri in ["not a uri", "file:///tmp/item.bin", "javascript:alert(1)"] {
            let outcome = block_on(service.execute(
                AppCommand::AddDownload(AddDownloadRequest {
                    source: AddDownloadSource::Uris(vec![uri.into()]),
                    destination: None,
                    file_conflict: FileConflictPolicy::default(),
                    selected_file_indices: None,
                    options: Vec::new(),
                }),
                &HashMap::new(),
            ));
            let CommandOutcome::Failure { failed } = outcome else {
                panic!("expected URI validation failure for {uri}");
            };
            assert_eq!(failed[0].error.code, ApplicationErrorCode::Validation);
        }
    }

    #[test]
    fn add_download_rejects_duplicate_mirror_sources_before_gateway_call() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new(profile_id, gateway.clone());

        let outcome = block_on(service.execute(
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Uris(vec![
                    "https://example.test/archive.iso".into(),
                    "https://example.test/archive.iso".into(),
                ]),
                destination: None,
                file_conflict: FileConflictPolicy::default(),
                selected_file_indices: None,
                options: Vec::new(),
            }),
            &HashMap::new(),
        ));

        let CommandOutcome::Failure { failed } = outcome else {
            panic!("expected duplicate mirror validation failure");
        };
        assert_eq!(failed[0].error.code, ApplicationErrorCode::Validation);
        assert!(
            gateway
                .adds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn add_download_rejects_empty_metadata_before_gateway_call() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new(profile_id, gateway.clone());

        let outcome = block_on(service.execute(
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Metalink(Arc::<[u8]>::from(&b""[..])),
                destination: None,
                file_conflict: FileConflictPolicy::Reject,
                selected_file_indices: None,
                options: Vec::new(),
            }),
            &HashMap::new(),
        ));

        let CommandOutcome::Failure { failed } = outcome else {
            panic!("expected empty metadata validation failure");
        };
        assert_eq!(failed[0].error.code, ApplicationErrorCode::Validation);
        assert!(
            gateway
                .adds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn add_download_rejects_invalid_or_non_metadata_file_selection() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new(profile_id, gateway.clone());

        for selected_file_indices in [
            Some(Vec::new()),
            Some(vec![0]),
            Some(vec![1, 1]),
            Some(vec![2, 1]),
        ] {
            let outcome = block_on(service.execute(
                AppCommand::AddDownload(AddDownloadRequest {
                    source: AddDownloadSource::Torrent(Arc::<[u8]>::from(&b"metadata"[..])),
                    destination: None,
                    file_conflict: FileConflictPolicy::Reject,
                    selected_file_indices,
                    options: Vec::new(),
                }),
                &HashMap::new(),
            ));
            let CommandOutcome::Failure { failed } = outcome else {
                panic!("expected metadata selection validation failure");
            };
            assert_eq!(failed[0].error.code, ApplicationErrorCode::Validation);
        }

        let uri_outcome = block_on(service.execute(
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Uris(vec!["https://example.test/archive.bin".into()]),
                destination: None,
                file_conflict: FileConflictPolicy::Reject,
                selected_file_indices: Some(vec![1]),
                options: Vec::new(),
            }),
            &HashMap::new(),
        ));
        let CommandOutcome::Failure { failed } = uri_outcome else {
            panic!("expected URI file selection validation failure");
        };
        assert_eq!(failed[0].error.code, ApplicationErrorCode::Validation);
        assert!(
            gateway
                .adds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn add_download_preserves_multiple_gateway_gids() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        *gateway
            .add_gids
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            Some(vec![Gid::from_u64(11), Gid::from_u64(12)]);
        let service = CommandService::new(profile_id, gateway);

        let outcome = block_on(service.execute(
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Metalink(Arc::<[u8]>::from(&b"metadata"[..])),
                destination: None,
                file_conflict: FileConflictPolicy::Reject,
                selected_file_indices: None,
                options: Vec::new(),
            }),
            &HashMap::new(),
        ));

        let CommandOutcome::Success { succeeded } = outcome else {
            panic!("expected metadata add success");
        };
        let gids = succeeded
            .into_iter()
            .map(|item| match item {
                CommandItem::Task(identity) => identity.gid,
            })
            .collect::<Vec<_>>();
        assert_eq!(gids, vec![Gid::from_u64(11), Gid::from_u64(12)]);
    }

    #[test]
    fn add_download_rejects_an_empty_gateway_result() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        *gateway
            .add_gids
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Vec::new());
        let service = CommandService::new(profile_id, gateway);

        let outcome = block_on(service.execute(
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Metalink(Arc::<[u8]>::from(&b"metadata"[..])),
                destination: None,
                file_conflict: FileConflictPolicy::Reject,
                selected_file_indices: None,
                options: Vec::new(),
            }),
            &HashMap::new(),
        ));

        let CommandOutcome::Failure { failed } = outcome else {
            panic!("expected empty gateway result failure");
        };
        assert_eq!(failed[0].error.code, ApplicationErrorCode::Internal);
        assert!(failed[0].error.retryable);
    }

    #[test]
    fn removal_targets_live_tasks_and_terminal_results_separately() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new(profile_id, gateway.clone());
        let live = TaskIdentity::new(profile_id, Gid::from_u64(1));
        let complete = TaskIdentity::new(profile_id, Gid::from_u64(2));
        let removed = TaskIdentity::new(profile_id, Gid::from_u64(3));

        let contexts = HashMap::from([
            (
                live,
                TaskCommandContext {
                    status: DownloadStatus::Active,
                    metadata: TaskMetadata::default(),
                },
            ),
            (
                complete,
                TaskCommandContext {
                    status: DownloadStatus::Complete,
                    metadata: TaskMetadata::default(),
                },
            ),
            (
                removed,
                TaskCommandContext {
                    status: DownloadStatus::Removed,
                    metadata: TaskMetadata::default(),
                },
            ),
        ]);
        let outcome = block_on(service.execute(
            AppCommand::RemoveTasks(RemoveTasksRequest {
                tasks: vec![live, complete, removed],
                scope: TaskRemovalScope::TaskOnly,
            }),
            &contexts,
        ));

        assert!(matches!(outcome, CommandOutcome::Success { .. }));
        assert_eq!(
            *gateway
                .removals
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![
                (live.gid, TaskRemovalTarget::LiveTask),
                (complete.gid, TaskRemovalTarget::DownloadResult),
                (removed.gid, TaskRemovalTarget::DownloadResult),
            ]
        );
    }

    #[test]
    fn queue_move_dispatches_to_the_gateway_for_a_live_task() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new(profile_id, gateway.clone());
        let waiting = TaskIdentity::new(profile_id, Gid::from_u64(4));
        let contexts = HashMap::from([(
            waiting,
            TaskCommandContext {
                status: DownloadStatus::Waiting,
                metadata: TaskMetadata::default(),
            },
        )]);

        let outcome = block_on(service.execute(
            AppCommand::MoveTaskInQueue(MoveTaskInQueueRequest {
                task: waiting,
                movement: QueueMove::Top,
            }),
            &contexts,
        ));

        assert_eq!(
            outcome,
            CommandOutcome::Success {
                succeeded: vec![CommandItem::Task(waiting)],
            }
        );
        assert_eq!(
            *gateway
                .queue_moves
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![(waiting.gid, QueueMove::Top)]
        );
    }

    #[test]
    fn queue_move_is_rejected_for_terminal_tasks_before_the_gateway() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new(profile_id, gateway.clone());
        let complete = TaskIdentity::new(profile_id, Gid::from_u64(5));
        let contexts = HashMap::from([(
            complete,
            TaskCommandContext {
                status: DownloadStatus::Complete,
                metadata: TaskMetadata::default(),
            },
        )]);

        let outcome = block_on(service.execute(
            AppCommand::MoveTaskInQueue(MoveTaskInQueueRequest {
                task: complete,
                movement: QueueMove::Up,
            }),
            &contexts,
        ));

        let CommandOutcome::Failure { failed } = outcome else {
            panic!("expected queue-move rejection for a terminal task");
        };
        assert_eq!(failed[0].error.code, ApplicationErrorCode::Rejected);
        assert!(
            gateway
                .queue_moves
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn retry_creates_a_new_task_from_the_known_source_and_destination() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new(profile_id, gateway.clone());
        let failed_task = TaskIdentity::new(profile_id, Gid::from_u64(7));
        let contexts = HashMap::from([(
            failed_task,
            TaskCommandContext {
                status: DownloadStatus::Error,
                metadata: TaskMetadata {
                    directory: Some(EnginePath::new("/downloads")),
                    primary_uri: Some("https://example.test/archive.iso".into()),
                    ..TaskMetadata::default()
                },
            },
        )]);

        let outcome =
            block_on(service.execute(AppCommand::RetryTasks(vec![failed_task]), &contexts));

        assert_eq!(
            outcome,
            CommandOutcome::Success {
                succeeded: vec![CommandItem::Task(TaskIdentity::new(
                    profile_id,
                    Gid::from_u64(99),
                ))],
            }
        );
        assert_eq!(
            *gateway
                .retries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![(
                failed_task.gid,
                AddDownloadRequest {
                    source: AddDownloadSource::Uris(vec![
                        "https://example.test/archive.iso".into(),
                    ]),
                    destination: Some(EnginePath::new("/downloads")),
                    file_conflict: FileConflictPolicy::default(),
                    selected_file_indices: None,
                    options: Vec::new(),
                }
            )]
        );
    }

    #[test]
    fn direct_uri_output_name_change_is_validated_and_forwarded() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new(profile_id, gateway.clone());
        let task = TaskIdentity::new(profile_id, Gid::from_u64(8));
        let contexts = HashMap::from([(
            task,
            TaskCommandContext {
                status: DownloadStatus::Waiting,
                metadata: TaskMetadata {
                    source_kind: TaskSourceKind::DirectUri,
                    ..TaskMetadata::default()
                },
            },
        )]);

        let outcome = block_on(service.execute(
            AppCommand::SetTaskOutputName(SetTaskOutputNameRequest {
                task,
                output_name: " renamed.iso ".into(),
            }),
            &contexts,
        ));

        assert_eq!(
            outcome,
            CommandOutcome::Success {
                succeeded: vec![CommandItem::Task(task)]
            }
        );
        assert_eq!(
            *gateway
                .changed_options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![(task.gid, vec![("out".into(), "renamed.iso".into())])]
        );
    }

    #[test]
    fn output_name_change_rejects_paths_and_non_uri_tasks() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new(profile_id, gateway.clone());
        let task = TaskIdentity::new(profile_id, Gid::from_u64(9));
        let contexts = HashMap::from([(
            task,
            TaskCommandContext {
                status: DownloadStatus::Waiting,
                metadata: TaskMetadata {
                    source_kind: TaskSourceKind::BitTorrent,
                    ..TaskMetadata::default()
                },
            },
        )]);

        for output_name in ["folder/file.iso", "archive.iso"] {
            let outcome = block_on(service.execute(
                AppCommand::SetTaskOutputName(SetTaskOutputNameRequest {
                    task,
                    output_name: output_name.into(),
                }),
                &contexts,
            ));
            assert!(matches!(outcome, CommandOutcome::Failure { .. }));
        }
        assert!(
            gateway
                .changed_options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn retry_rejects_non_failed_or_unreplayable_tasks_before_the_gateway() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new(profile_id, gateway.clone());
        let active = TaskIdentity::new(profile_id, Gid::from_u64(1));
        let missing_source = TaskIdentity::new(profile_id, Gid::from_u64(2));
        let contexts = HashMap::from([
            (
                active,
                TaskCommandContext {
                    status: DownloadStatus::Active,
                    metadata: TaskMetadata::default(),
                },
            ),
            (
                missing_source,
                TaskCommandContext {
                    status: DownloadStatus::Error,
                    metadata: TaskMetadata::default(),
                },
            ),
        ]);

        let outcome = block_on(service.execute(
            AppCommand::RetryTasks(vec![active, missing_source]),
            &contexts,
        ));
        let CommandOutcome::Failure { failed } = outcome else {
            panic!("expected retry failure");
        };
        assert_eq!(failed.len(), 2);
        assert_eq!(failed[0].error.code, ApplicationErrorCode::Rejected);
        assert_eq!(failed[1].error.code, ApplicationErrorCode::Unsupported);
        assert!(
            gateway
                .retries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }
}
