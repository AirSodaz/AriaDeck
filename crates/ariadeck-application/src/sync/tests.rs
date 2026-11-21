use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use ariadeck_domain::{
    ConnectionState, DownloadStatus, EnginePath, Gid, GlobalStat, ProfileId, TaskConnectionDetails,
    TaskDetails, TaskFile, TaskIdentity, TaskSnapshot, TaskSourceKind,
};
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{
    AppCommand, ApplicationError, ApplicationErrorCode, CommandOutcome, DownloadEngineGateway,
    DownloadProxyConfig, NullHistoryStore, TaskConnectionDetailsGateway, TaskDetailsGateway,
    TaskHistoryStore, TaskListQuery,
};

use super::{
    backoff::ReconnectBackoff,
    connected::{build_snapshot, emit_global_update, refresh_all},
    types::validate_capabilities,
    *,
};

struct FakeSession {
    initial: InitialSyncSnapshot,
    targeted_calls: Arc<Mutex<Vec<Vec<Gid>>>>,
}

#[async_trait]
impl DownloadSyncSession for FakeSession {
    async fn initial_snapshot(
        &self,
        _stopped_count: u32,
    ) -> Result<InitialSyncSnapshot, SyncError> {
        Ok(self.initial.clone())
    }

    async fn refresh_global_stat(&self) -> Result<GlobalStat, SyncError> {
        Ok(self.initial.global_stat)
    }

    async fn refresh_live(&self) -> Result<LiveSyncSnapshot, SyncError> {
        Ok(self.initial.live.clone())
    }

    async fn refresh_stopped_page(
        &self,
        _offset: usize,
        _count: u32,
    ) -> Result<StoppedPage, SyncError> {
        Ok(self.initial.stopped.clone())
    }

    async fn refresh_tasks(
        &self,
        gids: &[Gid],
    ) -> Result<Vec<(Gid, Option<TaskSnapshot>)>, SyncError> {
        self.targeted_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(gids.to_vec());
        Ok(gids
            .iter()
            .copied()
            .map(|gid| {
                (
                    gid,
                    Some(TaskSnapshot::new(gid, DownloadStatus::Active, "refreshed")),
                )
            })
            .collect())
    }

    async fn close(&self) {}
}

struct FailingInitialSession {
    error: SyncError,
}

#[async_trait]
impl DownloadSyncSession for FailingInitialSession {
    async fn initial_snapshot(
        &self,
        _stopped_count: u32,
    ) -> Result<InitialSyncSnapshot, SyncError> {
        Err(self.error.clone())
    }

    async fn refresh_global_stat(&self) -> Result<GlobalStat, SyncError> {
        Err(self.error.clone())
    }

    async fn refresh_live(&self) -> Result<LiveSyncSnapshot, SyncError> {
        Err(self.error.clone())
    }

    async fn refresh_stopped_page(
        &self,
        _offset: usize,
        _count: u32,
    ) -> Result<StoppedPage, SyncError> {
        Err(self.error.clone())
    }

    async fn refresh_tasks(
        &self,
        _gids: &[Gid],
    ) -> Result<Vec<(Gid, Option<TaskSnapshot>)>, SyncError> {
        Err(self.error.clone())
    }

    async fn close(&self) {}
}

struct HangingRefreshSession {
    initial: InitialSyncSnapshot,
    refresh_calls: Arc<AtomicUsize>,
    close_calls: Arc<AtomicUsize>,
}

#[async_trait]
impl DownloadSyncSession for HangingRefreshSession {
    async fn initial_snapshot(
        &self,
        _stopped_count: u32,
    ) -> Result<InitialSyncSnapshot, SyncError> {
        Ok(self.initial.clone())
    }

    async fn refresh_global_stat(&self) -> Result<GlobalStat, SyncError> {
        self.refresh_calls.fetch_add(1, Ordering::Relaxed);
        std::future::pending().await
    }

    async fn refresh_live(&self) -> Result<LiveSyncSnapshot, SyncError> {
        Ok(self.initial.live.clone())
    }

    async fn refresh_stopped_page(
        &self,
        _offset: usize,
        _count: u32,
    ) -> Result<StoppedPage, SyncError> {
        Ok(self.initial.stopped.clone())
    }

    async fn refresh_tasks(
        &self,
        _gids: &[Gid],
    ) -> Result<Vec<(Gid, Option<TaskSnapshot>)>, SyncError> {
        Ok(Vec::new())
    }

    async fn close(&self) {
        self.close_calls.fetch_add(1, Ordering::Relaxed);
    }
}

struct ScriptedConnector {
    steps: Mutex<VecDeque<Result<ConnectedSyncSession, SyncError>>>,
    calls: AtomicUsize,
}

