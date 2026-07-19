use std::{env, path::PathBuf, process::Command, sync::Arc, time::Duration};

use ariadeck_application::{
    AddDownloadRequest, AppCommand, ApplicationError, ApplicationErrorCode, CommandItem,
    CommandOutcome, CoordinatorConfig, RemoveTasksRequest, StoreSnapshot, SyncHandle,
    TaskListQuery, TaskRemovalScope, spawn_sync_coordinator,
};
use ariadeck_domain::{
    ConnectionState, DownloadFilter, DownloadStatus, DownloadTask, EngineSession, EngineSessionId,
    Gid, ProfileId, SessionGeneration, TaskDetails, TaskIdentity as DomainTaskIdentity,
    TaskProgress,
};
use ariadeck_engine::{
    ExternalEngineProfile, JsonProfileStore, LocalEngineConfig, LocalEngineProcess,
};
use ariadeck_rpc::{
    Aria2Client, AuthenticatedTransport, RpcSecret, RpcSyncConnector, WebSocketConfig,
    WebSocketTransport,
};
use ariadeck_ui::{
    AddDownloadRequestView, AddDownloadResultView, AppShell, AppShellEvent, CommandOutcomeView,
    ConnectionView, DownloadRowView, EngineSessionView, OperationErrorView, TaskCommandRequestView,
    TaskCommandResultView, TaskCommandView, TaskCountsView, TaskDetailsOutcomeView,
    TaskDetailsRequestView, TaskDetailsResultView, TaskDetailsView, TaskFileView, TaskIdentity,
    TaskStatusView, Theme, WorkspaceFilter, WorkspaceQuery, WorkspaceSnapshot,
};
use gpui::{AppContext as _, Context, Entity, IntoElement, Render, Subscription, Window};
use tokio::{runtime::Runtime, sync::watch};
use url::Url;

pub struct DesktopRoot {
    workspace: Entity<AppShell>,
    sync: Option<SyncHandle>,
    local_engine: Option<LocalEngineProcess>,
    runtime: Arc<Runtime>,
    query_sender: watch::Sender<TaskListQuery>,
    _workspace_subscription: Subscription,
}

impl DesktopRoot {
    #[must_use]
    pub fn new(runtime: Arc<Runtime>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (sync, local_engine, initial_snapshot) = match create_sync_handle(&runtime) {
            Ok((handle, local_engine)) => {
                let snapshot = WorkspaceSnapshot {
                    connection: ConnectionView::Connecting,
                    ..WorkspaceSnapshot::default()
                };
                (Some(handle), local_engine, snapshot)
            }
            Err(error) => {
                tracing::error!(%error, "failed to configure aria2 synchronization");
                let snapshot = WorkspaceSnapshot {
                    connection: ConnectionView::Failed {
                        summary: error,
                        retryable: false,
                    },
                    ..WorkspaceSnapshot::default()
                };
                (None, None, snapshot)
            }
        };
        let workspace = cx.new(|cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.set_snapshot(initial_snapshot, cx);
            shell
        });
        let (query_sender, query_receiver) = watch::channel(TaskListQuery::default());
        let workspace_subscription = cx.subscribe_in(
            &workspace,
            window,
            |this: &mut Self, _workspace, event: &AppShellEvent, window, cx| match event {
                AppShellEvent::QueryChanged(query) => {
                    this.query_sender.send_replace(map_query(query));
                }
                AppShellEvent::RetryRequested => {
                    if let Some(handle) = this.sync.clone() {
                        this.runtime.spawn(async move {
                            handle.force_refresh().await;
                        });
                    }
                }
                AppShellEvent::AddDownloadRequested(request) => {
                    this.spawn_add_download(request.clone(), window, cx);
                }
                AppShellEvent::TaskCommandRequested(request) => {
                    this.spawn_task_command(request.clone(), window, cx);
                }
                AppShellEvent::TaskDetailsRequested(request) => {
                    this.spawn_task_details(request.clone(), window, cx);
                }
            },
        );

        if let Some(handle) = sync.clone() {
            spawn_snapshot_bridge(handle, query_receiver, cx);
        }

