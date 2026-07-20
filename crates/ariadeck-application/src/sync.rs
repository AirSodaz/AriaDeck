use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ariadeck_domain::{
    ConnectionState, DownloadTask, EngineSession, EngineSessionId, Gid, GlobalStat, ProfileId,
    SessionGeneration, TaskDetails, TaskIdentity, TaskSnapshot,
};
use async_trait::async_trait;
use thiserror::Error;
use tokio::{
    sync::{broadcast, mpsc, oneshot, watch},
    time::{Instant, Interval, MissedTickBehavior},
};

use crate::{
    AppCommand, ApplicationError, ApplicationErrorCode, CommandOutcome, CommandService,
    DownloadEngineGateway, DownloadProxyConfig, DownloadStore, StoreError, StorePatch,
    TaskCommandContext, TaskCounts, TaskDetailsGateway, TaskListQuery, TaskListView,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineCapabilities {
    pub version: String,
    pub enabled_features: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveSyncSnapshot {
    pub active: Vec<TaskSnapshot>,
    pub waiting: Vec<TaskSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoppedPage {
    pub offset: usize,
    pub total: usize,
    pub tasks: Vec<TaskSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitialSyncSnapshot {
    pub capabilities: EngineCapabilities,
    pub global_stat: GlobalStat,
    pub live: LiveSyncSnapshot,
    pub stopped: StoppedPage,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RefreshHint {
    Task(Gid),
    Full,
}

#[async_trait]
pub trait DownloadSyncSession: Send + Sync {
    async fn initial_snapshot(&self, stopped_count: u32) -> Result<InitialSyncSnapshot, SyncError>;
    async fn refresh_global_stat(&self) -> Result<GlobalStat, SyncError>;
    async fn refresh_live(&self) -> Result<LiveSyncSnapshot, SyncError>;
    async fn refresh_stopped_page(
        &self,
        offset: usize,
        count: u32,
    ) -> Result<StoppedPage, SyncError>;
    async fn refresh_tasks(
        &self,
        gids: &[Gid],
    ) -> Result<Vec<(Gid, Option<TaskSnapshot>)>, SyncError>;
    async fn close(&self);
}

pub struct ConnectedSyncSession {
    session: Box<dyn DownloadSyncSession>,
    command_gateway: Option<Arc<dyn DownloadEngineGateway>>,
    details_gateway: Option<Arc<dyn TaskDetailsGateway>>,
    notifications: mpsc::Receiver<RefreshHint>,
}

struct ConnectedSessionParts {
    session: Box<dyn DownloadSyncSession>,
    command_gateway: Option<Arc<dyn DownloadEngineGateway>>,
    details_gateway: Option<Arc<dyn TaskDetailsGateway>>,
    notifications: mpsc::Receiver<RefreshHint>,
}

impl ConnectedSyncSession {
    #[must_use]
    pub fn new(
        session: Box<dyn DownloadSyncSession>,
        notifications: mpsc::Receiver<RefreshHint>,
    ) -> Self {
        Self {
            session,
            command_gateway: None,
            details_gateway: None,
            notifications,
        }
    }

    #[must_use]
    pub fn new_with_gateways(
        session: Box<dyn DownloadSyncSession>,
        command_gateway: Arc<dyn DownloadEngineGateway>,
        details_gateway: Arc<dyn TaskDetailsGateway>,
        notifications: mpsc::Receiver<RefreshHint>,
    ) -> Self {
        Self {
            session,
            command_gateway: Some(command_gateway),
            details_gateway: Some(details_gateway),
            notifications,
        }
    }

    fn into_parts(self) -> ConnectedSessionParts {
        ConnectedSessionParts {
            session: self.session,
            command_gateway: self.command_gateway,
            details_gateway: self.details_gateway,
            notifications: self.notifications,
        }
    }
}

#[async_trait]
pub trait DownloadSyncConnector: Send + Sync {
    async fn connect(&self) -> Result<ConnectedSyncSession, SyncError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncErrorKind {
    Configuration,
    Disconnected,
    Tls,
    Authentication,
    Timeout,
    Protocol,
    Internal,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{message}")]
pub struct SyncError {
    pub kind: SyncErrorKind,
    pub message: String,
    pub retryable: bool,
}

impl SyncError {
    #[must_use]
    pub fn new(kind: SyncErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable,
        }
    }

    fn store(error: StoreError) -> Self {
        Self::new(SyncErrorKind::Internal, error.to_string(), false)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ActivityMode {
    #[default]
    Foreground,
    Background,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PollIntervals {
    pub global_stat: Duration,
    pub live_tasks: Duration,
    pub stopped_tasks: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RefreshPolicy {
    pub foreground: PollIntervals,
    pub background: PollIntervals,
    pub notification_debounce: Duration,
}

impl Default for RefreshPolicy {
    fn default() -> Self {
        Self {
            foreground: PollIntervals {
                global_stat: Duration::from_millis(500),
                live_tasks: Duration::from_millis(500),
                stopped_tasks: Duration::from_secs(5),
            },
            background: PollIntervals {
                global_stat: Duration::from_secs(3),
                live_tasks: Duration::from_secs(3),
                stopped_tasks: Duration::from_secs(10),
            },
            notification_debounce: Duration::from_millis(100),
        }
    }
}

impl RefreshPolicy {
    const fn intervals(self, mode: ActivityMode) -> PollIntervals {
        match mode {
            ActivityMode::Foreground => self.foreground,
            ActivityMode::Background => self.background,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReconnectPolicy {
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub jitter_percent: u8,
    pub max_attempts: Option<u32>,
    pub reset_after: Duration,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            base_delay: Duration::from_millis(250),
            max_delay: Duration::from_secs(30),
            jitter_percent: 20,
            max_attempts: None,
            reset_after: Duration::from_secs(10),
        }
    }
}

#[derive(Clone, Debug)]
pub struct CoordinatorConfig {
    pub profile_id: ProfileId,
    pub refresh: RefreshPolicy,
    pub reconnect: ReconnectPolicy,
    pub stopped_page_size: u32,
}

impl CoordinatorConfig {
    #[must_use]
    pub fn new(profile_id: ProfileId) -> Self {
        Self {
            profile_id,
            refresh: RefreshPolicy::default(),
            reconnect: ReconnectPolicy::default(),
            stopped_page_size: 100,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyncEvent {
    ConnectionStateChanged(ConnectionState),
    CapabilitiesChanged(EngineCapabilities),
    StorePatched(StorePatch),
    SpeedHistoryChanged,
    Error(SyncError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreSnapshot {
    pub session: EngineSession,
    pub connection_state: ConnectionState,
    pub stale: bool,
    pub global_stat: GlobalStat,
    pub speed_history: crate::SpeedHistory,
    pub counts: TaskCounts,
    pub view: TaskListView,
    pub tasks: Vec<DownloadTask>,
}

#[derive(Clone)]
pub struct SyncHandle {
    commands: mpsc::Sender<Control>,
    events: broadcast::Sender<SyncEvent>,
    cancellation: watch::Sender<bool>,
    completion: watch::Receiver<bool>,
}

impl SyncHandle {
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<SyncEvent> {
        self.events.subscribe()
    }

    pub async fn set_activity(&self, mode: ActivityMode) {
        let _ = self.commands.send(Control::SetActivity(mode)).await;
    }

    pub async fn force_refresh(&self) {
        let _ = self.commands.send(Control::ForceRefresh).await;
    }

    pub async fn snapshot(&self, query: TaskListQuery) -> Option<StoreSnapshot> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(Control::Snapshot { query, sender })
            .await
            .ok()?;
        receiver.await.ok()
    }

    pub async fn execute(&self, session: EngineSession, command: AppCommand) -> CommandOutcome {
        let (sender, receiver) = oneshot::channel();
        if self
            .commands
            .send(Control::Execute {
                session,
                command,
                sender,
            })
            .await
            .is_err()
        {
            return unavailable_command_outcome("The synchronization coordinator is unavailable.");
        }
        receiver.await.unwrap_or_else(|_| {
            unavailable_command_outcome("The synchronization coordinator stopped unexpectedly.")
        })
    }

    pub async fn apply_download_proxy(
        &self,
        session: EngineSession,
        config: DownloadProxyConfig,
    ) -> Result<(), ApplicationError> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(Control::ApplyDownloadProxy {
                session,
                config,
                sender,
            })
            .await
            .map_err(|_| unavailable_error("The synchronization coordinator is unavailable."))?;
        receiver.await.map_err(|_| {
            unavailable_error("The synchronization coordinator stopped unexpectedly.")
        })?
    }

    pub async fn task_details(
        &self,
        session: EngineSession,
        task: TaskIdentity,
    ) -> Result<TaskDetails, ApplicationError> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .send(Control::TaskDetails {
                session,
                task,
                sender,
            })
            .await
            .map_err(|_| unavailable_error("The synchronization coordinator is unavailable."))?;
        receiver.await.map_err(|_| {
            unavailable_error("The synchronization coordinator stopped unexpectedly.")
        })?
    }

    pub async fn stop(&self) {
        let _ = self.cancellation.send(true);
        let mut completion = self.completion.clone();
        if *completion.borrow() {
            return;
        }
        let _ = completion.changed().await;
    }
}

enum Control {
    SetActivity(ActivityMode),
    ForceRefresh,
    Snapshot {
        query: TaskListQuery,
        sender: oneshot::Sender<StoreSnapshot>,
    },
    Execute {
        session: EngineSession,
        command: AppCommand,
        sender: oneshot::Sender<CommandOutcome>,
    },
    ApplyDownloadProxy {
        session: EngineSession,
        config: DownloadProxyConfig,
        sender: oneshot::Sender<Result<(), ApplicationError>>,
    },
    TaskDetails {
        session: EngineSession,
        task: TaskIdentity,
        sender: oneshot::Sender<Result<TaskDetails, ApplicationError>>,
    },
}

enum UnavailableControlDisposition {
    Continue,
    RetryNow,
    Stop,
}

fn handle_unavailable_control(
    control: Option<Control>,
    store: &DownloadStore,
    state: &ConnectionState,
    activity: &mut ActivityMode,
    force_refresh_retries: bool,
) -> UnavailableControlDisposition {
    match control {
        Some(Control::SetActivity(mode)) => {
            *activity = mode;
            UnavailableControlDisposition::Continue
        }
        Some(Control::ForceRefresh) if force_refresh_retries => {
            UnavailableControlDisposition::RetryNow
        }
        Some(Control::ForceRefresh) => UnavailableControlDisposition::Continue,
        Some(Control::Snapshot { query, sender }) => {
            let _ = sender.send(build_snapshot(store, state, &query));
            UnavailableControlDisposition::Continue
        }
        Some(Control::Execute { sender, .. }) => {
            let _ = sender.send(unavailable_command_outcome(
                "Task commands are unavailable until aria2 is connected and synchronized.",
            ));
            UnavailableControlDisposition::Continue
        }
        Some(Control::ApplyDownloadProxy { sender, .. }) => {
            let _ = sender.send(Err(unavailable_error(
                "Download proxy settings cannot be applied until aria2 is connected and synchronized.",
            )));
            UnavailableControlDisposition::Continue
        }
        Some(Control::TaskDetails { sender, .. }) => {
            let _ = sender.send(Err(unavailable_error(
                "Task details are unavailable until aria2 is connected and synchronized.",
            )));
            UnavailableControlDisposition::Continue
        }
        None => UnavailableControlDisposition::Stop,
    }
}

pub fn spawn_sync_coordinator(
    connector: Arc<dyn DownloadSyncConnector>,
    config: CoordinatorConfig,
) -> SyncHandle {
    let (commands, command_rx) = mpsc::channel(64);
    let (events, _) = broadcast::channel(256);
    let (cancellation, cancellation_rx) = watch::channel(false);
    let (completion_tx, completion) = watch::channel(false);
    tokio::spawn({
        let events = events.clone();
        async move {
            run_coordinator(connector, config, command_rx, events, cancellation_rx).await;
            let _ = completion_tx.send(true);
        }
    });
    SyncHandle {
        commands,
        events,
        cancellation,
        completion,
    }
}

async fn run_coordinator(
    connector: Arc<dyn DownloadSyncConnector>,
    config: CoordinatorConfig,
    mut commands: mpsc::Receiver<Control>,
    events: broadcast::Sender<SyncEvent>,
    mut cancellation: watch::Receiver<bool>,
) {
    let mut generation = SessionGeneration::initial();
    let mut store = DownloadStore::new(EngineSession::new(
        config.profile_id,
        EngineSessionId::new(),
        generation,
    ));
    let mut state = ConnectionState::Disconnected;
    let mut activity = ActivityMode::Foreground;
    let mut backoff = ReconnectBackoff::new(config.reconnect, backoff_seed());
    let mut first_attempt = true;

    loop {
        if cancellation_requested(&cancellation) {
            return;
        }
        if !first_attempt {
            generation = generation.next();
            let session = EngineSession::new(config.profile_id, EngineSessionId::new(), generation);
            match store.begin_session(session) {
                Ok(patch) => emit_patch(&events, patch),
                Err(error) => {
                    emit_error(&events, SyncError::store(error));
                    return;
                }
            }
        }
        let initial_attempt = first_attempt;
        first_attempt = false;

        let failure_attempt = backoff.attempt().saturating_add(1);
        set_state(
            &events,
            &mut state,
            if initial_attempt {
                ConnectionState::Connecting
            } else {
                ConnectionState::Reconnecting {
                    attempt: backoff.attempt().max(1),
                }
            },
        );

        let connect = connector.connect();
        tokio::pin!(connect);
        let connect_result = loop {
            tokio::select! {
                biased;
                () = wait_for_cancellation(&mut cancellation) => return,
                command = commands.recv() => match handle_unavailable_control(
                    command,
                    &store,
                    &state,
                    &mut activity,
                    false,
                ) {
                    UnavailableControlDisposition::Continue
                    | UnavailableControlDisposition::RetryNow => {}
                    UnavailableControlDisposition::Stop => return,
                },
                result = &mut connect => break result,
            }
        };
        let connected = match connect_result {
            Ok(connected) => connected,
            Err(error) => {
                if !handle_connection_failure(&events, &mut state, &mut store, generation, &error) {
                    if wait_for_manual_retry(
                        &mut commands,
                        &store,
                        &state,
                        &mut activity,
                        &mut cancellation,
                    )
                    .await
                    {
                        backoff.reset();
                        continue;
                    }
                    return;
                }
                if reconnect_limit_reached(config.reconnect, failure_attempt) {
                    set_state(
                        &events,
                        &mut state,
                        ConnectionState::Failed {
                            reason: connection_failure(&error),
                        },
                    );
                    if wait_for_manual_retry(
                        &mut commands,
                        &store,
                        &state,
                        &mut activity,
                        &mut cancellation,
                    )
                    .await
                    {
                        backoff.reset();
                        continue;
                    }
                    return;
                }
                let delay = backoff.next_delay();
                if !wait_for_retry_delay(
                    delay,
                    &mut commands,
                    &store,
                    &state,
                    &mut activity,
                    &mut cancellation,
                )
                .await
                {
                    return;
                }
                continue;
            }
        };

        let ConnectedSessionParts {
            session,
            command_gateway,
            details_gateway,
            notifications,
        } = connected.into_parts();
        set_state(&events, &mut state, ConnectionState::Authenticating);
        let initial_snapshot = session.initial_snapshot(config.stopped_page_size);
        tokio::pin!(initial_snapshot);
        let initial_result = loop {
            tokio::select! {
                biased;
                () = wait_for_cancellation(&mut cancellation) => {
                    session.close().await;
                    return;
                }
                command = commands.recv() => match handle_unavailable_control(
                    command,
                    &store,
                    &state,
                    &mut activity,
                    false,
                ) {
                    UnavailableControlDisposition::Continue
                    | UnavailableControlDisposition::RetryNow => {}
                    UnavailableControlDisposition::Stop => {
                        session.close().await;
                        return;
                    }
                },
                result = &mut initial_snapshot => break result,
            }
        };
        let initial = match initial_result {
            Ok(initial) => initial,
            Err(error) => {
                session.close().await;
                if !handle_connection_failure(&events, &mut state, &mut store, generation, &error) {
                    if wait_for_manual_retry(
                        &mut commands,
                        &store,
                        &state,
                        &mut activity,
                        &mut cancellation,
                    )
                    .await
                    {
                        backoff.reset();
                        continue;
                    }
                    return;
                }
                if reconnect_limit_reached(config.reconnect, failure_attempt) {
                    set_state(
                        &events,
                        &mut state,
                        ConnectionState::Failed {
                            reason: connection_failure(&error),
                        },
                    );
                    if wait_for_manual_retry(
                        &mut commands,
                        &store,
                        &state,
                        &mut activity,
                        &mut cancellation,
                    )
                    .await
                    {
                        backoff.reset();
                        continue;
                    }
                    return;
                }
                let delay = backoff.next_delay();
                if !wait_for_retry_delay(
                    delay,
                    &mut commands,
                    &store,
                    &state,
                    &mut activity,
                    &mut cancellation,
                )
                .await
                {
                    return;
                }
                continue;
            }
        };

        set_state(&events, &mut state, ConnectionState::Synchronizing);
        if let Err(error) = apply_initial_snapshot(&mut store, generation, initial, &events) {
            session.close().await;
            let _ = handle_connection_failure(&events, &mut state, &mut store, generation, &error);
            if wait_for_manual_retry(
                &mut commands,
                &store,
                &state,
                &mut activity,
                &mut cancellation,
            )
            .await
            {
                backoff.reset();
                continue;
            }
            return;
        }
        set_state(&events, &mut state, ConnectionState::Connected);
        let connected_at = Instant::now();

        match run_connected(
            session.as_ref(),
            command_gateway.as_ref(),
            details_gateway.as_ref(),
            notifications,
            &mut commands,
            &mut store,
            generation,
            &mut state,
            &mut activity,
            &config,
            &events,
            &mut cancellation,
        )
        .await
        {
            ConnectedExit::Stop => {
                session.close().await;
                set_state(&events, &mut state, ConnectionState::Disconnected);
                return;
            }
            ConnectedExit::Retry(error) => {
                session.close().await;
                if connected_at.elapsed() >= config.reconnect.reset_after {
                    backoff.reset();
                }
                let retryable =
                    handle_connection_failure(&events, &mut state, &mut store, generation, &error);
                if !retryable {
                    if wait_for_manual_retry(
                        &mut commands,
                        &store,
                        &state,
                        &mut activity,
                        &mut cancellation,
                    )
                    .await
                    {
                        backoff.reset();
                        continue;
                    }
                    return;
                }
                let failure_attempt = backoff.attempt().saturating_add(1);
                if reconnect_limit_reached(config.reconnect, failure_attempt) {
                    set_state(
                        &events,
                        &mut state,
                        ConnectionState::Failed {
                            reason: connection_failure(&error),
                        },
                    );
                    if wait_for_manual_retry(
                        &mut commands,
                        &store,
                        &state,
                        &mut activity,
                        &mut cancellation,
                    )
                    .await
                    {
                        backoff.reset();
                        continue;
                    }
                    return;
                }
                let delay = backoff.next_delay();
                if !wait_for_retry_delay(
                    delay,
                    &mut commands,
                    &store,
                    &state,
                    &mut activity,
                    &mut cancellation,
                )
                .await
                {
                    return;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_connected(
    session: &dyn DownloadSyncSession,
    command_gateway: Option<&Arc<dyn DownloadEngineGateway>>,
    details_gateway: Option<&Arc<dyn TaskDetailsGateway>>,
    mut notifications: mpsc::Receiver<RefreshHint>,
    commands: &mut mpsc::Receiver<Control>,
    store: &mut DownloadStore,
    generation: SessionGeneration,
    state: &mut ConnectionState,
    activity: &mut ActivityMode,
    config: &CoordinatorConfig,
    events: &broadcast::Sender<SyncEvent>,
    cancellation: &mut watch::Receiver<bool>,
) -> ConnectedExit {
    let mut timers = PollTimers::new(config.refresh, *activity);
    let mut pending_tasks = HashSet::new();
    let mut pending_full = false;

    loop {
        tokio::select! {
            biased;
            () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
            command = commands.recv() => {
                match command {
                    Some(Control::SetActivity(mode)) => {
                        *activity = mode;
                        timers = PollTimers::new(config.refresh, mode);
                    }
                    Some(Control::ForceRefresh) => {
                        let result = tokio::select! {
                            biased;
                            () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                            result = refresh_all(
                                session,
                                store,
                                generation,
                                config.stopped_page_size,
                                events,
                            ) => result,
                        };
                        if let Err(error) = result {
                            return ConnectedExit::Retry(error);
                        }
                    }
                    Some(Control::Snapshot { query, sender }) => {
                        let _ = sender.send(build_snapshot(store, state, &query));
                    }
                    Some(Control::Execute {
                        session: expected_session,
                        command,
                        sender,
                    }) => {
                        if expected_session != store.session() {
                            let _ = sender.send(CommandOutcome::failure(stale_session_error()));
                            continue;
                        }
                        let Some(gateway) = command_gateway else {
                            let _ = sender.send(CommandOutcome::failure(unsupported_error(
                                "The connected engine does not expose task commands.",
                            )));
                            continue;
                        };
                        let service = CommandService::new(config.profile_id, gateway.clone());
                        let task_contexts = match &command {
                            AppCommand::PauseTasks(tasks) | AppCommand::ResumeTasks(tasks) => {
                                Some(tasks.as_slice())
                            }
                            AppCommand::RemoveTasks(request) => Some(request.tasks.as_slice()),
                            AppCommand::RetryTasks(tasks) => Some(tasks.as_slice()),
                            AppCommand::SetTaskOutputName(request) => {
                                Some(std::slice::from_ref(&request.task))
                            }
                            _ => None,
                        }
                        .map_or_else(HashMap::new, |tasks| {
                            tasks
                                .iter()
                                .filter(|identity| identity.profile_id == config.profile_id)
                                .filter_map(|identity| {
                                    store.task(identity.gid).map(|task| {
                                        (
                                            *identity,
                                            TaskCommandContext {
                                                status: task.status,
                                                metadata: task.metadata.clone(),
                                            },
                                        )
                                    })
                                })
                                .collect::<HashMap<_, _>>()
                        });
                        let refresh_after_success = match &command {
                            AppCommand::SetTaskOutputName(request) => {
                                RefreshHint::Task(request.task.gid)
                            }
                            _ => RefreshHint::Full,
                        };
                        let custom_output_name = match &command {
                            AppCommand::SetTaskOutputName(request) => {
                                Some((request.task.gid, request.output_name.clone()))
                            }
                            _ => None,
                        };
                        let outcome = tokio::select! {
                            biased;
                            () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                            outcome = service.execute(command, &task_contexts) => outcome,
                        };
                        let refresh_authoritative_state = outcome.has_successes()
                            || outcome.has_unknown_outcome();
                        if outcome.has_successes()
                            && let Some((gid, output_name)) = custom_output_name
                        {
                            match store.set_custom_output_name(generation, gid, output_name) {
                                Ok(patch) => emit_patch(events, patch),
                                Err(error) => {
                                    return ConnectedExit::Retry(SyncError::store(error));
                                }
                            }
                        }
                        if refresh_authoritative_state {
                            match refresh_after_success {
                                RefreshHint::Task(gid) => {
                                    pending_tasks.insert(gid);
                                }
                                RefreshHint::Full => pending_full = true,
                            }
                        }
                        let _ = sender.send(outcome);
                    }
                    Some(Control::ApplyDownloadProxy {
                        session: expected_session,
                        config: proxy,
                        sender,
                    }) => {
                        if expected_session != store.session() {
                            let _ = sender.send(Err(stale_session_error()));
                            continue;
                        }
                        let Some(gateway) = command_gateway else {
                            let _ = sender.send(Err(unsupported_error(
                                "The connected engine does not expose global proxy settings.",
                            )));
                            continue;
                        };
                        let service = CommandService::new(config.profile_id, gateway.clone());
                        let result = tokio::select! {
                            biased;
                            () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                            result = service.apply_download_proxy(&proxy) => result,
                        };
                        let _ = sender.send(result);
                    }
                    Some(Control::TaskDetails {
                        session: expected_session,
                        task,
                        sender,
                    }) => {
                        if expected_session != store.session() {
                            let _ = sender.send(Err(stale_session_error()));
                            continue;
                        }
                        if task.profile_id != config.profile_id {
                            let _ = sender.send(Err(wrong_profile_error()));
                            continue;
                        }
                        let Some(gateway) = details_gateway else {
                            let _ = sender.send(Err(unsupported_error(
                                "The connected engine does not expose task details.",
                            )));
                            continue;
                        };
                        let result = tokio::select! {
                            biased;
                            () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                            result = gateway.task_details(task.gid) => result.map_err(Into::into),
                        };
                        let _ = sender.send(result);
                    }
                    None => return ConnectedExit::Stop,
                }
            }
            hint = notifications.recv() => {
                match hint {
                    Some(RefreshHint::Task(gid)) => { pending_tasks.insert(gid); }
                    Some(RefreshHint::Full) => pending_full = true,
                    None => {
                        return ConnectedExit::Retry(SyncError::new(
                            SyncErrorKind::Disconnected,
                            "RPC notification stream closed",
                            true,
                        ));
                    }
                }
            }
            _ = timers.notification.tick() => {
                if pending_full || !pending_tasks.is_empty() {
                    let refresh = async {
                        if pending_full {
                            pending_full = false;
                            pending_tasks.clear();
                            refresh_all(
                                session,
                                store,
                                generation,
                                config.stopped_page_size,
                                events,
                            ).await
                        } else {
                            let gids = pending_tasks.drain().collect::<Vec<_>>();
                            refresh_targeted(session, store, generation, &gids, events).await
                        }
                    };
                    let result = tokio::select! {
                        biased;
                        () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                        result = refresh => result,
                    };
                    if let Err(error) = result {
                        return ConnectedExit::Retry(error);
                    }
                }
            }
            _ = timers.global.tick() => {
                let result = tokio::select! {
                    biased;
                    () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                    result = session.refresh_global_stat() => result,
                };
                match result {
                    Ok(stat) => {
                        if let Err(error) = store.record_speed_sample(generation, stat) {
                            return ConnectedExit::Retry(SyncError::store(error));
                        }
                        match store.update_global_stat(generation, stat) {
                            Ok(patch) => emit_global_update(events, patch),
                            Err(error) => return ConnectedExit::Retry(SyncError::store(error)),
                        }
                    }
                    Err(error) => return ConnectedExit::Retry(error),
                }
            }
            _ = timers.live.tick() => {
                let result = tokio::select! {
                    biased;
                    () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                    result = session.refresh_live() => result,
                };
                match result {
                    Ok(live) => match store.reconcile_live(generation, live.active, live.waiting) {
                        Ok(patch) => emit_patch(events, patch),
                        Err(error) => return ConnectedExit::Retry(SyncError::store(error)),
                    },
                    Err(error) => return ConnectedExit::Retry(error),
                }
            }
            _ = timers.stopped.tick() => {
                let result = tokio::select! {
                    biased;
                    () = wait_for_cancellation(cancellation) => return ConnectedExit::Stop,
                    result = session.refresh_stopped_page(0, config.stopped_page_size) => result,
                };
                match result {
                    Ok(page) => match store.apply_stopped_page(
                        generation,
                        page.offset,
                        Some(page.total),
                        page.tasks,
                    ) {
                        Ok(patch) => emit_patch(events, patch),
                        Err(error) => return ConnectedExit::Retry(SyncError::store(error)),
                    },
                    Err(error) => return ConnectedExit::Retry(error),
                }
            }
        }
    }
}

async fn refresh_all(
    session: &dyn DownloadSyncSession,
    store: &mut DownloadStore,
    generation: SessionGeneration,
    stopped_page_size: u32,
    events: &broadcast::Sender<SyncEvent>,
) -> Result<(), SyncError> {
    let global = session.refresh_global_stat().await?;
    let live = session.refresh_live().await?;
    let stopped = session.refresh_stopped_page(0, stopped_page_size).await?;
    emit_patch(
        events,
        store
            .update_global_stat(generation, global)
            .map_err(SyncError::store)?,
    );
    emit_patch(
        events,
        store
            .apply_stopped_page(
                generation,
                stopped.offset,
                Some(stopped.total),
                stopped.tasks,
            )
            .map_err(SyncError::store)?,
    );
    emit_patch(
        events,
        store
            .reconcile_live(generation, live.active, live.waiting)
            .map_err(SyncError::store)?,
    );
    Ok(())
}

async fn refresh_targeted(
    session: &dyn DownloadSyncSession,
    store: &mut DownloadStore,
    generation: SessionGeneration,
    gids: &[Gid],
    events: &broadcast::Sender<SyncEvent>,
) -> Result<(), SyncError> {
    for (gid, snapshot) in session.refresh_tasks(gids).await? {
        emit_patch(
            events,
            store
                .apply_task_snapshot(generation, gid, snapshot)
                .map_err(SyncError::store)?,
        );
    }
    Ok(())
}

fn apply_initial_snapshot(
    store: &mut DownloadStore,
    generation: SessionGeneration,
    initial: InitialSyncSnapshot,
    events: &broadcast::Sender<SyncEvent>,
) -> Result<(), SyncError> {
    validate_capabilities(&initial.capabilities)?;
    let _ = events.send(SyncEvent::CapabilitiesChanged(initial.capabilities));
    store
        .record_speed_sample(generation, initial.global_stat)
        .map_err(SyncError::store)?;
    emit_global_update(
        events,
        store
            .update_global_stat(generation, initial.global_stat)
            .map_err(SyncError::store)?,
    );
    emit_patch(
        events,
        store
            .apply_stopped_page(
                generation,
                initial.stopped.offset,
                Some(initial.stopped.total),
                initial.stopped.tasks,
            )
            .map_err(SyncError::store)?,
    );
    emit_patch(
        events,
        store
            .reconcile_live(generation, initial.live.active, initial.live.waiting)
            .map_err(SyncError::store)?,
    );
    emit_patch(
        events,
        store
            .set_stale(generation, false)
            .map_err(SyncError::store)?,
    );
    Ok(())
}

fn validate_capabilities(capabilities: &EngineCapabilities) -> Result<(), SyncError> {
    if capabilities.version.trim().is_empty() {
        return Err(SyncError::new(
            SyncErrorKind::Protocol,
            "aria2 returned an empty version during capability verification",
            false,
        ));
    }
    Ok(())
}

fn handle_connection_failure(
    events: &broadcast::Sender<SyncEvent>,
    state: &mut ConnectionState,
    store: &mut DownloadStore,
    generation: SessionGeneration,
    error: &SyncError,
) -> bool {
    emit_error(events, error.clone());
    if let Ok(patch) = store.set_stale(generation, true) {
        emit_patch(events, patch);
    }
    if !error.retryable {
        set_state(
            events,
            state,
            ConnectionState::Failed {
                reason: connection_failure(error),
            },
        );
    } else {
        set_state(events, state, ConnectionState::Disconnected);
    }
    error.retryable
}

async fn wait_for_retry_delay(
    delay: Duration,
    commands: &mut mpsc::Receiver<Control>,
    store: &DownloadStore,
    state: &ConnectionState,
    activity: &mut ActivityMode,
    cancellation: &mut watch::Receiver<bool>,
) -> bool {
    let sleep = tokio::time::sleep(delay);
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            biased;
            () = wait_for_cancellation(cancellation) => return false,
            () = &mut sleep => return true,
            command = commands.recv() => match handle_unavailable_control(
                command,
                store,
                state,
                activity,
                true,
            ) {
                UnavailableControlDisposition::Continue => {}
                UnavailableControlDisposition::RetryNow => return true,
                UnavailableControlDisposition::Stop => return false,
            }
        }
    }
}

async fn wait_for_manual_retry(
    commands: &mut mpsc::Receiver<Control>,
    store: &DownloadStore,
    state: &ConnectionState,
    activity: &mut ActivityMode,
    cancellation: &mut watch::Receiver<bool>,
) -> bool {
    loop {
        tokio::select! {
            biased;
            () = wait_for_cancellation(cancellation) => return false,
            command = commands.recv() => match handle_unavailable_control(
                command,
                store,
                state,
                activity,
                true,
            ) {
                UnavailableControlDisposition::Continue => {}
                UnavailableControlDisposition::RetryNow => return true,
                UnavailableControlDisposition::Stop => return false,
            }
        }
    }
}

fn cancellation_requested(cancellation: &watch::Receiver<bool>) -> bool {
    *cancellation.borrow()
}

async fn wait_for_cancellation(cancellation: &mut watch::Receiver<bool>) {
    loop {
        if cancellation_requested(cancellation) {
            return;
        }
        if cancellation.changed().await.is_err() {
            return;
        }
    }
}

fn build_snapshot(
    store: &DownloadStore,
    state: &ConnectionState,
    query: &TaskListQuery,
) -> StoreSnapshot {
    let view = store.view(query);
    let tasks = view
        .visible_gids
        .iter()
        .filter_map(|gid| store.task(*gid).cloned())
        .collect();
    StoreSnapshot {
        session: store.session(),
        connection_state: state.clone(),
        stale: store.is_stale(),
        global_stat: store.global_stat(),
        speed_history: store.speed_history().clone(),
        counts: store.counts(),
        view,
        tasks,
    }
}

fn set_state(
    events: &broadcast::Sender<SyncEvent>,
    state: &mut ConnectionState,
    next: ConnectionState,
) {
    if *state != next {
        *state = next.clone();
        let _ = events.send(SyncEvent::ConnectionStateChanged(next));
    }
}

fn emit_patch(events: &broadcast::Sender<SyncEvent>, patch: StorePatch) {
    if !patch.is_empty() {
        let _ = events.send(SyncEvent::StorePatched(patch));
    }
}

fn emit_global_update(events: &broadcast::Sender<SyncEvent>, patch: StorePatch) {
    if patch.is_empty() {
        let _ = events.send(SyncEvent::SpeedHistoryChanged);
    } else {
        emit_patch(events, patch);
    }
}

fn emit_error(events: &broadcast::Sender<SyncEvent>, error: SyncError) {
    let _ = events.send(SyncEvent::Error(error));
}

fn unavailable_command_outcome(summary: &str) -> CommandOutcome {
    CommandOutcome::failure(unavailable_error(summary))
}

fn unavailable_error(summary: &str) -> ApplicationError {
    ApplicationError::new(ApplicationErrorCode::Disconnected, summary, true)
}

fn stale_session_error() -> ApplicationError {
    ApplicationError::new(
        ApplicationErrorCode::StaleSession,
        "The task view belongs to an obsolete engine session. Refresh and try again.",
        true,
    )
}

fn wrong_profile_error() -> ApplicationError {
    ApplicationError::new(
        ApplicationErrorCode::WrongProfile,
        "The task belongs to a different engine profile.",
        false,
    )
}

fn unsupported_error(summary: &str) -> ApplicationError {
    ApplicationError::new(ApplicationErrorCode::Unsupported, summary, false)
}

fn connection_failure(error: &SyncError) -> ariadeck_domain::ConnectionFailure {
    ariadeck_domain::ConnectionFailure {
        code: format!("sync.{:?}", error.kind).to_ascii_lowercase(),
        summary: error.message.clone(),
        retryable: error.retryable,
    }
}

const fn reconnect_limit_reached(policy: ReconnectPolicy, attempt: u32) -> bool {
    match policy.max_attempts {
        Some(max_attempts) => attempt >= max_attempts,
        None => false,
    }
}

enum ConnectedExit {
    Retry(SyncError),
    Stop,
}

struct PollTimers {
    global: Interval,
    live: Interval,
    stopped: Interval,
    notification: Interval,
}

impl PollTimers {
    fn new(policy: RefreshPolicy, mode: ActivityMode) -> Self {
        let intervals = policy.intervals(mode);
        Self {
            global: interval_after(intervals.global_stat),
            live: interval_after(intervals.live_tasks),
            stopped: interval_after(intervals.stopped_tasks),
            notification: interval_after(policy.notification_debounce),
        }
    }
}

fn interval_after(duration: Duration) -> Interval {
    let duration = duration.max(Duration::from_millis(1));
    let mut interval = tokio::time::interval_at(Instant::now() + duration, duration);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    interval
}

struct ReconnectBackoff {
    policy: ReconnectPolicy,
    attempt: u32,
    random_state: u64,
}

impl ReconnectBackoff {
    const fn new(policy: ReconnectPolicy, seed: u64) -> Self {
        Self {
            policy,
            attempt: 0,
            random_state: seed,
        }
    }

    const fn attempt(&self) -> u32 {
        self.attempt
    }

    fn reset(&mut self) {
        self.attempt = 0;
    }

    fn next_delay(&mut self) -> Duration {
        let exponent = self.attempt.min(31);
        self.attempt = self.attempt.saturating_add(1);
        let multiplier = 1_u32 << exponent;
        let base = self
            .policy
            .base_delay
            .saturating_mul(multiplier)
            .min(self.policy.max_delay);
        let jitter_percent = self.policy.jitter_percent.min(100);
        if jitter_percent == 0 {
            return base;
        }

        self.random_state ^= self.random_state << 13;
        self.random_state ^= self.random_state >> 7;
        self.random_state ^= self.random_state << 17;
        let base_millis = u64::try_from(base.as_millis()).unwrap_or(u64::MAX);
        let span = base_millis.saturating_mul(u64::from(jitter_percent)) / 100;
        if span == 0 {
            return base;
        }
        let width = span.saturating_mul(2).saturating_add(1);
        let offset = self.random_state % width;
        Duration::from_millis(base_millis.saturating_sub(span).saturating_add(offset))
            .min(self.policy.max_delay)
    }
}

fn backoff_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0x9e37_79b9_7f4a_7c15, |duration| {
            u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use ariadeck_domain::{
        DownloadStatus, EnginePath, TaskDetails, TaskFile, TaskSnapshot, TaskSourceKind,
    };

    use super::*;

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
        ) -> Result<Gid, crate::GatewayError> {
            let gid = Gid::from_u64(99);
            self.command_calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(("add", gid));
            Ok(gid)
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
                    uris: vec!["https://example.test/item".into()],
                    destination: None,
                    file_conflict: crate::FileConflictPolicy::default(),
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

        if let Err(error) = refresh_all(&session, &mut store, generation, 10, &events).await {
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
}