#[derive(Default)]
struct FakeInteractiveGateway {
    command_calls: Mutex<Vec<(&'static str, Gid)>>,
    details_calls: Mutex<Vec<Gid>>,
    proxy_calls: Mutex<Vec<DownloadProxyConfig>>,
    change_options_error: Mutex<Option<crate::GatewayError>>,
    change_options_attempts: AtomicUsize,
}

#[async_trait]
impl DownloadEngineGateway for FakeInteractiveGateway {
    async fn add_download(
        &self,
        _request: &crate::AddDownloadRequest,
    ) -> Result<Vec<Gid>, crate::GatewayError> {
        let gid = Gid::from_u64(99);
        self.command_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(("add", gid));
        Ok(vec![gid])
    }

    async fn retry_download(
        &self,
        gid: Gid,
        _fallback: &crate::AddDownloadRequest,
    ) -> Result<Gid, crate::GatewayError> {
        self.record_command("retry", gid);
        Ok(Gid::from_u64(100))
    }

    async fn pause(&self, gid: Gid) -> Result<(), crate::GatewayError> {
        self.record_command("pause", gid);
        Ok(())
    }

    async fn resume(&self, gid: Gid) -> Result<(), crate::GatewayError> {
        self.record_command("resume", gid);
        Ok(())
    }

    async fn change_options(
        &self,
        gid: Gid,
        _options: &[(String, String)],
    ) -> Result<(), crate::GatewayError> {
        self.change_options_attempts.fetch_add(1, Ordering::Relaxed);
        if let Some(error) = self
            .change_options_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            return Err(error);
        }
        self.record_command("change_options", gid);
        Ok(())
    }

    async fn apply_download_proxy(
        &self,
        config: &DownloadProxyConfig,
    ) -> Result<(), crate::GatewayError> {
        self.proxy_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(config.clone());
        Ok(())
    }

    async fn remove(
        &self,
        gid: Gid,
        _target: crate::TaskRemovalTarget,
    ) -> Result<(), crate::GatewayError> {
        self.record_command("remove", gid);
        Ok(())
    }
}

#[async_trait]
impl TaskDetailsGateway for FakeInteractiveGateway {
    async fn task_details(&self, gid: Gid) -> Result<TaskDetails, crate::GatewayError> {
        self.details_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(gid);
        Ok(TaskDetails {
            gid,
            directory: Some(EnginePath::new("/downloads")),
            info_hash: None,
            piece_length: None,
            piece_count: None,
            trackers: Vec::new(),
            files: vec![TaskFile {
                index: 1,
                path: EnginePath::new("/downloads/item.bin"),
                length: ariadeck_domain::ByteCount::new(10),
                completed_length: ariadeck_domain::ByteCount::new(5),
                selected: true,
            }],
        })
    }
}

#[async_trait]
impl TaskConnectionDetailsGateway for FakeInteractiveGateway {
    async fn connection_details(
        &self,
        gid: Gid,
        _active: bool,
        _is_bittorrent: bool,
    ) -> Result<TaskConnectionDetails, crate::GatewayError> {
        Ok(TaskConnectionDetails::new(gid))
    }
}

impl FakeInteractiveGateway {
    fn record_command(&self, operation: &'static str, gid: Gid) {
        self.command_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((operation, gid));
    }
}

struct HangingConnector;

#[async_trait]
impl DownloadSyncConnector for HangingConnector {
    async fn connect(&self) -> Result<ConnectedSyncSession, SyncError> {
        std::future::pending().await
    }
}

#[async_trait]
impl DownloadSyncConnector for ScriptedConnector {
    async fn connect(&self) -> Result<ConnectedSyncSession, SyncError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.steps
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_front()
            .unwrap_or_else(|| {
                Err(SyncError::new(
                    SyncErrorKind::Internal,
                    "connector script exhausted",
                    false,
                ))
            })
    }
}

fn initial_snapshot(gid: Gid) -> InitialSyncSnapshot {
    InitialSyncSnapshot {
        capabilities: EngineCapabilities {
            version: "1.37.0".into(),
            enabled_features: vec!["BitTorrent".into()],
            methods: Vec::new(),
        },
        global_stat: GlobalStat::default(),
        live: LiveSyncSnapshot {
            active: vec![TaskSnapshot::new(gid, DownloadStatus::Active, "initial")],
            waiting: Vec::new(),
        },
        stopped: StoppedPage {
            offset: 0,
            total: 0,
            tasks: Vec::new(),
        },
    }
}

fn test_config(profile_id: ProfileId) -> CoordinatorConfig {
    CoordinatorConfig {
        profile_id,
        refresh: RefreshPolicy {
            foreground: PollIntervals {
                global_stat: Duration::from_secs(60),
                live_tasks: Duration::from_secs(60),
                stopped_tasks: Duration::from_secs(60),
            },
            background: PollIntervals {
                global_stat: Duration::from_secs(60),
                live_tasks: Duration::from_secs(60),
                stopped_tasks: Duration::from_secs(60),
            },
            notification_debounce: Duration::from_millis(40),
        },
        reconnect: ReconnectPolicy {
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(1),
            jitter_percent: 0,
            max_attempts: Some(3),
            reset_after: Duration::from_secs(60),
        },
        stopped_page_size: 10,
        history: Arc::new(NullHistoryStore),
    }
}