        Self {
            workspace,
            sync,
            local_engine,
            runtime,
            query_sender,
            _workspace_subscription: workspace_subscription,
        }
    }

    fn spawn_add_download(
        &self,
        request: AddDownloadRequestView,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let sync = self.sync.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = execute_add_download(sync, request).await;
            this.update_in(cx, |this, window, cx| {
                this.workspace.update(cx, |workspace, cx| {
                    workspace.set_add_download_result(result, window, cx);
                });
            })
            .ok();
        })
        .detach();
    }

    fn spawn_task_command(
        &self,
        request: TaskCommandRequestView,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let sync = self.sync.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = execute_task_command(sync, request).await;
            this.update_in(cx, |this, _window, cx| {
                this.workspace.update(cx, |workspace, cx| {
                    workspace.set_task_command_result(result, cx);
                });
            })
            .ok();
        })
        .detach();
    }

    fn spawn_task_details(
        &self,
        request: TaskDetailsRequestView,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let sync = self.sync.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = execute_task_details(sync, request).await;
            this.update_in(cx, |this, _window, cx| {
                this.workspace.update(cx, |workspace, cx| {
                    workspace.set_task_details_result(result, cx);
                });
            })
            .ok();
        })
        .detach();
    }
}

impl Drop for DesktopRoot {
    fn drop(&mut self) {
        if let Some(handle) = self.sync.take() {
            self.runtime.block_on(handle.stop());
        }
        if let Some(mut process) = self.local_engine.take() {
            if let Err(error) = self
                .runtime
                .block_on(request_local_engine_shutdown(&process))
            {
                tracing::debug!(%error, "local aria2 graceful shutdown request was not completed");
            }
            if let Err(error) = process.shutdown() {
                tracing::warn!(%error, "failed to stop the local aria2 process cleanly");
            }
        }
    }
}

impl Render for DesktopRoot {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.workspace.clone()
    }
}

async fn execute_add_download(
    sync: Option<SyncHandle>,
    request: AddDownloadRequestView,
) -> AddDownloadResultView {
    let AddDownloadRequestView {
        request_id,
        session,
        uri,
    } = request;
    let outcome = match (sync, map_engine_session(&session)) {
        (Some(handle), Ok(engine_session)) => {
            let outcome = handle
                .execute(
                    engine_session,
                    AppCommand::AddDownload(AddDownloadRequest {
                        uris: vec![uri],
                        destination: None,
                        options: Vec::new(),
                    }),
                )
                .await;
            if outcome.has_successes() {
                handle.force_refresh().await;
            }
            map_command_outcome(outcome)
        }
        (None, _) => CommandOutcomeView::Failure(unavailable_operation_error()),
        (Some(_), Err(error)) => CommandOutcomeView::Failure(map_application_error(error)),
    };
    AddDownloadResultView {
        request_id,
        session,
        outcome,
    }
}

async fn execute_task_command(
    sync: Option<SyncHandle>,
    request: TaskCommandRequestView,
) -> TaskCommandResultView {
    let TaskCommandRequestView {
        request_id,
        session,
        identity,
        command,
    } = request;
    let mapped = map_engine_session(&session)
        .and_then(|engine_session| map_task_identity(&identity).map(|task| (engine_session, task)));
    let outcome = match (sync, mapped) {
        (Some(handle), Ok((engine_session, task))) => {
            let app_command = match command {
                TaskCommandView::Pause => AppCommand::PauseTasks(vec![task]),
                TaskCommandView::Resume => AppCommand::ResumeTasks(vec![task]),
                TaskCommandView::RemoveTask => AppCommand::RemoveTasks(RemoveTasksRequest {
                    tasks: vec![task],
                    scope: TaskRemovalScope::TaskOnly,
                }),
            };
            let outcome = handle.execute(engine_session, app_command).await;
            if outcome.has_successes() {
                handle.force_refresh().await;
            }
            map_command_outcome(outcome)
        }
        (None, _) => CommandOutcomeView::Failure(unavailable_operation_error()),
        (Some(_), Err(error)) => CommandOutcomeView::Failure(map_application_error(error)),
    };
    TaskCommandResultView {
        request_id,
        session,
        identity,
        command,
        outcome,
    }
}

