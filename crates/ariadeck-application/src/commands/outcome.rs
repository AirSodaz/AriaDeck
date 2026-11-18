use ariadeck_domain::TaskIdentity;

use crate::{GatewayError, GatewayErrorKind};

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
    pub fn item_failure(item: CommandItem, error: ApplicationError) -> Self {
        Self::Failure {
            failed: vec![ItemFailure {
                item: Some(item),
                error,
            }],
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

    /// Task identities that completed successfully in this outcome.
    #[must_use]
    pub fn succeeded_tasks(&self) -> Vec<TaskIdentity> {
        let succeeded = match self {
            Self::Success { succeeded } | Self::PartialSuccess { succeeded, .. } => succeeded,
            Self::Failure { .. } => return Vec::new(),
        };
        succeeded
            .iter()
            .map(|item| match item {
                CommandItem::Task(identity) => *identity,
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationErrorCode {
    Validation,
    Duplicate,
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
            Self::Duplicate => "validation.duplicate_task",
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