async fn wait_until_connected(handle: &SyncHandle) -> StoreSnapshot {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(snapshot) = handle.snapshot(TaskListQuery::default()).await
            && snapshot.connection_state == ConnectionState::Connected
        {
            return snapshot;
        }
        assert!(Instant::now() < deadline, "coordinator did not connect");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

async fn wait_until_failed(handle: &SyncHandle) -> StoreSnapshot {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(snapshot) = handle.snapshot(TaskListQuery::default()).await
            && matches!(snapshot.connection_state, ConnectionState::Failed { .. })
        {
            return snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "coordinator did not enter failed state"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[test]
fn capability_verification_rejects_missing_engine_version() {
    let error = validate_capabilities(&EngineCapabilities {
        version: "  ".into(),
        enabled_features: Vec::new(),
        methods: Vec::new(),
    });

    assert!(matches!(
        error,
        Err(SyncError {
            kind: SyncErrorKind::Protocol,
            retryable: false,
            ..
        })
    ));
}

#[test]
fn capability_helpers_treat_empty_probe_as_unknown_and_strict_as_false() {
    let unknown = EngineCapabilities {
        version: "1.37.0".into(),
        enabled_features: Vec::new(),
        methods: Vec::new(),
    };
    assert!(!unknown.methods_probed());
    assert!(!unknown.supports_method_strict("aria2.forcePause"));
    assert!(unknown.allows_method("aria2.forcePause"));
    assert!(unknown.supports_force_pause());

    let probed = EngineCapabilities {
        version: "1.37.0".into(),
        enabled_features: vec!["BitTorrent".into()],
        methods: vec![
            "aria2.pause".into(),
            "aria2.changeGlobalOption".into(),
            "system.listMethods".into(),
        ],
    };
    assert!(probed.methods_probed());
    assert!(probed.supports_change_global_option());
    assert!(!probed.supports_force_pause());
    assert!(!probed.supports_queue_positioning());
    assert!(
        EngineCapabilities::unsupported_method_message("aria2.forcePause").contains("force-pause")
    );
}

#[test]
fn reconnect_backoff_is_bounded_and_resettable() {
    let policy = ReconnectPolicy {
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_millis(400),
        jitter_percent: 0,
        max_attempts: Some(5),
        reset_after: Duration::from_secs(10),
    };
    let mut backoff = ReconnectBackoff::new(policy, 1);

    assert_eq!(backoff.next_delay(), Duration::from_millis(100));
    assert_eq!(backoff.next_delay(), Duration::from_millis(200));
    assert_eq!(backoff.next_delay(), Duration::from_millis(400));
    assert_eq!(backoff.next_delay(), Duration::from_millis(400));
    backoff.reset();
    assert_eq!(backoff.next_delay(), Duration::from_millis(100));
}

#[test]
fn jitter_never_exceeds_configured_max_delay() {
    let policy = ReconnectPolicy {
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_millis(100),
        jitter_percent: 20,
        max_attempts: None,
        reset_after: Duration::from_secs(10),
    };
    let mut backoff = ReconnectBackoff::new(policy, 7);

    for _ in 0..20 {
        let delay = backoff.next_delay();
        assert!(delay >= Duration::from_millis(80));
        assert!(delay <= Duration::from_millis(100));
    }
}

#[test]
fn unchanged_global_stat_still_publishes_a_bounded_speed_sample() {
    let profile_id = ProfileId::new();
    let generation = SessionGeneration::initial();
    let mut store = DownloadStore::new(EngineSession::new(
        profile_id,
        EngineSessionId::new(),
        generation,
    ));
    let stat = GlobalStat::default();
    store
        .record_speed_sample(generation, stat)
        .expect("record initial speed sample");
    let patch = store
        .update_global_stat(generation, stat)
        .expect("update unchanged global stat");
    assert!(patch.is_empty());
    let (events, mut receiver) = broadcast::channel(4);

    emit_global_update(&events, patch);

    assert!(matches!(
        receiver.try_recv(),
        Ok(SyncEvent::SpeedHistoryChanged)
    ));
    let snapshot = build_snapshot(
        &store,
        &ConnectionState::Connected,
        &TaskListQuery::default(),
        &EngineCapabilities::default(),
    );
    assert_eq!(snapshot.speed_history.samples().len(), 1);
}

#[tokio::test]
async fn coordinator_reconnects_after_retryable_connect_failure() {
    let profile_id = ProfileId::new();
    let gid = Gid::from_u64(1);
    let targeted_calls = Arc::new(Mutex::new(Vec::new()));
    let (notification_tx, notification_rx) = mpsc::channel(16);
    let connector = Arc::new(ScriptedConnector {
        steps: Mutex::new(VecDeque::from([
            Err(SyncError::new(
                SyncErrorKind::Disconnected,
                "first connection failed",
                true,
            )),
            Ok(ConnectedSyncSession::new(
                Box::new(FakeSession {
                    initial: initial_snapshot(gid),
                    targeted_calls,
                }),
                notification_rx,
            )),
        ])),
        calls: AtomicUsize::new(0),
    });
    let handle = spawn_sync_coordinator(connector.clone(), test_config(profile_id));

    let snapshot = wait_until_connected(&handle).await;

    assert!(!snapshot.stale);
    assert_eq!(snapshot.tasks.len(), 1);
    assert_eq!(connector.calls.load(Ordering::Relaxed), 2);
    handle.stop().await;
    drop(notification_tx);
}

#[tokio::test]
async fn commands_and_details_require_the_exact_connected_session() {
    let profile_id = ProfileId::new();
    let gid = Gid::from_u64(11);
    let targeted_calls = Arc::new(Mutex::new(Vec::new()));
    let gateway = Arc::new(FakeInteractiveGateway::default());
    let (notification_tx, notification_rx) = mpsc::channel(16);
    let mut initial = initial_snapshot(gid);
    initial.live.active[0].metadata.source_kind = TaskSourceKind::DirectUri;
    let connector = Arc::new(ScriptedConnector {
        steps: Mutex::new(VecDeque::from([Ok(
            ConnectedSyncSession::new_with_gateways(
                Box::new(FakeSession {
                    initial,
                    targeted_calls,
                }),
                gateway.clone(),
                gateway.clone(),
                gateway.clone(),
                notification_rx,
            ),
        )])),
        calls: AtomicUsize::new(0),
    });
    let handle = spawn_sync_coordinator(connector, test_config(profile_id));
    let snapshot = wait_until_connected(&handle).await;
    let identity = TaskIdentity::new(profile_id, gid);

    let outcome = handle
        .execute(snapshot.session, AppCommand::PauseTasks(vec![identity]))
        .await;
    assert!(matches!(outcome, CommandOutcome::Success { .. }));
    let output_name = handle
        .execute(
            snapshot.session,
            AppCommand::SetTaskOutputName(crate::SetTaskOutputNameRequest {
                task: identity,
                output_name: "renamed.bin".into(),
            }),
        )
        .await;
    assert!(matches!(output_name, CommandOutcome::Success { .. }));
    let details = handle.task_details(snapshot.session, identity).await;
    assert!(matches!(details, Ok(TaskDetails { gid: value, .. }) if value == gid));
    let connection_details = handle
        .connection_details(snapshot.session, identity, true, false)
        .await;
    assert!(matches!(
        connection_details,
        Ok(TaskConnectionDetails { gid: value, .. }) if value == gid
    ));
    let proxy = DownloadProxyConfig::default();
    assert!(
        handle
            .apply_download_proxy(snapshot.session, proxy.clone())
            .await
            .is_ok()
    );

    let stale_session = EngineSession::new(
        profile_id,
        EngineSessionId::new(),
        snapshot.session.generation,
    );
    let stale_outcome = handle
        .execute(stale_session, AppCommand::PauseTasks(vec![identity]))
        .await;
    let CommandOutcome::Failure { failed } = stale_outcome else {
        panic!("expected stale-session failure");
    };
    assert_eq!(failed[0].error.code, ApplicationErrorCode::StaleSession);
    let stale_details = handle.task_details(stale_session, identity).await;
    assert!(matches!(
        stale_details,
        Err(ApplicationError {
            code: ApplicationErrorCode::StaleSession,
            ..
        })
    ));
    let stale_connection_details = handle
        .connection_details(stale_session, identity, true, false)
        .await;
    assert!(matches!(
        stale_connection_details,
        Err(ApplicationError {
            code: ApplicationErrorCode::StaleSession,
            ..
        })
    ));
    let stale_proxy = handle
        .apply_download_proxy(stale_session, proxy.clone())
        .await;
    assert!(matches!(
        stale_proxy,
        Err(ApplicationError {
            code: ApplicationErrorCode::StaleSession,
            ..
        })
    ));

    assert_eq!(
        gateway
            .command_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_slice(),
        &[("pause", gid), ("change_options", gid)]
    );
    assert_eq!(
        gateway
            .details_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_slice(),
        &[gid]
    );
    assert_eq!(
        gateway
            .proxy_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_slice(),
        &[proxy]
    );
    handle.stop().await;
    drop(notification_tx);
}

#[tokio::test]
async fn unknown_output_name_outcome_triggers_a_targeted_refresh_without_replay() {
    let profile_id = ProfileId::new();
    let gid = Gid::from_u64(12);
    let targeted_calls = Arc::new(Mutex::new(Vec::new()));
    let gateway = Arc::new(FakeInteractiveGateway::default());
    *gateway
        .change_options_error
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(crate::GatewayError::new(
        crate::GatewayErrorKind::OutcomeUnknown,
        "response lost after mutation",
        false,
    ));
    let (notification_tx, notification_rx) = mpsc::channel(16);
    let mut initial = initial_snapshot(gid);
    initial.live.active[0].metadata.source_kind = TaskSourceKind::DirectUri;
    let connector = Arc::new(ScriptedConnector {
        steps: Mutex::new(VecDeque::from([Ok(
            ConnectedSyncSession::new_with_gateways(
                Box::new(FakeSession {
                    initial,
                    targeted_calls: targeted_calls.clone(),
                }),
                gateway.clone(),
                gateway.clone(),
                gateway.clone(),
                notification_rx,
            ),
        )])),
        calls: AtomicUsize::new(0),
    });
    let handle = spawn_sync_coordinator(connector, test_config(profile_id));
    let snapshot = wait_until_connected(&handle).await;
    let identity = TaskIdentity::new(profile_id, gid);

    let outcome = handle
        .execute(
            snapshot.session,
            AppCommand::SetTaskOutputName(crate::SetTaskOutputNameRequest {
                task: identity,
                output_name: "renamed.bin".into(),
            }),
        )
        .await;
    assert!(outcome.has_unknown_outcome());

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if targeted_calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .any(|call| call == &[gid])
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("unknown mutation must trigger a targeted refresh");
    assert_eq!(gateway.change_options_attempts.load(Ordering::Relaxed), 1);

    handle.stop().await;
    drop(notification_tx);
}

#[tokio::test]
async fn commands_are_rejected_while_the_connector_is_still_pending() {
    let profile_id = ProfileId::new();
    let handle = spawn_sync_coordinator(Arc::new(HangingConnector), test_config(profile_id));
    let snapshot = tokio::time::timeout(
        Duration::from_millis(100),
        handle.snapshot(TaskListQuery::default()),
    )
    .await
    .unwrap_or_else(|_| panic!("snapshot request was not serviced while connecting"))
    .unwrap_or_else(|| panic!("coordinator stopped while connecting"));

    let outcome = tokio::time::timeout(
        Duration::from_millis(100),
        handle.execute(
            snapshot.session,
            AppCommand::AddDownload(crate::AddDownloadRequest {
                source: crate::AddDownloadSource::Uris(vec!["https://example.test/item".into()]),
                destination: None,
                file_conflict: crate::FileConflictPolicy::default(),
                selected_file_indices: None,
                advanced: Default::default(),
                options: Vec::new(),
            }),
        ),
    )
    .await
    .unwrap_or_else(|_| panic!("command was queued while connecting"));
    let CommandOutcome::Failure { failed } = outcome else {
        panic!("expected disconnected command failure");
    };
    assert_eq!(failed[0].error.code, ApplicationErrorCode::Disconnected);

    handle.stop().await;
}

#[tokio::test]
async fn notification_storm_is_deduplicated_into_one_targeted_batch() {
    let profile_id = ProfileId::new();
    let gid = Gid::from_u64(2);
    let targeted_calls = Arc::new(Mutex::new(Vec::new()));
    let (notification_tx, notification_rx) = mpsc::channel(32);
    let connector = Arc::new(ScriptedConnector {
        steps: Mutex::new(VecDeque::from([Ok(ConnectedSyncSession::new(
            Box::new(FakeSession {
                initial: initial_snapshot(gid),
                targeted_calls: targeted_calls.clone(),
            }),
            notification_rx,
        ))])),
        calls: AtomicUsize::new(0),
    });
    let handle = spawn_sync_coordinator(connector, test_config(profile_id));
    let _ = wait_until_connected(&handle).await;

    for _ in 0..10 {
        if notification_tx.send(RefreshHint::Task(gid)).await.is_err() {
            panic!("notification channel closed unexpectedly");
        }
    }
    tokio::time::sleep(Duration::from_millis(90)).await;

    {
        let calls = targeted_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(calls.as_slice(), &[vec![gid]]);
    }
    handle.stop().await;
    drop(notification_tx);
}

#[tokio::test]
async fn retry_limit_applies_to_initial_snapshot_failures() {
    let profile_id = ProfileId::new();
    let error = SyncError::new(SyncErrorKind::Timeout, "initial snapshot timed out", true);
    let mut steps = VecDeque::new();
    let mut notification_senders = Vec::new();
    for _ in 0..3 {
        let (sender, receiver) = mpsc::channel(1);
        notification_senders.push(sender);
        steps.push_back(Ok(ConnectedSyncSession::new(
            Box::new(FailingInitialSession {
                error: error.clone(),
            }),
            receiver,
        )));
    }
    let connector = Arc::new(ScriptedConnector {
        steps: Mutex::new(steps),
        calls: AtomicUsize::new(0),
    });
    let handle = spawn_sync_coordinator(connector.clone(), test_config(profile_id));

    let snapshot = wait_until_failed(&handle).await;

    assert!(snapshot.stale);
    assert_eq!(connector.calls.load(Ordering::Relaxed), 3);
    handle.stop().await;
    drop(notification_senders);
}

#[tokio::test]
async fn short_lived_connections_do_not_reset_retry_budget() {
    let profile_id = ProfileId::new();
    let gid = Gid::from_u64(3);
    let targeted_calls = Arc::new(Mutex::new(Vec::new()));
    let mut steps = VecDeque::new();
    for _ in 0..3 {
        let (sender, receiver) = mpsc::channel(1);
        drop(sender);
        steps.push_back(Ok(ConnectedSyncSession::new(
            Box::new(FakeSession {
                initial: initial_snapshot(gid),
                targeted_calls: targeted_calls.clone(),
            }),
            receiver,
        )));
    }
    let connector = Arc::new(ScriptedConnector {
        steps: Mutex::new(steps),
        calls: AtomicUsize::new(0),
    });
    let handle = spawn_sync_coordinator(connector.clone(), test_config(profile_id));

    let snapshot = wait_until_failed(&handle).await;

    assert!(snapshot.stale);
    assert_eq!(connector.calls.load(Ordering::Relaxed), 3);
    handle.stop().await;
}

#[tokio::test]
async fn stop_cancels_in_flight_refresh_and_waits_for_session_close() {
    let profile_id = ProfileId::new();
    let gid = Gid::from_u64(4);
    let refresh_calls = Arc::new(AtomicUsize::new(0));
    let close_calls = Arc::new(AtomicUsize::new(0));
    let (notification_tx, notification_rx) = mpsc::channel(1);
    let connector = Arc::new(ScriptedConnector {
        steps: Mutex::new(VecDeque::from([Ok(ConnectedSyncSession::new(
            Box::new(HangingRefreshSession {
                initial: initial_snapshot(gid),
                refresh_calls: refresh_calls.clone(),
                close_calls: close_calls.clone(),
            }),
            notification_rx,
        ))])),
        calls: AtomicUsize::new(0),
    });
    let handle = spawn_sync_coordinator(connector, test_config(profile_id));
    let _ = wait_until_connected(&handle).await;
    handle.force_refresh().await;

    let deadline = Instant::now() + Duration::from_secs(1);
    while refresh_calls.load(Ordering::Relaxed) == 0 {
        assert!(Instant::now() < deadline, "refresh did not start");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let stopped = tokio::time::timeout(Duration::from_millis(250), handle.stop()).await;
    assert!(stopped.is_ok(), "stop did not cancel the in-flight refresh");
    assert_eq!(close_calls.load(Ordering::Relaxed), 1);
    drop(notification_tx);
}

#[tokio::test]
async fn full_refresh_moves_terminal_task_without_removal_patch() {
    let profile_id = ProfileId::new();
    let generation = SessionGeneration::initial();
    let session_id = EngineSessionId::new();
    let gid = Gid::from_u64(5);
    let mut store = DownloadStore::new(EngineSession::new(profile_id, session_id, generation));
    if let Err(error) = store.reconcile_live(
        generation,
        vec![TaskSnapshot::new(gid, DownloadStatus::Active, "active")],
        Vec::new(),
    ) {
        panic!("failed to seed live task: {error}");
    }
    let session = FakeSession {
        initial: InitialSyncSnapshot {
            capabilities: EngineCapabilities {
                version: "1.37.0".into(),
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
                total: 1,
                tasks: vec![TaskSnapshot::new(gid, DownloadStatus::Complete, "done")],
            },
        },
        targeted_calls: Arc::new(Mutex::new(Vec::new())),
    };
    let (events, mut receiver) = broadcast::channel(16);

    let history: Arc<dyn TaskHistoryStore> = Arc::new(NullHistoryStore);
    let profile_id = store.session().profile_id;
    if let Err(error) = refresh_all(
        &session, &mut store, generation, 10, &events, &history, profile_id,
    )
    .await
    {
        panic!("full refresh failed: {error}");
    }

    while let Ok(event) = receiver.try_recv() {
        if let SyncEvent::StorePatched(patch) = event {
            assert!(!patch.removed.contains(&gid));
        }
    }
    assert_eq!(
        store.task(gid).map(|task| task.status),
        Some(DownloadStatus::Complete)
    );
    assert_eq!(store.counts().completed, 1);
}

#[tokio::test]
async fn set_activity_switches_poll_intervals_without_disconnect() {
    let profile_id = ProfileId::new();
    let gid = Gid::from_u64(42);
    let (notification_tx, notification_rx) = mpsc::channel(8);
    let connector = Arc::new(ScriptedConnector {
        steps: Mutex::new(VecDeque::from([Ok(ConnectedSyncSession::new(
            Box::new(FakeSession {
                initial: initial_snapshot(gid),
                targeted_calls: Arc::new(Mutex::new(Vec::new())),
            }),
            notification_rx,
        ))])),
        calls: AtomicUsize::new(0),
    });
    let handle = spawn_sync_coordinator(connector, test_config(profile_id));
    let _ = wait_until_connected(&handle).await;

    handle.set_activity(ActivityMode::Background).await;
    handle.set_activity(ActivityMode::Foreground).await;

    let snapshot = handle
        .snapshot(TaskListQuery::default())
        .await
        .expect("snapshot after activity changes");
    assert_eq!(snapshot.connection_state, ConnectionState::Connected);
    assert!(
        snapshot.tasks.iter().any(|task| task.gid == gid),
        "activity mode changes must not drop tasks"
    );

    handle.stop().await;
    drop(notification_tx);
}

#[test]
fn history_write_tracker_retries_batch_when_queue_was_full() {
    use super::history_sync::HistoryWriteTracker;

    let profile_id = ProfileId::new();
    let gid = Gid::from_u64(77);
    let tasks = vec![TaskSnapshot::new(
        gid,
        DownloadStatus::Complete,
        "archive.zip",
    )];
    let mut tracker = HistoryWriteTracker::default();

    // First attempt: simulate a full channel by NOT committing the batch.
    let first = tracker.diff_changed_records(profile_id, &tasks);
    assert_eq!(first.records.len(), 1);
    drop(first);

    // Next poll must still produce the same batch instead of dropping it.
    let second = tracker.diff_changed_records(profile_id, &tasks);
    assert_eq!(second.records.len(), 1);
    let (_records, fingerprints) = second.into_parts();
    tracker.commit_enqueued(fingerprints);

    // Once committed, an identical snapshot produces no duplicate write.
    let third = tracker.diff_changed_records(profile_id, &tasks);
    assert!(third.is_empty());
}

#[tokio::test]
async fn force_refresh_clears_queued_full_notification_refresh() {
    struct CountingSession {
        initial: InitialSyncSnapshot,
        live_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl DownloadSyncSession for CountingSession {
        async fn initial_snapshot(
            &self,
            _stopped_count: u32,
        ) -> Result<InitialSyncSnapshot, SyncError> {
            Ok(self.initial.clone())
        }

        async fn refresh_global_stat(&self) -> Result<GlobalStat, SyncError> {
            Ok(self.initial.global_stat)
        }

        async fn refresh_live(&self) -> Result<LiveSyncSnapshot, SyncError> {
            self.live_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.initial.live.clone())
        }

        async fn refresh_stopped_page(
            &self,
            _offset: usize,
            _count: u32,
        ) -> Result<StoppedPage, SyncError> {
            Ok(self.initial.stopped.clone())
        }

        async fn refresh_tasks(
            &self,
            _gids: &[Gid],
        ) -> Result<Vec<(Gid, Option<TaskSnapshot>)>, SyncError> {
            Ok(Vec::new())
        }

        async fn close(&self) {}
    }

    let profile_id = ProfileId::new();
    let gid = Gid::from_u64(88);
    let live_calls = Arc::new(AtomicUsize::new(0));
    let (notification_tx, notification_rx) = mpsc::channel(8);
    let connector = Arc::new(ScriptedConnector {
        steps: Mutex::new(VecDeque::from([Ok(ConnectedSyncSession::new(
            Box::new(CountingSession {
                initial: initial_snapshot(gid),
                live_calls: Arc::clone(&live_calls),
            }),
            notification_rx,
        ))])),
        calls: AtomicUsize::new(0),
    });

    let mut config = test_config(profile_id);
    config.refresh.notification_debounce = Duration::from_millis(60);
    config.refresh.foreground.live_tasks = Duration::from_secs(60);
    config.refresh.foreground.stopped_tasks = Duration::from_secs(60);
    config.refresh.foreground.global_stat = Duration::from_secs(60);

    let handle = spawn_sync_coordinator(connector, config);
    let _ = wait_until_connected(&handle).await;

    // Queue a full refresh hint, then immediately trigger ForceRefresh before
    // the notification coalesce window fires.
    notification_tx
        .send(RefreshHint::Full)
        .await
        .expect("send full refresh hint");
    handle.force_refresh().await;
    // Wait past the notification coalesce interval.
    tokio::time::sleep(Duration::from_millis(150)).await;

    assert_eq!(
        live_calls.load(Ordering::SeqCst),
        1,
        "ForceRefresh must clear pending_full so the notification tick does not run a second full refresh"
    );

    handle.stop().await;
    drop(notification_tx);
}

#[tokio::test]
async fn history_only_task_can_be_removed_and_history_writer_serializes_delete_after_upsert() {
    use super::history_sync::HistoryWriter;
    use crate::{HistoryRecord, HistoryStoreError, RemoveTasksRequest, TaskRemovalScope};

    #[derive(Default)]
    struct InMemoryHistoryStore {
        rows: Mutex<Vec<HistoryRecord>>,
        categories: Mutex<Vec<(ProfileId, Gid, String)>>,
    }

    impl TaskHistoryStore for InMemoryHistoryStore {
        fn upsert(&self, record: &HistoryRecord) -> Result<(), HistoryStoreError> {
            let mut rows = self
                .rows
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            rows.retain(|existing| {
                !(existing.profile_id == record.profile_id && existing.gid == record.gid)
            });
            rows.push(record.clone());
            Ok(())
        }

        fn remove(&self, profile_id: ProfileId, gid: Gid) -> Result<(), HistoryStoreError> {
            self.rows
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .retain(|row| !(row.profile_id == profile_id && row.gid == gid));
            self.categories
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .retain(|(p, g, _)| !(*p == profile_id && *g == gid));
            Ok(())
        }

        fn list(
            &self,
            profile_id: ProfileId,
            limit: usize,
        ) -> Result<Vec<HistoryRecord>, HistoryStoreError> {
            let rows = self
                .rows
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            Ok(rows
                .iter()
                .filter(|row| row.profile_id == profile_id)
                .take(limit)
                .cloned()
                .collect())
        }

        fn count(&self, profile_id: ProfileId) -> Result<usize, HistoryStoreError> {
            Ok(self
                .rows
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .filter(|row| row.profile_id == profile_id)
                .count())
        }
    }

    let profile_id = ProfileId::new();
    let gid_history = Gid::from_u64(0xDEAD_BEEF);
    let history = Arc::new(InMemoryHistoryStore::default());

    let writer = HistoryWriter::spawn(history.clone());
    let record = HistoryRecord {
        profile_id,
        gid: gid_history,
        status: DownloadStatus::Complete,
        display_name: "archived.iso".into(),
        directory: Some(EnginePath::new("/downloads")),
        info_hash: None,
        source_kind: TaskSourceKind::DirectUri,
        total_length: ariadeck_domain::ByteCount::new(100),
        completed_length: ariadeck_domain::ByteCount::new(100),
        error: None,
        primary_uri_redacted: Some("https://cdn.example/archived.iso".into()),
        recorded_at_ms: 10,
        updated_at_ms: 10,
    };
    assert!(writer.try_upsert_batch(vec![record.clone()]));
    let history_trait: Arc<dyn TaskHistoryStore> = history.clone();
    writer
        .remove_batch(&history_trait, profile_id, vec![gid_history])
        .await;
    writer.shutdown().await;
    assert_eq!(history.count(profile_id).expect("count"), 0);

    history.upsert(&record).expect("seed record");
    let gateway = Arc::new(FakeInteractiveGateway::default());
    let (_notification_tx, notification_rx) = mpsc::channel(8);
    let empty_initial = InitialSyncSnapshot {
        capabilities: EngineCapabilities {
            version: "1.37.0".into(),
            enabled_features: Vec::new(),
            methods: vec![
                "aria2.getVersion".into(),
                "aria2.remove".into(),
                "aria2.removeDownloadResult".into(),
            ],
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
    };
    let mut session = ConnectedSyncSession::new(
        Box::new(FakeSession {
            initial: empty_initial,
            targeted_calls: Arc::new(Mutex::new(Vec::new())),
        }),
        notification_rx,
    );
    session.command_gateway = Some(gateway.clone());
    let connector = Arc::new(ScriptedConnector {
        steps: Mutex::new(VecDeque::from([Ok(session)])),
        calls: AtomicUsize::new(0),
    });

    let mut config = test_config(profile_id);
    config.history = history_trait;
    let handle = spawn_sync_coordinator(connector, config);
    let _ = wait_until_connected(&handle).await;

    let before = handle
        .snapshot(TaskListQuery::default())
        .await
        .expect("snapshot before remove");
    assert!(before.tasks.iter().any(|t| t.gid == gid_history));

    let outcome = handle
        .execute(
            before.session,
            AppCommand::RemoveTasks(RemoveTasksRequest {
                tasks: vec![TaskIdentity::new(profile_id, gid_history)],
                scope: TaskRemovalScope::TaskOnly,
            }),
        )
        .await;
    assert!(matches!(outcome, CommandOutcome::Success { .. }));
    assert!(
        gateway
            .command_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty(),
        "removing a history-only task must not call aria2 RPC"
    );
    assert_eq!(history.count(profile_id).expect("count after remove"), 0);

    let after = handle
        .snapshot(TaskListQuery::default())
        .await
        .expect("snapshot after remove");
    assert!(!after.tasks.iter().any(|t| t.gid == gid_history));

    handle.stop().await;
}