async fn execute_task_details(
    sync: Option<SyncHandle>,
    request: TaskDetailsRequestView,
) -> TaskDetailsResultView {
    let TaskDetailsRequestView {
        request_id,
        session,
        identity,
    } = request;
    let mapped = map_engine_session(&session)
        .and_then(|engine_session| map_task_identity(&identity).map(|task| (engine_session, task)));
    let outcome = match (sync, mapped) {
        (Some(handle), Ok((engine_session, task))) => handle
            .task_details(engine_session, task)
            .await
            .map(map_task_details)
            .map_or_else(
                |error| TaskDetailsOutcomeView::Failed(map_application_error(error)),
                TaskDetailsOutcomeView::Ready,
            ),
        (None, _) => TaskDetailsOutcomeView::Failed(unavailable_operation_error()),
        (Some(_), Err(error)) => TaskDetailsOutcomeView::Failed(map_application_error(error)),
    };
    TaskDetailsResultView {
        request_id,
        session,
        identity,
        outcome,
    }
}

fn map_engine_session(session: &EngineSessionView) -> Result<EngineSession, ApplicationError> {
    let profile_id = session.profile_id.parse::<ProfileId>().map_err(|error| {
        ApplicationError::new(
            ApplicationErrorCode::Internal,
            format!("Invalid UI profile identity: {error}"),
            false,
        )
    })?;
    let session_id = session
        .session_id
        .parse::<EngineSessionId>()
        .map_err(|error| {
            ApplicationError::new(
                ApplicationErrorCode::Internal,
                format!("Invalid UI engine-session identity: {error}"),
                false,
            )
        })?;
    if session.generation == 0 {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Internal,
            "The UI supplied an invalid zero session generation.",
            false,
        ));
    }
    Ok(EngineSession::new(
        profile_id,
        session_id,
        SessionGeneration::from_u64(session.generation),
    ))
}

fn map_task_identity(identity: &TaskIdentity) -> Result<DomainTaskIdentity, ApplicationError> {
    let profile_id = identity.profile_id.parse::<ProfileId>().map_err(|error| {
        ApplicationError::new(
            ApplicationErrorCode::Internal,
            format!("Invalid UI task profile identity: {error}"),
            false,
        )
    })?;
    let gid = identity.gid.parse::<Gid>().map_err(|error| {
        ApplicationError::new(
            ApplicationErrorCode::Internal,
            format!("Invalid UI aria2 GID: {error}"),
            false,
        )
    })?;
    Ok(DomainTaskIdentity::new(profile_id, gid))
}

fn map_command_outcome(outcome: CommandOutcome) -> CommandOutcomeView {
    match outcome {
        CommandOutcome::Success { succeeded } => CommandOutcomeView::Success {
            task: succeeded.into_iter().next().map(map_command_item),
        },
        CommandOutcome::PartialSuccess {
            mut succeeded,
            failed,
        } => succeeded.pop().map_or_else(
            || {
                CommandOutcomeView::Failure(
                    failed
                        .into_iter()
                        .next()
                        .map(|failure| map_application_error(failure.error))
                        .unwrap_or_else(internal_operation_error),
                )
            },
            |item| CommandOutcomeView::Success {
                task: Some(map_command_item(item)),
            },
        ),
        CommandOutcome::Failure { failed } => CommandOutcomeView::Failure(
            failed
                .into_iter()
                .next()
                .map(|failure| map_application_error(failure.error))
                .unwrap_or_else(internal_operation_error),
        ),
    }
}

fn map_command_item(item: CommandItem) -> TaskIdentity {
    let CommandItem::Task(identity) = item;
    TaskIdentity {
        profile_id: identity.profile_id.to_string(),
        gid: identity.gid.to_string(),
    }
}

fn map_application_error(error: ApplicationError) -> OperationErrorView {
    OperationErrorView {
        code: error.code.as_str().into(),
        summary: error.summary,
        retryable: error.retryable,
    }
}

