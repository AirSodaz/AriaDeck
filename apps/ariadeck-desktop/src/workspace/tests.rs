use super::*;

use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use ariadeck_application::{
    ConnectedSyncSession, DownloadEngineGateway, DownloadSyncConnector, DownloadSyncSession,
    EngineCapabilities, GatewayError, GatewayErrorKind, InitialSyncSnapshot, ItemFailure,
    LiveSyncSnapshot, RefreshHint, StoppedPage, SyncError, SyncErrorKind,
    TaskConnectionDetailsGateway, TaskDetailsGateway, TaskFileRemovalPreview,
    TaskFileRemovalReport, TaskRemovalTarget,
};
use ariadeck_domain::{
    ByteCount, ByteRate, EnginePath, Gid, GlobalStat, TaskDetails, TaskFile, TaskSnapshot,
};
use async_trait::async_trait;
use tokio::sync::mpsc;

#[derive(Default)]
struct FakeProxyCredentialStore {
    passwords: Mutex<HashMap<ProxyCredentialRef, String>>,
}

impl ProxyCredentialStore for FakeProxyCredentialStore {
    fn load(
        &self,
        credential: ProxyCredentialRef,
    ) -> Result<Option<SecretString>, ariadeck_settings::ProxyCredentialError> {
        Ok(self
            .passwords
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&credential)
            .cloned()
            .map(SecretString::new))
    }

    fn save(
        &self,
        credential: ProxyCredentialRef,
        password: &SecretString,
    ) -> Result<(), ariadeck_settings::ProxyCredentialError> {
        use secrecy::ExposeSecret as _;

        self.passwords
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(credential, password.expose_secret().clone());
        Ok(())
    }

    fn delete(
        &self,
        credential: ProxyCredentialRef,
    ) -> Result<(), ariadeck_settings::ProxyCredentialError> {
        self.passwords
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&credential);
        Ok(())
    }
}

struct UnknownAcceptedAddGateway {
    accepted: Arc<AtomicBool>,
    add_calls: Arc<AtomicUsize>,
}

#[async_trait]
impl DownloadEngineGateway for UnknownAcceptedAddGateway {
    async fn add_download(&self, _request: &AddDownloadRequest) -> Result<Vec<Gid>, GatewayError> {
        self.add_calls.fetch_add(1, Ordering::Relaxed);
        self.accepted.store(true, Ordering::Release);
        Err(GatewayError::new(
            GatewayErrorKind::OutcomeUnknown,
            "response lost after aria2 registered the task",
            false,
        ))
    }

    async fn retry_download(
        &self,
        _gid: Gid,
        _fallback: &AddDownloadRequest,
    ) -> Result<Gid, GatewayError> {
        Err(unsupported_test_gateway_error())
    }

    async fn pause(&self, _gid: Gid) -> Result<(), GatewayError> {
        Err(unsupported_test_gateway_error())
    }

    async fn resume(&self, _gid: Gid) -> Result<(), GatewayError> {
        Err(unsupported_test_gateway_error())
    }

    async fn change_options(
        &self,
        _gid: Gid,
        _options: &[(String, String)],
    ) -> Result<(), GatewayError> {
        Err(unsupported_test_gateway_error())
    }

    async fn remove(&self, _gid: Gid, _target: TaskRemovalTarget) -> Result<(), GatewayError> {
        Err(unsupported_test_gateway_error())
    }
}

#[async_trait]
impl TaskDetailsGateway for UnknownAcceptedAddGateway {
    async fn task_details(&self, _gid: Gid) -> Result<TaskDetails, GatewayError> {
        Err(unsupported_test_gateway_error())
    }
}

#[async_trait]
impl TaskConnectionDetailsGateway for UnknownAcceptedAddGateway {}

struct UnknownAcceptedAddSession {
    accepted: Arc<AtomicBool>,
}

impl UnknownAcceptedAddSession {
    fn live(&self) -> LiveSyncSnapshot {
        let waiting = self.accepted.load(Ordering::Acquire).then(|| {
            let mut task =
                TaskSnapshot::new(Gid::from_u64(42), DownloadStatus::Waiting, "resolved.bin");
            task.metadata.primary_uri = Some("https://example.test/resolved.bin".into());
            task
        });
        LiveSyncSnapshot {
            active: Vec::new(),
            waiting: waiting.into_iter().collect(),
        }
    }
}

#[async_trait]
impl DownloadSyncSession for UnknownAcceptedAddSession {
    async fn initial_snapshot(
        &self,
        _stopped_count: u32,
    ) -> Result<InitialSyncSnapshot, SyncError> {
        Ok(InitialSyncSnapshot {
            capabilities: EngineCapabilities {
                version: "test".into(),
                enabled_features: Vec::new(),
                methods: Vec::new(),
            },
            global_stat: GlobalStat::default(),
            live: LiveSyncSnapshot {
                active: Vec::new(),
                waiting: Vec::new(),
            },
            stopped: StoppedPage {
                offset: 0,
                total: 0,
                tasks: Vec::new(),
            },
        })
    }

    async fn refresh_global_stat(&self) -> Result<GlobalStat, SyncError> {
        Ok(GlobalStat::default())
    }

    async fn refresh_live(&self) -> Result<LiveSyncSnapshot, SyncError> {
        Ok(self.live())
    }

    async fn refresh_stopped_page(
        &self,
        offset: usize,
        _count: u32,
    ) -> Result<StoppedPage, SyncError> {
        Ok(StoppedPage {
            offset,
            total: 0,
            tasks: Vec::new(),
        })
    }

    async fn refresh_tasks(
        &self,
        gids: &[Gid],
    ) -> Result<Vec<(Gid, Option<TaskSnapshot>)>, SyncError> {
        let live = self.live();
        Ok(gids
            .iter()
            .copied()
            .map(|gid| {
                let task = live.waiting.iter().find(|task| task.gid == gid).cloned();
                (gid, task)
            })
            .collect())
    }

    async fn close(&self) {}
}

struct UnknownAcceptedAddConnector {
    accepted: Arc<AtomicBool>,
    add_calls: Arc<AtomicUsize>,
    notifications_rx: Mutex<Option<mpsc::Receiver<RefreshHint>>>,
    _notifications_tx: mpsc::Sender<RefreshHint>,
}

struct UnknownAcceptedRetryGateway {
    accepted: Arc<AtomicBool>,
    retry_calls: Arc<AtomicUsize>,
    observe_replacement: bool,
}

#[async_trait]
impl DownloadEngineGateway for UnknownAcceptedRetryGateway {
    async fn add_download(&self, _request: &AddDownloadRequest) -> Result<Vec<Gid>, GatewayError> {
        Err(unsupported_test_gateway_error())
    }

    async fn retry_download(
        &self,
        _gid: Gid,
        _fallback: &AddDownloadRequest,
    ) -> Result<Gid, GatewayError> {
        self.retry_calls.fetch_add(1, Ordering::Relaxed);
        if self.observe_replacement {
            self.accepted.store(true, Ordering::Release);
        }
        Err(GatewayError::new(
            GatewayErrorKind::OutcomeUnknown,
            "response lost after aria2 registered the retry task",
            false,
        ))
    }

    async fn pause(&self, _gid: Gid) -> Result<(), GatewayError> {
        Err(unsupported_test_gateway_error())
    }

    async fn resume(&self, _gid: Gid) -> Result<(), GatewayError> {
        Err(unsupported_test_gateway_error())
    }

    async fn change_options(
        &self,
        _gid: Gid,
        _options: &[(String, String)],
    ) -> Result<(), GatewayError> {
        Err(unsupported_test_gateway_error())
    }

    async fn remove(&self, _gid: Gid, _target: TaskRemovalTarget) -> Result<(), GatewayError> {
        Err(unsupported_test_gateway_error())
    }
}

#[async_trait]
impl TaskDetailsGateway for UnknownAcceptedRetryGateway {
    async fn task_details(&self, _gid: Gid) -> Result<TaskDetails, GatewayError> {
        Err(unsupported_test_gateway_error())
    }
}

#[async_trait]
impl TaskConnectionDetailsGateway for UnknownAcceptedRetryGateway {}

struct UnknownAcceptedRetrySession {
    accepted: Arc<AtomicBool>,
}

impl UnknownAcceptedRetrySession {
    fn failed_task() -> TaskSnapshot {
        let mut task = TaskSnapshot::new(Gid::from_u64(7), DownloadStatus::Error, "failed.bin");
        task.metadata.primary_uri = Some("https://example.test/retry.bin".into());
        task
    }

    fn live(&self) -> LiveSyncSnapshot {
        let replacement = self.accepted.load(Ordering::Acquire).then(|| {
            let mut task =
                TaskSnapshot::new(Gid::from_u64(42), DownloadStatus::Waiting, "retry.bin");
            task.metadata.primary_uri = Some("https://example.test/retry.bin".into());
            task
        });
        LiveSyncSnapshot {
            active: Vec::new(),
            waiting: replacement.into_iter().collect(),
        }
    }
}

#[async_trait]
impl DownloadSyncSession for UnknownAcceptedRetrySession {
    async fn initial_snapshot(
        &self,
        _stopped_count: u32,
    ) -> Result<InitialSyncSnapshot, SyncError> {
        Ok(InitialSyncSnapshot {
            capabilities: EngineCapabilities {
                version: "test".into(),
                enabled_features: Vec::new(),
                methods: Vec::new(),
            },
            global_stat: GlobalStat::default(),
            live: self.live(),
            stopped: StoppedPage {
                offset: 0,
                total: 1,
                tasks: vec![Self::failed_task()],
            },
        })
    }

    async fn refresh_global_stat(&self) -> Result<GlobalStat, SyncError> {
        Ok(GlobalStat::default())
    }

    async fn refresh_live(&self) -> Result<LiveSyncSnapshot, SyncError> {
        Ok(self.live())
    }

    async fn refresh_stopped_page(
        &self,
        offset: usize,
        _count: u32,
    ) -> Result<StoppedPage, SyncError> {
        Ok(StoppedPage {
            offset,
            total: 1,
            tasks: vec![Self::failed_task()],
        })
    }

