use std::{collections::HashSet, path::PathBuf, sync::Arc};

use ariadeck_domain::{ProfileId, TaskIdentity};

use crate::{DownloadEngineGateway, GatewayError, GatewayErrorKind};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AddDownloadRequest {
    pub uris: Vec<String>,
    pub destination: Option<PathBuf>,
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
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoveTasksRequest {
    pub tasks: Vec<TaskIdentity>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    AddDownload(AddDownloadRequest),
    PauseTasks(Vec<TaskIdentity>),
    ResumeTasks(Vec<TaskIdentity>),
    RemoveTasks(RemoveTasksRequest),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationErrorCode {
    Validation,
    WrongProfile,
    Disconnected,
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
            Self::Disconnected => "rpc.disconnected",
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

    pub async fn execute(&self, command: AppCommand) -> CommandOutcome {
        match command {
            AppCommand::AddDownload(request) => self.add_download(request).await,
            AppCommand::PauseTasks(tasks) => self.execute_batch(tasks, TaskOperation::Pause).await,
            AppCommand::ResumeTasks(tasks) => {
                self.execute_batch(tasks, TaskOperation::Resume).await
            }
            AppCommand::RemoveTasks(request) => {
                self.execute_batch(request.tasks, TaskOperation::Remove)
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
                TaskOperation::Remove => self.gateway.remove(identity.gid).await,
            };
            match result {
                Ok(()) => succeeded.push(item),
                Err(error) => failed.push(ItemFailure {
                    item: Some(item),
                    error: error.into(),
                }),
            }
        }

        match (succeeded.is_empty(), failed.is_empty()) {
            (false, true) => CommandOutcome::Success { succeeded },
            (false, false) => CommandOutcome::PartialSuccess { succeeded, failed },
            (true, false) => CommandOutcome::Failure { failed },
            (true, true) => CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: None,
                    error: ApplicationError::new(
                        ApplicationErrorCode::Internal,
                        "The command produced no result.",
                        false,
                    ),
                }],
            },
        }
    }
}

#[derive(Clone, Copy)]
enum TaskOperation {
    Pause,
    Resume,
    Remove,
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use ariadeck_domain::Gid;
    use async_trait::async_trait;
    use futures::executor::block_on;

    use super::*;

    #[derive(Default)]
    struct FakeGateway {
        calls: Mutex<Vec<(TaskOperation, Gid)>>,
        fail_gid: Option<Gid>,
    }

    #[async_trait]
    impl DownloadEngineGateway for FakeGateway {
        async fn add_download(&self, _request: &AddDownloadRequest) -> Result<Gid, GatewayError> {
            Ok(Gid::from_u64(99))
        }

        async fn pause(&self, gid: Gid) -> Result<(), GatewayError> {
            self.record(TaskOperation::Pause, gid)
        }

        async fn resume(&self, gid: Gid) -> Result<(), GatewayError> {
            self.record(TaskOperation::Resume, gid)
        }

        async fn remove(&self, gid: Gid) -> Result<(), GatewayError> {
            self.record(TaskOperation::Remove, gid)
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
            calls: Mutex::default(),
            fail_gid: Some(Gid::from_u64(2)),
        });
        let service = CommandService::new(profile_id, gateway.clone());
        let one = TaskIdentity::new(profile_id, Gid::from_u64(1));
        let two = TaskIdentity::new(profile_id, Gid::from_u64(2));

        let outcome = block_on(service.execute(AppCommand::PauseTasks(vec![one, one, two])));

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

        let outcome = block_on(service.execute(AppCommand::RemoveTasks(RemoveTasksRequest {
            tasks: vec![foreign],
        })));

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
}