fn unavailable_operation_error() -> OperationErrorView {
    OperationErrorView {
        code: ApplicationErrorCode::Disconnected.as_str().into(),
        summary: "The synchronization coordinator is unavailable.".into(),
        retryable: false,
    }
}

fn internal_operation_error() -> OperationErrorView {
    OperationErrorView {
        code: ApplicationErrorCode::Internal.as_str().into(),
        summary: "The command returned no result.".into(),
        retryable: false,
    }
}

fn map_task_details(details: TaskDetails) -> TaskDetailsView {
    TaskDetailsView {
        directory: details.directory.map(|path| path.to_string()),
        info_hash: details.info_hash,
        piece_length: details.piece_length.map(|length| length.get()),
        piece_count: details.piece_count,
        files: details
            .files
            .into_iter()
            .map(|file| TaskFileView {
                index: file.index,
                path: file.path.to_string(),
                length: file.length.get(),
                completed_length: file.completed_length.get(),
                selected: file.selected,
            })
            .collect(),
    }
}

fn create_sync_handle(
    runtime: &Runtime,
) -> Result<(SyncHandle, Option<LocalEngineProcess>), String> {
    let (endpoint, secret, local_engine, profile_id) = if let Some(endpoint) =
        env::var("ARIADECK_RPC_URL")
            .ok()
            .filter(|endpoint| !endpoint.trim().is_empty())
    {
        let endpoint =
            Url::parse(&endpoint).map_err(|error| format!("Invalid RPC URL: {error}"))?;
        let profile_id = env::var("ARIADECK_PROFILE_ID")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or_default();
        let secret = env::var("ARIADECK_RPC_SECRET")
            .ok()
            .filter(|secret| !secret.is_empty())
            .map(RpcSecret::new);
        (endpoint, secret, None, profile_id)
    } else {
        let config = resolve_local_engine_config()?;
        let profile_id = config.profile_id;
        let process = LocalEngineProcess::spawn(&config)
            .map_err(|error| format!("Failed to start local aria2: {error}"))?;
        let endpoint = process.endpoint().clone();
        let secret = Some(RpcSecret::new(process.secret().to_owned()));
        (endpoint, secret, Some(process), profile_id)
    };

    let mut websocket = WebSocketConfig::new(endpoint.clone());
    websocket.connect_timeout = Duration::from_millis(750);
    websocket.request_timeout = Duration::from_secs(5);
    let connector = Arc::new(RpcSyncConnector::new(websocket, secret));
    let coordinator = CoordinatorConfig::new(profile_id);
    tracing::info!(
        scheme = endpoint.scheme(),
        host = endpoint.host_str().unwrap_or("unknown"),
        port = endpoint.port_or_known_default(),
        "configured external aria2 RPC profile"
    );
    let _runtime_guard = runtime.enter();
    Ok((spawn_sync_coordinator(connector, coordinator), local_engine))
}

fn resolve_local_engine_config() -> Result<LocalEngineConfig, String> {
    let executable = env::var_os("ARIADECK_ARIA2C_PATH")
        .map(PathBuf::from)
        .or_else(discover_aria2_executable)
        .ok_or_else(|| {
            "No aria2 executable found. Set ARIADECK_ARIA2C_PATH or ARIADECK_RPC_URL.".to_owned()
        })?;
    let data_dir = env::var_os("ARIADECK_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(default_data_dir);
    let download_dir = env::var_os("ARIADECK_DOWNLOAD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir.join("downloads"));
    let profile_store = JsonProfileStore::new(data_dir.join("profiles.json"));
    let stored = if profile_store.path().is_file() {
        Some(
            profile_store
                .load()
                .map_err(|error| format!("Failed to load local aria2 profile: {error}"))?,
        )
    } else {
        None
    };
    let profile = ExternalEngineProfile::new(
        stored
            .as_ref()
            .map_or_else(ProfileId::new, |profile| profile.profile_id),
        env::var("ARIADECK_PROFILE_NAME").unwrap_or_else(|_| "Local aria2".into()),
        executable,
        data_dir,
        download_dir,
    );
    profile_store
        .save(&profile)
        .map_err(|error| format!("Failed to save local aria2 profile: {error}"))?;
    Ok(profile.local_config())
}

async fn request_local_engine_shutdown(process: &LocalEngineProcess) -> Result<(), String> {
    let mut websocket = WebSocketConfig::new(process.endpoint().clone());
    websocket.connect_timeout = Duration::from_millis(500);
    websocket.request_timeout = Duration::from_millis(750);
    let transport = WebSocketTransport::connect(websocket)
        .await
        .map_err(|error| error.to_string())?;
    let authenticated = AuthenticatedTransport::new(
        transport.clone(),
        Some(RpcSecret::new(process.secret().to_owned())),
    );
    let client = Aria2Client::new(authenticated);
    let result = client.shutdown().await.map_err(|error| error.to_string());
    transport.close().await;
    result
}

fn discover_aria2_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(user_profile) = env::var_os("USERPROFILE") {
        candidates.push(PathBuf::from(user_profile).join("scoop/apps/aria2/current/aria2c.exe"));
    }
    if let Some(home) = env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join("scoop/apps/aria2/current/aria2c.exe"));
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .or_else(|| {
            Command::new("aria2c")
                .arg("--version")
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|_| PathBuf::from("aria2c"))
        })
}