    async fn refresh_tasks(
        &self,
        gids: &[Gid],
    ) -> Result<Vec<(Gid, Option<TaskSnapshot>)>, SyncError> {
        let live = self.live();
        Ok(gids
            .iter()
            .copied()
            .map(|gid| {
                let task = if gid == Gid::from_u64(7) {
                    Some(Self::failed_task())
                } else {
                    live.waiting.iter().find(|task| task.gid == gid).cloned()
                };
                (gid, task)
            })
            .collect())
    }

    async fn close(&self) {}
}

struct UnknownAcceptedRetryConnector {
    accepted: Arc<AtomicBool>,
    retry_calls: Arc<AtomicUsize>,
    observe_replacement: bool,
    notifications_rx: Mutex<Option<mpsc::Receiver<RefreshHint>>>,
    _notifications_tx: mpsc::Sender<RefreshHint>,
}

#[async_trait]
impl DownloadSyncConnector for UnknownAcceptedRetryConnector {
    async fn connect(&self) -> Result<ConnectedSyncSession, SyncError> {
        let notifications = self
            .notifications_rx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .ok_or_else(|| SyncError::new(SyncErrorKind::Internal, "connector reused", false))?;
        let gateway = Arc::new(UnknownAcceptedRetryGateway {
            accepted: self.accepted.clone(),
            retry_calls: self.retry_calls.clone(),
            observe_replacement: self.observe_replacement,
        });
        Ok(ConnectedSyncSession::new_with_gateways(
            Box::new(UnknownAcceptedRetrySession {
                accepted: self.accepted.clone(),
            }),
            gateway.clone(),
            gateway.clone(),
            gateway,
            notifications,
        ))
    }
}

#[async_trait]
impl DownloadSyncConnector for UnknownAcceptedAddConnector {
    async fn connect(&self) -> Result<ConnectedSyncSession, SyncError> {
        let notifications = self
            .notifications_rx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .ok_or_else(|| SyncError::new(SyncErrorKind::Internal, "connector reused", false))?;
        let gateway = Arc::new(UnknownAcceptedAddGateway {
            accepted: self.accepted.clone(),
            add_calls: self.add_calls.clone(),
        });
        Ok(ConnectedSyncSession::new_with_gateways(
            Box::new(UnknownAcceptedAddSession {
                accepted: self.accepted.clone(),
            }),
            gateway.clone(),
            gateway.clone(),
            gateway,
            notifications,
        ))
    }
}

struct RemovalWorkflowGateway {
    removed: Arc<AtomicBool>,
    remove_calls: Arc<AtomicUsize>,
    events: Arc<Mutex<Vec<&'static str>>>,
    outcome_unknown: bool,
}

#[async_trait]
impl DownloadEngineGateway for RemovalWorkflowGateway {
    async fn add_download(&self, _request: &AddDownloadRequest) -> Result<Vec<Gid>, GatewayError> {
        Err(unsupported_test_gateway_error())
    }

    async fn retry_download(
        &self,
        _gid: Gid,
        _fallback: &AddDownloadRequest,
    ) -> Result<Gid, GatewayError> {
        Err(unsupported_test_gateway_error())
    }

    async fn pause(&self, _gid: Gid) -> Result<(), GatewayError> {
        Err(unsupported_test_gateway_error())
    }

    async fn resume(&self, _gid: Gid) -> Result<(), GatewayError> {
        Err(unsupported_test_gateway_error())
    }

    async fn change_options(
        &self,
        _gid: Gid,
        _options: &[(String, String)],
    ) -> Result<(), GatewayError> {
        Err(unsupported_test_gateway_error())
    }

