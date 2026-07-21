use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

use ariadeck_application::{
    AddDownloadAdvancedOptions, AddDownloadRequest, AddDownloadSource, AppCommand,
    ApplicationError, ApplicationErrorCode, CommandItem, CommandOutcome, CoordinatorConfig,
    DownloadDestinationFile, DownloadDestinationGateway, DownloadDestinationRequest,
    DownloadProxyConfig, DownloadProxyMode as ApplicationProxyMode, FileConflictPolicy,
    ItemFailure, MoveTaskInQueueRequest, QueueMove, ReconnectPolicy, RemoveTasksRequest,
    SetTaskOptionsRequest, SetTaskOutputNameRequest, SetTaskSpeedLimitRequest, StoreSnapshot,
    SyncHandle, TaskFileGateway, TaskFileRemovalRequest, TaskListQuery, TaskOpenRequest,
    TaskOpenTarget, TaskRemovalScope, spawn_sync_coordinator,
};
use ariadeck_domain::{
    ByteRate, ConnectionState, DownloadFilter, DownloadSort, DownloadStatus, DownloadTask,
    EnginePath, EngineSession, EngineSessionId, Gid, ProfileId, SessionGeneration, SortDirection,
    SortKey, SpeedLimitConfig, TaskConnectionDetails, TaskDetails,
    TaskIdentity as DomainTaskIdentity, TaskProgress, TaskUriStatus,
};
use ariadeck_engine::{
    ExternalEngineProfile, JsonProfileStore, LocalDownloadDestinationGateway,
    LocalDownloadRootRegistry, LocalEngineConfig, LocalEngineHealth, LocalEngineHealthHandle,
    LocalEngineSupervisor, LocalTaskFileGateway,
};
use ariadeck_rpc::{
    Aria2Client, AuthenticatedTransport, RpcSecret, RpcSyncConnector, WebSocketConfig,
    WebSocketTransport,
};
use ariadeck_settings::{
    AppSettings, ColorScheme, DownloadProxyMode, DownloadProxySettings, JsonSettingsStore,
    ProxyCredentialRef, ProxyCredentialStore, SpeedLimitSettings, SystemProxyCredentialStore,
};
use ariadeck_ui::{
    AddDownloadAdvancedOptionsView, AddDownloadItemResultView, AddDownloadMetadataFileView,
    AddDownloadMetadataKindView, AddDownloadMetadataPreviewItemView,
    AddDownloadMetadataPreviewOutcomeView, AddDownloadMetadataPreviewRequestView,
    AddDownloadMetadataPreviewResultView, AddDownloadMetadataPreviewView, AddDownloadModeView,
    AddDownloadRequestView, AddDownloadResultView, AddDownloadSourceView, AppShell, AppShellEvent,
    BatchCommandOutcomeView, BatchTaskCommandRequestView, BatchTaskCommandResultView,
    BatchTaskCommandView, BatchTaskFailureView, ColorSchemeView, CommandOutcomeView,
    ConnectionView, DownloadProxySettingsView, DownloadRowView, EngineHealthView,
    EngineSessionView, FileConflictPolicyView, GlobalTaskCommandRequestView,
    GlobalTaskCommandResultView, GlobalTaskCommandView, OperationErrorView, ProxyModeView,
    ProxyPasswordUpdateView, SettingsSaveOutcomeView, SettingsSaveRequestView,
    SettingsSaveResultView, SettingsView, SpeedLimitSettingsView, SpeedSampleView,
    StoppedHistoryView, TaskCommandRequestView, TaskCommandResultView, TaskCommandView,
    TaskCountsView, TaskDetailsOutcomeView, TaskDetailsRequestView, TaskDetailsResultView,
    TaskDetailsView, TaskErrorView, TaskFileView, TaskIdentity, TaskNameStateView,
    TaskOpenOutcomeView, TaskOpenRequestView, TaskOpenResultView, TaskOpenTargetView,
    TaskOptionView, TaskPathValidationView, TaskPeerView, TaskServerView, TaskSourceKindView,
    TaskStatusView, TaskTrackerView, TaskUriStatusView, TaskUriView, WorkspaceFilter,
    WorkspaceQuery, WorkspaceSnapshot, WorkspaceSortDirection, WorkspaceSortKey,
    format_speed_limit_field,
};
use data_encoding::BASE32_NOPAD;
use gpui::{AppContext as _, Context, Entity, IntoElement, Render, Subscription, Window};
use secrecy::SecretString;
use tokio::{
    runtime::Runtime,
    sync::{mpsc, watch},
    task::JoinHandle,
};
use url::Url;

use crate::metadata::parse_metadata;

pub struct DesktopRoot {
    workspace: Entity<AppShell>,
    sync: Option<SyncHandle>,
    download_destination_gateway: Option<Arc<dyn DownloadDestinationGateway>>,
    task_file_gateway: Option<Arc<dyn TaskFileGateway>>,
    local_engine: Option<LocalEngineSupervisor>,
    runtime: Arc<Runtime>,
    query_sender: watch::Sender<TaskListQuery>,
    settings_sender: Option<mpsc::UnboundedSender<SettingsPersistenceRequest>>,
    settings_task: Option<JoinHandle<()>>,
    settings: AppSettings,
    _workspace_subscription: Subscription,
}

#[derive(Clone)]
struct SettingsPersistenceRequest {
    request_id: ariadeck_ui::RequestId,
    settings: AppSettings,
    previous_settings: AppSettings,
    proxy_password: ProxyPasswordUpdate,
    apply_proxy: bool,
    apply_speed_limit: bool,
}

#[derive(Clone)]
enum ProxyPasswordUpdate {
    Unchanged,
    Clear,
    Set(SecretString),
}

struct SettingsPersistenceResult {
    request_id: ariadeck_ui::RequestId,
    settings: AppSettings,
    result: Result<(), String>,
}

impl DesktopRoot {
    #[must_use]
    pub fn new(runtime: Arc<Runtime>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let data_dir = default_data_dir();
        let defaults = AppSettings::new(
            env::var_os("ARIADECK_DOWNLOAD_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| data_dir.join("downloads")),
        );
        let settings_store = JsonSettingsStore::new(data_dir.join("settings.json"));
        let (mut settings, settings_store, startup_notice) =
            match settings_store.load_or_initialize(&defaults) {
                Ok(loaded) => {
                    let notice = loaded.recovery.map(|recovery| {
                        tracing::warn!(
                            reason = %recovery.reason,
                            backup = %recovery.backup_path.display(),
                            "recovered invalid application settings"
                        );
                        format!(
                            "Invalid settings were reset; the original was preserved at {}.",
                            recovery.backup_path.display()
                        )
                    });
                    (loaded.settings, Some(settings_store), notice)
                }
                Err(error) => {
                    tracing::error!(%error, "failed to load application settings");
                    (
                        defaults,
                        None,
                        Some(format!(
                            "Settings could not be loaded and saving is disabled: {error}"
                        )),
                    )
                }
            };
        if let Some(download_directory) = env::var_os("ARIADECK_DOWNLOAD_DIR") {
            settings.download_directory = PathBuf::from(download_directory);
        }

        let (sync, local_engine, mut initial_snapshot) =
            match create_sync_handle(&runtime, &data_dir, &settings) {
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
        let local_engine_health = local_engine
            .as_ref()
            .map(LocalEngineSupervisor::health_handle);
        let local_download_roots = local_engine
            .as_ref()
            .map(|_| LocalDownloadRootRegistry::new(settings.download_directory.clone()));
        let download_destination_gateway = local_download_roots.as_ref().map(|roots| {
            Arc::new(LocalDownloadDestinationGateway::with_roots(roots.clone()))
                as Arc<dyn DownloadDestinationGateway>
        });
        let task_file_gateway = local_download_roots.map(|roots| {
            Arc::new(LocalTaskFileGateway::with_roots(roots)) as Arc<dyn TaskFileGateway>
        });
        initial_snapshot.local_path_actions_available = task_file_gateway.is_some();
        let credential_store =
            Arc::new(SystemProxyCredentialStore::default()) as Arc<dyn ProxyCredentialStore>;
        let proxy_reapply_store = settings_store.clone();
        let (settings_sender, settings_task, settings_results) = settings_store.map_or_else(
            || (None, None, None),
            |store| {
                let (sender, task, results) = spawn_settings_persistence(
                    runtime.clone(),
                    store,
                    download_destination_gateway.clone(),
                    sync.clone(),
                    credential_store.clone(),
                );
                (Some(sender), Some(task), Some(results))
            },
        );
        let initial_engine_health = local_engine_health
            .as_ref()
            .and_then(LocalEngineHealthHandle::health)
            .map(map_local_engine_health)
            .unwrap_or(EngineHealthView::External);
        let settings_view = map_settings(&settings);
        let workspace = cx.new(|cx| {
            let mut shell = AppShell::new_with_settings(settings_view, window, cx);
            shell.set_snapshot(initial_snapshot, cx);
            shell.set_engine_health(initial_engine_health, cx);
            if let Some(message) = startup_notice {
                shell.set_startup_notice(message, true, cx);
            }
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
                AppShellEvent::LoadMoreStoppedRequested => {
                    this.spawn_load_more_stopped(window, cx);
                }
                AppShellEvent::AddDownloadRequested(request) => {
                    this.spawn_add_download(request.clone(), window, cx);
                }
                AppShellEvent::AddDownloadMetadataPreviewRequested(request) => {
                    this.spawn_add_download_metadata_preview(request.clone(), window, cx);
                }
                AppShellEvent::TaskCommandRequested(request) => {
                    this.spawn_task_command(request.clone(), window, cx);
                }
                AppShellEvent::BatchTaskCommandRequested(request) => {
                    this.spawn_batch_task_command(request.clone(), window, cx);
                }
                AppShellEvent::GlobalTaskCommandRequested(request) => {
                    this.spawn_global_task_command(request.clone(), window, cx);
                }
                AppShellEvent::TaskDetailsRequested(request) => {
                    this.spawn_task_details(request.clone(), window, cx);
                }
                AppShellEvent::TaskOpenRequested(request) => {
                    this.spawn_task_open(request.clone(), window, cx);
                }
                AppShellEvent::SettingsSaveRequested(request) => {
                    this.enqueue_settings_save(request.clone(), window, cx);
                }
            },
        );

        if let Some(handle) = sync.clone() {
            spawn_snapshot_bridge(handle, query_receiver, task_file_gateway.is_some(), cx);
        }
        if let Some(results) = settings_results {
            spawn_settings_result_bridge(results, window, cx);
        }
        if let (Some(handle), Some(store)) = (sync.clone(), proxy_reapply_store) {
            spawn_proxy_reapply_bridge(
                runtime.handle().clone(),
                handle,
                store,
                credential_store,
                cx,
            );
        }
        if let Some(health) = local_engine_health {
            spawn_local_engine_health_bridge(health, cx);
        }

        Self {
            workspace,
            sync,
            download_destination_gateway,
            task_file_gateway,
            local_engine,
            runtime,
            query_sender,
            settings_sender,
            settings_task,
            settings,
            _workspace_subscription: workspace_subscription,
        }
    }

    fn enqueue_settings_save(
        &self,
        request: SettingsSaveRequestView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (settings, proxy_password) = match map_settings_request(
            &request.settings,
            &self.settings,
            request.proxy_password.clone(),
        ) {
            Ok(mapped) => mapped,
            Err(error) => {
                self.deliver_settings_error(request, error, window, cx);
                return;
            }
        };
        let apply_proxy = settings.download_proxy != self.settings.download_proxy
            || !matches!(proxy_password, ProxyPasswordUpdate::Unchanged);
        let apply_speed_limit = settings.speed_limits != self.settings.speed_limits;
        let Some(sender) = &self.settings_sender else {
            self.deliver_settings_error(
                request,
                "Settings persistence is unavailable for this session.".into(),
                window,
                cx,
            );
            return;
        };
        if sender
            .send(SettingsPersistenceRequest {
                request_id: request.request_id,
                settings,
                previous_settings: self.settings.clone(),
                proxy_password,
                apply_proxy,
                apply_speed_limit,
            })
            .is_err()
        {
            self.deliver_settings_error(
                request,
                "The settings persistence worker stopped unexpectedly.".into(),
                window,
                cx,
            );
        }
    }

    fn deliver_settings_error(
        &self,
        request: SettingsSaveRequestView,
        summary: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_settings_save_result(
                SettingsSaveResultView {
                    request_id: request.request_id,
                    settings: request.settings,
                    outcome: SettingsSaveOutcomeView::Failure(OperationErrorView {
                        code: "settings.save_failed".into(),
                        summary,
                        retryable: true,
                    }),
                },
                window,
                cx,
            );
        });
    }