fn default_data_dir() -> PathBuf {
    if let Some(path) = env::var_os("LOCALAPPDATA") {
        return PathBuf::from(path).join("AriaDeck");
    }
    if let Some(path) = env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(path).join("ariadeck");
    }
    if let Some(path) = env::var_os("HOME") {
        return PathBuf::from(path).join(".local/share/ariadeck");
    }
    PathBuf::from(".ariadeck")
}

fn spawn_snapshot_bridge(
    handle: SyncHandle,
    mut query_receiver: watch::Receiver<TaskListQuery>,
    cx: &mut Context<DesktopRoot>,
) {
    let mut events = handle.subscribe();
    cx.spawn(async move |this, cx| {
        loop {
            let query = query_receiver.borrow().clone();
            let Some(snapshot) = handle.snapshot(query.clone()).await else {
                break;
            };
            if *query_receiver.borrow() == query {
                let snapshot = map_snapshot(snapshot);
                if this
                    .update(cx, |this, cx| {
                        this.workspace.update(cx, |workspace, cx| {
                            workspace.set_snapshot(snapshot, cx);
                        });
                    })
                    .is_err()
                {
                    break;
                }
            }

            tokio::select! {
                event = events.recv() => match event {
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                changed = query_receiver.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
            }
        }
    })
    .detach();
}

fn map_query(query: &WorkspaceQuery) -> TaskListQuery {
    TaskListQuery {
        filter: match query.filter {
            WorkspaceFilter::All => DownloadFilter::All,
            WorkspaceFilter::Active => DownloadFilter::Active,
            WorkspaceFilter::Waiting => DownloadFilter::Waiting,
            WorkspaceFilter::Paused => DownloadFilter::Paused,
            WorkspaceFilter::Completed => DownloadFilter::Completed,
            WorkspaceFilter::Failed => DownloadFilter::Failed,
        },
        search: query.search.clone(),
        ..TaskListQuery::default()
    }
}

fn map_snapshot(snapshot: StoreSnapshot) -> WorkspaceSnapshot {
    let profile_id = snapshot.session.profile_id.to_string();
    WorkspaceSnapshot {
        profile_id: profile_id.clone(),
        session_id: snapshot.session.session_id.to_string(),
        generation: snapshot.session.generation.get(),
        source_revision: snapshot.view.source_revision,
        connection: map_connection(snapshot.connection_state),
        stale: snapshot.stale,
        download_rate: snapshot.global_stat.download_speed.get(),
        upload_rate: snapshot.global_stat.upload_speed.get(),
        counts: TaskCountsView {
            all: snapshot.counts.all,
            active: snapshot.counts.active,
            waiting: snapshot.counts.waiting,
            paused: snapshot.counts.paused,
            completed: snapshot.counts.completed,
            failed: snapshot.counts.failed,
        },
        tasks: snapshot
            .tasks
            .into_iter()
            .map(|task| map_task(&profile_id, task))
            .collect(),
    }
}

fn map_connection(state: ConnectionState) -> ConnectionView {
    match state {
        ConnectionState::Disconnected => ConnectionView::Disconnected,
        ConnectionState::Connecting => ConnectionView::Connecting,
        ConnectionState::Authenticating => ConnectionView::Authenticating,
        ConnectionState::Synchronizing => ConnectionView::Synchronizing,
        ConnectionState::Connected => ConnectionView::Connected,
        ConnectionState::Reconnecting { attempt } => ConnectionView::Reconnecting { attempt },
        ConnectionState::Failed { reason } => ConnectionView::Failed {
            summary: reason.summary,
            retryable: reason.retryable,
        },
    }
}

fn map_task(profile_id: &str, task: DownloadTask) -> DownloadRowView {
    let eta_seconds = TaskProgress::new(task.completed_length, task.total_length)
        .eta(task.download_speed)
        .map(|duration| duration.as_secs());
    DownloadRowView {
        identity: TaskIdentity {
            profile_id: profile_id.into(),
            gid: task.gid.to_string(),
        },
        display_name: task.display_name,
        status: match task.status {
            DownloadStatus::Active => TaskStatusView::Active,
            DownloadStatus::Waiting => TaskStatusView::Waiting,
            DownloadStatus::Paused => TaskStatusView::Paused,
            DownloadStatus::Complete => TaskStatusView::Complete,
            DownloadStatus::Error => TaskStatusView::Failed,
            DownloadStatus::Removed => TaskStatusView::Removed,
            DownloadStatus::Verifying => TaskStatusView::Verifying,
            DownloadStatus::Unknown => TaskStatusView::Unknown,
        },
        total_bytes: task.total_length.get(),
        completed_bytes: task.completed_length.get(),
        download_rate: task.download_speed.get(),
        upload_rate: task.upload_speed.get(),
        eta_seconds,
        revision: task.revision,
    }
}

#[cfg(test)]
mod tests {
    use ariadeck_application::ItemFailure;
    use ariadeck_domain::{ByteCount, ByteRate, EnginePath, Gid, TaskFile, TaskSnapshot};

    use super::*;

    #[test]
    fn domain_task_mapping_preserves_identity_progress_and_eta() {
        let mut snapshot = TaskSnapshot::new(Gid::from_u64(9), DownloadStatus::Active, "video.mkv");
        snapshot.total_length = ByteCount::new(1_000);
        snapshot.completed_length = ByteCount::new(400);
        snapshot.download_speed = ByteRate::new(100);
        let task = DownloadTask::from_snapshot(snapshot);

        let mapped = map_task("profile", task);

        assert_eq!(mapped.identity.profile_id, "profile");
        assert_eq!(mapped.identity.gid, "0000000000000009");
        assert_eq!(mapped.status, TaskStatusView::Active);
        assert_eq!(mapped.eta_seconds, Some(6));
    }

    #[test]
    fn presentation_filter_maps_to_application_query() {
        let query = map_query(&WorkspaceQuery {
            filter: WorkspaceFilter::Completed,
            search: "archive".into(),
        });

        assert_eq!(query.filter, DownloadFilter::Completed);
        assert_eq!(query.search, "archive");
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
    fn details_mapping_keeps_remote_paths_as_display_strings() {
        let mapped = map_task_details(TaskDetails {
            gid: Gid::from_u64(7),
            directory: Some(EnginePath::new("/srv/downloads")),
            info_hash: None,
            piece_length: Some(ByteCount::new(1_024)),
            piece_count: Some(2),
            files: vec![TaskFile {
                index: 1,
                path: EnginePath::new("/srv/downloads/archive.bin"),
                length: ByteCount::new(2_048),
                completed_length: ByteCount::new(1_024),
                selected: true,
            }],
        });

        assert_eq!(mapped.directory.as_deref(), Some("/srv/downloads"));
        assert_eq!(mapped.files[0].path, "/srv/downloads/archive.bin");
        assert_eq!(mapped.files[0].completed_length, 1_024);
    }
}