    async fn remove(&self, _gid: Gid, _target: TaskRemovalTarget) -> Result<(), GatewayError> {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push("engine_remove");
        self.remove_calls.fetch_add(1, Ordering::Relaxed);
        self.removed.store(true, Ordering::Release);
        if self.outcome_unknown {
            Err(GatewayError::new(
                GatewayErrorKind::OutcomeUnknown,
                "response lost after aria2 removed the task",
                false,
            ))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl TaskDetailsGateway for RemovalWorkflowGateway {
    async fn task_details(&self, gid: Gid) -> Result<TaskDetails, GatewayError> {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push("details");
        Ok(TaskDetails {
            gid,
            directory: Some(EnginePath::new("D:/downloads")),
            info_hash: None,
            piece_length: None,
            piece_count: None,
            trackers: Vec::new(),
            files: vec![TaskFile {
                index: 1,
                path: EnginePath::new("D:/downloads/item.bin"),
                length: ByteCount::new(10),
                completed_length: ByteCount::new(5),
                selected: true,
            }],
        })
    }
}

#[async_trait]
impl TaskConnectionDetailsGateway for RemovalWorkflowGateway {}

struct RemovalWorkflowSession {
    removed: Arc<AtomicBool>,
    terminal: bool,
}

impl RemovalWorkflowSession {
    fn original_task(&self) -> TaskSnapshot {
        let status = if self.terminal {
            DownloadStatus::Error
        } else {
            DownloadStatus::Active
        };
        let mut task = TaskSnapshot::new(Gid::from_u64(7), status, "item.bin");
        task.metadata.directory = Some(EnginePath::new("D:/downloads"));
        task
    }

    fn removed_task(&self) -> TaskSnapshot {
        let mut task = TaskSnapshot::new(Gid::from_u64(7), DownloadStatus::Removed, "item.bin");
        task.metadata.directory = Some(EnginePath::new("D:/downloads"));
        task
    }

    fn live(&self) -> LiveSyncSnapshot {
        let active =
            (!self.terminal && !self.removed.load(Ordering::Acquire)).then(|| self.original_task());
        LiveSyncSnapshot {
            active: active.into_iter().collect(),
            waiting: Vec::new(),
        }
    }

    fn stopped(&self, offset: usize) -> StoppedPage {
        let removed = self.removed.load(Ordering::Acquire);
        let tasks = if self.terminal && !removed {
            vec![self.original_task()]
        } else if !self.terminal && removed {
            vec![self.removed_task()]
        } else {
            Vec::new()
        };
        StoppedPage {
            offset,
            total: tasks.len(),
            tasks,
        }
    }
}

#[async_trait]
impl DownloadSyncSession for RemovalWorkflowSession {
    async fn initial_snapshot(
        &self,
        _stopped_count: u32,
    ) -> Result<InitialSyncSnapshot, SyncError> {
        Ok(InitialSyncSnapshot {
            capabilities: EngineCapabilities {
                version: "test".into(),
                enabled_features: Vec::new(),
                methods: Vec::new(),
            },
            global_stat: GlobalStat::default(),
            live: self.live(),
            stopped: self.stopped(0),
        })
    }

    async fn refresh_global_stat(&self) -> Result<GlobalStat, SyncError> {
        Ok(GlobalStat::default())
    }

    async fn refresh_live(&self) -> Result<LiveSyncSnapshot, SyncError> {
        Ok(self.live())
    }

    async fn refresh_stopped_page(
        &self,
        offset: usize,
        _count: u32,
    ) -> Result<StoppedPage, SyncError> {
        Ok(self.stopped(offset))
    }

    async fn refresh_tasks(
        &self,
        gids: &[Gid],
    ) -> Result<Vec<(Gid, Option<TaskSnapshot>)>, SyncError> {
        let live = self.live();
        let stopped = self.stopped(0);
        Ok(gids
            .iter()
            .copied()
            .map(|gid| {
                let task = live
                    .active
                    .iter()
                    .chain(stopped.tasks.iter())
                    .find(|task| task.gid == gid)
                    .cloned();
                (gid, task)
            })
            .collect())
    }

    async fn close(&self) {}
}

struct RemovalWorkflowConnector {
    removed: Arc<AtomicBool>,
    remove_calls: Arc<AtomicUsize>,
    events: Arc<Mutex<Vec<&'static str>>>,
    terminal: bool,
    outcome_unknown: bool,
    notifications_rx: Mutex<Option<mpsc::Receiver<RefreshHint>>>,
    _notifications_tx: mpsc::Sender<RefreshHint>,
}

#[async_trait]
impl DownloadSyncConnector for RemovalWorkflowConnector {
    async fn connect(&self) -> Result<ConnectedSyncSession, SyncError> {
        let notifications = self
            .notifications_rx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .ok_or_else(|| SyncError::new(SyncErrorKind::Internal, "connector reused", false))?;
        let gateway = Arc::new(RemovalWorkflowGateway {
            removed: self.removed.clone(),
            remove_calls: self.remove_calls.clone(),
            events: self.events.clone(),
            outcome_unknown: self.outcome_unknown,
        });
        Ok(ConnectedSyncSession::new_with_gateways(
            Box::new(RemovalWorkflowSession {
                removed: self.removed.clone(),
                terminal: self.terminal,
            }),
            gateway.clone(),
            gateway.clone(),
            gateway,
            notifications,
        ))
    }
}

struct RecordingTaskFileGateway {
    events: Arc<Mutex<Vec<&'static str>>>,
}

#[async_trait]
impl TaskFileGateway for RecordingTaskFileGateway {
    fn preflight(
        &self,
        _request: &TaskFileRemovalRequest,
    ) -> Result<TaskFileRemovalPreview, GatewayError> {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push("preflight");
        Ok(TaskFileRemovalPreview {
            content_files: 1,
            control_files: 1,
            missing_paths: 0,
        })
    }

    async fn move_to_trash(
        &self,
        _request: &TaskFileRemovalRequest,
    ) -> Result<TaskFileRemovalReport, GatewayError> {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push("trash");
        Ok(TaskFileRemovalReport {
            moved_to_trash: 2,
            missing_paths: 0,
        })
    }
}

fn unsupported_test_gateway_error() -> GatewayError {
    GatewayError::new(
        GatewayErrorKind::Unsupported,
        "unused test operation",
        false,
    )
}

#[test]
fn domain_task_mapping_preserves_identity_progress_and_eta() {
    let mut snapshot = TaskSnapshot::new(Gid::from_u64(9), DownloadStatus::Active, "video.mkv");
    snapshot.total_length = ByteCount::new(1_000);
    snapshot.completed_length = ByteCount::new(400);
    snapshot.download_speed = ByteRate::new(100);
    let task = DownloadTask::from_snapshot(snapshot);

    let mapped = map_task("profile", task, None, None);

    assert_eq!(mapped.identity.profile_id, "profile");
    assert_eq!(mapped.identity.gid, "0000000000000009");
    assert_eq!(mapped.status, TaskStatusView::Active);
    assert_eq!(mapped.eta_seconds, Some(6));
}

#[test]
fn domain_seeding_mapping_preserves_upload_metrics_and_observed_time() {
    let mut snapshot = TaskSnapshot::new(Gid::from_u64(10), DownloadStatus::Seeding, "seed.bin");
    snapshot.total_length = ByteCount::new(100);
    snapshot.completed_length = ByteCount::new(100);
    snapshot.upload_length = ByteCount::new(250);
    snapshot.upload_speed = ByteRate::new(64);

    let mapped = map_task(
        "profile",
        DownloadTask::from_snapshot(snapshot),
        Some(65),
        None,
    );

    assert_eq!(mapped.status, TaskStatusView::Seeding);
    assert_eq!(mapped.uploaded_bytes, 250);
    assert_eq!(mapped.upload_rate, 64);
    assert_eq!(mapped.share_ratio_milli(), Some(2_500));
    assert_eq!(mapped.observed_seeding_seconds, Some(65));
    assert_eq!(mapped.eta_seconds, None);
}

#[test]
fn presentation_filter_maps_to_application_query() {
    let query = map_query(&WorkspaceQuery {
        filter: WorkspaceFilter::Completed,
        search: "archive".into(),
        sort_key: WorkspaceSortKey::Progress,
        sort_direction: WorkspaceSortDirection::Descending,
        category_id: None,
    });

    assert_eq!(query.filter, DownloadFilter::Completed);
    assert_eq!(query.search, "archive");
    assert_eq!(query.sort.key, SortKey::Progress);
    assert_eq!(query.sort.direction, SortDirection::Descending);
}

#[test]
fn ui_session_round_trip_preserves_the_exact_engine_identity() {
    let expected = EngineSession::new(
        ProfileId::new(),
        EngineSessionId::new(),
        SessionGeneration::from_u64(42),
    );
    let view = EngineSessionView {
        profile_id: expected.profile_id.to_string(),
        session_id: expected.session_id.to_string(),
        generation: expected.generation.get(),
    };

    assert_eq!(map_engine_session(&view), Ok(expected));
}

#[test]
fn command_error_mapping_preserves_unknown_outcome_semantics() {
    let outcome = map_command_outcome(CommandOutcome::Failure {
        failed: vec![ItemFailure {
            item: None,
            error: ApplicationError::new(
                ApplicationErrorCode::OutcomeUnknown,
                "The socket closed after the request was sent.",
                false,
            ),
        }],
    });

    let CommandOutcomeView::Failure(error) = outcome else {
        panic!("unknown command outcome must remain a failure")
    };
    assert!(error.outcome_unknown());
    assert!(!error.retryable);
}

#[test]
fn batch_outcome_mapping_preserves_successes_and_item_failures() {
    let profile_id = ProfileId::new();
    let succeeded = DomainTaskIdentity::new(profile_id, Gid::from_u64(1));
    let failed = DomainTaskIdentity::new(profile_id, Gid::from_u64(2));
    let outcome = map_batch_command_outcome(CommandOutcome::PartialSuccess {
        succeeded: vec![CommandItem::Task(succeeded)],
        failed: vec![ItemFailure {
            item: Some(CommandItem::Task(failed)),
            error: ApplicationError::new(
                ApplicationErrorCode::Rejected,
                "Task is already complete.",
                false,
            ),
        }],
    });

    let BatchCommandOutcomeView::PartialSuccess { succeeded, failed } = outcome else {
        panic!("partial batch outcome must remain partial")
    };
    assert_eq!(succeeded.len(), 1);
    assert_eq!(succeeded[0].gid, Gid::from_u64(1).to_string());
    assert_eq!(failed.len(), 1);
    assert_eq!(
        failed[0].identity.as_ref().map(|task| task.gid.as_str()),
        Some("0000000000000002")
    );
    assert_eq!(failed[0].error.code, "rpc.command_rejected");
}

#[test]
fn batch_retry_reconciliation_resolves_unknown_items_in_partial_success() {
    let profile_id = ProfileId::new();
    let known_success = DomainTaskIdentity::new(profile_id, Gid::from_u64(20));
    let unknown_original = DomainTaskIdentity::new(profile_id, Gid::from_u64(8));
    let mut original = TaskSnapshot::new(unknown_original.gid, DownloadStatus::Error, "failed.bin");
    original.metadata.primary_uri = Some("https://example.test/failed.bin".into());
    let original = DownloadTask::from_snapshot(original);
    let mut replacement =
        TaskSnapshot::new(Gid::from_u64(21), DownloadStatus::Waiting, "failed.bin");
    replacement.metadata.primary_uri = original.metadata.primary_uri.clone();
    let replacement = DownloadTask::from_snapshot(replacement);
    let outcome = CommandOutcome::PartialSuccess {
        succeeded: vec![CommandItem::Task(known_success)],
        failed: vec![ItemFailure {
            item: Some(CommandItem::Task(unknown_original)),
            error: ApplicationError::new(
                ApplicationErrorCode::OutcomeUnknown,
                "retry response was lost",
                false,
            ),
        }],
    };
    assert!(command_outcome_is_unknown(&outcome));

    let reconciled = reconcile_retry_outcome(
        RetryReconciliationBaseline {
            known_gids: HashSet::from([unknown_original.gid]),
            originals: HashMap::from([(unknown_original, original)]),
        },
        profile_id,
        &[replacement],
        outcome,
    );

    let CommandOutcome::Success { succeeded } = reconciled else {
        panic!("all retry items should be reconciled as successes")
    };
    assert_eq!(
        succeeded,
        vec![
            CommandItem::Task(known_success),
            CommandItem::Task(DomainTaskIdentity::new(profile_id, Gid::from_u64(21))),
        ]
    );
}

#[test]
fn system_proxy_mode_ignores_persisted_manual_fields_and_credentials() {
    // Persisted manual endpoints / username must not leak into System apply path.
    // Concrete host values come from the OS/env and are covered in ariadeck-settings tests.
    let settings = AppSettings {
        download_proxy: DownloadProxySettings {
            mode: DownloadProxyMode::System,
            all_proxy: Some("http://stale-manual.example:1".into()),
            http_proxy: Some("http://stale-http.example:2".into()),
            username: Some("stale-user".into()),
            ..DownloadProxySettings::default()
        },
        ..AppSettings::new("downloads")
    };
    let config =
        map_download_proxy_config(&settings, Some(SecretString::new("stale-secret".into())))
            .expect("resolve system proxy");
    assert!(
        matches!(
            config.mode,
            ApplicationProxyMode::System | ApplicationProxyMode::Disabled
        ),
        "system mode maps to System (or Disabled when OS has no proxy), got {:?}",
        config.mode
    );
    assert_ne!(
        config.all_proxy.as_deref(),
        Some("http://stale-manual.example:1")
    );
    assert_ne!(
        config.http_proxy.as_deref(),
        Some("http://stale-http.example:2")
    );
    assert!(config.username.is_none());
    assert!(config.password.is_none());
}

#[test]
fn details_mapping_keeps_remote_paths_as_display_strings() {
    let gid = Gid::from_u64(7);
    let details = TaskDetails {
        gid,
        directory: Some(EnginePath::new("/srv/downloads")),
        info_hash: None,
        piece_length: Some(ByteCount::new(1_024)),
        piece_count: Some(2),
        trackers: vec![ariadeck_domain::TaskTracker {
            tier: 1,
            uri: "https://tracker.example/announce/passkey-secret?token=x".into(),
        }],
        files: vec![TaskFile {
            index: 1,
            path: EnginePath::new("/srv/downloads/archive.bin"),
            length: ByteCount::new(2_048),
            completed_length: ByteCount::new(1_024),
            selected: true,
        }],
    };
    let mut connection = TaskConnectionDetails::new(gid);
    connection.uris.push(ariadeck_domain::TaskUri {
        uri: "https://user:secret@example.test/archive.bin?token=private".into(),
        status: TaskUriStatus::Used,
    });
    connection.servers.push(ariadeck_domain::TaskServer {
        file_index: 1,
        uri: "https://user:secret@cdn.example/file?sig=abc".into(),
        current_uri: "https://user:secret@cdn.example/file?sig=abc".into(),
        download_speed: ByteRate::new(1_024),
    });
    connection.options.push(ariadeck_domain::TaskOptionEntry {
        key: "http-passwd".into(),
        value: String::new(),
        redacted: true,
    });
    let mapped = map_task_details(details, connection, TaskPathValidationView::Unavailable);

    assert_eq!(mapped.directory.as_deref(), Some("/srv/downloads"));
    assert_eq!(mapped.files[0].path, "/srv/downloads/archive.bin");
    assert_eq!(mapped.files[0].completed_length, 1_024);
    assert_eq!(mapped.trackers[0].tier, 1);
    assert_eq!(mapped.trackers[0].uri, "https://tracker.example/");
    assert!(!mapped.trackers[0].uri.contains("passkey"));
    assert_eq!(mapped.uris[0].status, TaskUriStatusView::Used);
    assert_eq!(mapped.uris[0].uri, "https://example.test/archive.bin");
    assert!(!mapped.uris[0].uri.contains("secret"));
    assert_eq!(mapped.servers[0].uri, "https://cdn.example/file");
    assert_eq!(mapped.servers[0].current_uri, "https://cdn.example/file");
    assert_eq!(
        mapped.primary_source.as_deref(),
        Some("https://example.test/archive.bin")
    );
    assert!(mapped.options[0].redacted);
    assert!(mapped.options[0].value.is_empty());
}

#[tokio::test]
async fn managed_details_reuse_the_safe_file_preflight_without_blocking_remote_profiles() {
    let gid = Gid::from_u64(12);
    let details = TaskDetails {
        gid,
        directory: Some(EnginePath::new("D:/Downloads")),
        info_hash: None,
        piece_length: None,
        piece_count: None,
        trackers: Vec::new(),
        files: vec![TaskFile {
            index: 1,
            path: EnginePath::new("D:/Downloads/item.bin"),
            length: ByteCount::new(10),
            completed_length: ByteCount::new(5),
            selected: true,
        }],
    };
    let events = Arc::new(Mutex::new(Vec::new()));
    let gateway = Arc::new(RecordingTaskFileGateway {
        events: events.clone(),
    }) as Arc<dyn TaskFileGateway>;

    let local =
        validate_task_paths(&tokio::runtime::Handle::current(), Some(gateway), &details).await;
    assert_eq!(
        local,
        TaskPathValidationView::Valid {
            existing_files: 1,
            missing_paths: 0,
        }
    );
    assert_eq!(
        events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_slice(),
        ["preflight"]
    );
    assert_eq!(
        validate_task_paths(&tokio::runtime::Handle::current(), None, &details).await,
        TaskPathValidationView::Unavailable
    );
}

#[test]
fn task_mapping_exposes_a_specific_disk_space_failure() {
    let mut snapshot = TaskSnapshot::new(Gid::from_u64(9), DownloadStatus::Error, "large.iso");
    snapshot.error = Some(ariadeck_domain::TaskError {
        code: Some(9),
        message: "File allocation failed".into(),
    });

    let mapped = map_task("profile", DownloadTask::from_snapshot(snapshot), None, None);

    assert_eq!(mapped.status, TaskStatusView::Failed);
    assert_eq!(
        mapped.error,
        Some(TaskErrorView {
            code: Some(9),
            summary: "Not enough disk space in the download directory.".into(),
            details: Some("File allocation failed".into()),
        })
    );
}

#[tokio::test]
async fn advanced_source_controls_map_into_typed_add_options() {
    let advanced = AddDownloadAdvancedOptionsView {
        referer: "https://cdn.example/ref".into(),
        user_agent: "AriaDeck-Test/1.0".into(),
        headers: "X-Token: one
Accept: */*"
            .into(),
        cookie: Some(ariadeck_ui::SecretStringView::new("session=secret")),
        http_user: "alice".into(),
        http_passwd: Some(ariadeck_ui::SecretStringView::new("s3cret")),
        checksum: format!("sha-256={}", "ab".repeat(32)),
    };
    let request = prepare_add_download_request(
        &tokio::runtime::Handle::current(),
        &[AddDownloadSourceView::Uri {
            line: 1,
            uri: "https://example.test/archive.iso".into(),
        }],
        None,
        FileConflictPolicyView::AutoRename,
        advanced,
    )
    .await
    .expect("advanced URI request maps");

    let pairs = request.request.advanced.to_option_pairs();
    assert!(
        pairs
            .iter()
            .any(|(k, v)| k == "referer" && v == "https://cdn.example/ref")
    );
    assert!(
        pairs
            .iter()
            .any(|(k, v)| k == "user-agent" && v == "AriaDeck-Test/1.0")
    );
    assert!(
        pairs
            .iter()
            .any(|(k, v)| k == "header" && v == "X-Token: one")
    );
    assert!(
        pairs
            .iter()
            .any(|(k, v)| k == "header" && v == "Accept: */*")
    );
    assert!(
        pairs
            .iter()
            .any(|(k, v)| k == "header" && v == "Cookie: session=secret")
    );
    assert!(pairs.iter().any(|(k, v)| k == "http-user" && v == "alice"));
    assert!(
        pairs
            .iter()
            .any(|(k, v)| k == "http-passwd" && v == "s3cret")
    );
    assert!(
        pairs
            .iter()
            .any(|(k, v)| k == "checksum" && v.starts_with("sha-256="))
    );
    let debug = format!("{:?}", request.request.advanced);
    assert!(!debug.contains("s3cret"));
    assert!(!debug.contains("session=secret"));
}

#[tokio::test]
async fn configured_destination_is_forwarded_to_the_application_command() {
    let request = prepare_add_download_request(
        &tokio::runtime::Handle::current(),
        &[AddDownloadSourceView::Uri {
            line: 1,
            uri: "https://example.test/archive.iso".into(),
        }],
        Some("D:/Transfers".into()),
        FileConflictPolicyView::Reject,
        AddDownloadAdvancedOptionsView::default(),
    )
    .await
    .expect("URI request maps");

    assert!(matches!(
        request.request.source,
        AddDownloadSource::Uris(uris) if uris == vec!["https://example.test/archive.iso"]
    ));
    assert_eq!(
        request.request.destination.as_ref().map(EnginePath::as_str),
        Some("D:/Transfers")
    );
    assert_eq!(request.request.file_conflict, FileConflictPolicy::Reject);
    assert!(request.destination_files.is_empty());
}

#[test]
fn metadata_upload_reads_files_with_an_explicit_runtime_outside_tokio_context() {
    let root = tempfile::tempdir().expect("temporary metadata directory");
    let path = root.path().join("sample.metalink");
    let content = br#"<metalink><file name="one.bin"><size>12</size></file></metalink>"#;
    fs::write(&path, content).expect("write metadata fixture");
    let preview = parse_metadata(AddDownloadMetadataKindView::Metalink, content)
        .expect("metadata fixture parses");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("test runtime");

    let request = futures::executor::block_on(prepare_add_download_request(
        runtime.handle(),
        &[AddDownloadSourceView::MetadataFile {
            path,
            kind: AddDownloadMetadataKindView::Metalink,
            content_sha256: preview.content_sha256,
            info_hash: preview.info_hash,
            selected_file_indices: vec![1],
        }],
        Some("D:/Transfers".into()),
        FileConflictPolicyView::Overwrite,
        AddDownloadAdvancedOptionsView::default(),
    ))
    .expect("metadata request maps");

    assert_eq!(request.request.file_conflict, FileConflictPolicy::Reject);
    assert_eq!(
        request.request.destination.as_ref().map(EnginePath::as_str),
        Some("D:/Transfers")
    );
    assert!(matches!(
        request.request.source,
        AddDownloadSource::Metalink(uploaded) if uploaded.as_ref() == content
    ));
    assert_eq!(request.request.selected_file_indices, None);
    assert_eq!(request.required_bytes, Some(12));
    assert_eq!(
        request.destination_files,
        vec![DownloadDestinationFile {
            relative_path: EnginePath::new("one.bin"),
            reject_existing: true,
        }]
    );
}

#[test]
fn metadata_upload_rejects_invalid_extension_empty_and_oversized_files() {
    let root = tempfile::tempdir().expect("temporary metadata directory");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    let cases = [
        (
            "wrong.txt",
            b"content".as_slice(),
            AddDownloadMetadataKindView::Torrent,
        ),
        (
            "empty.torrent",
            b"".as_slice(),
            AddDownloadMetadataKindView::Torrent,
        ),
    ];
    for (name, content, kind) in cases {
        let path = root.path().join(name);
        fs::write(&path, content).expect("write metadata fixture");
        let result = futures::executor::block_on(prepare_add_download_request(
            runtime.handle(),
            &[AddDownloadSourceView::MetadataFile {
                path,
                kind,
                content_sha256: String::new(),
                info_hash: None,
                selected_file_indices: Vec::new(),
            }],
            None,
            FileConflictPolicyView::AutoRename,
            AddDownloadAdvancedOptionsView::default(),
        ));
        assert!(result.is_err(), "invalid metadata file must be rejected");
    }

    let oversized = root.path().join("large.metalink");
    let oversized_content = vec![b'x'; (MAX_METADATA_FILE_BYTES + 1) as usize];
    fs::write(&oversized, oversized_content).expect("write oversized metadata fixture");
    let result = futures::executor::block_on(prepare_add_download_request(
        runtime.handle(),
        &[AddDownloadSourceView::MetadataFile {
            path: oversized,
            kind: AddDownloadMetadataKindView::Metalink,
            content_sha256: String::new(),
            info_hash: None,
            selected_file_indices: Vec::new(),
        }],
        None,
        FileConflictPolicyView::AutoRename,
        AddDownloadAdvancedOptionsView::default(),
    ));
    assert!(result.is_err(), "oversized metadata file must be rejected");
}

#[test]
fn metadata_upload_binds_selection_to_the_preview_digest_and_file_indexes() {
    let root = tempfile::tempdir().expect("temporary metadata directory");
    let path = root.path().join("sample.meta4");
    let content = br#"<metalink>
        <file name="one.bin"><size>10</size></file>
        <file name="two.bin"><size>20</size></file>
    </metalink>"#;
    fs::write(&path, content).expect("write metadata fixture");
    let preview = parse_metadata(AddDownloadMetadataKindView::Metalink, content)
        .expect("metadata fixture parses");

    let partial = read_metadata_source_with_selection(
        &path,
        AddDownloadMetadataKindView::Metalink,
        &preview.content_sha256,
        &[2],
    )
    .expect("partial selection maps");
    assert_eq!(partial.selected_file_indices, Some(vec![2]));

    for invalid in [&[][..], &[0][..], &[1, 1][..], &[2, 1][..], &[3][..]] {
        let error = read_metadata_source_with_selection(
            &path,
            AddDownloadMetadataKindView::Metalink,
            &preview.content_sha256,
            invalid,
        )
        .expect_err("invalid selection must fail");
        assert_eq!(error.code, ApplicationErrorCode::Validation);
    }

    fs::write(
        &path,
        br#"<metalink><file name="replacement.bin"><size>1</size></file></metalink>"#,
    )
    .expect("replace metadata fixture");
    let changed = read_metadata_source_with_selection(
        &path,
        AddDownloadMetadataKindView::Metalink,
        &preview.content_sha256,
        &[1],
    )
    .expect_err("changed metadata must require a new preview");
    assert_eq!(changed.code, ApplicationErrorCode::Validation);
    assert!(changed.summary.contains("changed after preview"));
}

#[tokio::test]
async fn managed_local_add_rejects_an_unsafe_destination_before_submission() {
    let engine_session = EngineSession::new(
        ProfileId::new(),
        EngineSessionId::new(),
        SessionGeneration::initial(),
    );
    let result = execute_add_download(
        tokio::runtime::Handle::current(),
        None,
        Some(Arc::new(LocalDownloadDestinationGateway::new())),
        AddDownloadRequestView {
            request_id: ariadeck_ui::RequestId::from_u64(1),
            session: EngineSessionView {
                profile_id: engine_session.profile_id.to_string(),
                session_id: engine_session.session_id.to_string(),
                generation: engine_session.generation.get(),
            },
            sources: vec![AddDownloadSourceView::Uri {
                line: 1,
                uri: "https://example.test/archive.iso".into(),
            }],
            mode: AddDownloadModeView::SeparateTasks,
            destination: Some("relative/downloads".into()),
            category_id: None,
            required_bytes: None,
            file_conflict: FileConflictPolicyView::default(),
            advanced: AddDownloadAdvancedOptionsView::default(),
        },
    )
    .await;

    assert!(matches!(
        &result.items[0].outcome,
        CommandOutcomeView::Failure(error)
            if error.code == ApplicationErrorCode::UnsafePath.as_str()
    ));
}

#[tokio::test]
async fn managed_local_add_rejects_a_known_size_larger_than_free_space() {
    let downloads = tempfile::tempdir().expect("temporary download directory");
    let engine_session = EngineSession::new(
        ProfileId::new(),
        EngineSessionId::new(),
        SessionGeneration::initial(),
    );
    let result = execute_add_download(
        tokio::runtime::Handle::current(),
        None,
        Some(Arc::new(LocalDownloadDestinationGateway::new())),
        AddDownloadRequestView {
            request_id: ariadeck_ui::RequestId::from_u64(1),
            session: EngineSessionView {
                profile_id: engine_session.profile_id.to_string(),
                session_id: engine_session.session_id.to_string(),
                generation: engine_session.generation.get(),
            },
            sources: vec![AddDownloadSourceView::Uri {
                line: 1,
                uri: "https://example.test/archive.iso".into(),
            }],
            mode: AddDownloadModeView::SeparateTasks,
            destination: Some(downloads.path().to_string_lossy().into_owned()),
            category_id: None,
            required_bytes: Some(u64::MAX),
            file_conflict: FileConflictPolicyView::default(),
            advanced: AddDownloadAdvancedOptionsView::default(),
        },
    )
    .await;

    assert!(matches!(
        &result.items[0].outcome,
        CommandOutcomeView::Failure(error)
            if error.code == ApplicationErrorCode::Filesystem.as_str()
    ));
}

#[test]
fn add_reconciliation_only_matches_new_tasks_with_the_submitted_source() {
    let source = AddDownloadSourceView::Uri {
        line: 1,
        uri: "https://example.test/archive.iso".into(),
    };
    let mut matching = TaskSnapshot::new(Gid::from_u64(7), DownloadStatus::Waiting, "archive.iso");
    let AddDownloadSourceView::Uri { uri, .. } = &source else {
        unreachable!();
    };
    matching.metadata.primary_uri = Some(uri.clone());
    let matching = DownloadTask::from_snapshot(matching);
    let unrelated = DownloadTask::from_snapshot(TaskSnapshot::new(
        Gid::from_u64(8),
        DownloadStatus::Waiting,
        "other.iso",
    ));
    let tasks = vec![matching, unrelated];

    assert_eq!(
        find_new_matching_add_task(&tasks, std::slice::from_ref(&source), &HashSet::new())
            .map(|task| task.gid),
        Some(Gid::from_u64(7))
    );
    assert!(
        find_new_matching_add_task(&tasks, &[source], &HashSet::from([Gid::from_u64(7)])).is_none(),
        "a pre-existing matching task must not resolve this submission"
    );
}

#[test]
fn magnet_reconciliation_normalizes_base32_btih_values() {
    let bytes = (0_u8..20).collect::<Vec<_>>();
    let info_hash = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let encoded = data_encoding::BASE32_NOPAD.encode(&bytes);
    let source = AddDownloadSourceView::Uri {
        line: 1,
        uri: format!("magnet:?xt=URN:BTIH:{encoded}"),
    };
    let mut snapshot = TaskSnapshot::new(Gid::from_u64(9), DownloadStatus::Waiting, "metadata");
    snapshot.metadata.info_hash = Some(info_hash);
    let task = DownloadTask::from_snapshot(snapshot);

    assert!(task_matches_add_sources(&task, &[source]));
}

#[test]
fn torrent_metadata_matches_an_existing_task_by_info_hash() {
    let mut snapshot = TaskSnapshot::new(
        Gid::from_u64(10),
        DownloadStatus::Waiting,
        "archive.torrent",
    );
    snapshot.metadata.info_hash = Some("0123456789abcdef0123456789abcdef01234567".into());
    let task = DownloadTask::from_snapshot(snapshot);
    let source = AddDownloadSourceView::MetadataFile {
        path: PathBuf::from("archive.torrent"),
        kind: AddDownloadMetadataKindView::Torrent,
        content_sha256: "digest".into(),
        info_hash: Some("0123456789ABCDEF0123456789ABCDEF01234567".into()),
        selected_file_indices: vec![1],
    };

    assert!(task_matches_add_sources(&task, &[source]));
}

#[test]
fn magnet_tracker_variants_share_one_submission_key() {
    let first = AddDownloadSourceView::Uri {
        line: 1,
        uri:
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&tr=https%3A%2F%2Fone.test"
                .into(),
    };
    let second = AddDownloadSourceView::Uri {
        line: 2,
        uri:
            "magnet:?tr=https%3A%2F%2Ftwo.test&xt=URN:BTIH:0123456789ABCDEF0123456789ABCDEF01234567"
                .into(),
    };

    assert_eq!(
        add_source_submission_key(&first),
        add_source_submission_key(&second)
    );
}

#[test]
fn equivalent_uri_spellings_share_one_submission_duplicate_key() {
    assert_eq!(
        normalize_add_uri_key("HTTP://EXAMPLE.TEST:80/archive.iso"),
        normalize_add_uri_key("http://example.test/archive.iso")
    );
}

#[test]
fn task_sources_are_sanitized_before_entering_row_or_info_views() {
    assert_eq!(
        sanitize_source_uri("https://user:secret@example.test/archive.iso?token=private#fragment"),
        "https://example.test/archive.iso"
    );
    assert_eq!(
        sanitize_source_uri(
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&tr=https%3A%2F%2Ftracker.test&dn=private"
        ),
        "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"
    );
}

#[test]
fn aria2_filesystem_errors_keep_raw_details_and_actionable_summaries() {
    let expected = [
        (9, "Not enough disk space"),
        (10, "piece length"),
        (11, "Output conflict"),
        (12, "Output conflict"),
        (13, "Output conflict"),
        (14, "rename"),
        (15, "open"),
        (16, "create"),
        (17, "filesystem input/output"),
        (18, "create the download directory"),
    ];
    for (code, summary_fragment) in expected {
        let mapped = classify_task_error(ariadeck_domain::TaskError {
            code: Some(code),
            message: format!("raw aria2 error {code}"),
        });
        assert!(mapped.summary.contains(summary_fragment));
        assert_eq!(mapped.details, Some(format!("raw aria2 error {code}")));
    }

    let permission = classify_task_error(ariadeck_domain::TaskError {
        code: Some(17),
        message: "Access is denied".into(),
    });
    assert!(permission.summary.contains("Permission denied"));
    let path_length = classify_task_error(ariadeck_domain::TaskError {
        code: Some(17),
        message: "Windows error 206: path too long".into(),
    });
    assert!(path_length.summary.contains("too long"));
}

#[tokio::test]
async fn add_execution_groups_separate_tasks_and_mirrors_explicitly() {
    let engine_session = EngineSession::new(
        ProfileId::new(),
        EngineSessionId::new(),
        SessionGeneration::initial(),
    );
    let session = EngineSessionView {
        profile_id: engine_session.profile_id.to_string(),
        session_id: engine_session.session_id.to_string(),
        generation: engine_session.generation.get(),
    };
    let sources = vec![
        AddDownloadSourceView::Uri {
            line: 1,
            uri: "https://example.test/file".into(),
        },
        AddDownloadSourceView::Uri {
            line: 2,
            uri: "https://example.test/file".into(),
        },
    ];

    let separate = execute_add_download(
        tokio::runtime::Handle::current(),
        None,
        None,
        AddDownloadRequestView {
            request_id: ariadeck_ui::RequestId::from_u64(1),
            session: session.clone(),
            sources: sources.clone(),
            mode: AddDownloadModeView::SeparateTasks,
            destination: None,
            category_id: None,
            required_bytes: None,
            file_conflict: FileConflictPolicyView::default(),
            advanced: AddDownloadAdvancedOptionsView::default(),
        },
    )
    .await;
    assert_eq!(separate.items.len(), 2);
    assert!(matches!(
        &separate.items[1].outcome,
        CommandOutcomeView::Failure(error)
            if error.code == ApplicationErrorCode::Validation.as_str()
    ));

    let mirrors = execute_add_download(
        tokio::runtime::Handle::current(),
        None,
        None,
        AddDownloadRequestView {
            request_id: ariadeck_ui::RequestId::from_u64(2),
            session,
            sources,
            mode: AddDownloadModeView::Mirrors,
            destination: None,
            category_id: None,
            required_bytes: None,
            file_conflict: FileConflictPolicyView::default(),
            advanced: AddDownloadAdvancedOptionsView::default(),
        },
    )
    .await;
    assert_eq!(mirrors.items.len(), 1);
    assert_eq!(mirrors.items[0].sources.len(), 2);
}

#[tokio::test]
async fn unknown_add_outcome_refreshes_and_resolves_the_new_gid_without_replay() {
    let accepted = Arc::new(AtomicBool::new(false));
    let add_calls = Arc::new(AtomicUsize::new(0));
    let (notifications_tx, notifications_rx) = mpsc::channel(4);
    let profile_id = ProfileId::new();
    let connector = Arc::new(UnknownAcceptedAddConnector {
        accepted,
        add_calls: add_calls.clone(),
        notifications_rx: Mutex::new(Some(notifications_rx)),
        _notifications_tx: notifications_tx,
    });
    let handle = spawn_sync_coordinator(connector, CoordinatorConfig::new(profile_id));
    let connected = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(snapshot) = handle
                .snapshot(ariadeck_application::TaskListQuery::default())
                .await
                && snapshot.connection_state == ConnectionState::Connected
                && !snapshot.stale
            {
                break snapshot;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("coordinator connects");
    let session = EngineSessionView {
        profile_id: connected.session.profile_id.to_string(),
        session_id: connected.session.session_id.to_string(),
        generation: connected.session.generation.get(),
    };

    let result = execute_add_download(
        tokio::runtime::Handle::current(),
        Some(handle.clone()),
        None,
        AddDownloadRequestView {
            request_id: ariadeck_ui::RequestId::from_u64(9),
            session,
            sources: vec![AddDownloadSourceView::Uri {
                line: 1,
                uri: "https://example.test/resolved.bin".into(),
            }],
            mode: AddDownloadModeView::SeparateTasks,
            destination: None,
            category_id: None,
            required_bytes: None,
            file_conflict: FileConflictPolicyView::default(),
            advanced: AddDownloadAdvancedOptionsView::default(),
        },
    )
    .await;

    assert_eq!(result.items.len(), 1);
    assert!(matches!(
        &result.items[0].outcome,
        CommandOutcomeView::Success { tasks }
            if tasks.iter().any(|task| task.gid == Gid::from_u64(42).to_string())
    ));
    assert_eq!(add_calls.load(Ordering::Relaxed), 1);
    handle.stop().await;
}

#[tokio::test]
async fn unknown_retry_outcome_refreshes_and_resolves_one_new_gid_without_replay() {
    let (result, retry_calls, gids) = run_unknown_retry(true).await;

    assert!(matches!(
        result.outcome,
        CommandOutcomeView::Success { tasks }
            if tasks.iter().any(|task| task.gid == Gid::from_u64(42).to_string())
    ));
    assert_eq!(retry_calls, 1);
    assert!(
        gids.contains(&Gid::from_u64(7)),
        "old failed result remains"
    );
    assert!(
        gids.contains(&Gid::from_u64(42)),
        "new retry task is visible"
    );
}

#[tokio::test]
async fn authoritative_retry_refresh_without_a_new_task_is_safe_to_retry() {
    let (result, retry_calls, gids) = run_unknown_retry(false).await;

    let CommandOutcomeView::Failure(error) = result.outcome else {
        panic!("unobserved retry must remain a failure")
    };
    assert_eq!(error.code, ApplicationErrorCode::RetryNotObserved.as_str());
    assert!(error.retryable);
    assert_eq!(retry_calls, 1);
    assert_eq!(gids, vec![Gid::from_u64(7)]);
}

async fn run_unknown_retry(observe_replacement: bool) -> (TaskCommandResultView, usize, Vec<Gid>) {
    let accepted = Arc::new(AtomicBool::new(false));
    let retry_calls = Arc::new(AtomicUsize::new(0));
    let (notifications_tx, notifications_rx) = mpsc::channel(4);
    let profile_id = ProfileId::new();
    let connector = Arc::new(UnknownAcceptedRetryConnector {
        accepted,
        retry_calls: retry_calls.clone(),
        observe_replacement,
        notifications_rx: Mutex::new(Some(notifications_rx)),
        _notifications_tx: notifications_tx,
    });
    let handle = spawn_sync_coordinator(connector, CoordinatorConfig::new(profile_id));
    let connected = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(snapshot) = handle
                .snapshot(ariadeck_application::TaskListQuery::default())
                .await
                && snapshot.connection_state == ConnectionState::Connected
                && !snapshot.stale
                && snapshot
                    .tasks
                    .iter()
                    .any(|task| task.gid == Gid::from_u64(7))
            {
                break snapshot;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("coordinator connects with the failed task");
    let session = EngineSessionView {
        profile_id: connected.session.profile_id.to_string(),
        session_id: connected.session.session_id.to_string(),
        generation: connected.session.generation.get(),
    };
    let identity = TaskIdentity {
        profile_id: session.profile_id.clone(),
        gid: Gid::from_u64(7).to_string(),
    };

    let result = execute_task_command(
        Some(handle.clone()),
        None,
        TaskCommandRequestView {
            request_id: ariadeck_ui::RequestId::from_u64(10),
            session,
            identity,
            command: TaskCommandView::Retry,
        },
    )
    .await;
    let gids = handle
        .snapshot(ariadeck_application::TaskListQuery::default())
        .await
        .expect("post-retry snapshot")
        .tasks
        .into_iter()
        .map(|task| task.gid)
        .collect();
    handle.stop().await;
    (result, retry_calls.load(Ordering::Relaxed), gids)
}

#[tokio::test]
async fn live_file_removal_waits_for_unknown_engine_removal_before_trash() {
    let (result, remove_calls, events) = run_file_removal(false, true, true).await;

    assert!(matches!(result.outcome, CommandOutcomeView::Success { .. }));
    assert_eq!(remove_calls, 1);
    assert_eq!(
        events,
        vec!["details", "preflight", "engine_remove", "trash"]
    );
}

#[tokio::test]
async fn terminal_file_removal_moves_files_before_forgetting_the_record() {
    let (result, remove_calls, events) = run_file_removal(true, false, true).await;

    assert!(matches!(result.outcome, CommandOutcomeView::Success { .. }));
    assert_eq!(remove_calls, 1);
    assert_eq!(
        events,
        vec!["details", "preflight", "trash", "engine_remove"]
    );
}

#[tokio::test]
async fn external_engine_file_removal_is_rejected_before_any_mutation() {
    let (result, remove_calls, events) = run_file_removal(false, false, false).await;

    let CommandOutcomeView::Failure(error) = result.outcome else {
        panic!("external file removal must fail")
    };
    assert_eq!(error.code, ApplicationErrorCode::Unsupported.as_str());
    assert_eq!(remove_calls, 0);
    assert!(events.is_empty());
}

#[test]
fn authoritative_unchanged_task_makes_unknown_removal_safe_to_retry() {
    let profile_id = ProfileId::new();
    let identity = DomainTaskIdentity::new(profile_id, Gid::from_u64(7));
    let original = DownloadTask::from_snapshot(TaskSnapshot::new(
        identity.gid,
        DownloadStatus::Active,
        "item.bin",
    ));
    let outcome = CommandOutcome::Failure {
        failed: vec![ItemFailure {
            item: Some(CommandItem::Task(identity)),
            error: ApplicationError::new(
                ApplicationErrorCode::OutcomeUnknown,
                "response lost",
                false,
            ),
        }],
    };
    let reconciled = reconcile_remove_outcome(
        &RemoveReconciliationBaseline {
            originals: HashMap::from([(identity, original.clone())]),
        },
        &[original],
        outcome,
    );

    let CommandOutcome::Failure { failed } = reconciled else {
        panic!("unchanged removal must remain a failure")
    };
    assert_eq!(
        failed[0].error.code,
        ApplicationErrorCode::RemovalNotObserved
    );
    assert!(failed[0].error.retryable);
}

async fn run_file_removal(
    terminal: bool,
    outcome_unknown: bool,
    local_files: bool,
) -> (TaskCommandResultView, usize, Vec<&'static str>) {
    let removed = Arc::new(AtomicBool::new(false));
    let remove_calls = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(Mutex::new(Vec::new()));
    let (notifications_tx, notifications_rx) = mpsc::channel(4);
    let profile_id = ProfileId::new();
    let connector = Arc::new(RemovalWorkflowConnector {
        removed,
        remove_calls: remove_calls.clone(),
        events: events.clone(),
        terminal,
        outcome_unknown,
        notifications_rx: Mutex::new(Some(notifications_rx)),
        _notifications_tx: notifications_tx,
    });
    let handle = spawn_sync_coordinator(connector, CoordinatorConfig::new(profile_id));
    let connected = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(snapshot) = handle
                .snapshot(ariadeck_application::TaskListQuery::default())
                .await
                && snapshot.connection_state == ConnectionState::Connected
                && !snapshot.stale
                && snapshot
                    .tasks
                    .iter()
                    .any(|task| task.gid == Gid::from_u64(7))
            {
                break snapshot;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("coordinator connects with removable task");
    let session = EngineSessionView {
        profile_id: connected.session.profile_id.to_string(),
        session_id: connected.session.session_id.to_string(),
        generation: connected.session.generation.get(),
    };
    let file_gateway = local_files.then(|| {
        Arc::new(RecordingTaskFileGateway {
            events: events.clone(),
        }) as Arc<dyn TaskFileGateway>
    });
    let result = execute_task_command(
        Some(handle.clone()),
        file_gateway,
        TaskCommandRequestView {
            request_id: ariadeck_ui::RequestId::from_u64(11),
            identity: TaskIdentity {
                profile_id: session.profile_id.clone(),
                gid: Gid::from_u64(7).to_string(),
            },
            session,
            command: TaskCommandView::RemoveTaskAndFiles,
        },
    )
    .await;
    handle.stop().await;
    let event_log = events
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    (result, remove_calls.load(Ordering::Relaxed), event_log)
}

#[test]
fn settings_mapping_preserves_theme_and_download_directory() {
    let general_id = ariadeck_settings::Uuid::new_v4().to_string();
    let settings = SettingsView {
        color_scheme: ColorSchemeView::Light,
        language: LanguagePreferenceView::default(),
        download_directory: "D:/Transfers".into(),
        categories: vec![DownloadCategoryView {
            id: general_id,
            name: "General".into(),
            directory: "D:/Transfers".into(),
            extensions: String::new(),
            is_fallback: true,
        }],
        ..SettingsView::default()
    };

    let current = AppSettings::new("D:/Downloads");
    let (mapped, password) =
        map_settings_request(&settings, &current, ProxyPasswordUpdateView::Unchanged)
            .expect("valid settings");
    assert!(matches!(password, ProxyPasswordUpdate::Unchanged));
    assert_eq!(mapped.color_scheme, ColorScheme::Light);
    assert_eq!(mapped.download_directory, PathBuf::from("D:/Transfers"));
    assert_eq!(map_settings(&mapped), settings);
}

#[test]
fn system_theme_and_list_preferences_round_trip_through_settings_mapping() {
    let mut current = AppSettings::new("D:/Downloads");
    current.color_scheme = ColorScheme::System;
    current.ui = UiPreferences {
        list_filter: ListFilterPreference::Paused,
        list_sort_key: ListSortKeyPreference::Name,
        list_sort_direction: ListSortDirectionPreference::Descending,
    };
    let settings = map_settings(&current);
    assert_eq!(settings.color_scheme, ColorSchemeView::System);

    let (mapped, _) = map_settings_request(&settings, &current, ProxyPasswordUpdateView::Unchanged)
        .expect("valid system theme settings");
    assert_eq!(mapped.color_scheme, ColorScheme::System);
    assert_eq!(mapped.ui, current.ui);

    let query = map_ui_preferences_to_query(&current.ui);
    assert_eq!(query.filter, WorkspaceFilter::Paused);
    assert_eq!(query.sort_key, WorkspaceSortKey::Name);
    assert_eq!(query.sort_direction, WorkspaceSortDirection::Descending);
    assert!(query.search.is_empty());
}

#[test]
fn rpc_runtime_settings_have_remote_defaults_and_accept_bounded_overrides() {
    let local = RpcRuntimeConfig::from_values(false, |_| None).expect("local defaults");
    let remote = RpcRuntimeConfig::from_values(true, |_| None).expect("remote defaults");
    assert_eq!(local.connect_timeout, Duration::from_millis(750));
    assert_eq!(local.request_timeout, Duration::from_secs(5));
    assert_eq!(remote.connect_timeout, Duration::from_secs(10));
    assert_eq!(remote.request_timeout, Duration::from_secs(15));
    assert_eq!(remote.reconnect, ReconnectPolicy::default());
    assert!(!remote.allow_insecure_remote);

    let values = HashMap::from([
        ("ARIADECK_RPC_CONNECT_TIMEOUT_MS", "1200"),
        ("ARIADECK_RPC_REQUEST_TIMEOUT_MS", "3400"),
        ("ARIADECK_RPC_RECONNECT_BASE_DELAY_MS", "100"),
        ("ARIADECK_RPC_RECONNECT_MAX_DELAY_MS", "8000"),
        ("ARIADECK_RPC_RECONNECT_RESET_AFTER_MS", "2500"),
        ("ARIADECK_RPC_RECONNECT_MAX_ATTEMPTS", "7"),
        ("ARIADECK_RPC_ALLOW_INSECURE_REMOTE", "true"),
    ]);
    let configured =
        RpcRuntimeConfig::from_values(true, |name| values.get(name).map(ToString::to_string))
            .expect("valid overrides");

    assert_eq!(configured.connect_timeout, Duration::from_millis(1200));
    assert_eq!(configured.request_timeout, Duration::from_millis(3400));
    assert_eq!(configured.reconnect.base_delay, Duration::from_millis(100));
    assert_eq!(configured.reconnect.max_delay, Duration::from_secs(8));
    assert_eq!(
        configured.reconnect.reset_after,
        Duration::from_millis(2500)
    );
    assert_eq!(configured.reconnect.max_attempts, Some(7));
    assert!(configured.allow_insecure_remote);
}

#[test]
fn invalid_rpc_runtime_settings_fail_without_echoing_the_value() {
    let sensitive = "do-not-echo-this";
    let error = RpcRuntimeConfig::from_values(true, |name| {
        (name == "ARIADECK_RPC_CONNECT_TIMEOUT_MS").then(|| sensitive.to_owned())
    })
    .expect_err("invalid duration must fail");
    assert!(error.contains("ARIADECK_RPC_CONNECT_TIMEOUT_MS"));
    assert!(!error.contains(sensitive));

    let reversed = HashMap::from([
        ("ARIADECK_RPC_RECONNECT_BASE_DELAY_MS", "5000"),
        ("ARIADECK_RPC_RECONNECT_MAX_DELAY_MS", "1000"),
    ]);
    assert!(
        RpcRuntimeConfig::from_values(true, |name| { reversed.get(name).map(ToString::to_string) })
            .is_err()
    );

    assert!(
        RpcRuntimeConfig::from_values(true, |name| {
            (name == "ARIADECK_RPC_RECONNECT_MAX_ATTEMPTS").then(|| "0".into())
        })
        .is_err()
    );
}

#[test]
fn settings_mapping_allocates_a_credential_reference_without_exposing_the_password() {
    let current = AppSettings::new("D:/Downloads");
    let settings = SettingsView {
        color_scheme: ColorSchemeView::Dark,
        language: LanguagePreferenceView::default(),
        download_directory: "D:/Downloads".into(),
        download_proxy: DownloadProxySettingsView {
            mode: ProxyModeView::Manual,
            all_proxy: "proxy.example:8080".into(),
            username: "proxy-user".into(),
            has_password: true,
            ..DownloadProxySettingsView::default()
        },
        speed_limits: SpeedLimitSettingsView::default(),
        transfer_policy: TransferPolicySettingsView::default(),
        notifications: NotificationSettingsView::default(),
        platform: PlatformSettingsView::default(),
        categories: vec![DownloadCategoryView {
            id: ariadeck_settings::Uuid::new_v4().to_string(),
            name: "General".into(),
            directory: "D:/Downloads".into(),
            extensions: String::new(),
            is_fallback: true,
        }],
        tracker_list: Default::default(),
    };

    let (mapped, password) = map_settings_request(
        &settings,
        &current,
        ProxyPasswordUpdateView::Set(ariadeck_ui::SecretStringView::new("secret-value")),
    )
    .expect("valid proxy settings");

    assert!(mapped.download_proxy.credential.is_some());
    assert!(matches!(password, ProxyPasswordUpdate::Set(_)));
    assert_eq!(map_settings(&mapped), settings);
    assert!(!format!("{:?}", map_settings(&mapped)).contains("secret-value"));
}

#[test]
fn settings_mapping_clears_the_credential_only_when_explicitly_requested() {
    let credential = ProxyCredentialRef::new();
    let mut current = AppSettings::new("D:/Downloads");
    current.download_proxy.username = Some("proxy-user".into());
    current.download_proxy.credential = Some(credential);
    let mut settings = map_settings(&current);
    settings.download_proxy.has_password = false;

    let (unchanged, update) =
        map_settings_request(&settings, &current, ProxyPasswordUpdateView::Unchanged)
            .expect("unchanged credential remains valid");
    assert_eq!(unchanged.download_proxy.credential, Some(credential));
    assert!(matches!(update, ProxyPasswordUpdate::Unchanged));

    let (detached, update) =
        map_settings_request(&settings, &current, ProxyPasswordUpdateView::Detach)
            .expect("credential detach is valid");
    assert!(detached.download_proxy.credential.is_none());
    assert!(matches!(update, ProxyPasswordUpdate::Detach));

    let (cleared, update) =
        map_settings_request(&settings, &current, ProxyPasswordUpdateView::Clear)
            .expect("explicit credential clear is valid");
    assert!(cleared.download_proxy.credential.is_none());
    assert!(matches!(update, ProxyPasswordUpdate::Clear));
}

#[test]
fn credential_detach_keeps_the_keychain_entry_untouched() {
    use secrecy::ExposeSecret as _;

    let credentials = FakeProxyCredentialStore::default();
    let credential = ProxyCredentialRef::new();
    let mut previous = AppSettings::new("D:/Downloads");
    previous.download_proxy.username = Some("proxy-user".into());
    previous.download_proxy.credential = Some(credential);
    credentials
        .save(credential, &SecretString::new("local-secret".into()))
        .expect("seed credential");
    let mut next = previous.clone();
    next.download_proxy.credential = None;
    let previous_password =
        load_proxy_password(&credentials, &previous).expect("load previous password");

    let (password, mutation) = apply_credential_update(
        &credentials,
        &previous,
        &next,
        &ProxyPasswordUpdate::Detach,
        previous_password,
    )
    .expect("detach credential");

    assert!(password.is_none());
    assert!(mutation.credential.is_none());
    assert_eq!(
        credentials
            .load(credential)
            .expect("load detached credential")
            .expect("credential remains")
            .expose_secret(),
        "local-secret"
    );
}

#[test]
fn credential_update_can_restore_the_previous_password() {
    use secrecy::ExposeSecret as _;

    let credentials = FakeProxyCredentialStore::default();
    let credential = ProxyCredentialRef::new();
    let mut previous = AppSettings::new("D:/Downloads");
    previous.download_proxy.username = Some("proxy-user".into());
    previous.download_proxy.credential = Some(credential);
    credentials
        .save(credential, &SecretString::new("old-secret".into()))
        .expect("seed credential");
    let next = previous.clone();
    let previous_password =
        load_proxy_password(&credentials, &previous).expect("load previous password");

    let (password, mutation) = apply_credential_update(
        &credentials,
        &previous,
        &next,
        &ProxyPasswordUpdate::Set(SecretString::new("new-secret".into())),
        previous_password,
    )
    .expect("replace credential");

    assert_eq!(
        password
            .as_ref()
            .map(|password| password.expose_secret().as_str()),
        Some("new-secret")
    );
    assert_eq!(
        credentials
            .load(credential)
            .expect("load replacement")
            .as_ref()
            .map(|password| password.expose_secret().as_str()),
        Some("new-secret")
    );

    rollback_credential(&credentials, &mutation).expect("restore credential");
    let restored = credentials
        .load(credential)
        .expect("load restored credential")
        .expect("restored password");
    assert_eq!(restored.expose_secret(), "old-secret");
}

#[test]
fn proxy_settings_load_uses_the_explicit_runtime_outside_a_tokio_context() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    let expected = AppSettings::new(root.path().join("downloads"));
    store.save(&expected).expect("save settings");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("test runtime");

    let load = spawn_proxy_settings_load(
        runtime.handle(),
        store,
        Arc::new(FakeProxyCredentialStore::default()),
    );
    let (loaded, password) = runtime
        .block_on(load)
        .expect("settings task completes")
        .expect("settings load succeeds");

    assert_eq!(loaded, expected);
    assert!(password.is_none());
}

#[test]
fn save_settings_and_profile_env_dual_writes_active_bag() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    let env_store = ProfileEnvironmentStore::new(root.path());
    let profile_id = ProfileId::new();
    let mut settings = AppSettings::new(root.path().join("downloads"));
    settings.speed_limits.download_limit = 4_096;
    save_settings_and_profile_env(&store, &env_store, Some(profile_id), &settings)
        .expect("dual write");
    let reloaded = store.load().expect("reload settings");
    assert_eq!(reloaded.speed_limits.download_limit, 4_096);
    let env = env_store
        .load(profile_id.as_uuid())
        .expect("reload env bag");
    assert_eq!(env.speed_limits.download_limit, 4_096);
    assert_eq!(env.download_directory, settings.download_directory);
}

#[test]
fn settings_worker_persists_requests_in_order_and_drains_on_close() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime"),
    );
    let profile_env_store = ProfileEnvironmentStore::new(root.path());
    let (sender, task, mut results) = spawn_settings_persistence(
        runtime.clone(),
        store.clone(),
        profile_env_store,
        Some(Arc::new(LocalDownloadDestinationGateway::new())),
        None,
        Arc::new(SystemProxyCredentialStore::new("AriaDeck test")),
    );
    let first = AppSettings {
        color_scheme: ColorScheme::Dark,
        language: LanguagePreference::default(),
        download_directory: root.path().join("first"),
        download_proxy: DownloadProxySettings::default(),
        speed_limits: SpeedLimitSettings::default(),
        transfer_policy: TransferPolicySettings::default(),
        notifications: NotificationSettings::default(),
        platform: PlatformSettings::default(),
        ui: UiPreferences::default(),
        categories: ariadeck_settings::default_download_categories(root.path().join("first")),
        tracker_list: Default::default(),
    };
    let second = AppSettings {
        color_scheme: ColorScheme::Light,
        language: LanguagePreference::default(),
        download_directory: root.path().join("second"),
        download_proxy: DownloadProxySettings::default(),
        speed_limits: SpeedLimitSettings::default(),
        transfer_policy: TransferPolicySettings::default(),
        notifications: NotificationSettings::default(),
        platform: PlatformSettings::default(),
        ui: UiPreferences::default(),
        categories: ariadeck_settings::default_download_categories(root.path().join("second")),
        tracker_list: Default::default(),
    };
    sender
        .send(SettingsPersistenceRequest {
            request_id: ariadeck_ui::RequestId::from_u64(1),
            settings: first.clone(),
            previous_settings: first.clone(),
            proxy_password: ProxyPasswordUpdate::Unchanged,
            apply_proxy: false,
            apply_speed_limit: false,
            apply_transfer_policy: false,
            apply_bt_tracker: false,
            active_profile_id: None,
        })
        .expect("queue first settings");
    sender
        .send(SettingsPersistenceRequest {
            request_id: ariadeck_ui::RequestId::from_u64(2),
            settings: second.clone(),
            previous_settings: first.clone(),
            proxy_password: ProxyPasswordUpdate::Unchanged,
            apply_proxy: false,
            apply_speed_limit: false,
            apply_transfer_policy: false,
            apply_bt_tracker: false,
            active_profile_id: None,
        })
        .expect("queue second settings");
    drop(sender);

    runtime.block_on(async {
        let first_result = results.recv().await.expect("first result");
        let second_result = results.recv().await.expect("second result");
        assert_eq!(first_result.request_id.get(), 1);
        assert!(
            first_result.result.is_ok(),
            "first settings save failed: {:?}",
            first_result.result
        );
        assert_eq!(second_result.request_id.get(), 2);
        assert!(
            second_result.result.is_ok(),
            "second settings save failed: {:?}",
            second_result.result
        );
        task.await.expect("settings worker join");
    });

    assert!(first.download_directory.is_dir());
    assert!(second.download_directory.is_dir());
    assert_eq!(store.load().expect("load final settings"), second);
}

#[test]
fn external_engine_settings_do_not_touch_the_desktop_filesystem() {
    let root = tempfile::tempdir().expect("temporary directory");
    let store = JsonSettingsStore::new(root.path().join("settings.json"));
    let remote_path = root.path().join("remote-engine-only");
    let settings = AppSettings {
        color_scheme: ColorScheme::Dark,
        language: LanguagePreference::default(),
        download_directory: remote_path.clone(),
        download_proxy: DownloadProxySettings::default(),
        speed_limits: SpeedLimitSettings::default(),
        transfer_policy: TransferPolicySettings::default(),
        notifications: NotificationSettings::default(),
        platform: PlatformSettings::default(),
        ui: UiPreferences::default(),
        categories: ariadeck_settings::default_download_categories(&remote_path),
        tracker_list: Default::default(),
    };

    persist_settings(&store, &settings, None).expect("persist external engine path");

    assert!(!remote_path.exists());
    assert_eq!(store.load().expect("load persisted settings"), settings);
}

#[test]
fn local_engine_health_mapping_preserves_recovery_and_failure_context() {
    assert_eq!(
        map_local_engine_health(LocalEngineHealth::Running { restarts: 2 }),
        EngineHealthView::Running { restarts: 2 }
    );
    assert_eq!(
        map_local_engine_health(LocalEngineHealth::Failed {
            restarts: 2,
            reason: "restart budget exhausted".into(),
        }),
        EngineHealthView::Failed {
            summary: "restart budget exhausted".into(),
        }
    );
}

#[test]
fn profile_catalog_save_preserves_remote_secret_ref_when_unchanged() {
    let root = tempfile::tempdir().expect("temporary directory");
    let data_dir = root.path().to_path_buf();
    let settings = AppSettings::new(data_dir.join("downloads"));
    let secret_ref = ariadeck_engine::RpcSecretRef::new();
    let remote = ProfileEntry::remote_rpc(
        ProfileId::new(),
        "NAS",
        "wss://nas.example/jsonrpc",
        data_dir.join("downloads"),
        Some(secret_ref),
    )
    .expect("remote profile");
    let local = ProfileEntry::local_managed(
        ProfileId::new(),
        "Local",
        PathBuf::from("aria2c"),
        data_dir.clone(),
        data_dir.join("downloads"),
    );
    let existing = ProfileCatalog {
        schema_version: ariadeck_engine::PROFILE_CATALOG_SCHEMA_VERSION,
        active_profile_id: local.profile_id,
        profiles: vec![local.clone(), remote.clone()],
    };
    let view = ProfileCatalogView {
        active_profile_id: local.profile_id.to_string(),
        profiles: vec![
            ProfileEntryView {
                profile_id: local.profile_id.to_string(),
                name: "Local".into(),
                kind: ProfileKindView::LocalManaged,
                executable: String::new(), // managed-core opt-in
                download_dir: data_dir.join("downloads").to_string_lossy().into_owned(),
                endpoint: String::new(),
                has_secret: false,
            },
            ProfileEntryView {
                profile_id: remote.profile_id.to_string(),
                name: "NAS".into(),
                kind: ProfileKindView::RemoteRpc,
                executable: String::new(),
                download_dir: data_dir.join("downloads").to_string_lossy().into_owned(),
                endpoint: "wss://nas.example/jsonrpc".into(),
                has_secret: true,
            },
        ],
    };
    let mapped = map_profile_catalog_request(
        &view,
        &std::collections::HashMap::new(),
        &existing,
        &data_dir,
        &settings,
    )
    .expect("map catalog");
    let mapped_remote = mapped
        .profiles
        .iter()
        .find(|profile| profile.profile_id == remote.profile_id)
        .expect("remote retained");
    assert_eq!(mapped_remote.secret_ref, Some(secret_ref));
    assert!(mapped_remote.has_secret);
    let mapped_local = mapped
        .profiles
        .iter()
        .find(|profile| profile.profile_id == local.profile_id)
        .expect("local retained");
    assert!(mapped_local.uses_managed_core());
}

#[test]
fn resolve_data_dir_prefers_explicit_env_over_portable_and_os() {
    let root = tempfile::tempdir().expect("temp");
    let explicit = root.path().join("custom-data");
    let exe_dir = root.path().join("app");
    fs::create_dir_all(&exe_dir).expect("exe dir");
    fs::write(exe_dir.join(PORTABLE_MARKER_FILE), b"").expect("marker");
    let local_app = root.path().join("LocalAppData");

    let resolved = resolve_data_dir(
        |key| match key {
            "ARIADECK_DATA_DIR" => Some(explicit.as_os_str().to_owned()),
            "LOCALAPPDATA" => Some(local_app.as_os_str().to_owned()),
            _ => None,
        },
        Some(exe_dir.as_path()),
    );
    assert_eq!(resolved, explicit);
}

#[test]
fn resolve_data_dir_uses_portable_data_when_marker_present() {
    let root = tempfile::tempdir().expect("temp");
    let exe_dir = root.path().join("portable-app");
    fs::create_dir_all(&exe_dir).expect("exe dir");
    fs::write(exe_dir.join(PORTABLE_MARKER_FILE), b"").expect("marker");
    let local_app = root.path().join("LocalAppData");

    let resolved = resolve_data_dir(
        |key| match key {
            "LOCALAPPDATA" => Some(local_app.as_os_str().to_owned()),
            _ => None,
        },
        Some(exe_dir.as_path()),
    );
    assert_eq!(resolved, exe_dir.join("data"));
}

#[test]
fn resolve_data_dir_falls_back_to_localappdata_without_marker() {
    let root = tempfile::tempdir().expect("temp");
    let exe_dir = root.path().join("installed-app");
    fs::create_dir_all(&exe_dir).expect("exe dir");
    let local_app = root.path().join("LocalAppData");

    let resolved = resolve_data_dir(
        |key| match key {
            "LOCALAPPDATA" => Some(local_app.as_os_str().to_owned()),
            _ => None,
        },
        Some(exe_dir.as_path()),
    );
    assert_eq!(resolved, local_app.join("AriaDeck"));
}

#[test]
fn resolve_download_dir_prefers_env_then_userprofile_downloads() {
    let data = PathBuf::from("/tmp/ariadeck-data");
    let from_env = resolve_download_dir(
        |key| match key {
            "ARIADECK_DOWNLOAD_DIR" => Some(std::ffi::OsString::from("/custom/dl")),
            _ => None,
        },
        data.clone(),
    );
    assert_eq!(from_env, PathBuf::from("/custom/dl"));

    let from_profile = resolve_download_dir(
        |key| match key {
            "USERPROFILE" => Some(std::ffi::OsString::from("C:/Users/demo")),
            _ => None,
        },
        data.clone(),
    );
    assert_eq!(from_profile, PathBuf::from("C:/Users/demo/Downloads"));

    let fallback = resolve_download_dir(|_| None, data.clone());
    assert_eq!(fallback, data.join("downloads"));
}