    fn spawn_add_download(
        &self,
        request: AddDownloadRequestView,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let runtime = self.runtime.handle().clone();
        let sync = self.sync.clone();
        let download_destination_gateway = self.download_destination_gateway.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result =
                execute_add_download(runtime, sync, download_destination_gateway, request).await;
            this.update_in(cx, |this, window, cx| {
                this.workspace.update(cx, |workspace, cx| {
                    workspace.set_add_download_result(result, window, cx);
                });
            })
            .ok();
        })
        .detach();
    }

    fn spawn_add_download_metadata_preview(
        &self,
        request: AddDownloadMetadataPreviewRequestView,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let runtime = self.runtime.handle().clone();
        let fallback_request = request.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = runtime
                .spawn_blocking(move || preview_metadata_files(request))
                .await
                .unwrap_or_else(|error| metadata_preview_worker_failure(fallback_request, error));
            this.update_in(cx, |this, _window, cx| {
                this.workspace.update(cx, |workspace, cx| {
                    workspace.set_add_download_metadata_preview_result(result, cx);
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
        let task_file_gateway = self.task_file_gateway.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = execute_task_command(sync, task_file_gateway, request).await;
            this.update_in(cx, |this, window, cx| {
                this.workspace.update(cx, |workspace, cx| {
                    workspace.set_task_command_result(result, window, cx);
                });
            })
            .ok();
        })
        .detach();
    }

    fn spawn_batch_task_command(
        &self,
        request: BatchTaskCommandRequestView,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let sync = self.sync.clone();
        let task_file_gateway = self.task_file_gateway.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = execute_batch_task_command(sync, task_file_gateway, request).await;
            this.update_in(cx, |this, window, cx| {
                this.workspace.update(cx, |workspace, cx| {
                    workspace.set_batch_task_command_result(result, window, cx);
                });
            })
            .ok();
        })
        .detach();
    }

    fn spawn_global_task_command(
        &self,
        request: GlobalTaskCommandRequestView,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let sync = self.sync.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = execute_global_task_command(sync, request).await;
            this.update(cx, |this, cx| {
                this.workspace.update(cx, |workspace, cx| {
                    workspace.set_global_task_command_result(result, cx);
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
        let runtime = self.runtime.handle().clone();
        let task_file_gateway = self.task_file_gateway.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = execute_task_details(runtime, sync, task_file_gateway, request).await;
            this.update_in(cx, |this, _window, cx| {
                this.workspace.update(cx, |workspace, cx| {
                    workspace.set_task_details_result(result, cx);
                });
            })
            .ok();
        })
        .detach();
    }

    fn spawn_task_open(
        &self,
        request: TaskOpenRequestView,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let sync = self.sync.clone();
        let task_file_gateway = self.task_file_gateway.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = execute_task_open(sync, task_file_gateway, request).await;
            this.update_in(cx, |this, _window, cx| {
                this.workspace.update(cx, |workspace, cx| {
                    workspace.set_task_open_result(result, cx);
                });
            })
            .ok();
        })
        .detach();
    }

    fn spawn_load_more_stopped(&self, window: &Window, cx: &mut Context<Self>) {
        let sync = self.sync.clone();
        cx.spawn_in(window, async move |this, cx| {
            let (success, message) = match sync {
                Some(handle) => match handle.load_more_stopped().await {
                    Some(history) if history.can_load_more => (
                        true,
                        Some(format!(
                            "Loaded more history ({} of {}).",
                            history.loaded,
                            history.total.unwrap_or(history.loaded)
                        )),
                    ),
                    Some(history) => (
                        true,
                        Some(format!(
                            "Loaded all available history ({} of {}).",
                            history.loaded,
                            history.total.unwrap_or(history.loaded)
                        )),
                    ),
                    None => (
                        false,
                        Some(
                            "Stopped history could not be loaded while aria2 is unavailable."
                                .into(),
                        ),
                    ),
                },
                None => (
                    false,
                    Some("Stopped history is unavailable without a connected engine.".into()),
                ),
            };
            this.update(cx, |this, cx| {
                this.workspace.update(cx, |workspace, cx| {
                    workspace.set_load_more_stopped_result(success, message, cx);
                });
            })
            .ok();
        })
        .detach();
    }
}

impl Drop for DesktopRoot {
    fn drop(&mut self) {
        self.settings_sender.take();
        if let Some(task) = self.settings_task.take()
            && let Err(error) = self.runtime.block_on(task)
        {
            tracing::warn!(%error, "settings persistence worker did not stop cleanly");
        }
        if let Some(handle) = self.sync.take() {
            self.runtime.block_on(handle.stop());
        }
        if let Some(mut process) = self.local_engine.take() {
            process.stop_monitoring();
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
    runtime: tokio::runtime::Handle,
    sync: Option<SyncHandle>,
    destination_gateway: Option<Arc<dyn DownloadDestinationGateway>>,
    request: AddDownloadRequestView,
) -> AddDownloadResultView {
    let AddDownloadRequestView {
        request_id,
        session,
        sources,
        mode,
        destination,
        required_bytes,
        file_conflict,
        advanced,
    } = request;
    let mapped_session = map_engine_session(&session);
    let destination_error = match (destination_gateway.clone(), destination.as_deref()) {
        (Some(gateway), Some(directory)) => {
            let request = DownloadDestinationRequest {
                directory: EnginePath::new(directory),
                required_bytes,
                files: Vec::new(),
            };
            match runtime
                .spawn_blocking(move || gateway.preflight(&request))
                .await
            {
                Ok(Ok(_)) => None,
                Ok(Err(error)) => Some(map_application_error(error.into())),
                Err(error) => Some(map_application_error(ApplicationError::new(
                    ApplicationErrorCode::Internal,
                    format!("Download destination preflight worker stopped: {error}"),
                    true,
                ))),
            }
        }
        _ => None,
    };
    let known_tasks = match (&sync, &mapped_session) {
        (Some(handle), Ok(_)) => handle
            .snapshot(ariadeck_application::TaskListQuery::default())
            .await
            .filter(|snapshot| {
                !snapshot.stale && matches!(snapshot.connection_state, ConnectionState::Connected)
            })
            .map(|snapshot| snapshot.tasks),
        _ => None,
    };
    let mut known_gids = known_tasks
        .as_ref()
        .map(|tasks| tasks.iter().map(|task| task.gid).collect::<HashSet<_>>());
    let groups = match mode {
        AddDownloadModeView::SeparateTasks => {
            sources.into_iter().map(|source| vec![source]).collect()
        }
        AddDownloadModeView::Mirrors => vec![sources],
    };
    let mut seen = HashSet::new();
    let mut items = Vec::with_capacity(groups.len());
    let mut has_successes = false;

    for group in groups {
        let duplicate_in_submission = mode == AddDownloadModeView::SeparateTasks
            && group
                .first()
                .is_some_and(|source| !seen.insert(add_source_submission_key(source)));
        let existing_task = (!duplicate_in_submission)
            .then(|| {
                known_tasks
                    .as_deref()
                    .and_then(|tasks| find_matching_add_task(tasks, &group))
                    .map(|task| TaskIdentity {
                        profile_id: session.profile_id.clone(),
                        gid: task.gid.to_string(),
                    })
            })
            .flatten();
        let outcome = if duplicate_in_submission {
            CommandOutcomeView::Failure(OperationErrorView {
                code: ApplicationErrorCode::Validation.as_str().into(),
                summary: "Duplicate source in this submission; the first occurrence was used."
                    .into(),
                retryable: false,
            })
        } else if existing_task.is_some() {
            CommandOutcomeView::Failure(OperationErrorView {
                code: ApplicationErrorCode::Duplicate.as_str().into(),
                summary: "This download is already present in the transfer list.".into(),
                retryable: false,
            })
        } else if let Some(error) = &destination_error {
            CommandOutcomeView::Failure(error.clone())
        } else {
            match (&sync, &mapped_session) {
                (Some(handle), Ok(engine_session)) => {
                    let request = prepare_add_download_request(
                        &runtime,
                        &group,
                        destination.clone(),
                        file_conflict,
                        advanced.clone(),
                    )
                    .await;
                    match request {
                        Ok(prepared) => {
                            let PreparedAddDownloadRequest {
                                request,
                                destination_files,
                                required_bytes,
                            } = prepared;
                            let metadata_destination_error = match (
                                destination_gateway.clone(),
                                destination.as_deref(),
                                destination_files.is_empty(),
                            ) {
                                (Some(gateway), Some(directory), false) => {
                                    let request = DownloadDestinationRequest {
                                        directory: EnginePath::new(directory),
                                        required_bytes,
                                        files: destination_files,
                                    };
                                    match runtime
                                        .spawn_blocking(move || gateway.preflight(&request))
                                        .await
                                    {
                                        Ok(Ok(_)) => None,
                                        Ok(Err(error)) => Some(map_application_error(error.into())),
                                        Err(error) => {
                                            Some(map_application_error(ApplicationError::new(
                                                ApplicationErrorCode::Internal,
                                                format!(
                                                    "Metadata destination preflight worker stopped: {error}"
                                                ),
                                                true,
                                            )))
                                        }
                                    }
                                }
                                _ => None,
                            };
                            if let Some(error) = metadata_destination_error {
                                CommandOutcomeView::Failure(error)
                            } else {
                                let outcome = handle
                                    .execute(*engine_session, AppCommand::AddDownload(request))
                                    .await;
                                if command_outcome_is_unknown(&outcome)
                                    && add_sources_are_uris(&group)
                                {
                                    reconcile_unknown_add(
                                        handle,
                                        &group,
                                        &mut known_gids,
                                        map_command_outcome(outcome),
                                    )
                                    .await
                                } else {
                                    let mapped = map_command_outcome(outcome);
                                    if let CommandOutcomeView::Success { tasks } = &mapped
                                        && let Some(known) = &mut known_gids
                                    {
                                        for task in tasks {
                                            if let Ok(identity) = map_task_identity(task) {
                                                known.insert(identity.gid);
                                            }
                                        }
                                    }
                                    mapped
                                }
                            }
                        }
                        Err(error) => CommandOutcomeView::Failure(map_application_error(error)),
                    }
                }
                (None, _) => CommandOutcomeView::Failure(unavailable_operation_error()),
                (Some(_), Err(error)) => {
                    CommandOutcomeView::Failure(map_application_error(error.clone()))
                }
            }
        };
        has_successes |= matches!(outcome, CommandOutcomeView::Success { .. });
        items.push(AddDownloadItemResultView {
            sources: group,
            existing_task,
            outcome,
        });
    }
    if has_successes && let Some(handle) = sync {
        handle.force_refresh().await;
    }
    AddDownloadResultView {
        request_id,
        session,
        items,
    }
}

const MAX_METADATA_FILE_BYTES: u64 = 16 * 1024 * 1024;

fn preview_metadata_files(
    request: AddDownloadMetadataPreviewRequestView,
) -> AddDownloadMetadataPreviewResultView {
    AddDownloadMetadataPreviewResultView {
        request_id: request.request_id,
        items: request
            .paths
            .into_iter()
            .map(preview_metadata_file)
            .collect(),
    }
}

fn preview_metadata_file(path: PathBuf) -> AddDownloadMetadataPreviewItemView {
    let outcome = metadata_kind_from_path(&path)
        .ok_or_else(|| {
            ApplicationError::new(
                ApplicationErrorCode::Validation,
                format!("Unsupported metadata file extension: {}", path.display()),
                false,
            )
        })
        .and_then(|kind| {
            let content = read_metadata_content(&path, kind)?;
            let preview = parse_metadata(kind, &content).map_err(|error| {
                ApplicationError::new(ApplicationErrorCode::Validation, error, false)
            })?;
            let selected_file_indices = preview.files.iter().map(|file| file.index).collect();
            Ok(AddDownloadMetadataPreviewView {
                path: path.clone(),
                kind,
                content_sha256: preview.content_sha256,
                info_hash: preview.info_hash,
                files: preview
                    .files
                    .into_iter()
                    .map(|file| AddDownloadMetadataFileView {
                        index: file.index,
                        path: file.path,
                        length: file.length,
                    })
                    .collect(),
                selected_file_indices,
            })
        });
    AddDownloadMetadataPreviewItemView {
        path,
        outcome: match outcome {
            Ok(preview) => AddDownloadMetadataPreviewOutcomeView::Ready(preview),
            Err(error) => {
                AddDownloadMetadataPreviewOutcomeView::Failed(map_application_error(error))
            }
        },
    }
}

fn metadata_preview_worker_failure(
    request: AddDownloadMetadataPreviewRequestView,
    error: tokio::task::JoinError,
) -> AddDownloadMetadataPreviewResultView {
    AddDownloadMetadataPreviewResultView {
        request_id: request.request_id,
        items: request
            .paths
            .into_iter()
            .map(|path| AddDownloadMetadataPreviewItemView {
                path,
                outcome: AddDownloadMetadataPreviewOutcomeView::Failed(OperationErrorView {
                    code: ApplicationErrorCode::Internal.as_str().into(),
                    summary: format!("Metadata preview worker stopped unexpectedly: {error}"),
                    retryable: true,
                }),
            })
            .collect(),
    }
}

async fn prepare_add_download_request(
    runtime: &tokio::runtime::Handle,
    sources: &[AddDownloadSourceView],
    destination: Option<String>,
    file_conflict: FileConflictPolicyView,
    advanced: AddDownloadAdvancedOptionsView,
) -> Result<PreparedAddDownloadRequest, ApplicationError> {
    let source = match sources {
        [
            AddDownloadSourceView::MetadataFile {
                path,
                kind,
                content_sha256,
                info_hash: _,
                selected_file_indices,
            },
        ] => {
            let path = path.clone();
            let kind = *kind;
            let content_sha256 = content_sha256.clone();
            let selected_file_indices = selected_file_indices.clone();
            runtime
                .spawn_blocking(move || {
                    read_metadata_source_with_selection(
                        &path,
                        kind,
                        &content_sha256,
                        &selected_file_indices,
                    )
                })
                .await
                .map_err(|error| {
                    ApplicationError::new(
                        ApplicationErrorCode::Internal,
                        format!("Metadata file reader stopped unexpectedly: {error}"),
                        true,
                    )
                })??
        }
        [] => {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "At least one download source is required.",
                false,
            ));
        }
        sources
            if sources
                .iter()
                .all(|source| matches!(source, AddDownloadSourceView::Uri { .. })) =>
        {
            PreparedMetadataSource {
                source: AddDownloadSource::Uris(
                    sources
                        .iter()
                        .filter_map(|source| match source {
                            AddDownloadSourceView::Uri { uri, .. } => Some(uri.clone()),
                            AddDownloadSourceView::MetadataFile { .. } => None,
                        })
                        .collect(),
                ),
                selected_file_indices: None,
                destination_files: Vec::new(),
                required_bytes: None,
            }
        }
        _ => {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "Torrent and Metalink files must be submitted one file at a time, separately from links.",
                false,
            ));
        }
    };
    let file_conflict = if matches!(source.source, AddDownloadSource::Uris(_)) {
        match file_conflict {
            FileConflictPolicyView::AutoRename => FileConflictPolicy::AutoRename,
            FileConflictPolicyView::Reject => FileConflictPolicy::Reject,
            FileConflictPolicyView::Overwrite => FileConflictPolicy::Overwrite,
        }
    } else {
        FileConflictPolicy::Reject
    };
    let advanced = map_add_advanced_options(advanced, &source.source)?;
    Ok(PreparedAddDownloadRequest {
        request: AddDownloadRequest {
            source: source.source,
            destination: destination.map(EnginePath::new),
            file_conflict,
            selected_file_indices: source.selected_file_indices,
            advanced,
            options: Vec::new(),
        },
        destination_files: source.destination_files,
        required_bytes: source.required_bytes,
    })
}

