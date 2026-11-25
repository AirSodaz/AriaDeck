use super::{service::TaskOperation, *};
use crate::{
    DownloadEngineGateway, EngineCapabilities, GatewayError, GatewayErrorKind, TaskRemovalTarget,
};
use ariadeck_domain::{
    DownloadStatus, EnginePath, Gid, ProfileId, SpeedLimitConfig, TaskConnectionPolicy,
    TaskIdentity, TaskMetadata, TaskSourceKind, TransferPolicyConfig,
};
use async_trait::async_trait;
use futures::executor::block_on;
use secrecy::SecretString;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[test]
fn proxy_config_debug_output_redacts_the_password() {
    let config = DownloadProxyConfig {
        mode: DownloadProxyMode::Manual,
        all_proxy: Some("proxy.example:8080".into()),
        username: Some("proxy-user".into()),
        password: Some(SecretString::from("never-log-this")),
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
        error: ApplicationError::new(ApplicationErrorCode::OutcomeUnknown, "response lost", false),
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
                error: ApplicationError::new(ApplicationErrorCode::Rejected, "rejected", false,),
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
    global_options: Mutex<Vec<Vec<(String, String)>>>,
    queue_moves: Mutex<Vec<(Gid, QueueMove)>>,
    fail_gid: Option<Gid>,
}

#[async_trait]
impl DownloadEngineGateway for FakeGateway {
    async fn add_download(&self, request: &AddDownloadRequest) -> Result<Vec<Gid>, GatewayError> {
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

    async fn force_pause(&self, gid: Gid) -> Result<(), GatewayError> {
        self.record(TaskOperation::ForcePause, gid)
    }

    async fn resume(&self, gid: Gid) -> Result<(), GatewayError> {
        self.record(TaskOperation::Resume, gid)
    }

    async fn force_pause_all(&self) -> Result<(), GatewayError> {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((TaskOperation::ForcePause, Gid::from_u64(0)));
        Ok(())
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

    async fn apply_speed_limit(&self, config: &SpeedLimitConfig) -> Result<(), GatewayError> {
        self.global_options
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(vec![
                (
                    "max-overall-download-limit".into(),
                    config.download_limit.get().to_string(),
                ),
                (
                    "max-overall-upload-limit".into(),
                    config.upload_limit.get().to_string(),
                ),
            ]);
        Ok(())
    }

    async fn apply_bt_tracker(&self, trackers: &[String]) -> Result<(), GatewayError> {
        self.global_options
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(vec![("bt-tracker".into(), trackers.join(","))]);
        Ok(())
    }

    async fn apply_transfer_policy(
        &self,
        config: &TransferPolicyConfig,
    ) -> Result<(), GatewayError> {
        self.global_options
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(vec![
                (
                    "max-concurrent-downloads".into(),
                    config.max_concurrent_downloads.to_string(),
                ),
                (
                    "max-connection-per-server".into(),
                    config.max_connection_per_server.to_string(),
                ),
                ("split".into(), config.split.to_string()),
                ("min-split-size".into(), config.min_split_size.to_string()),
                (
                    "file-allocation".into(),
                    config.file_allocation.as_aria2().to_owned(),
                ),
                (
                    "check-integrity".into(),
                    if config.check_integrity {
                        "true".into()
                    } else {
                        "false".into()
                    },
                ),
            ]);
        Ok(())
    }

    async fn remove(&self, gid: Gid, target: TaskRemovalTarget) -> Result<(), GatewayError> {
        self.removals
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((gid, target));
        self.record(TaskOperation::Remove(TaskRemovalScope::TaskOnly), gid)
    }

    async fn force_remove(&self, gid: Gid, target: TaskRemovalTarget) -> Result<(), GatewayError> {
        self.removals
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((gid, target));
        self.record(TaskOperation::ForceRemove(TaskRemovalScope::TaskOnly), gid)
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
        global_options: Mutex::default(),
        queue_moves: Mutex::default(),
        fail_gid: Some(Gid::from_u64(2)),
    });
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let one = TaskIdentity::new(profile_id, Gid::from_u64(1));
    let two = TaskIdentity::new(profile_id, Gid::from_u64(2));
    let contexts = HashMap::from([
        (
            one,
            TaskCommandContext {
                status: DownloadStatus::Active,
                metadata: TaskMetadata::default(),
                is_history_only: false,
            },
        ),
        (
            two,
            TaskCommandContext {
                status: DownloadStatus::Active,
                metadata: TaskMetadata::default(),
                is_history_only: false,
            },
        ),
    ]);

    let outcome = block_on(service.execute(AppCommand::PauseTasks(vec![one, one, two]), &contexts));

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
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let active = TaskIdentity::new(profile_id, Gid::from_u64(1));
    let complete = TaskIdentity::new(profile_id, Gid::from_u64(2));
    let contexts = HashMap::from([
        (
            active,
            TaskCommandContext {
                status: DownloadStatus::Active,
                metadata: TaskMetadata::default(),
                is_history_only: false,
            },
        ),
        (
            complete,
            TaskCommandContext {
                status: DownloadStatus::Complete,
                metadata: TaskMetadata::default(),
                is_history_only: false,
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
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
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
    let service = CommandService::new_unprobed(profile_id, gateway);

    for uri in ["not a uri", "file:///tmp/item.bin", "javascript:alert(1)"] {
        let outcome = block_on(service.execute(
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Uris(vec![uri.into()]),
                destination: None,
                file_conflict: FileConflictPolicy::default(),
                selected_file_indices: None,
                advanced: Default::default(),
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
    let service = CommandService::new_unprobed(profile_id, gateway.clone());

    let outcome = block_on(service.execute(
        AppCommand::AddDownload(AddDownloadRequest {
            source: AddDownloadSource::Uris(vec![
                "https://user:secret@example.test/archive.iso?token=private".into(),
                "https://user:secret@example.test/archive.iso?token=private".into(),
            ]),
            destination: None,
            file_conflict: FileConflictPolicy::default(),
            selected_file_indices: None,
            advanced: Default::default(),
            options: Vec::new(),
        }),
        &HashMap::new(),
    ));

    let CommandOutcome::Failure { failed } = outcome else {
        panic!("expected duplicate mirror validation failure");
    };
    assert_eq!(failed[0].error.code, ApplicationErrorCode::Validation);
    assert!(
        !failed[0].error.summary.contains("secret"),
        "duplicate validation must redact credentials: {}",
        failed[0].error.summary
    );
    assert!(!failed[0].error.summary.contains("token=private"));
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
    let service = CommandService::new_unprobed(profile_id, gateway.clone());

    let outcome = block_on(service.execute(
        AppCommand::AddDownload(AddDownloadRequest {
            source: AddDownloadSource::Metalink(Arc::<[u8]>::from(&b""[..])),
            destination: None,
            file_conflict: FileConflictPolicy::Reject,
            selected_file_indices: None,
            advanced: Default::default(),
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
    let service = CommandService::new_unprobed(profile_id, gateway.clone());

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
                advanced: Default::default(),
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
            advanced: Default::default(),
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
    let service = CommandService::new_unprobed(profile_id, gateway);

    let outcome = block_on(service.execute(
        AppCommand::AddDownload(AddDownloadRequest {
            source: AddDownloadSource::Metalink(Arc::<[u8]>::from(&b"metadata"[..])),
            destination: None,
            file_conflict: FileConflictPolicy::Reject,
            selected_file_indices: None,
            advanced: Default::default(),
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
    let service = CommandService::new_unprobed(profile_id, gateway);

    let outcome = block_on(service.execute(
        AppCommand::AddDownload(AddDownloadRequest {
            source: AddDownloadSource::Metalink(Arc::<[u8]>::from(&b"metadata"[..])),
            destination: None,
            file_conflict: FileConflictPolicy::Reject,
            selected_file_indices: None,
            advanced: Default::default(),
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
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let live = TaskIdentity::new(profile_id, Gid::from_u64(1));
    let complete = TaskIdentity::new(profile_id, Gid::from_u64(2));
    let removed = TaskIdentity::new(profile_id, Gid::from_u64(3));

    let contexts = HashMap::from([
        (
            live,
            TaskCommandContext {
                status: DownloadStatus::Active,
                metadata: TaskMetadata::default(),
                is_history_only: false,
            },
        ),
        (
            complete,
            TaskCommandContext {
                status: DownloadStatus::Complete,
                metadata: TaskMetadata::default(),
                is_history_only: false,
            },
        ),
        (
            removed,
            TaskCommandContext {
                status: DownloadStatus::Removed,
                metadata: TaskMetadata::default(),
                is_history_only: false,
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
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let waiting = TaskIdentity::new(profile_id, Gid::from_u64(4));
    let contexts = HashMap::from([(
        waiting,
        TaskCommandContext {
            status: DownloadStatus::Waiting,
            metadata: TaskMetadata::default(),
            is_history_only: false,
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
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let complete = TaskIdentity::new(profile_id, Gid::from_u64(5));
    let contexts = HashMap::from([(
        complete,
        TaskCommandContext {
            status: DownloadStatus::Complete,
            metadata: TaskMetadata::default(),
            is_history_only: false,
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
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
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
            is_history_only: false,
        },
    )]);

    let outcome = block_on(service.execute(AppCommand::RetryTasks(vec![failed_task]), &contexts));

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
                source: AddDownloadSource::Uris(vec!["https://example.test/archive.iso".into(),]),
                destination: Some(EnginePath::new("/downloads")),
                file_conflict: FileConflictPolicy::default(),
                selected_file_indices: None,
                advanced: Default::default(),
                options: Vec::new(),
            }
        )]
    );
}

#[test]
fn direct_uri_output_name_change_is_validated_and_forwarded() {
    let profile_id = ProfileId::new();
    let gateway = Arc::new(FakeGateway::default());
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let task = TaskIdentity::new(profile_id, Gid::from_u64(8));
    let contexts = HashMap::from([(
        task,
        TaskCommandContext {
            status: DownloadStatus::Waiting,
            metadata: TaskMetadata {
                source_kind: TaskSourceKind::DirectUri,
                ..TaskMetadata::default()
            },
            is_history_only: false,
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
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let task = TaskIdentity::new(profile_id, Gid::from_u64(9));
    let contexts = HashMap::from([(
        task,
        TaskCommandContext {
            status: DownloadStatus::Waiting,
            metadata: TaskMetadata {
                source_kind: TaskSourceKind::BitTorrent,
                ..TaskMetadata::default()
            },
            is_history_only: false,
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
fn task_speed_limit_forwards_typed_change_options_for_a_live_task() {
    let profile_id = ProfileId::new();
    let gateway = Arc::new(FakeGateway::default());
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let task = TaskIdentity::new(profile_id, Gid::from_u64(11));
    let contexts = HashMap::from([(
        task,
        TaskCommandContext {
            status: DownloadStatus::Active,
            metadata: TaskMetadata::default(),
            is_history_only: false,
        },
    )]);

    let outcome = block_on(service.execute(
        AppCommand::SetTaskSpeedLimit(SetTaskSpeedLimitRequest {
            task,
            download_limit: ariadeck_domain::ByteRate::new(2 * 1024 * 1024),
            upload_limit: ariadeck_domain::ByteRate::new(0),
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
        vec![(
            task.gid,
            vec![
                ("max-download-limit".into(), (2 * 1024 * 1024).to_string()),
                ("max-upload-limit".into(), "0".into()),
            ]
        )]
    );
}

#[test]
fn task_speed_limit_rejects_terminal_tasks_before_the_gateway() {
    let profile_id = ProfileId::new();
    let gateway = Arc::new(FakeGateway::default());
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let task = TaskIdentity::new(profile_id, Gid::from_u64(12));
    let contexts = HashMap::from([(
        task,
        TaskCommandContext {
            status: DownloadStatus::Complete,
            metadata: TaskMetadata::default(),
            is_history_only: false,
        },
    )]);

    let outcome = block_on(service.execute(
        AppCommand::SetTaskSpeedLimit(SetTaskSpeedLimitRequest {
            task,
            download_limit: ariadeck_domain::ByteRate::new(1024),
            upload_limit: ariadeck_domain::ByteRate::new(1024),
        }),
        &contexts,
    ));

    assert!(matches!(outcome, CommandOutcome::Failure { .. }));
    assert!(
        gateway
            .changed_options
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty()
    );
}

#[test]
fn global_speed_limit_forwards_overall_options_to_the_gateway() {
    let profile_id = ProfileId::new();
    let gateway = Arc::new(FakeGateway::default());
    let service = CommandService::new_unprobed(profile_id, gateway.clone());

    let result = block_on(service.apply_speed_limit(&SpeedLimitConfig {
        download_limit: ariadeck_domain::ByteRate::new(5 * 1024 * 1024),
        upload_limit: ariadeck_domain::ByteRate::new(512 * 1024),
    }));

    assert!(result.is_ok());
    assert_eq!(
        *gateway
            .global_options
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        vec![vec![
            (
                "max-overall-download-limit".into(),
                (5 * 1024 * 1024).to_string()
            ),
            ("max-overall-upload-limit".into(), (512 * 1024).to_string()),
        ]]
    );
}

#[test]
fn task_connection_policy_forwards_typed_change_options_for_a_live_task() {
    let profile_id = ProfileId::new();
    let gateway = Arc::new(FakeGateway::default());
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let task = TaskIdentity::new(profile_id, Gid::from_u64(21));
    let contexts = HashMap::from([(
        task,
        TaskCommandContext {
            status: DownloadStatus::Active,
            metadata: TaskMetadata::default(),
            is_history_only: false,
        },
    )]);

    let outcome = block_on(service.execute(
        AppCommand::SetTaskConnectionPolicy(SetTaskConnectionPolicyRequest {
            task,
            policy: TaskConnectionPolicy {
                max_connection_per_server: 8,
                split: 16,
                min_split_size: 1024 * 1024,
            },
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
        vec![(
            task.gid,
            vec![
                ("max-connection-per-server".into(), "8".into()),
                ("split".into(), "16".into()),
                ("min-split-size".into(), (1024 * 1024).to_string()),
            ]
        )]
    );
}

#[test]
fn task_connection_policy_rejects_terminal_and_out_of_range_values() {
    let profile_id = ProfileId::new();
    let gateway = Arc::new(FakeGateway::default());
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let task = TaskIdentity::new(profile_id, Gid::from_u64(22));
    let contexts = HashMap::from([(
        task,
        TaskCommandContext {
            status: DownloadStatus::Error,
            metadata: TaskMetadata::default(),
            is_history_only: false,
        },
    )]);

    let invalid = block_on(service.execute(
        AppCommand::SetTaskConnectionPolicy(SetTaskConnectionPolicyRequest {
            task,
            policy: TaskConnectionPolicy {
                max_connection_per_server: 32,
                split: 5,
                min_split_size: 1024,
            },
        }),
        &contexts,
    ));
    assert!(matches!(invalid, CommandOutcome::Failure { .. }));

    let terminal = block_on(service.execute(
        AppCommand::SetTaskConnectionPolicy(SetTaskConnectionPolicyRequest {
            task,
            policy: TaskConnectionPolicy {
                max_connection_per_server: 4,
                split: 5,
                min_split_size: 1024,
            },
        }),
        &contexts,
    ));
    assert!(matches!(terminal, CommandOutcome::Failure { .. }));
    assert!(
        gateway
            .changed_options
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty()
    );
}

#[test]
fn global_transfer_policy_forwards_typed_options_to_the_gateway() {
    let profile_id = ProfileId::new();
    let gateway = Arc::new(FakeGateway::default());
    let service = CommandService::new_unprobed(profile_id, gateway.clone());

    let result = block_on(service.apply_transfer_policy(&TransferPolicyConfig {
        max_concurrent_downloads: 3,
        max_connection_per_server: 8,
        split: 16,
        min_split_size: 1024 * 1024,
        file_allocation: ariadeck_domain::FileAllocationMethod::Falloc,
        check_integrity: true,
    }));

    assert!(result.is_ok());
    assert_eq!(
        *gateway
            .global_options
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        vec![vec![
            ("max-concurrent-downloads".into(), "3".into()),
            ("max-connection-per-server".into(), "8".into()),
            ("split".into(), "16".into()),
            ("min-split-size".into(), (1024 * 1024).to_string()),
            ("file-allocation".into(), "falloc".into()),
            ("check-integrity".into(), "true".into()),
        ]]
    );
}

#[test]
fn retry_rejects_non_failed_or_unreplayable_tasks_before_the_gateway() {
    let profile_id = ProfileId::new();
    let gateway = Arc::new(FakeGateway::default());
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let active = TaskIdentity::new(profile_id, Gid::from_u64(1));
    let missing_source = TaskIdentity::new(profile_id, Gid::from_u64(2));
    let contexts = HashMap::from([
        (
            active,
            TaskCommandContext {
                status: DownloadStatus::Active,
                metadata: TaskMetadata::default(),
                is_history_only: false,
            },
        ),
        (
            missing_source,
            TaskCommandContext {
                status: DownloadStatus::Error,
                metadata: TaskMetadata::default(),
                is_history_only: false,
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

#[test]
fn force_pause_and_force_remove_forward_to_the_gateway() {
    let profile_id = ProfileId::new();
    let gateway = Arc::new(FakeGateway::default());
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let task = TaskIdentity::new(profile_id, Gid::from_u64(21));
    let contexts = HashMap::from([(
        task,
        TaskCommandContext {
            status: DownloadStatus::Active,
            metadata: TaskMetadata::default(),
            is_history_only: false,
        },
    )]);

    let pause = block_on(service.execute(AppCommand::ForcePauseTasks(vec![task]), &contexts));
    assert_eq!(
        pause,
        CommandOutcome::Success {
            succeeded: vec![CommandItem::Task(task)]
        }
    );

    let remove = block_on(service.execute(
        AppCommand::ForceRemoveTasks(RemoveTasksRequest {
            tasks: vec![task],
            scope: TaskRemovalScope::TaskOnly,
        }),
        &contexts,
    ));
    assert_eq!(
        remove,
        CommandOutcome::Success {
            succeeded: vec![CommandItem::Task(task)]
        }
    );

    let calls = gateway
        .calls
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    assert!(calls.contains(&(TaskOperation::ForcePause, task.gid)));
    assert!(calls.contains(&(
        TaskOperation::ForceRemove(TaskRemovalScope::TaskOnly),
        task.gid
    )));
}

#[test]
fn set_task_options_maps_typed_seed_and_file_selection_fields() {
    let profile_id = ProfileId::new();
    let gateway = Arc::new(FakeGateway::default());
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let task = TaskIdentity::new(profile_id, Gid::from_u64(22));
    let contexts = HashMap::from([(
        task,
        TaskCommandContext {
            status: DownloadStatus::Seeding,
            metadata: TaskMetadata {
                source_kind: TaskSourceKind::BitTorrent,
                ..TaskMetadata::default()
            },
            is_history_only: false,
        },
    )]);

    let outcome = block_on(service.execute(
        AppCommand::SetTaskOptions(SetTaskOptionsRequest {
            task,
            seed_ratio: Some("1.5".into()),
            seed_time_minutes: Some(60),
            selected_file_indices: Some(vec![1, 2, 4]),
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
        vec![(
            task.gid,
            vec![
                ("seed-ratio".into(), "1.5".into()),
                ("seed-time".into(), "60".into()),
                ("select-file".into(), "1-2,4".into()),
            ]
        )]
    );
}

#[test]
fn set_task_options_rejects_seed_rules_for_direct_uri_tasks() {
    let profile_id = ProfileId::new();
    let gateway = Arc::new(FakeGateway::default());
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let task = TaskIdentity::new(profile_id, Gid::from_u64(23));
    let contexts = HashMap::from([(
        task,
        TaskCommandContext {
            status: DownloadStatus::Active,
            metadata: TaskMetadata {
                source_kind: TaskSourceKind::DirectUri,
                ..TaskMetadata::default()
            },
            is_history_only: false,
        },
    )]);

    let outcome = block_on(service.execute(
        AppCommand::SetTaskOptions(SetTaskOptionsRequest {
            task,
            seed_ratio: Some("1.0".into()),
            seed_time_minutes: None,
            selected_file_indices: None,
        }),
        &contexts,
    ));

    assert!(matches!(outcome, CommandOutcome::Failure { .. }));
    assert!(
        gateway
            .changed_options
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty()
    );
}

#[test]
fn task_option_request_formats_selected_file_ranges() {
    let request = SetTaskOptionsRequest {
        task: TaskIdentity::new(ProfileId::new(), Gid::from_u64(1)),
        seed_ratio: None,
        seed_time_minutes: None,
        selected_file_indices: Some(vec![1, 2, 3, 5, 7, 8]),
    };
    assert_eq!(
        request.to_option_pairs(),
        vec![("select-file".into(), "1-3,5,7-8".into())]
    );
}

#[test]
fn advanced_options_flatten_headers_cookie_auth_and_checksum() {
    let advanced = AddDownloadAdvancedOptions {
        referer: Some("https://example.test/page".into()),
        user_agent: Some("AriaDeck/test".into()),
        headers: vec!["X-Token: abc".into(), "Accept: */*".into()],
        cookie: Some(SecretString::from("session=secret")),
        http_user: Some("alice".into()),
        http_passwd: Some(SecretString::from("s3cret")),
        checksum: Some(format!("sha-256={}", "ab".repeat(32))),
    };
    assert!(advanced.validate().is_ok());
    assert_eq!(
        advanced.to_option_pairs(),
        vec![
            ("referer".into(), "https://example.test/page".into()),
            ("user-agent".into(), "AriaDeck/test".into()),
            ("header".into(), "X-Token: abc".into()),
            ("header".into(), "Accept: */*".into()),
            ("header".into(), "Cookie: session=secret".into()),
            ("http-user".into(), "alice".into()),
            ("http-passwd".into(), "s3cret".into()),
            ("checksum".into(), format!("sha-256={}", "ab".repeat(32))),
        ]
    );
    let debug = format!("{advanced:?}");
    assert!(!debug.contains("s3cret"));
    assert!(!debug.contains("session=secret"));
    assert!(debug.contains("[REDACTED]"));
}

#[test]
fn advanced_options_reject_secret_headers_and_bad_checksums() {
    let cookie_header = AddDownloadAdvancedOptions {
        headers: vec!["Cookie: session=abc".into()],
        ..AddDownloadAdvancedOptions::default()
    };
    assert!(cookie_header.validate().is_err());

    let auth_header = AddDownloadAdvancedOptions {
        headers: vec!["Authorization: Bearer token".into()],
        ..AddDownloadAdvancedOptions::default()
    };
    assert!(auth_header.validate().is_err());

    let bad_checksum = AddDownloadAdvancedOptions {
        checksum: Some("sha-256=deadbeef".into()),
        ..AddDownloadAdvancedOptions::default()
    };
    assert!(bad_checksum.validate().is_err());

    let password_without_user = AddDownloadAdvancedOptions {
        http_passwd: Some(SecretString::from("x")),
        ..AddDownloadAdvancedOptions::default()
    };
    assert!(password_without_user.validate().is_err());
}

#[test]
fn add_download_forwards_typed_advanced_options_only_for_direct_uris() {
    let profile_id = ProfileId::new();
    let gateway = Arc::new(FakeGateway::default());
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let advanced = AddDownloadAdvancedOptions {
        referer: Some("https://cdn.example/ref".into()),
        headers: vec!["X-Request-Id: 1".into()],
        cookie: Some(SecretString::from("k=v")),
        checksum: Some(format!("sha-256={}", "cd".repeat(32))),
        ..AddDownloadAdvancedOptions::default()
    };

    let outcome = block_on(service.execute(
        AppCommand::AddDownload(AddDownloadRequest {
            source: AddDownloadSource::Uris(vec!["https://example.test/file.bin".into()]),
            destination: None,
            file_conflict: FileConflictPolicy::default(),
            selected_file_indices: None,
            advanced: advanced.clone(),
            options: Vec::new(),
        }),
        &HashMap::new(),
    ));
    assert!(matches!(outcome, CommandOutcome::Success { .. }));
    let adds = gateway
        .adds
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    assert_eq!(adds.len(), 1);
    assert_eq!(adds[0].options, advanced.to_option_pairs());

    let rejected = block_on(service.execute(
        AppCommand::AddDownload(AddDownloadRequest {
            source: AddDownloadSource::Torrent(Arc::<[u8]>::from(&b"torrent"[..])),
            destination: None,
            file_conflict: FileConflictPolicy::Reject,
            selected_file_indices: None,
            advanced,
            options: Vec::new(),
        }),
        &HashMap::new(),
    ));
    assert!(matches!(rejected, CommandOutcome::Failure { .. }));
    assert_eq!(
        gateway
            .adds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len(),
        1,
        "invalid metadata advanced options must not reach the gateway"
    );
}

#[test]
fn force_pause_is_rejected_before_the_gateway_when_list_methods_omits_it() {
    let profile_id = ProfileId::new();
    let gateway = Arc::new(FakeGateway::default());
    let capabilities = EngineCapabilities {
        version: "1.37.0".into(),
        enabled_features: Vec::new(),
        methods: vec![
            "aria2.pause".into(),
            "aria2.unpause".into(),
            "aria2.remove".into(),
        ],
    };
    let service = CommandService::new(profile_id, gateway.clone(), capabilities);
    let task = TaskIdentity::new(profile_id, Gid::from_u64(99));
    let contexts = HashMap::from([(
        task,
        TaskCommandContext {
            status: DownloadStatus::Active,
            metadata: TaskMetadata::default(),
            is_history_only: false,
        },
    )]);

    let outcome = block_on(service.execute(AppCommand::ForcePauseTasks(vec![task]), &contexts));
    match outcome {
        CommandOutcome::Failure { failed } => {
            assert_eq!(failed.len(), 1);
            assert_eq!(failed[0].error.code, ApplicationErrorCode::Unsupported);
            assert!(
                failed[0].error.summary.contains("force-pause")
                    || failed[0].error.summary.contains("forcePause"),
                "{}",
                failed[0].error.summary
            );
        }
        other => panic!("expected capability rejection, got {other:?}"),
    }
    assert!(
        gateway
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty(),
        "unsupported force-pause must not reach the gateway"
    );
}

#[test]
fn change_global_option_controls_are_gated_when_missing_from_list_methods() {
    let profile_id = ProfileId::new();
    let gateway = Arc::new(FakeGateway::default());
    let capabilities = EngineCapabilities {
        version: "1.36.0".into(),
        enabled_features: Vec::new(),
        methods: vec!["aria2.getVersion".into(), "aria2.tellActive".into()],
    };
    let service = CommandService::new(profile_id, gateway.clone(), capabilities);
    let err = block_on(service.apply_download_proxy(&DownloadProxyConfig::default()))
        .expect_err("proxy apply must be gated");
    assert_eq!(err.code, ApplicationErrorCode::Unsupported);
    assert!(err.summary.contains("changeGlobalOption") || err.summary.contains("global option"));
}

#[test]
fn terminal_remove_gates_on_remove_download_result_not_remove() {
    let profile_id = ProfileId::new();
    let gid = Gid::from_u64(0x55);
    let gateway = Arc::new(FakeGateway::default());
    let capabilities = EngineCapabilities {
        version: "1.37.0".into(),
        enabled_features: Vec::new(),
        // Only removeDownloadResult is advertised, not aria2.remove.
        methods: vec!["aria2.removeDownloadResult".into()],
    };
    let service = CommandService::new(profile_id, gateway.clone(), capabilities);
    let identity = TaskIdentity::new(profile_id, gid);
    let mut contexts = HashMap::new();
    contexts.insert(
        identity,
        TaskCommandContext {
            status: DownloadStatus::Complete,
            metadata: TaskMetadata::default(),
            is_history_only: false,
        },
    );
    let outcome = block_on(service.execute(
        AppCommand::RemoveTasks(RemoveTasksRequest {
            tasks: vec![identity],
            scope: TaskRemovalScope::TaskOnly,
        }),
        &contexts,
    ));
    assert!(matches!(outcome, CommandOutcome::Success { .. }));
}

#[test]
fn magnet_uris_reject_advanced_http_options_and_debug_redacts_secrets() {
    let request = AddDownloadRequest {
        source: AddDownloadSource::Uris(vec![
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&tr=https://secret:pass@t.example/announce".into(),
        ]),
        advanced: AddDownloadAdvancedOptions {
            referer: Some("https://example.com".into()),
            cookie: Some(SecretString::from("sid=supersecret")),
            ..AddDownloadAdvancedOptions::default()
        },
        options: vec![
            ("http-passwd".into(), "topsecret".into()),
            ("header".into(), "Cookie: sid=supersecret".into()),
            ("split".into(), "4".into()),
        ],
        ..AddDownloadRequest::default()
    };
    assert!(request.validate().is_err());
    let debug_text = format!("{request:?}");
    assert!(!debug_text.contains("supersecret"));
    assert!(!debug_text.contains("topsecret"));
    assert!(!debug_text.contains("secret:pass"));
}

#[test]
fn history_only_removal_succeeds_without_calling_engine_and_redacted_retry_fails_cleanly() {
    let profile_id = ProfileId::new();
    let gateway = Arc::new(FakeGateway::default());
    let service = CommandService::new_unprobed(profile_id, gateway.clone());
    let history_task = TaskIdentity::new(profile_id, Gid::from_u64(0x70));
    let redacted_failed = TaskIdentity::new(profile_id, Gid::from_u64(0x71));
    let contexts = HashMap::from([
        (
            history_task,
            TaskCommandContext {
                status: DownloadStatus::Complete,
                metadata: TaskMetadata::default(),
                is_history_only: true,
            },
        ),
        (
            redacted_failed,
            TaskCommandContext {
                status: DownloadStatus::Error,
                metadata: TaskMetadata {
                    primary_uri: Some(ariadeck_domain::REDACTED_SOURCE_PLACEHOLDER.into()),
                    ..TaskMetadata::default()
                },
                is_history_only: true,
            },
        ),
    ]);

    let remove_outcome = block_on(service.execute(
        AppCommand::RemoveTasks(RemoveTasksRequest {
            tasks: vec![history_task],
            scope: TaskRemovalScope::TaskOnly,
        }),
        &contexts,
    ));
    assert_eq!(
        remove_outcome,
        CommandOutcome::Success {
            succeeded: vec![CommandItem::Task(history_task)],
        }
    );
    assert!(
        gateway
            .removals
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty(),
        "history-only remove must not call aria2.removeDownloadResult"
    );

    let retry_outcome =
        block_on(service.execute(AppCommand::RetryTasks(vec![redacted_failed]), &contexts));
    let CommandOutcome::Failure { failed } = retry_outcome else {
        panic!("redacted placeholder retry must fail cleanly");
    };
    assert_eq!(failed[0].error.code, ApplicationErrorCode::Unsupported);
    assert!(
        gateway
            .retries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty()
    );
}

#[test]
fn download_proxy_debug_redacts_credentials_and_validate_rejects_whitespace_and_crlf() {
    let proxy = DownloadProxyConfig {
        mode: DownloadProxyMode::Manual,
        all_proxy: Some("http://alice:hunter2@proxy.example:8080/path?token=abc".into()),
        ..DownloadProxyConfig::default()
    };
    let debug = format!("{proxy:?}");
    assert!(!debug.contains("hunter2"));
    assert!(!debug.contains("alice"));
    assert!(!debug.contains("token=abc"));

    let blank = DownloadProxyConfig {
        mode: DownloadProxyMode::Manual,
        all_proxy: Some("   ".into()),
        ..DownloadProxyConfig::default()
    };
    assert!(blank.validate().is_err());

    let crlf = DownloadProxyConfig {
        mode: DownloadProxyMode::Manual,
        http_proxy: Some("http://proxy.example:8080\r\nInjected".into()),
        ..DownloadProxyConfig::default()
    };
    assert!(crlf.validate().is_err());
}
