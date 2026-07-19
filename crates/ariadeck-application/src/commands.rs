use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use ariadeck_domain::{DownloadStatus, EnginePath, ProfileId, TaskIdentity, TaskMetadata};
use url::Url;

use crate::{DownloadEngineGateway, GatewayError, GatewayErrorKind, TaskRemovalTarget};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AddDownloadRequest {
    pub uris: Vec<String>,
    pub destination: Option<EnginePath>,
    pub options: Vec<(String, String)>,
}

impl AddDownloadRequest {
    fn validate(&self) -> Result<(), ApplicationError> {
        if self.uris.is_empty() || self.uris.iter().any(|uri| uri.trim().is_empty()) {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "At least one non-empty URL or magnet link is required.",
                false,
            ));
        }
        for uri in &self.uris {
            let parsed = Url::parse(uri.trim()).map_err(|error| {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskRemovalScope {
    TaskOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    AddDownload(AddDownloadRequest),
    PauseTasks(Vec<TaskIdentity>),
    ResumeTasks(Vec<TaskIdentity>),
    RetryTasks(Vec<TaskIdentity>),
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
            Self::Success { succeeded } | Self::PartialSuccess { succeeded, .. } => {
                !succeeded.is_empty()
            }
            Self::Failure { .. } => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationErrorCode {
    Validation,
    WrongProfile,
    StaleSession,
    Disconnected,
    OutcomeUnknown,
    Authentication,
    Timeout,
    Rejected,
    Unsupported,
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
            Self::Authentication => "rpc.authentication_failed",
            Self::Timeout => "rpc.timeout",
            Self::Rejected => "rpc.command_rejected",
            Self::Unsupported => "command.unsupported",
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
            AppCommand::PauseTasks(tasks) => {
                self.execute_batch(tasks, TaskOperation::Pause, task_contexts)
                    .await
            }
            AppCommand::ResumeTasks(tasks) => {
                self.execute_batch(tasks, TaskOperation::Resume, task_contexts)
                    .await
            }
            AppCommand::RetryTasks(tasks) => self.retry_tasks(tasks, task_contexts).await,
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

    async fn add_download(&self, request: AddDownloadRequest) -> CommandOutcome {
        if let Err(error) = request.validate() {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure { item: None, error }],
            };
        }

        match self.gateway.add_download(&request).await {
            Ok(gid) => CommandOutcome::Success {
                succeeded: vec![CommandItem::Task(TaskIdentity::new(self.profile_id, gid))],
            },
            Err(error) => CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: None,
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

            let result = match operation {
                TaskOperation::Pause => self.gateway.pause(identity.gid).await,
                TaskOperation::Resume => self.gateway.resume(identity.gid).await,
                TaskOperation::Remove(TaskRemovalScope::TaskOnly) => {
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
                uris: vec![source],
                destination: context.metadata.directory.clone(),
                options: Vec::new(),
            };
            match self.gateway.add_download(&request).await {
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

#[derive(Clone, Copy)]
enum TaskOperation {
    Pause,
    Resume,
    Remove(TaskRemovalScope),
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use ariadeck_domain::Gid;
    use async_trait::async_trait;
    use futures::executor::block_on;

    use super::*;

    #[derive(Default)]
    struct FakeGateway {
        adds: Mutex<Vec<AddDownloadRequest>>,
        calls: Mutex<Vec<(TaskOperation, Gid)>>,
        removals: Mutex<Vec<(Gid, TaskRemovalTarget)>>,
        fail_gid: Option<Gid>,
    }

    #[async_trait]
    impl DownloadEngineGateway for FakeGateway {
        async fn add_download(&self, request: &AddDownloadRequest) -> Result<Gid, GatewayError> {
            self.adds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(request.clone());
            Ok(Gid::from_u64(99))
        }

        async fn pause(&self, gid: Gid) -> Result<(), GatewayError> {
            self.record(TaskOperation::Pause, gid)
        }

        async fn resume(&self, gid: Gid) -> Result<(), GatewayError> {
            self.record(TaskOperation::Resume, gid)
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
            calls: Mutex::default(),
            removals: Mutex::default(),
            fail_gid: Some(Gid::from_u64(2)),
        });
        let service = CommandService::new(profile_id, gateway.clone());
        let one = TaskIdentity::new(profile_id, Gid::from_u64(1));
        let two = TaskIdentity::new(profile_id, Gid::from_u64(2));

        let outcome =
            block_on(service.execute(AppCommand::PauseTasks(vec![one, one, two]), &HashMap::new()));

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
                    uris: vec![uri.into()],
                    destination: None,
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
    fn removal_targets_live_tasks_and_terminal_results_separately() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new(profile_id, gateway.clone());
        let live = TaskIdentity::new(profile_id, Gid::from_u64(1));
        let complete = TaskIdentity::new(profile_id, Gid::from_u64(2));

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
        ]);
        let outcome = block_on(service.execute(
            AppCommand::RemoveTasks(RemoveTasksRequest {
                tasks: vec![live, complete],
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
            ]
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
                .adds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![AddDownloadRequest {
                uris: vec!["https://example.test/archive.iso".into()],
                destination: Some(EnginePath::new("/downloads")),
                options: Vec::new(),
            }]
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
                .adds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }
}