fn map_add_advanced_options(
    advanced: AddDownloadAdvancedOptionsView,
    source: &AddDownloadSource,
) -> Result<AddDownloadAdvancedOptions, ApplicationError> {
    if advanced.is_empty() {
        return Ok(AddDownloadAdvancedOptions::default());
    }
    if !matches!(source, AddDownloadSource::Uris(_)) {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            "Referer, headers, cookies, authentication, and checksum apply only to direct URL downloads.",
            false,
        ));
    }
    let headers = advanced
        .headers
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mapped = AddDownloadAdvancedOptions {
        referer: nonempty_optional(advanced.referer),
        user_agent: nonempty_optional(advanced.user_agent),
        headers,
        cookie: advanced
            .cookie
            .map(|value| SecretString::new(value.into_inner())),
        http_user: nonempty_optional(advanced.http_user),
        http_passwd: advanced
            .http_passwd
            .map(|value| SecretString::new(value.into_inner())),
        checksum: nonempty_optional(advanced.checksum),
    };
    mapped.validate()?;
    Ok(mapped)
}

fn nonempty_optional(value: String) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

#[derive(Debug)]
struct PreparedAddDownloadRequest {
    request: AddDownloadRequest,
    destination_files: Vec<DownloadDestinationFile>,
    required_bytes: Option<u64>,
}

#[derive(Debug)]
struct PreparedMetadataSource {
    source: AddDownloadSource,
    selected_file_indices: Option<Vec<u32>>,
    destination_files: Vec<DownloadDestinationFile>,
    required_bytes: Option<u64>,
}

fn read_metadata_source_with_selection(
    path: &Path,
    requested_kind: AddDownloadMetadataKindView,
    expected_sha256: &str,
    selected_file_indices: &[u32],
) -> Result<PreparedMetadataSource, ApplicationError> {
    let content = read_metadata_content(path, requested_kind)?;
    let preview = parse_metadata(requested_kind, &content)
        .map_err(|error| ApplicationError::new(ApplicationErrorCode::Validation, error, false))?;
    if !expected_sha256.is_empty() && preview.content_sha256 != expected_sha256 {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            format!(
                "Metadata file changed after preview; review it again before adding: {}",
                path.display()
            ),
            false,
        ));
    }
    let requested_indices = selected_file_indices;
    let selected_file_indices = if requested_indices.is_empty() {
        if expected_sha256.is_empty() {
            None
        } else {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                format!("Select at least one file from {}.", path.display()),
                false,
            ));
        }
    } else {
        let all_indexes = preview
            .files
            .iter()
            .map(|file| file.index)
            .collect::<HashSet<_>>();
        if requested_indices.first() == Some(&0)
            || requested_indices.windows(2).any(|pair| pair[0] >= pair[1])
            || requested_indices
                .iter()
                .any(|index| !all_indexes.contains(index))
        {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                format!(
                    "Metadata file selection is stale or invalid: {}",
                    path.display()
                ),
                false,
            ));
        }
        (requested_indices.len() != preview.files.len()).then(|| requested_indices.to_vec())
    };
    let mut required_bytes = 0_u64;
    let mut destination_files = Vec::with_capacity(requested_indices.len());
    for file in &preview.files {
        if requested_indices.is_empty() || requested_indices.binary_search(&file.index).is_ok() {
            if let Some(length) = file.length {
                required_bytes = required_bytes.checked_add(length).ok_or_else(|| {
                    ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        format!(
                            "Selected metadata file sizes exceed the supported range: {}",
                            path.display()
                        ),
                        false,
                    )
                })?;
            }
            destination_files.push(DownloadDestinationFile {
                relative_path: EnginePath::new(&file.path),
                reject_existing: true,
            });
        }
    }
    let source = match requested_kind {
        AddDownloadMetadataKindView::Torrent => AddDownloadSource::Torrent(content),
        AddDownloadMetadataKindView::Metalink => AddDownloadSource::Metalink(content),
    };
    Ok(PreparedMetadataSource {
        source,
        selected_file_indices,
        destination_files,
        required_bytes: Some(required_bytes),
    })
}

fn read_metadata_content(
    path: &Path,
    requested_kind: AddDownloadMetadataKindView,
) -> Result<Arc<[u8]>, ApplicationError> {
    let detected_kind = metadata_kind_from_path(path).ok_or_else(|| {
        ApplicationError::new(
            ApplicationErrorCode::Validation,
            format!("Unsupported metadata file extension: {}", path.display()),
            false,
        )
    })?;
    if detected_kind != requested_kind {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            format!(
                "Metadata file type changed before it could be read: {}",
                path.display()
            ),
            false,
        ));
    }

    let file = fs::File::open(path).map_err(|error| metadata_filesystem_error(path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| metadata_filesystem_error(path, error))?;
    if !metadata.is_file() {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            format!("Metadata source is not a regular file: {}", path.display()),
            false,
        ));
    }
    if metadata.len() == 0 {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            format!("Metadata file is empty: {}", path.display()),
            false,
        ));
    }
    if metadata.len() > MAX_METADATA_FILE_BYTES {
        return Err(metadata_file_too_large(path));
    }

    let mut content = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_METADATA_FILE_BYTES + 1)
        .read_to_end(&mut content)
        .map_err(|error| metadata_filesystem_error(path, error))?;
    if content.is_empty() {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            format!("Metadata file is empty: {}", path.display()),
            false,
        ));
    }
    if content.len() as u64 > MAX_METADATA_FILE_BYTES {
        return Err(metadata_file_too_large(path));
    }
    Ok(Arc::<[u8]>::from(content))
}

fn metadata_kind_from_path(path: &Path) -> Option<AddDownloadMetadataKindView> {
    let extension = path.extension()?.to_string_lossy();
    if extension.eq_ignore_ascii_case("torrent") {
        Some(AddDownloadMetadataKindView::Torrent)
    } else if extension.eq_ignore_ascii_case("metalink") || extension.eq_ignore_ascii_case("meta4")
    {
        Some(AddDownloadMetadataKindView::Metalink)
    } else {
        None
    }
}

fn metadata_filesystem_error(path: &Path, error: std::io::Error) -> ApplicationError {
    ApplicationError::new(
        ApplicationErrorCode::Filesystem,
        format!("Failed to read metadata file {}: {error}", path.display()),
        true,
    )
}

fn metadata_file_too_large(path: &Path) -> ApplicationError {
    ApplicationError::new(
        ApplicationErrorCode::Validation,
        format!(
            "Metadata file exceeds the 16 MiB upload limit: {}",
            path.display()
        ),
        false,
    )
}

fn add_sources_are_uris(sources: &[AddDownloadSourceView]) -> bool {
    !sources.is_empty()
        && sources
            .iter()
            .all(|source| matches!(source, AddDownloadSourceView::Uri { .. }))
}

fn add_source_submission_key(source: &AddDownloadSourceView) -> String {
    match source {
        AddDownloadSourceView::Uri { uri, .. } => magnet_info_hash(uri).map_or_else(
            || format!("uri:{}", normalize_add_uri_key(uri)),
            |info_hash| format!("info-hash:{}", info_hash.to_ascii_lowercase()),
        ),
        AddDownloadSourceView::MetadataFile {
            path, info_hash, ..
        } => {
            if let Some(info_hash) = info_hash {
                return format!("info-hash:{}", info_hash.to_ascii_lowercase());
            }
            let path = path.to_string_lossy().replace('\\', "/");
            let path = if cfg!(windows) {
                path.to_ascii_lowercase()
            } else {
                path
            };
            format!("metadata:{path}")
        }
    }
}

fn command_outcome_is_unknown(outcome: &CommandOutcome) -> bool {
    match outcome {
        CommandOutcome::PartialSuccess { failed, .. } | CommandOutcome::Failure { failed } => {
            failed
                .iter()
                .any(|failure| failure.error.code == ApplicationErrorCode::OutcomeUnknown)
        }
        CommandOutcome::Success { .. } => false,
    }
}

async fn reconcile_unknown_add(
    handle: &SyncHandle,
    sources: &[AddDownloadSourceView],
    known_gids: &mut Option<HashSet<Gid>>,
    unresolved: CommandOutcomeView,
) -> CommandOutcomeView {
    let Some(known) = known_gids.as_mut() else {
        return unresolved;
    };
    handle.force_refresh().await;
    let Some(snapshot) = handle
        .snapshot(ariadeck_application::TaskListQuery::default())
        .await
    else {
        return unresolved;
    };
    if snapshot.stale || !matches!(snapshot.connection_state, ConnectionState::Connected) {
        return unresolved;
    }
    if let Some(task) = find_new_matching_add_task(&snapshot.tasks, sources, known) {
        known.insert(task.gid);
        return CommandOutcomeView::Success {
            tasks: vec![TaskIdentity {
                profile_id: snapshot.session.profile_id.to_string(),
                gid: task.gid.to_string(),
            }],
        };
    }
    CommandOutcomeView::Failure(map_application_error(ApplicationError::new(
        ApplicationErrorCode::NotObserved,
        "aria2 did not report a new matching task after an authoritative refresh. This source can be submitted again safely.",
        true,
    )))
}

fn find_new_matching_add_task<'a>(
    tasks: &'a [DownloadTask],
    sources: &[AddDownloadSourceView],
    known_gids: &HashSet<Gid>,
) -> Option<&'a DownloadTask> {
    tasks
        .iter()
        .find(|task| !known_gids.contains(&task.gid) && task_matches_add_sources(task, sources))
}

fn find_matching_add_task<'a>(
    tasks: &'a [DownloadTask],
    sources: &[AddDownloadSourceView],
) -> Option<&'a DownloadTask> {
    tasks
        .iter()
        .find(|task| task_matches_add_sources(task, sources))
}

fn task_matches_add_sources(task: &DownloadTask, sources: &[AddDownloadSourceView]) -> bool {
    if let Some(primary_uri) = task.metadata.primary_uri.as_deref()
        && sources
            .iter()
            .filter_map(|source| match source {
                AddDownloadSourceView::Uri { uri, .. } => Some(uri.as_str()),
                AddDownloadSourceView::MetadataFile { .. } => None,
            })
            .any(|uri| add_uris_equal(primary_uri, uri))
    {
        return true;
    }
    let Some(task_info_hash) = task.metadata.info_hash.as_deref() else {
        return false;
    };
    sources.iter().any(|source| match source {
        AddDownloadSourceView::Uri { uri, .. } => magnet_info_hash(uri)
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(task_info_hash)),
        AddDownloadSourceView::MetadataFile { info_hash, .. } => info_hash
            .as_deref()
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(task_info_hash)),
    })
}

fn add_uris_equal(left: &str, right: &str) -> bool {
    match (Url::parse(left.trim()), Url::parse(right.trim())) {
        (Ok(left), Ok(right)) => left == right,
        _ => left.trim() == right.trim(),
    }
}

fn normalize_add_uri_key(uri: &str) -> String {
    Url::parse(uri.trim()).map_or_else(|_| uri.trim().to_owned(), |parsed| parsed.to_string())
}

fn magnet_info_hash(uri: &str) -> Option<String> {
    let parsed = Url::parse(uri.trim()).ok()?;
    if !parsed.scheme().eq_ignore_ascii_case("magnet") {
        return None;
    }
    let value = parsed
        .query_pairs()
        .find(|(key, _)| key.eq_ignore_ascii_case("xt"))?
        .1;
    let value = value.as_ref();
    const BTIH_PREFIX: &str = "urn:btih:";
    let prefix = value.get(..BTIH_PREFIX.len())?;
    if !prefix.eq_ignore_ascii_case(BTIH_PREFIX) {
        return None;
    }
    let hash = value.get(BTIH_PREFIX.len()..)?;
    if hash.len() == 40 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Some(hash.to_ascii_lowercase());
    }
    if hash.len() == 32 {
        let decoded = BASE32_NOPAD
            .decode(hash.to_ascii_uppercase().as_bytes())
            .ok()?;
        return Some(decoded.iter().map(|byte| format!("{byte:02x}")).collect());
    }
    None
}

async fn execute_global_task_command(
    sync: Option<SyncHandle>,
    request: GlobalTaskCommandRequestView,
) -> GlobalTaskCommandResultView {
    let GlobalTaskCommandRequestView {
        request_id,
        session,
        command,
    } = request;
    let app_command = match command {
        GlobalTaskCommandView::PauseAll => AppCommand::PauseAll,
        GlobalTaskCommandView::ForcePauseAll => AppCommand::ForcePauseAll,
        GlobalTaskCommandView::ResumeAll => AppCommand::ResumeAll,
    };
    let outcome = match (sync, map_engine_session(&session)) {
        (Some(handle), Ok(engine_session)) => {
            let outcome = handle.execute(engine_session, app_command).await;
            // D-014 global command rule / D-010: a success or unknown outcome
            // forces an authoritative refresh; an unknown mutation is never
            // replayed in the same session.
            if outcome.has_successes() || outcome.has_unknown_outcome() {
                handle.force_refresh().await;
            }
            map_command_outcome(outcome)
        }
        (None, _) => CommandOutcomeView::Failure(unavailable_operation_error()),
        (Some(_), Err(error)) => CommandOutcomeView::Failure(map_application_error(error)),
    };
    GlobalTaskCommandResultView {
        request_id,
        session,
        command,
        outcome,
    }
}

async fn execute_task_command(
    sync: Option<SyncHandle>,
    task_file_gateway: Option<Arc<dyn TaskFileGateway>>,
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
            let retry_baseline = if matches!(&command, TaskCommandView::Retry) {
                capture_retry_baseline(&handle, std::slice::from_ref(&task)).await
            } else {
                None
            };
            let remove_baseline = if matches!(
                &command,
                TaskCommandView::RemoveTask
                    | TaskCommandView::ForceRemoveTask
                    | TaskCommandView::RemoveTaskAndFiles
            ) {
                capture_remove_baseline(&handle, std::slice::from_ref(&task)).await
            } else {
                None
            };
            if matches!(&command, TaskCommandView::RemoveTaskAndFiles) {
                let outcome = execute_remove_with_files(
                    &handle,
                    task_file_gateway.as_deref(),
                    engine_session,
                    task,
                    remove_baseline,
                )
                .await;
                if outcome.has_successes() {
                    handle.force_refresh().await;
                }
                return TaskCommandResultView {
                    request_id,
                    session,
                    identity,
                    command,
                    outcome: map_command_outcome(outcome),
                };
            }
            let app_command = match &command {
                TaskCommandView::Pause => AppCommand::PauseTasks(vec![task]),
                TaskCommandView::ForcePause => AppCommand::ForcePauseTasks(vec![task]),
                TaskCommandView::Resume => AppCommand::ResumeTasks(vec![task]),
                TaskCommandView::MoveToQueueTop => {
                    AppCommand::MoveTaskInQueue(MoveTaskInQueueRequest {
                        task,
                        movement: QueueMove::Top,
                    })
                }
                TaskCommandView::MoveUpInQueue => {
                    AppCommand::MoveTaskInQueue(MoveTaskInQueueRequest {
                        task,
                        movement: QueueMove::Up,
                    })
                }
                TaskCommandView::MoveDownInQueue => {
                    AppCommand::MoveTaskInQueue(MoveTaskInQueueRequest {
                        task,
                        movement: QueueMove::Down,
                    })
                }
                TaskCommandView::MoveToQueueBottom => {
                    AppCommand::MoveTaskInQueue(MoveTaskInQueueRequest {
                        task,
                        movement: QueueMove::Bottom,
                    })
                }
                TaskCommandView::Retry => AppCommand::RetryTasks(vec![task]),
                TaskCommandView::SetOutputName { output_name } => {
                    AppCommand::SetTaskOutputName(SetTaskOutputNameRequest {
                        task,
                        output_name: output_name.clone(),
                    })
                }
                TaskCommandView::SetSpeedLimit {
                    download_limit,
                    upload_limit,
                } => AppCommand::SetTaskSpeedLimit(SetTaskSpeedLimitRequest {
                    task,
                    download_limit: ByteRate::new(*download_limit),
                    upload_limit: ByteRate::new(*upload_limit),
                }),
                TaskCommandView::SetOptions {
                    seed_ratio,
                    seed_time_minutes,
                    selected_file_indices,
                } => AppCommand::SetTaskOptions(SetTaskOptionsRequest {
                    task,
                    seed_ratio: seed_ratio.clone(),
                    seed_time_minutes: seed_time_minutes
                        .as_ref()
                        .and_then(|value| value.parse().ok()),
                    selected_file_indices: selected_file_indices.clone(),
                }),
                TaskCommandView::RemoveTask => AppCommand::RemoveTasks(RemoveTasksRequest {
                    tasks: vec![task],
                    scope: TaskRemovalScope::TaskOnly,
                }),
                TaskCommandView::ForceRemoveTask => {
                    AppCommand::ForceRemoveTasks(RemoveTasksRequest {
                        tasks: vec![task],
                        scope: TaskRemovalScope::TaskOnly,
                    })
                }
                TaskCommandView::RemoveTaskAndFiles => unreachable!("handled above"),
            };
            let mut outcome = handle.execute(engine_session, app_command).await;
            if matches!(&command, TaskCommandView::Retry) {
                outcome = reconcile_unknown_retries(&handle, retry_baseline, outcome).await;
            } else if matches!(
                &command,
                TaskCommandView::RemoveTask | TaskCommandView::ForceRemoveTask
            ) {
                outcome = reconcile_unknown_removals(&handle, remove_baseline, outcome).await;
            }
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

async fn execute_batch_task_command(
    sync: Option<SyncHandle>,
    task_file_gateway: Option<Arc<dyn TaskFileGateway>>,
    request: BatchTaskCommandRequestView,
) -> BatchTaskCommandResultView {
    let BatchTaskCommandRequestView {
        request_id,
        session,
        identities,
        command,
    } = request;
    let mapped = map_engine_session(&session).and_then(|engine_session| {
        identities
            .iter()
            .map(map_task_identity)
            .collect::<Result<Vec<_>, _>>()
            .map(|tasks| (engine_session, tasks))
    });
    let outcome = match (sync, mapped) {
        (Some(handle), Ok((engine_session, tasks))) => {
            let retry_baseline = if command == BatchTaskCommandView::Retry {
                capture_retry_baseline(&handle, &tasks).await
            } else {
                None
            };
            let remove_baseline = if matches!(
                command,
                BatchTaskCommandView::RemoveTask
                    | BatchTaskCommandView::ForceRemoveTask
                    | BatchTaskCommandView::RemoveTaskAndFiles
            ) {
                capture_remove_baseline(&handle, &tasks).await
            } else {
                None
            };
            if command == BatchTaskCommandView::RemoveTaskAndFiles {
                let outcome = execute_batch_remove_with_files(
                    &handle,
                    task_file_gateway.as_deref(),
                    engine_session,
                    &tasks,
                    remove_baseline,
                )
                .await;
                if outcome.has_successes() {
                    handle.force_refresh().await;
                }
                return BatchTaskCommandResultView {
                    request_id,
                    session,
                    identities,
                    command,
                    outcome: map_batch_command_outcome(outcome),
                };
            }
            let app_command = match command {
                BatchTaskCommandView::Pause => AppCommand::PauseTasks(tasks),
                BatchTaskCommandView::ForcePause => AppCommand::ForcePauseTasks(tasks),
                BatchTaskCommandView::Resume => AppCommand::ResumeTasks(tasks),
                BatchTaskCommandView::Retry => AppCommand::RetryTasks(tasks),
                BatchTaskCommandView::RemoveTask => AppCommand::RemoveTasks(RemoveTasksRequest {
                    tasks,
                    scope: TaskRemovalScope::TaskOnly,
                }),
                BatchTaskCommandView::ForceRemoveTask => {
                    AppCommand::ForceRemoveTasks(RemoveTasksRequest {
                        tasks,
                        scope: TaskRemovalScope::TaskOnly,
                    })
                }
                BatchTaskCommandView::RemoveTaskAndFiles => unreachable!("handled above"),
            };
            let mut outcome = handle.execute(engine_session, app_command).await;
            if command == BatchTaskCommandView::Retry {
                outcome = reconcile_unknown_retries(&handle, retry_baseline, outcome).await;
            } else if matches!(
                command,
                BatchTaskCommandView::RemoveTask | BatchTaskCommandView::ForceRemoveTask
            ) {
                outcome = reconcile_unknown_removals(&handle, remove_baseline, outcome).await;
            }
            if outcome.has_successes() {
                handle.force_refresh().await;
            }
            map_batch_command_outcome(outcome)
        }
        (None, _) => BatchCommandOutcomeView::Failure {
            failed: vec![BatchTaskFailureView {
                identity: None,
                error: unavailable_operation_error(),
            }],
        },
        (Some(_), Err(error)) => BatchCommandOutcomeView::Failure {
            failed: vec![BatchTaskFailureView {
                identity: None,
                error: map_application_error(error),
            }],
        },
    };
    BatchTaskCommandResultView {
        request_id,
        session,
        identities,
        command,
        outcome,
    }
}

#[derive(Clone)]
struct RemoveReconciliationBaseline {
    originals: HashMap<DomainTaskIdentity, DownloadTask>,
}

async fn capture_remove_baseline(
    handle: &SyncHandle,
    tasks: &[DomainTaskIdentity],
) -> Option<RemoveReconciliationBaseline> {
    let snapshot = handle
        .snapshot(ariadeck_application::TaskListQuery::default())
        .await?;
    if snapshot.stale || !matches!(snapshot.connection_state, ConnectionState::Connected) {
        return None;
    }
    let requested = tasks.iter().map(|task| task.gid).collect::<HashSet<_>>();
    let profile_id = snapshot.session.profile_id;
    Some(RemoveReconciliationBaseline {
        originals: snapshot
            .tasks
            .iter()
            .filter(|task| requested.contains(&task.gid))
            .map(|task| (DomainTaskIdentity::new(profile_id, task.gid), task.clone()))
            .collect(),
    })
}

async fn execute_remove_with_files(
    handle: &SyncHandle,
    file_gateway: Option<&dyn TaskFileGateway>,
    session: EngineSession,
    task: DomainTaskIdentity,
    baseline: Option<RemoveReconciliationBaseline>,
) -> CommandOutcome {
    let item = CommandItem::Task(task);
    let Some(file_gateway) = file_gateway else {
        return command_item_failure(
            item,
            ApplicationError::new(
                ApplicationErrorCode::Unsupported,
                "Local file removal is unavailable for this external engine profile.",
                false,
            ),
        );
    };
    let Some(original) = baseline
        .as_ref()
        .and_then(|baseline| baseline.originals.get(&task))
    else {
        return command_item_failure(
            item,
            ApplicationError::new(
                ApplicationErrorCode::Rejected,
                "The task is no longer available for a safe local-file preflight.",
                false,
            ),
        );
    };
    let details = match handle.task_details(session, task).await {
        Ok(details) => details,
        Err(error) => return command_item_failure(item, error),
    };
    let Some(directory) = details
        .directory
        .or_else(|| original.metadata.directory.clone())
    else {
        return command_item_failure(
            item,
            ApplicationError::new(
                ApplicationErrorCode::UnsafePath,
                "aria2 did not report a task directory; no local files were touched.",
                false,
            ),
        );
    };
    let file_request = TaskFileRemovalRequest {
        directory,
        files: details.files.into_iter().map(|file| file.path).collect(),
        include_control_files: original.status != DownloadStatus::Complete,
    };
    let original_status = original.status;
    let preview = match file_gateway.preflight(&file_request) {
        Ok(preview) => preview,
        Err(error) => return command_item_failure(item, error.into()),
    };
    tracing::info!(
        content_files = preview.content_files,
        control_files = preview.control_files,
        missing_paths = preview.missing_paths,
        "validated local task file removal"
    );

    if original_status.is_terminal()
        && let Err(error) = move_task_files_to_trash(file_gateway, &file_request).await
    {
        return command_item_failure(item, error);
    }

    let command = AppCommand::RemoveTasks(RemoveTasksRequest {
        tasks: vec![task],
        scope: TaskRemovalScope::TaskOnly,
    });
    let outcome = handle.execute(session, command).await;
    let outcome = reconcile_unknown_removals(handle, baseline, outcome).await;
    if !outcome.has_successes() || original_status.is_terminal() {
        return outcome;
    }
    if let Err(error) = move_task_files_to_trash(file_gateway, &file_request).await {
        return command_item_failure(item, error);
    }
    outcome
}

async fn execute_batch_remove_with_files(
    handle: &SyncHandle,
    file_gateway: Option<&dyn TaskFileGateway>,
    session: EngineSession,
    tasks: &[DomainTaskIdentity],
    baseline: Option<RemoveReconciliationBaseline>,
) -> CommandOutcome {
    if tasks.is_empty() {
        return CommandOutcome::failure(ApplicationError::new(
            ApplicationErrorCode::Validation,
            "At least one task must be selected.",
            false,
        ));
    }
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    let mut seen = HashSet::new();
    for task in tasks.iter().copied().filter(|task| seen.insert(*task)) {
        let outcome =
            execute_remove_with_files(handle, file_gateway, session, task, baseline.clone()).await;
        let (item_successes, item_failures) = split_command_outcome(outcome);
        succeeded.extend(item_successes);
        failed.extend(item_failures);
    }
    finish_reconciled_outcome(succeeded, failed)
}

async fn move_task_files_to_trash(
    gateway: &dyn TaskFileGateway,
    request: &TaskFileRemovalRequest,
) -> Result<(), ApplicationError> {
    let report = gateway
        .move_to_trash(request)
        .await
        .map_err(ApplicationError::from)?;
    tracing::info!(
        moved_to_trash = report.moved_to_trash,
        missing_paths = report.missing_paths,
        "moved local task files to Trash"
    );
    Ok(())
}

async fn reconcile_unknown_removals(
    handle: &SyncHandle,
    baseline: Option<RemoveReconciliationBaseline>,
    outcome: CommandOutcome,
) -> CommandOutcome {
    if !command_outcome_is_unknown(&outcome) {
        return outcome;
    }
    let Some(baseline) = baseline else {
        return outcome;
    };
    handle.force_refresh().await;
    let Some(snapshot) = handle
        .snapshot(ariadeck_application::TaskListQuery::default())
        .await
    else {
        return outcome;
    };
    if snapshot.stale || !matches!(snapshot.connection_state, ConnectionState::Connected) {
        return outcome;
    }
    reconcile_remove_outcome(&baseline, &snapshot.tasks, outcome)
}

fn reconcile_remove_outcome(
    baseline: &RemoveReconciliationBaseline,
    tasks: &[DownloadTask],
    outcome: CommandOutcome,
) -> CommandOutcome {
    let (mut succeeded, failed) = split_command_outcome(outcome);
    let mut remaining_failures = Vec::new();
    for mut failure in failed {
        if failure.error.code != ApplicationErrorCode::OutcomeUnknown {
            remaining_failures.push(failure);
            continue;
        }
        let Some(CommandItem::Task(identity)) = failure.item else {
            remaining_failures.push(failure);
            continue;
        };
        let Some(original) = baseline.originals.get(&identity) else {
            remaining_failures.push(failure);
            continue;
        };
        let observed = tasks.iter().find(|task| task.gid == identity.gid);
        let removal_observed = if original.status.is_terminal() {
            observed.is_none()
        } else {
            observed.is_none_or(|task| task.status == DownloadStatus::Removed)
        };
        if removal_observed {
            succeeded.push(CommandItem::Task(identity));
        } else {
            failure.error = ApplicationError::new(
                ApplicationErrorCode::RemovalNotObserved,
                "aria2 did not report the task as removed after an authoritative refresh. The removal can be requested again safely.",
                true,
            );
            remaining_failures.push(failure);
        }
    }
    finish_reconciled_outcome(succeeded, remaining_failures)
}

fn command_item_failure(item: CommandItem, error: ApplicationError) -> CommandOutcome {
    CommandOutcome::Failure {
        failed: vec![ItemFailure {
            item: Some(item),
            error,
        }],
    }
}

#[derive(Clone)]
struct RetryReconciliationBaseline {
    known_gids: HashSet<Gid>,
    originals: HashMap<DomainTaskIdentity, DownloadTask>,
}

async fn capture_retry_baseline(
    handle: &SyncHandle,
    tasks: &[DomainTaskIdentity],
) -> Option<RetryReconciliationBaseline> {
    let snapshot = handle
        .snapshot(ariadeck_application::TaskListQuery::default())
        .await?;
    if snapshot.stale || !matches!(snapshot.connection_state, ConnectionState::Connected) {
        return None;
    }

    let requested = tasks.iter().map(|task| task.gid).collect::<HashSet<_>>();
    let profile_id = snapshot.session.profile_id;
    let originals = snapshot
        .tasks
        .iter()
        .filter(|task| requested.contains(&task.gid))
        .map(|task| (DomainTaskIdentity::new(profile_id, task.gid), task.clone()))
        .collect();
    Some(RetryReconciliationBaseline {
        known_gids: snapshot.tasks.iter().map(|task| task.gid).collect(),
        originals,
    })
}

async fn reconcile_unknown_retries(
    handle: &SyncHandle,
    baseline: Option<RetryReconciliationBaseline>,
    outcome: CommandOutcome,
) -> CommandOutcome {
    if !command_outcome_is_unknown(&outcome) {
        return outcome;
    }
    let Some(baseline) = baseline else {
        return outcome;
    };

    handle.force_refresh().await;
    let Some(snapshot) = handle
        .snapshot(ariadeck_application::TaskListQuery::default())
        .await
    else {
        return outcome;
    };
    if snapshot.stale || !matches!(snapshot.connection_state, ConnectionState::Connected) {
        return outcome;
    }

    reconcile_retry_outcome(
        baseline,
        snapshot.session.profile_id,
        &snapshot.tasks,
        outcome,
    )
}

fn reconcile_retry_outcome(
    baseline: RetryReconciliationBaseline,
    profile_id: ProfileId,
    candidates: &[DownloadTask],
    outcome: CommandOutcome,
) -> CommandOutcome {
    let (mut succeeded, failed) = split_command_outcome(outcome);
    let mut reserved_gids = baseline.known_gids;
    reserved_gids.extend(succeeded.iter().map(|item| match item {
        CommandItem::Task(identity) => identity.gid,
    }));
    let mut remaining_failures = Vec::new();
    for mut failure in failed {
        if failure.error.code != ApplicationErrorCode::OutcomeUnknown {
            remaining_failures.push(failure);
            continue;
        }
        let Some(CommandItem::Task(original_identity)) = failure.item else {
            remaining_failures.push(failure);
            continue;
        };
        let Some(original) = baseline.originals.get(&original_identity) else {
            remaining_failures.push(failure);
            continue;
        };
        if let Some(replacement) = candidates.iter().find(|candidate| {
            !reserved_gids.contains(&candidate.gid)
                && task_matches_retry_source(candidate, original)
        }) {
            reserved_gids.insert(replacement.gid);
            succeeded.push(CommandItem::Task(DomainTaskIdentity::new(
                profile_id,
                replacement.gid,
            )));
        } else {
            failure.error = ApplicationError::new(
                ApplicationErrorCode::RetryNotObserved,
                "aria2 did not report a new matching retry task after an authoritative refresh. The failed task can be retried again safely.",
                true,
            );
            remaining_failures.push(failure);
        }
    }
    finish_reconciled_outcome(succeeded, remaining_failures)
}

fn task_matches_retry_source(candidate: &DownloadTask, original: &DownloadTask) -> bool {
    if let (Some(candidate_uri), Some(original_uri)) = (
        candidate.metadata.primary_uri.as_deref(),
        original.metadata.primary_uri.as_deref(),
    ) && add_uris_equal(candidate_uri, original_uri)
    {
        return true;
    }

    let original_hash = original.metadata.info_hash.clone().or_else(|| {
        original
            .metadata
            .primary_uri
            .as_deref()
            .and_then(magnet_info_hash)
    });
    match (
        candidate.metadata.info_hash.as_deref(),
        original_hash.as_deref(),
    ) {
        (Some(candidate), Some(original)) => candidate.eq_ignore_ascii_case(original),
        _ => false,
    }
}

fn split_command_outcome(outcome: CommandOutcome) -> (Vec<CommandItem>, Vec<ItemFailure>) {
    match outcome {
        CommandOutcome::Success { succeeded } => (succeeded, Vec::new()),
        CommandOutcome::PartialSuccess { succeeded, failed } => (succeeded, failed),
        CommandOutcome::Failure { failed } => (Vec::new(), failed),
    }
}

fn finish_reconciled_outcome(
    succeeded: Vec<CommandItem>,
    failed: Vec<ItemFailure>,
) -> CommandOutcome {
    match (succeeded.is_empty(), failed.is_empty()) {
        (false, true) => CommandOutcome::Success { succeeded },
        (false, false) => CommandOutcome::PartialSuccess { succeeded, failed },
        (true, false) => CommandOutcome::Failure { failed },
        (true, true) => CommandOutcome::Failure {
            failed: vec![ItemFailure {
                item: None,
                error: ApplicationError::new(
                    ApplicationErrorCode::Internal,
                    "Retry reconciliation produced no result.",
                    false,
                ),
            }],
        },
    }
}

async fn execute_task_details(
    runtime: tokio::runtime::Handle,
    sync: Option<SyncHandle>,
    task_file_gateway: Option<Arc<dyn TaskFileGateway>>,
    request: TaskDetailsRequestView,
) -> TaskDetailsResultView {
    let TaskDetailsRequestView {
        request_id,
        session,
        identity,
        active,
        is_bittorrent,
    } = request;
    let mapped = map_engine_session(&session)
        .and_then(|engine_session| map_task_identity(&identity).map(|task| (engine_session, task)));
    let outcome = match (sync, mapped) {
        (Some(handle), Ok((engine_session, task))) => {
            let task_details = handle.task_details(engine_session, task);
            let connection_details =
                handle.connection_details(engine_session, task, active, is_bittorrent);
            match tokio::join!(task_details, connection_details) {
                (Ok(details), Ok(connection)) if details.gid == connection.gid => {
                    let path_validation =
                        validate_task_paths(&runtime, task_file_gateway, &details).await;
                    TaskDetailsOutcomeView::Ready(Box::new(map_task_details(
                        details,
                        connection,
                        path_validation,
                    )))
                }
                (Ok(_), Ok(_)) => TaskDetailsOutcomeView::Failed(OperationErrorView {
                    code: ApplicationErrorCode::Internal.as_str().into(),
                    summary: "aria2 returned mismatched task detail identities.".into(),
                    retryable: false,
                }),
                (Err(error), _) | (_, Err(error)) => {
                    TaskDetailsOutcomeView::Failed(map_application_error(error))
                }
            }
        }
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

async fn validate_task_paths(
    runtime: &tokio::runtime::Handle,
    gateway: Option<Arc<dyn TaskFileGateway>>,
    details: &TaskDetails,
) -> TaskPathValidationView {
    let Some(gateway) = gateway else {
        return TaskPathValidationView::Unavailable;
    };
    let Some(directory) = details.directory.clone() else {
        return TaskPathValidationView::Warning(OperationErrorView {
            code: ApplicationErrorCode::Validation.as_str().into(),
            summary:
                "aria2 did not report a task directory, so the local path could not be validated."
                    .into(),
            retryable: true,
        });
    };
    if details.files.is_empty() {
        return TaskPathValidationView::Warning(OperationErrorView {
            code: ApplicationErrorCode::Validation.as_str().into(),
            summary:
                "aria2 did not report any task files, so the local path could not be validated."
                    .into(),
            retryable: true,
        });
    }
    let request = TaskFileRemovalRequest {
        directory,
        files: details.files.iter().map(|file| file.path.clone()).collect(),
        include_control_files: false,
    };
    match runtime
        .spawn_blocking(move || gateway.preflight(&request))
        .await
    {
        Ok(Ok(preview)) => TaskPathValidationView::Valid {
            existing_files: preview.content_files,
            missing_paths: preview.missing_paths,
        },
        Ok(Err(error)) => TaskPathValidationView::Warning(map_application_error(error.into())),
        Err(error) => TaskPathValidationView::Warning(OperationErrorView {
            code: ApplicationErrorCode::Internal.as_str().into(),
            summary: format!("Local path validation worker stopped unexpectedly: {error}"),
            retryable: true,
        }),
    }
}

async fn execute_task_open(
    sync: Option<SyncHandle>,
    task_file_gateway: Option<Arc<dyn TaskFileGateway>>,
    request: TaskOpenRequestView,
) -> TaskOpenResultView {
    let mapped = map_engine_session(&request.session)
        .and_then(|session| map_task_identity(&request.identity).map(|task| (session, task)));
    let outcome = match (sync, task_file_gateway, mapped) {
        (Some(handle), Some(gateway), Ok((session, task))) => {
            match handle.task_details(session, task).await {
                Ok(details) if details.gid == task.gid => {
                    let Some(directory) = details.directory else {
                        return TaskOpenResultView {
                            request_id: request.request_id,
                            session: request.session,
                            identity: request.identity,
                            target: request.target,
                            outcome: TaskOpenOutcomeView::Failure(OperationErrorView {
                                code: ApplicationErrorCode::Validation.as_str().into(),
                                summary: "aria2 did not report a task directory.".into(),
                                retryable: true,
                            }),
                        };
                    };
                    let open_request = TaskOpenRequest {
                        directory,
                        files: details.files.into_iter().map(|file| file.path).collect(),
                        target: match request.target {
                            TaskOpenTargetView::Download => TaskOpenTarget::Download,
                            TaskOpenTargetView::Folder => TaskOpenTarget::Folder,
                        },
                    };
                    match gateway.open(&open_request).await {
                        Ok(()) => TaskOpenOutcomeView::Success,
                        Err(error) => {
                            TaskOpenOutcomeView::Failure(map_application_error(error.into()))
                        }
                    }
                }
                Ok(_) => TaskOpenOutcomeView::Failure(OperationErrorView {
                    code: ApplicationErrorCode::Internal.as_str().into(),
                    summary: "aria2 returned details for a different task.".into(),
                    retryable: false,
                }),
                Err(error) => TaskOpenOutcomeView::Failure(map_application_error(error)),
            }
        }
        (_, None, _) => TaskOpenOutcomeView::Failure(OperationErrorView {
            code: ApplicationErrorCode::Unsupported.as_str().into(),
            summary: "Opening downloads is available only for the managed local engine.".into(),
            retryable: false,
        }),
        (None, _, _) => TaskOpenOutcomeView::Failure(unavailable_operation_error()),
        (Some(_), Some(_), Err(error)) => {
            TaskOpenOutcomeView::Failure(map_application_error(error))
        }
    };
    TaskOpenResultView {
        request_id: request.request_id,
        session: request.session,
        identity: request.identity,
        target: request.target,
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
            tasks: succeeded.into_iter().map(map_command_item).collect(),
        },
        CommandOutcome::PartialSuccess { succeeded, failed } => {
            if succeeded.is_empty() {
                CommandOutcomeView::Failure(
                    failed
                        .into_iter()
                        .next()
                        .map(|failure| map_application_error(failure.error))
                        .unwrap_or_else(internal_operation_error),
                )
            } else {
                CommandOutcomeView::Success {
                    tasks: succeeded.into_iter().map(map_command_item).collect(),
                }
            }
        }
        CommandOutcome::Failure { failed } => CommandOutcomeView::Failure(
            failed
                .into_iter()
                .next()
                .map(|failure| map_application_error(failure.error))
                .unwrap_or_else(internal_operation_error),
        ),
    }
}

fn map_batch_command_outcome(outcome: CommandOutcome) -> BatchCommandOutcomeView {
    match outcome {
        CommandOutcome::Success { succeeded } => BatchCommandOutcomeView::Success {
            succeeded: succeeded.into_iter().map(map_command_item).collect(),
        },
        CommandOutcome::PartialSuccess { succeeded, failed } => {
            BatchCommandOutcomeView::PartialSuccess {
                succeeded: succeeded.into_iter().map(map_command_item).collect(),
                failed: failed.into_iter().map(map_batch_failure).collect(),
            }
        }
        CommandOutcome::Failure { failed } => BatchCommandOutcomeView::Failure {
            failed: failed.into_iter().map(map_batch_failure).collect(),
        },
    }
}

fn map_batch_failure(failure: ItemFailure) -> BatchTaskFailureView {
    BatchTaskFailureView {
        identity: failure.item.map(|item| match item {
            CommandItem::Task(identity) => TaskIdentity {
                profile_id: identity.profile_id.to_string(),
                gid: identity.gid.to_string(),
            },
        }),
        error: map_application_error(failure.error),
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

fn map_task_details(
    details: TaskDetails,
    connection: TaskConnectionDetails,
    path_validation: TaskPathValidationView,
) -> TaskDetailsView {
    let directory = details.directory.as_ref().map(ToString::to_string);
    let output_path = if details.files.len() == 1 {
        details.files.first().map(|file| file.path.to_string())
    } else {
        directory.clone()
    };
    let primary_source = connection
        .uris
        .first()
        .map(|source| sanitize_source_uri(&source.uri));
    TaskDetailsView {
        directory,
        primary_source,
        output_path,
        path_validation,
        info_hash: details.info_hash,
        piece_length: details.piece_length.map(|length| length.get()),
        piece_count: details.piece_count,
        trackers: details
            .trackers
            .into_iter()
            .map(|tracker| TaskTrackerView {
                tier: tracker.tier,
                uri: tracker.uri,
            })
            .collect(),
        uris: connection
            .uris
            .into_iter()
            .map(|uri| TaskUriView {
                uri: uri.uri,
                status: match uri.status {
                    TaskUriStatus::Used => TaskUriStatusView::Used,
                    TaskUriStatus::Waiting => TaskUriStatusView::Waiting,
                    TaskUriStatus::Unknown => TaskUriStatusView::Unknown,
                },
            })
            .collect(),
        servers: connection
            .servers
            .into_iter()
            .map(|server| TaskServerView {
                file_index: server.file_index,
                uri: server.uri,
                current_uri: server.current_uri,
                download_rate: server.download_speed.get(),
            })
            .collect(),
        peers: connection
            .peers
            .into_iter()
            .map(|peer| TaskPeerView {
                address: peer.address,
                port: peer.port,
                download_rate: peer.download_speed.get(),
                upload_rate: peer.upload_speed.get(),
                seeder: peer.seeder,
            })
            .collect(),
        options: connection
            .options
            .into_iter()
            .map(|option| TaskOptionView {
                key: option.key,
                value: option.value,
                redacted: option.redacted,
            })
            .collect(),
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
    data_dir: &Path,
    settings: &AppSettings,
) -> Result<(SyncHandle, Option<LocalEngineSupervisor>), String> {
    let external_endpoint = env::var("ARIADECK_RPC_URL")
        .ok()
        .filter(|endpoint| !endpoint.trim().is_empty());
    let rpc_runtime =
        RpcRuntimeConfig::from_values(external_endpoint.is_some(), |name| env::var(name).ok())?;
    let (endpoint, secret, local_engine, profile_id) = if let Some(endpoint) = external_endpoint {
        if endpoint.trim() != endpoint {
            return Err("ARIADECK_RPC_URL must not contain surrounding whitespace.".into());
        }
        let endpoint =
            Url::parse(&endpoint).map_err(|error| format!("Invalid ARIADECK_RPC_URL: {error}"))?;
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
        let config = resolve_local_engine_config(data_dir, &settings.download_directory)?;
        let profile_id = config.profile_id;
        let process = LocalEngineSupervisor::spawn(&config)
            .map_err(|error| format!("Failed to start local aria2: {error}"))?;
        let endpoint = process.endpoint().clone();
        let secret = Some(RpcSecret::new(process.secret().to_owned()));
        (endpoint, secret, Some(process), profile_id)
    };

    let mut websocket = WebSocketConfig::new(endpoint.clone());
    websocket.connect_timeout = rpc_runtime.connect_timeout;
    websocket.request_timeout = rpc_runtime.request_timeout;
    websocket.allow_insecure_remote = rpc_runtime.allow_insecure_remote;
    websocket.validate().map_err(|error| error.to_string())?;
    let connector = Arc::new(RpcSyncConnector::new(websocket, secret));
    let mut coordinator = CoordinatorConfig::new(profile_id);
    coordinator.reconnect = rpc_runtime.reconnect;
    tracing::info!(
        scheme = endpoint.scheme(),
        host = endpoint.host_str().unwrap_or("unknown"),
        port = endpoint.port_or_known_default(),
        connect_timeout_ms = rpc_runtime.connect_timeout.as_millis(),
        request_timeout_ms = rpc_runtime.request_timeout.as_millis(),
        reconnect_base_ms = rpc_runtime.reconnect.base_delay.as_millis(),
        reconnect_max_ms = rpc_runtime.reconnect.max_delay.as_millis(),
        reconnect_max_attempts = ?rpc_runtime.reconnect.max_attempts,
        "configured external aria2 RPC profile"
    );
    let _runtime_guard = runtime.enter();
    Ok((spawn_sync_coordinator(connector, coordinator), local_engine))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RpcRuntimeConfig {
    connect_timeout: Duration,
    request_timeout: Duration,
    reconnect: ReconnectPolicy,
    allow_insecure_remote: bool,
}

impl RpcRuntimeConfig {
    fn from_values(
        external: bool,
        mut value: impl FnMut(&str) -> Option<String>,
    ) -> Result<Self, String> {
        let defaults = ReconnectPolicy::default();
        let connect_timeout = parse_millisecond_setting(
            "ARIADECK_RPC_CONNECT_TIMEOUT_MS",
            value("ARIADECK_RPC_CONNECT_TIMEOUT_MS"),
            if external {
                Duration::from_secs(10)
            } else {
                Duration::from_millis(750)
            },
        )?;
        let request_timeout = parse_millisecond_setting(
            "ARIADECK_RPC_REQUEST_TIMEOUT_MS",
            value("ARIADECK_RPC_REQUEST_TIMEOUT_MS"),
            if external {
                Duration::from_secs(15)
            } else {
                Duration::from_secs(5)
            },
        )?;
        let base_delay = parse_millisecond_setting(
            "ARIADECK_RPC_RECONNECT_BASE_DELAY_MS",
            value("ARIADECK_RPC_RECONNECT_BASE_DELAY_MS"),
            defaults.base_delay,
        )?;
        let max_delay = parse_millisecond_setting(
            "ARIADECK_RPC_RECONNECT_MAX_DELAY_MS",
            value("ARIADECK_RPC_RECONNECT_MAX_DELAY_MS"),
            defaults.max_delay,
        )?;
        if base_delay > max_delay {
            return Err(
                "ARIADECK_RPC_RECONNECT_BASE_DELAY_MS must not exceed ARIADECK_RPC_RECONNECT_MAX_DELAY_MS."
                    .into(),
            );
        }
        let reset_after = parse_millisecond_setting(
            "ARIADECK_RPC_RECONNECT_RESET_AFTER_MS",
            value("ARIADECK_RPC_RECONNECT_RESET_AFTER_MS"),
            defaults.reset_after,
        )?;
        let max_attempts = parse_max_attempts(value("ARIADECK_RPC_RECONNECT_MAX_ATTEMPTS"))?;
        let allow_insecure_remote = parse_boolean_setting(
            "ARIADECK_RPC_ALLOW_INSECURE_REMOTE",
            value("ARIADECK_RPC_ALLOW_INSECURE_REMOTE"),
            false,
        )?;
        Ok(Self {
            connect_timeout,
            request_timeout,
            reconnect: ReconnectPolicy {
                base_delay,
                max_delay,
                jitter_percent: defaults.jitter_percent,
                max_attempts,
                reset_after,
            },
            allow_insecure_remote,
        })
    }
}

fn parse_millisecond_setting(
    name: &'static str,
    value: Option<String>,
    default: Duration,
) -> Result<Duration, String> {
    let Some(value) = value else {
        return Ok(default);
    };
    let milliseconds = value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be an integer number of milliseconds."))?;
    if !(1..=3_600_000).contains(&milliseconds) {
        return Err(format!(
            "{name} must be between 1 and 3600000 milliseconds."
        ));
    }
    Ok(Duration::from_millis(milliseconds))
}

fn parse_max_attempts(value: Option<String>) -> Result<Option<u32>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let attempts = value.parse::<u32>().map_err(|_| {
        "ARIADECK_RPC_RECONNECT_MAX_ATTEMPTS must be a positive integer.".to_owned()
    })?;
    if attempts == 0 {
        return Err("ARIADECK_RPC_RECONNECT_MAX_ATTEMPTS must be at least 1.".into());
    }
    Ok(Some(attempts))
}

fn parse_boolean_setting(
    name: &'static str,
    value: Option<String>,
    default: bool,
) -> Result<bool, String> {
    let Some(value) = value else {
        return Ok(default);
    };
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(format!("{name} must be true, false, 1, or 0.")),
    }
}

fn resolve_local_engine_config(
    data_dir: &Path,
    download_dir: &Path,
) -> Result<LocalEngineConfig, String> {
    let executable = env::var_os("ARIADECK_ARIA2C_PATH")
        .map(PathBuf::from)
        .or_else(discover_aria2_executable)
        .ok_or_else(|| {
            "No aria2 executable found. Set ARIADECK_ARIA2C_PATH or ARIADECK_RPC_URL.".to_owned()
        })?;
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
        data_dir.to_path_buf(),
        download_dir.to_path_buf(),
    );
    profile_store
        .save(&profile)
        .map_err(|error| format!("Failed to save local aria2 profile: {error}"))?;
    Ok(profile.local_config())
}

async fn request_local_engine_shutdown(process: &LocalEngineSupervisor) -> Result<(), String> {
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

fn map_settings(settings: &AppSettings) -> SettingsView {
    SettingsView {
        color_scheme: match settings.color_scheme {
            ColorScheme::Light => ColorSchemeView::Light,
            ColorScheme::Dark => ColorSchemeView::Dark,
        },
        download_directory: settings.download_directory.to_string_lossy().into_owned(),
        download_proxy: DownloadProxySettingsView {
            mode: match settings.download_proxy.mode {
                DownloadProxyMode::Disabled => ProxyModeView::Disabled,
                DownloadProxyMode::Manual => ProxyModeView::Manual,
            },
            all_proxy: settings
                .download_proxy
                .all_proxy
                .clone()
                .unwrap_or_default(),
            http_proxy: settings
                .download_proxy
                .http_proxy
                .clone()
                .unwrap_or_default(),
            https_proxy: settings
                .download_proxy
                .https_proxy
                .clone()
                .unwrap_or_default(),
            ftp_proxy: settings
                .download_proxy
                .ftp_proxy
                .clone()
                .unwrap_or_default(),
            no_proxy: settings.download_proxy.no_proxy.clone(),
            username: settings.download_proxy.username.clone().unwrap_or_default(),
            has_password: settings.download_proxy.credential.is_some(),
        },
        speed_limits: SpeedLimitSettingsView {
            download_limit: format_speed_limit_field(settings.speed_limits.download_limit),
            upload_limit: format_speed_limit_field(settings.speed_limits.upload_limit),
        },
    }
}

fn map_settings_request(
    settings: &SettingsView,
    current: &AppSettings,
    password: ProxyPasswordUpdateView,
) -> Result<(AppSettings, ProxyPasswordUpdate), String> {
    let password = match password {
        ProxyPasswordUpdateView::Unchanged => ProxyPasswordUpdate::Unchanged,
        ProxyPasswordUpdateView::Clear => ProxyPasswordUpdate::Clear,
        ProxyPasswordUpdateView::Set(password) => {
            let password = password.into_inner();
            if password.is_empty() {
                return Err("Proxy password must not be empty.".into());
            }
            ProxyPasswordUpdate::Set(SecretString::new(password))
        }
    };
    let credential = match &password {
        ProxyPasswordUpdate::Unchanged => current.download_proxy.credential,
        ProxyPasswordUpdate::Clear => None,
        ProxyPasswordUpdate::Set(_) => Some(current.download_proxy.credential.unwrap_or_default()),
    };
    let mapped = AppSettings {
        color_scheme: match settings.color_scheme {
            ColorSchemeView::Light => ColorScheme::Light,
            ColorSchemeView::Dark => ColorScheme::Dark,
        },
        download_directory: PathBuf::from(settings.download_directory.trim()),
        download_proxy: DownloadProxySettings {
            mode: match settings.download_proxy.mode {
                ProxyModeView::Disabled => DownloadProxyMode::Disabled,
                ProxyModeView::Manual => DownloadProxyMode::Manual,
            },
            all_proxy: trimmed_value(&settings.download_proxy.all_proxy),
            http_proxy: trimmed_value(&settings.download_proxy.http_proxy),
            https_proxy: trimmed_value(&settings.download_proxy.https_proxy),
            ftp_proxy: trimmed_value(&settings.download_proxy.ftp_proxy),
            no_proxy: settings
                .download_proxy
                .no_proxy
                .iter()
                .map(|entry| entry.trim())
                .filter(|entry| !entry.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
            username: trimmed_value(&settings.download_proxy.username),
            credential,
        },
        speed_limits: SpeedLimitSettings {
            download_limit: settings
                .speed_limits
                .parse_download_limit()
                .ok_or_else(|| "Download speed limit must be bytes/second or a K/M/G value (or empty for unlimited).".to_owned())?,
            upload_limit: settings
                .speed_limits
                .parse_upload_limit()
                .ok_or_else(|| "Upload speed limit must be bytes/second or a K/M/G value (or empty for unlimited).".to_owned())?,
        },
    };
    mapped.validate().map_err(|error| error.to_string())?;
    Ok((mapped, password))
}

fn trimmed_value(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn spawn_settings_persistence(
    runtime: Arc<Runtime>,
    store: JsonSettingsStore,
    destination_gateway: Option<Arc<dyn DownloadDestinationGateway>>,
    sync: Option<SyncHandle>,
    credential_store: Arc<dyn ProxyCredentialStore>,
) -> (
    mpsc::UnboundedSender<SettingsPersistenceRequest>,
    JoinHandle<()>,
    mpsc::UnboundedReceiver<SettingsPersistenceResult>,
) {
    let (requests, mut request_receiver) = mpsc::unbounded_channel::<SettingsPersistenceRequest>();
    let (results, result_receiver) = mpsc::unbounded_channel();
    let task = runtime.spawn(async move {
        while let Some(request) = request_receiver.recv().await {
            let result = persist_settings_request(
                store.clone(),
                destination_gateway.clone(),
                sync.clone(),
                credential_store.clone(),
                request.clone(),
            )
            .await;
            let _ = results.send(SettingsPersistenceResult {
                request_id: request.request_id,
                settings: request.settings,
                result,
            });
        }
    });
    (requests, task, result_receiver)
}

async fn persist_settings_request(
    store: JsonSettingsStore,
    destination_gateway: Option<Arc<dyn DownloadDestinationGateway>>,
    sync: Option<SyncHandle>,
    credential_store: Arc<dyn ProxyCredentialStore>,
    request: SettingsPersistenceRequest,
) -> Result<(), String> {
    let settings_for_preflight = request.settings.clone();
    tokio::task::spawn_blocking(move || {
        preflight_settings(&settings_for_preflight, destination_gateway.as_deref())
    })
    .await
    .map_err(|error| format!("settings preflight task failed: {error}"))??;

    // Speed limits carry no credentials, so they are applied independently of
    // the proxy credential dance. When only speed limits change we still push
    // them to the running engine before persisting, then persist and roll the
    // engine back on a save failure so disk and engine stay consistent.
    if request.apply_speed_limit && !request.apply_proxy {
        return apply_speed_limit_only(store, sync, request).await;
    }

    if !request.apply_proxy {
        let settings = request.settings;
        return tokio::task::spawn_blocking(move || {
            store.save(&settings).map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| format!("settings persistence task failed: {error}"))?;
    }

    let previous_settings = request.previous_settings.clone();
    let next_settings = request.settings.clone();
    let password_update = request.proxy_password.clone();
    let credentials = credential_store.clone();
    let (previous_password, password, mutation) = tokio::task::spawn_blocking(move || {
        let previous_password = load_proxy_password(credentials.as_ref(), &previous_settings)?;
        let (password, mutation) = apply_credential_update(
            credentials.as_ref(),
            &previous_settings,
            &next_settings,
            &password_update,
            previous_password.clone(),
        )?;
        Ok::<_, String>((previous_password, password, mutation))
    })
    .await
    .map_err(|error| format!("credential update task failed: {error}"))??;
    let Some(sync) = sync else {
        rollback_credential_async(credential_store, mutation).await?;
        return Err(
            "Download proxy settings cannot be applied because aria2 is unavailable.".into(),
        );
    };
    let Some(snapshot) = sync.snapshot(TaskListQuery::default()).await else {
        rollback_credential_async(credential_store, mutation).await?;
        return Err("Download proxy settings cannot be applied because the synchronization coordinator is unavailable.".into());
    };
    let next_proxy = map_download_proxy_config(&request.settings, password);
    if let Err(error) = sync
        .apply_download_proxy(snapshot.session, next_proxy)
        .await
    {
        return match rollback_credential_async(credential_store, mutation).await {
            Ok(()) => Err(error.summary),
            Err(rollback) => Err(format!(
                "{} Credential rollback also failed: {rollback}",
                error.summary
            )),
        };
    }

    if request.apply_speed_limit
        && let Err(error) = sync
            .apply_speed_limit(snapshot.session, map_speed_limit_config(&request.settings))
            .await
    {
        // Roll the proxy and credential mutation back so the engine matches the
        // still-unchanged persisted settings.
        let rollback_proxy =
            map_download_proxy_config(&request.previous_settings, previous_password);
        let engine_rollback = sync
            .apply_download_proxy(snapshot.session, rollback_proxy)
            .await
            .err()
            .map(|error| error.summary);
        let credential_rollback = rollback_credential_async(credential_store, mutation)
            .await
            .err();
        let mut summary = error.summary;
        if let Some(error) = engine_rollback {
            summary.push_str(&format!(" Proxy rollback also failed: {error}"));
        }
        if let Some(error) = credential_rollback {
            summary.push_str(&format!(" Credential rollback also failed: {error}"));
        }
        return Err(summary);
    }

    let settings_to_save = request.settings.clone();
    let save_store = store.clone();
    if let Err(error) = tokio::task::spawn_blocking(move || {
        save_store
            .save(&settings_to_save)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("settings persistence task failed: {error}"))?
    {
        let rollback_proxy =
            map_download_proxy_config(&request.previous_settings, previous_password);
        let engine_rollback = sync
            .apply_download_proxy(snapshot.session, rollback_proxy)
            .await
            .err()
            .map(|error| error.summary);
        let speed_rollback = if request.apply_speed_limit {
            sync.apply_speed_limit(
                snapshot.session,
                map_speed_limit_config(&request.previous_settings),
            )
            .await
            .err()
            .map(|error| error.summary)
        } else {
            None
        };
        let credential_rollback = rollback_credential_async(credential_store, mutation)
            .await
            .err();
        let mut summary = format!("Failed to persist proxy settings: {error}");
        if let Some(error) = engine_rollback {
            summary.push_str(&format!(" Engine rollback also failed: {error}"));
        }
        if let Some(error) = speed_rollback {
            summary.push_str(&format!(" Speed-limit rollback also failed: {error}"));
        }
        if let Some(error) = credential_rollback {
            summary.push_str(&format!(" Credential rollback also failed: {error}"));
        }
        return Err(summary);
    }
    Ok(())
}

/// Apply and persist a speed-limit-only settings change.
///
/// Pushes the new limits to the running engine first, then persists to disk and
/// rolls the engine back to the previous limits if persistence fails, so the
/// engine never diverges from the source-of-truth settings file.
async fn apply_speed_limit_only(
    store: JsonSettingsStore,
    sync: Option<SyncHandle>,
    request: SettingsPersistenceRequest,
) -> Result<(), String> {
    let Some(sync) = sync else {
        return Err("Speed limits cannot be applied because aria2 is unavailable.".into());
    };
    let Some(snapshot) = sync.snapshot(TaskListQuery::default()).await else {
        return Err(
            "Speed limits cannot be applied because the synchronization coordinator is unavailable."
                .into(),
        );
    };
    if let Err(error) = sync
        .apply_speed_limit(snapshot.session, map_speed_limit_config(&request.settings))
        .await
    {
        return Err(error.summary);
    }

    let settings_to_save = request.settings.clone();
    let save_store = store.clone();
    if let Err(error) = tokio::task::spawn_blocking(move || {
        save_store
            .save(&settings_to_save)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("settings persistence task failed: {error}"))?
    {
        let engine_rollback = sync
            .apply_speed_limit(
                snapshot.session,
                map_speed_limit_config(&request.previous_settings),
            )
            .await
            .err()
            .map(|error| error.summary);
        let mut summary = format!("Failed to persist speed limits: {error}");
        if let Some(error) = engine_rollback {
            summary.push_str(&format!(" Engine rollback also failed: {error}"));
        }
        return Err(summary);
    }
    Ok(())
}

#[derive(Clone)]
struct CredentialMutation {
    credential: Option<ProxyCredentialRef>,
    previous_password: Option<SecretString>,
}

async fn rollback_credential_async(
    store: Arc<dyn ProxyCredentialStore>,
    mutation: CredentialMutation,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || rollback_credential(store.as_ref(), &mutation))
        .await
        .map_err(|error| format!("credential rollback task failed: {error}"))?
}

fn apply_credential_update(
    store: &dyn ProxyCredentialStore,
    previous: &AppSettings,
    next: &AppSettings,
    update: &ProxyPasswordUpdate,
    previous_password: Option<SecretString>,
) -> Result<(Option<SecretString>, CredentialMutation), String> {
    match update {
        ProxyPasswordUpdate::Unchanged => {
            if next.download_proxy.credential.is_some() && previous_password.is_none() {
                return Err("The saved proxy password is missing from the system credential store. Enter it again or clear the saved password.".into());
            }
            Ok((
                previous_password,
                CredentialMutation {
                    credential: None,
                    previous_password: None,
                },
            ))
        }
        ProxyPasswordUpdate::Clear => {
            if let Some(credential) = previous.download_proxy.credential {
                store
                    .delete(credential)
                    .map_err(|error| error.to_string())?;
            }
            Ok((
                None,
                CredentialMutation {
                    credential: previous.download_proxy.credential,
                    previous_password,
                },
            ))
        }
        ProxyPasswordUpdate::Set(password) => {
            let credential = next.download_proxy.credential.ok_or_else(|| {
                "A proxy credential reference was not allocated for the new password.".to_owned()
            })?;
            store
                .save(credential, password)
                .map_err(|error| error.to_string())?;
            Ok((
                Some(password.clone()),
                CredentialMutation {
                    credential: Some(credential),
                    previous_password,
                },
            ))
        }
    }
}

fn rollback_credential(
    store: &dyn ProxyCredentialStore,
    mutation: &CredentialMutation,
) -> Result<(), String> {
    let Some(credential) = mutation.credential else {
        return Ok(());
    };
    if let Some(password) = &mutation.previous_password {
        store
            .save(credential, password)
            .map_err(|error| error.to_string())
    } else {
        store.delete(credential).map_err(|error| error.to_string())
    }
}

fn load_proxy_password(
    store: &dyn ProxyCredentialStore,
    settings: &AppSettings,
) -> Result<Option<SecretString>, String> {
    settings
        .download_proxy
        .credential
        .map_or(Ok(None), |credential| {
            store.load(credential).map_err(|error| error.to_string())
        })
}

fn map_download_proxy_config(
    settings: &AppSettings,
    password: Option<SecretString>,
) -> DownloadProxyConfig {
    DownloadProxyConfig {
        mode: match settings.download_proxy.mode {
            DownloadProxyMode::Disabled => ApplicationProxyMode::Disabled,
            DownloadProxyMode::Manual => ApplicationProxyMode::Manual,
        },
        all_proxy: settings.download_proxy.all_proxy.clone(),
        http_proxy: settings.download_proxy.http_proxy.clone(),
        https_proxy: settings.download_proxy.https_proxy.clone(),
        ftp_proxy: settings.download_proxy.ftp_proxy.clone(),
        no_proxy: settings.download_proxy.no_proxy.clone(),
        username: settings.download_proxy.username.clone(),
        password,
    }
}

fn map_speed_limit_config(settings: &AppSettings) -> SpeedLimitConfig {
    SpeedLimitConfig {
        download_limit: ByteRate::new(settings.speed_limits.download_limit),
        upload_limit: ByteRate::new(settings.speed_limits.upload_limit),
    }
}

#[cfg(test)]
fn persist_settings(
    store: &JsonSettingsStore,
    settings: &AppSettings,
    destination_gateway: Option<&dyn DownloadDestinationGateway>,
) -> Result<(), String> {
    preflight_settings(settings, destination_gateway)?;
    store.save(settings).map_err(|error| error.to_string())
}

fn preflight_settings(
    settings: &AppSettings,
    destination_gateway: Option<&dyn DownloadDestinationGateway>,
) -> Result<(), String> {
    settings.validate().map_err(|error| error.to_string())?;
    if let Some(gateway) = destination_gateway {
        if !settings.download_directory.is_absolute() {
            return Err(format!(
                "Local download directory must be absolute: {}",
                settings.download_directory.display()
            ));
        }
        fs::create_dir_all(&settings.download_directory).map_err(|error| {
            format!(
                "Failed to create download directory {}: {error}",
                settings.download_directory.display()
            )
        })?;
        gateway
            .preflight(&DownloadDestinationRequest {
                directory: EnginePath::new(settings.download_directory.to_string_lossy()),
                required_bytes: None,
                files: Vec::new(),
            })
            .map_err(|error| error.message)?;
    }
    Ok(())
}

fn spawn_settings_result_bridge(
    mut results: mpsc::UnboundedReceiver<SettingsPersistenceResult>,
    window: &Window,
    cx: &mut Context<DesktopRoot>,
) {
    cx.spawn_in(window, async move |this, cx| {
        while let Some(result) = results.recv().await {
            if this
                .update_in(cx, |this, window, cx| {
                    if result.result.is_ok() {
                        this.settings = result.settings.clone();
                    }
                    let outcome = result.result.map_or_else(
                        |summary| {
                            SettingsSaveOutcomeView::Failure(OperationErrorView {
                                code: "settings.save_failed".into(),
                                summary,
                                retryable: true,
                            })
                        },
                        |()| SettingsSaveOutcomeView::Success,
                    );
                    this.workspace.update(cx, |workspace, cx| {
                        workspace.set_settings_save_result(
                            SettingsSaveResultView {
                                request_id: result.request_id,
                                settings: map_settings(&result.settings),
                                outcome,
                            },
                            window,
                            cx,
                        );
                    });
                })
                .is_err()
            {
                break;
            }
        }
    })
    .detach();
}

fn spawn_proxy_reapply_bridge(
    runtime: tokio::runtime::Handle,
    handle: SyncHandle,
    store: JsonSettingsStore,
    credential_store: Arc<dyn ProxyCredentialStore>,
    cx: &mut Context<DesktopRoot>,
) {
    let mut events = handle.subscribe();
    cx.spawn(async move |this, cx| {
        let mut attempted_session = None;
        loop {
            let Some(snapshot) = handle.snapshot(TaskListQuery::default()).await else {
                break;
            };
            if matches!(snapshot.connection_state, ConnectionState::Connected)
                && attempted_session.as_ref() != Some(&snapshot.session)
            {
                attempted_session = Some(snapshot.session);
                let settings_store = store.clone();
                let credentials = credential_store.clone();
                let loaded = spawn_proxy_settings_load(&runtime, settings_store, credentials)
                    .await
                    .map_err(|error| format!("proxy configuration task failed: {error}"))
                    .and_then(|result| result);
                let result = match loaded {
                    Ok((settings, password)) => {
                        let proxy = map_download_proxy_config(&settings, password);
                        let proxy_result = handle
                            .apply_download_proxy(snapshot.session, proxy)
                            .await
                            .map_err(|error| {
                                format!(
                                    "Download proxy settings were not applied: {}",
                                    error.summary
                                )
                            });
                        // Reapply persisted speed limits on the fresh session so
                        // a reconnect restores the user's throttle. Zero limits
                        // are still sent so aria2 explicitly clears any default.
                        let speed_result = handle
                            .apply_speed_limit(snapshot.session, map_speed_limit_config(&settings))
                            .await
                            .map_err(|error| {
                                format!("Speed limits were not applied: {}", error.summary)
                            });
                        proxy_result.and(speed_result)
                    }
                    Err(error) => Err(error),
                };
                if let Err(error) = result
                    && this
                        .update(cx, |this, cx| {
                            this.workspace.update(cx, |workspace, cx| {
                                workspace.set_startup_notice(error, true, cx);
                            });
                        })
                        .is_err()
                {
                    break;
                }
            }

            match events.recv().await {
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    })
    .detach();
}

fn spawn_proxy_settings_load(
    runtime: &tokio::runtime::Handle,
    store: JsonSettingsStore,
    credential_store: Arc<dyn ProxyCredentialStore>,
) -> JoinHandle<Result<(AppSettings, Option<SecretString>), String>> {
    runtime.spawn_blocking(move || {
        let settings = store.load().map_err(|error| error.to_string())?;
        let password = load_proxy_password(credential_store.as_ref(), &settings)?;
        if settings.download_proxy.credential.is_some() && password.is_none() {
            return Err(
                "The saved proxy password is missing from the system credential store. Enter it again or clear the saved password."
                    .into(),
            );
        }
        Ok((settings, password))
    })
}

fn spawn_local_engine_health_bridge(
    health_handle: LocalEngineHealthHandle,
    cx: &mut Context<DesktopRoot>,
) {
    let executor = cx.background_executor().clone();
    cx.spawn(async move |this, cx| {
        let mut previous = None;
        while let Some(health) = health_handle.health() {
            if previous.as_ref() != Some(&health) {
                let view = map_local_engine_health(health.clone());
                previous = Some(health);
                if this
                    .update(cx, |this, cx| {
                        this.workspace.update(cx, |workspace, cx| {
                            workspace.set_engine_health(view, cx);
                        });
                    })
                    .is_err()
                {
                    break;
                }
            }
            executor.timer(Duration::from_millis(250)).await;
        }
    })
    .detach();
}

fn spawn_snapshot_bridge(
    handle: SyncHandle,
    mut query_receiver: watch::Receiver<TaskListQuery>,
    local_path_actions_available: bool,
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
                let snapshot = map_snapshot(snapshot, local_path_actions_available);
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
        sort: DownloadSort {
            key: match query.sort_key {
                WorkspaceSortKey::Queue => SortKey::Queue,
                WorkspaceSortKey::Name => SortKey::Name,
                WorkspaceSortKey::Status => SortKey::Status,
                WorkspaceSortKey::Progress => SortKey::Progress,
                WorkspaceSortKey::DownloadSpeed => SortKey::DownloadSpeed,
                WorkspaceSortKey::Size => SortKey::Size,
            },
            direction: match query.sort_direction {
                WorkspaceSortDirection::Ascending => SortDirection::Ascending,
                WorkspaceSortDirection::Descending => SortDirection::Descending,
            },
        },
    }
}

fn map_snapshot(snapshot: StoreSnapshot, local_path_actions_available: bool) -> WorkspaceSnapshot {
    let profile_id = snapshot.session.profile_id.to_string();
    let observed_seeding_seconds = snapshot.observed_seeding_seconds;
    WorkspaceSnapshot {
        profile_id: profile_id.clone(),
        session_id: snapshot.session.session_id.to_string(),
        generation: snapshot.session.generation.get(),
        source_revision: snapshot.view.source_revision,
        connection: map_connection(snapshot.connection_state),
        stale: snapshot.stale,
        local_path_actions_available,
        download_rate: snapshot.global_stat.download_speed.get(),
        upload_rate: snapshot.global_stat.upload_speed.get(),
        speed_history: snapshot
            .speed_history
            .samples()
            .iter()
            .map(|sample| SpeedSampleView {
                download_rate: sample.download.get(),
                upload_rate: sample.upload.get(),
            })
            .collect(),
        counts: TaskCountsView {
            all: snapshot.counts.all,
            active: snapshot.counts.active,
            waiting: snapshot.counts.waiting,
            paused: snapshot.counts.paused,
            completed: snapshot.counts.completed,
            failed: snapshot.counts.failed,
        },
        stopped_history: StoppedHistoryView {
            loaded: snapshot.stopped_history.loaded,
            total: snapshot.stopped_history.total,
            can_load_more: snapshot.stopped_history.can_load_more,
        },
        tasks: snapshot
            .tasks
            .into_iter()
            .map(|task| {
                let observed = observed_seeding_seconds.get(&task.gid).copied();
                map_task(&profile_id, task, observed)
            })
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

fn map_local_engine_health(health: LocalEngineHealth) -> EngineHealthView {
    match health {
        LocalEngineHealth::Running { restarts } => EngineHealthView::Running { restarts },
        LocalEngineHealth::Restarting { attempt } => EngineHealthView::Restarting { attempt },
        LocalEngineHealth::Failed { reason, .. } => EngineHealthView::Failed { summary: reason },
    }
}

fn map_task(
    profile_id: &str,
    task: DownloadTask,
    observed_seeding_seconds: Option<u64>,
) -> DownloadRowView {
    let eta_seconds = (task.status != DownloadStatus::Seeding)
        .then(|| {
            TaskProgress::new(task.completed_length, task.total_length)
                .eta(task.download_speed)
                .map(|duration| duration.as_secs())
        })
        .flatten();
    let primary_source = task
        .metadata
        .primary_uri
        .as_deref()
        .map(sanitize_source_uri);
    let directory = task.metadata.directory.as_ref().map(ToString::to_string);
    DownloadRowView {
        identity: TaskIdentity {
            profile_id: profile_id.into(),
            gid: task.gid.to_string(),
        },
        display_name: task.display_name,
        name_state: match task.name_state {
            ariadeck_domain::TaskNameState::Resolving => TaskNameStateView::Resolving,
            ariadeck_domain::TaskNameState::Resolved => TaskNameStateView::Resolved,
            ariadeck_domain::TaskNameState::Custom => TaskNameStateView::Custom,
        },
        source_kind: match task.metadata.source_kind {
            ariadeck_domain::TaskSourceKind::Unknown => TaskSourceKindView::Unknown,
            ariadeck_domain::TaskSourceKind::DirectUri => TaskSourceKindView::DirectUri,
            ariadeck_domain::TaskSourceKind::Magnet => TaskSourceKindView::Magnet,
            ariadeck_domain::TaskSourceKind::BitTorrent => TaskSourceKindView::BitTorrent,
            ariadeck_domain::TaskSourceKind::Metalink => TaskSourceKindView::Metalink,
        },
        primary_source,
        directory,
        followed_by: task
            .metadata
            .followed_by
            .into_iter()
            .map(|gid| gid.to_string())
            .collect(),
        belongs_to: task.metadata.belongs_to.map(|gid| gid.to_string()),
        status: match task.status {
            DownloadStatus::Active => TaskStatusView::Active,
            DownloadStatus::Seeding => TaskStatusView::Seeding,
            DownloadStatus::Waiting => TaskStatusView::Waiting,
            DownloadStatus::Paused => TaskStatusView::Paused,
            DownloadStatus::Complete => TaskStatusView::Complete,
            DownloadStatus::Error => TaskStatusView::Failed,
            DownloadStatus::Removed => TaskStatusView::Removed,
            DownloadStatus::Verifying => TaskStatusView::Verifying,
            DownloadStatus::Unknown => TaskStatusView::Unknown,
        },
        error: task.error.map(classify_task_error),
        total_bytes: task.total_length.get(),
        completed_bytes: task.completed_length.get(),
        uploaded_bytes: task.upload_length.get(),
        download_rate: task.download_speed.get(),
        upload_rate: task.upload_speed.get(),
        eta_seconds,
        observed_seeding_seconds,
        revision: task.revision,
    }
}

fn classify_task_error(error: ariadeck_domain::TaskError) -> TaskErrorView {
    let message = error.message.trim();
    let normalized = message.to_ascii_lowercase();
    let summary = if normalized.contains("permission denied")
        || normalized.contains("access is denied")
        || normalized.contains("access denied")
    {
        "Permission denied. Check the download directory and file permissions.".into()
    } else if normalized.contains("file name too long")
        || normalized.contains("filename too long")
        || normalized.contains("path too long")
        || normalized.contains("error 206")
    {
        "The output path is too long. Choose a shorter directory or filename.".into()
    } else {
        match error.code {
            Some(9) => "Not enough disk space in the download directory.".into(),
            Some(10) => "The downloaded piece length does not match the expected metadata.".into(),
            Some(11) => "Output conflict: another task is downloading the same file.".into(),
            Some(12) => "Output conflict: the same Torrent is already downloading.".into(),
            Some(13) => "Output conflict: the destination file already exists.".into(),
            Some(14) => {
                "aria2 could not rename the output file. Check the path and permissions.".into()
            }
            Some(15) => {
                "aria2 could not open the output file. Check that it exists and is accessible."
                    .into()
            }
            Some(16) => {
                "aria2 could not create the output file. Check the directory and permissions."
                    .into()
            }
            Some(17) => "A filesystem input/output error interrupted the download.".into(),
            Some(18) => "aria2 could not create the download directory.".into(),
            _ if !message.is_empty() => message.into(),
            Some(code) => format!("aria2 reported error code {code}."),
            None => "aria2 reported an unspecified download error.".into(),
        }
    };
    TaskErrorView {
        code: error.code,
        summary,
        details: (!message.is_empty()).then(|| message.to_owned()),
    }
}

fn sanitize_source_uri(uri: &str) -> String {
    if let Some(info_hash) = magnet_info_hash(uri) {
        return format!("magnet:?xt=urn:btih:{}", info_hash.to_ascii_lowercase());
    }
    let Ok(mut parsed) = Url::parse(uri.trim()) else {
        return "Source available (details redacted)".into();
    };
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    parsed.set_query(None);
    parsed.set_fragment(None);
    parsed.to_string()
}

#[cfg(test)]
mod tests {
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

    use super::*;

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
        async fn add_download(
            &self,
            _request: &AddDownloadRequest,
        ) -> Result<Vec<Gid>, GatewayError> {
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
        async fn add_download(
            &self,
            _request: &AddDownloadRequest,
        ) -> Result<Vec<Gid>, GatewayError> {
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
                .ok_or_else(|| {
                    SyncError::new(SyncErrorKind::Internal, "connector reused", false)
                })?;
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
                .ok_or_else(|| {
                    SyncError::new(SyncErrorKind::Internal, "connector reused", false)
                })?;
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
        async fn add_download(
            &self,
            _request: &AddDownloadRequest,
        ) -> Result<Vec<Gid>, GatewayError> {
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
            let active = (!self.terminal && !self.removed.load(Ordering::Acquire))
                .then(|| self.original_task());
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
                .ok_or_else(|| {
                    SyncError::new(SyncErrorKind::Internal, "connector reused", false)
                })?;
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

        let mapped = map_task("profile", task, None);

        assert_eq!(mapped.identity.profile_id, "profile");
        assert_eq!(mapped.identity.gid, "0000000000000009");
        assert_eq!(mapped.status, TaskStatusView::Active);
        assert_eq!(mapped.eta_seconds, Some(6));
    }

    #[test]
    fn domain_seeding_mapping_preserves_upload_metrics_and_observed_time() {
        let mut snapshot =
            TaskSnapshot::new(Gid::from_u64(10), DownloadStatus::Seeding, "seed.bin");
        snapshot.total_length = ByteCount::new(100);
        snapshot.completed_length = ByteCount::new(100);
        snapshot.upload_length = ByteCount::new(250);
        snapshot.upload_speed = ByteRate::new(64);

        let mapped = map_task("profile", DownloadTask::from_snapshot(snapshot), Some(65));

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
        let mut original =
            TaskSnapshot::new(unknown_original.gid, DownloadStatus::Error, "failed.bin");
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
                uri: "https://tracker.example/announce".into(),
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
            uri: "https://example.test/archive.bin".into(),
            status: TaskUriStatus::Used,
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
        assert_eq!(mapped.uris[0].status, TaskUriStatusView::Used);
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

        let mapped = map_task("profile", DownloadTask::from_snapshot(snapshot), None);

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
        let mut matching =
            TaskSnapshot::new(Gid::from_u64(7), DownloadStatus::Waiting, "archive.iso");
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
            find_new_matching_add_task(&tasks, &[source], &HashSet::from([Gid::from_u64(7)]))
                .is_none(),
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
        let encoded = BASE32_NOPAD.encode(&bytes);
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
            uri: "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&tr=https%3A%2F%2Fone.test".into(),
        };
        let second = AddDownloadSourceView::Uri {
            line: 2,
            uri: "magnet:?tr=https%3A%2F%2Ftwo.test&xt=URN:BTIH:0123456789ABCDEF0123456789ABCDEF01234567".into(),
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
            sanitize_source_uri(
                "https://user:secret@example.test/archive.iso?token=private#fragment"
            ),
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

    async fn run_unknown_retry(
        observe_replacement: bool,
    ) -> (TaskCommandResultView, usize, Vec<Gid>) {
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
        let settings = SettingsView {
            color_scheme: ColorSchemeView::Light,
            download_directory: "D:/Transfers".into(),
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
            RpcRuntimeConfig::from_values(true, |name| {
                reversed.get(name).map(ToString::to_string)
            })
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
            download_directory: "D:/Downloads".into(),
            download_proxy: DownloadProxySettingsView {
                mode: ProxyModeView::Manual,
                all_proxy: "proxy.example:8080".into(),
                username: "proxy-user".into(),
                has_password: true,
                ..DownloadProxySettingsView::default()
            },
            speed_limits: SpeedLimitSettingsView::default(),
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

        let (cleared, update) =
            map_settings_request(&settings, &current, ProxyPasswordUpdateView::Clear)
                .expect("explicit credential clear is valid");
        assert!(cleared.download_proxy.credential.is_none());
        assert!(matches!(update, ProxyPasswordUpdate::Clear));
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
    fn settings_worker_persists_requests_in_order_and_drains_on_close() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("test runtime"),
        );
        let (sender, task, mut results) = spawn_settings_persistence(
            runtime.clone(),
            store.clone(),
            Some(Arc::new(LocalDownloadDestinationGateway::new())),
            None,
            Arc::new(SystemProxyCredentialStore::new("AriaDeck test")),
        );
        let first = AppSettings {
            color_scheme: ColorScheme::Dark,
            download_directory: root.path().join("first"),
            download_proxy: DownloadProxySettings::default(),
            speed_limits: SpeedLimitSettings::default(),
        };
        let second = AppSettings {
            color_scheme: ColorScheme::Light,
            download_directory: root.path().join("second"),
            download_proxy: DownloadProxySettings::default(),
            speed_limits: SpeedLimitSettings::default(),
        };
        sender
            .send(SettingsPersistenceRequest {
                request_id: ariadeck_ui::RequestId::from_u64(1),
                settings: first.clone(),
                previous_settings: first.clone(),
                proxy_password: ProxyPasswordUpdate::Unchanged,
                apply_proxy: false,
                apply_speed_limit: false,
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
            download_directory: remote_path.clone(),
            download_proxy: DownloadProxySettings::default(),
            speed_limits: SpeedLimitSettings::default(),
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
}
