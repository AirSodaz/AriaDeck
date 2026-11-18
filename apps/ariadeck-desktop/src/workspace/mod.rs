use std::{
    env,
    path::PathBuf,
    sync::{Arc, mpsc::Receiver},
    time::{Duration, Instant},
};

use ariadeck_application::{
    ActivityMode, DownloadDestinationGateway, SyncHandle, TaskFileGateway, TaskListQuery,
};
use ariadeck_domain::ProfileId;
use ariadeck_engine::{
    CoreStore, ExternalEngineProfile, JsonProfileStore, LocalDownloadDestinationGateway,
    LocalDownloadRootRegistry, LocalEngineHealthHandle, LocalEngineSupervisor,
    LocalTaskFileGateway, ProfileCatalog, ProfileKind,
};
use ariadeck_ipc::LaunchRequest;
use ariadeck_settings::{
    AppSettings, JsonSettingsStore, JsonWindowGeometryStore, ListFilterPreference,
    ListSortDirectionPreference, ListSortKeyPreference, ProfileEnvironmentStore,
    ProxyCredentialStore, SystemProxyCredentialStore, UiPreferences, WindowGeometry,
};
use ariadeck_ui::{
    AddDownloadMetadataPreviewRequestView, AddDownloadRequestView, AppShell, AppShellEvent,
    BatchCommandOutcomeView, BatchTaskCommandRequestView, BatchTaskCommandResultView,
    CommandOutcomeView, CompletionPowerActionView, ConnectionView, CoreCommandOutcomeView,
    CoreCommandRequestView, CoreCommandResultView, CoreDiscoveryResultView, CoreDownloadResultView,
    CoreRegistryView, DiagnosticExportOutcomeView, DiagnosticExportRequestView,
    DiagnosticExportResultView, EngineHealthView, GlobalTaskCommandRequestView, OperationErrorView,
    PlatformCapabilitiesView, SaveProfileCatalogOutcomeView, SaveProfileCatalogRequestView,
    SaveProfileCatalogResultView, SettingsExportOutcomeView, SettingsExportRequestView,
    SettingsExportResultView, SettingsImportOutcomeView, SettingsImportRequestView,
    SettingsImportResultView, SettingsSaveOutcomeView, SettingsSaveRequestView,
    SettingsSaveResultView, SwitchProfileOutcomeView, SwitchProfileRequestView,
    SwitchProfileResultView, TaskCommandRequestView, TaskCommandResultView, TaskDetailsRequestView,
    TaskOpenRequestView, TrackerListSourceView, WorkspaceFilter, WorkspaceSnapshot,
    WorkspaceSortDirection, WorkspaceSortKey,
};
use gpui::{AppContext as _, Context, Entity, IntoElement, Render, Subscription, Window};
use secrecy::SecretString;
use tokio::{
    runtime::Runtime,
    sync::{mpsc, watch},
    task::JoinHandle,
};

use crate::platform::{self, TrayAction};

mod core_fetch;
mod diagnostics;
mod engine_setup;
mod http;
mod launch_queue;
mod mapping;
mod ops;
mod persistence;
mod platform_controller;
mod settings_bridge;
#[cfg(test)]
mod test_support;
mod tracker_list;
mod updates;

use core_fetch::{CoreDownloadRequest, CoreDownloadResult, spawn_core_download_bridge};
use engine_setup::{
    EngineBootstrapOutput, EngineSessionRecovery, create_sync_handle, default_download_dir,
    discover_aria2_executable, local_engine_executable_missing, request_local_engine_shutdown,
};
use launch_queue::LaunchQueue;
use mapping::{
    map_core_download_offer, map_core_registry, map_discovered_cores, map_local_engine_health,
    map_profile_catalog, map_profile_catalog_request, map_query, map_settings,
    map_settings_request, map_ui_preferences_to_query, trimmed_value,
};
use ops::{
    execute_add_download, execute_batch_task_command, execute_core_command_on_store,
    execute_global_task_command, execute_task_command, execute_task_details, execute_task_open,
    metadata_preview_worker_failure, preview_metadata_files,
};
use persistence::{
    PersistenceCommand, PersistenceWorkerDeps, ProfileCatalogSaveJob, ProfileSwitchRequest,
    spawn_persistence_worker,
};
use platform_controller::{PlatformController, show_native_window};
use settings_bridge::{
    spawn_core_download_result_bridge, spawn_local_engine_health_bridge,
    spawn_persistence_event_bridge, spawn_proxy_reapply_bridge, spawn_snapshot_bridge,
    spawn_tracker_list_result_bridge,
};
use tracker_list::{
    TrackerListRefreshRequest, TrackerListRefreshResult, spawn_tracker_list_auto_refresh,
    spawn_tracker_list_refresh_bridge,
};
use updates::{UpdateController, is_read_only_update_install, updates_supported};

/// Process-wide flag: true while AriaDeck is intentionally hidden to tray with
/// no open window. Prevents `on_window_closed` from quitting the app.
pub(crate) static TRAY_SESSION_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn host_platform_capabilities() -> PlatformCapabilitiesView {
    PlatformCapabilitiesView {
        keep_awake_supported: cfg!(target_os = "windows"),
        browser_bridge_supported: cfg!(target_os = "windows"),
    }
}

fn translator_for_app_language(
    language: ariadeck_settings::LanguagePreference,
) -> ariadeck_i18n::Translator {
    let locale = match language {
        ariadeck_settings::LanguagePreference::System => ariadeck_i18n::LocaleId::from_system_env(),
        ariadeck_settings::LanguagePreference::En => ariadeck_i18n::LocaleId::En,
        ariadeck_settings::LanguagePreference::ZhCn => ariadeck_i18n::LocaleId::ZhCn,
    };
    ariadeck_i18n::Translator::new(locale)
}

enum StartupNoticeKind {
    SettingsReset(PathBuf),
    SettingsLoadFailed(String),
    ProfilesReset(PathBuf),
    ProfilesRecovered,
    ProfilesLoadFailed(String),
    ProfileEnvFailed(String),
    SessionRecovered(EngineSessionRecovery),
}

impl StartupNoticeKind {
    fn format(&self, translator: &ariadeck_i18n::Translator) -> String {
        use ariadeck_i18n::{FluentArgs, FluentValue};
        let mut args = FluentArgs::new();
        match self {
            Self::SettingsReset(path) => {
                args.set("path", FluentValue::from(path.display().to_string()));
                translator.t_args("notice-startup-settings-reset", Some(&args))
            }
            Self::SettingsLoadFailed(summary) => {
                args.set("summary", FluentValue::from(summary.as_str()));
                translator.t_args("notice-startup-settings-load-failed", Some(&args))
            }
            Self::ProfilesReset(path) => {
                args.set("path", FluentValue::from(path.display().to_string()));
                translator.t_args("notice-startup-profiles-reset", Some(&args))
            }
            Self::ProfilesRecovered => translator.t("notice-startup-profiles-recovered"),
            Self::ProfilesLoadFailed(summary) => {
                args.set("summary", FluentValue::from(summary.as_str()));
                translator.t_args("notice-startup-profiles-load-failed", Some(&args))
            }
            Self::ProfileEnvFailed(summary) => {
                args.set("summary", FluentValue::from(summary.as_str()));
                translator.t_args("notice-startup-profile-env-failed", Some(&args))
            }
            Self::SessionRecovered(recovery) => match &recovery.backup_path {
                Some(path) => {
                    args.set("path", FluentValue::from(path.display().to_string()));
                    translator.t_args("notice-startup-session-recovered", Some(&args))
                }
                None => translator.t("notice-startup-session-recovered-no-backup"),
            },
        }
    }
}

pub struct DesktopRoot {
    workspace: Entity<AppShell>,
    sync: Option<SyncHandle>,
    download_destination_gateway: Option<Arc<dyn DownloadDestinationGateway>>,
    task_file_gateway: Option<Arc<dyn TaskFileGateway>>,
    local_engine: Option<LocalEngineSupervisor>,
    _child_process_job: Option<Arc<platform::ChildProcessJob>>,
    runtime: Arc<Runtime>,
    shutdown_slot: Arc<std::sync::Mutex<Option<JoinHandle<()>>>>,
    query_sender: watch::Sender<TaskListQuery>,
    persistence_sender: Option<mpsc::UnboundedSender<PersistenceCommand>>,
    persistence_task: Option<JoinHandle<()>>,
    settings_persistence_available: bool,
    tracker_list_sender: Option<mpsc::UnboundedSender<TrackerListRefreshRequest>>,
    /// Verified aria2 download worker (B4). Only ever fed by a button press.
    core_download_sender: Option<mpsc::UnboundedSender<CoreDownloadRequest>>,
    settings: AppSettings,
    data_dir: PathBuf,
    profile_catalog: ProfileCatalog,
    core_store: CoreStore,
    updates: UpdateController,
    platform: PlatformController,
    launch_queue: LaunchQueue,
    _workspace_subscription: Subscription,
}

#[derive(Clone)]
pub(crate) struct SettingsPersistenceRequest {
    pub(crate) request_id: ariadeck_ui::RequestId,
    pub(crate) settings: AppSettings,
    pub(crate) previous_settings: AppSettings,
    pub(crate) proxy_password: ProxyPasswordUpdate,
    pub(crate) apply_proxy: bool,
    pub(crate) apply_speed_limit: bool,
    pub(crate) apply_transfer_policy: bool,
    pub(crate) apply_bt_tracker: bool,
    /// Active profile bag to update alongside `settings.json` (engine-env fields).
    pub(crate) active_profile_id: Option<ProfileId>,
}

#[derive(Clone)]
pub(crate) enum ProxyPasswordUpdate {
    Unchanged,
    Detach,
    Clear,
    Set(SecretString),
}

pub(crate) struct SettingsPersistenceResult {
    pub(crate) request_id: ariadeck_ui::RequestId,
    pub(crate) settings: AppSettings,
    pub(crate) result: Result<(), String>,
}

impl DesktopRoot {
    #[must_use]
    pub fn tray_session_active() -> bool {
        TRAY_SESSION_ACTIVE.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Application data directory used for settings, profiles, and window geometry.
    #[must_use]
    pub fn default_data_dir() -> PathBuf {
        ariadeck_ipc::default_data_dir()
    }

    #[must_use]
    pub fn new(
        runtime: Arc<Runtime>,
        shutdown_slot: Arc<std::sync::Mutex<Option<JoinHandle<()>>>>,
        initial_request: LaunchRequest,
        instance_requests: Option<Receiver<LaunchRequest>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let startup_started = Instant::now();
        let data_dir = ariadeck_ipc::default_data_dir();
        let defaults = AppSettings::new(default_download_dir());
        let settings_store = JsonSettingsStore::new(data_dir.join("settings.json"));
        let (mut settings, settings_store, startup_notice_kind) =
            match settings_store.load_or_initialize(&defaults) {
                Ok(loaded) => {
                    let notice = loaded.recovery.map(|recovery| {
                        tracing::warn!(
                            reason = %recovery.reason,
                            backup = %recovery.backup_path.display(),
                            "recovered invalid application settings"
                        );
                        StartupNoticeKind::SettingsReset(recovery.backup_path)
                    });
                    (loaded.settings, Some(settings_store), notice)
                }
                Err(error) => {
                    tracing::error!(%error, "failed to load application settings");
                    (
                        defaults,
                        None,
                        Some(StartupNoticeKind::SettingsLoadFailed(error.to_string())),
                    )
                }
            };
        let translator = translator_for_app_language(settings.language);
        let startup_notice = startup_notice_kind.map(|kind| kind.format(&translator));
        tracing::info!(
            elapsed_ms = startup_started.elapsed().as_millis(),
            "settings loaded"
        );

        // D-043: catalog first, then active profile env bag, then optional
        // ARIADECK_DOWNLOAD_DIR override (dev/smoke; not persisted into the bag
        // until the user saves settings).
        let profile_store = JsonProfileStore::new(data_dir.join("profiles.json"));
        let core_store = CoreStore::new(&data_dir);
        let managed_executable = core_store.resolve_active_executable().ok().flatten();
        let executable = env::var_os("ARIADECK_ARIA2C_PATH")
            .map(PathBuf::from)
            .or(managed_executable.clone())
            .or_else(discover_aria2_executable)
            .unwrap_or_else(|| PathBuf::from("aria2c"));
        let default_profile = ExternalEngineProfile::new(
            ProfileId::new(),
            env::var("ARIADECK_PROFILE_NAME").unwrap_or_else(|_| "Local aria2".into()),
            executable,
            data_dir.clone(),
            settings.download_directory.clone(),
        );
        let (mut profile_catalog, mut profile_notice) =
            match profile_store.load_catalog_or_recover(&default_profile) {
                Ok(loaded) => {
                    let mut notice = loaded.recovery.map(|recovery| {
                        StartupNoticeKind::ProfilesReset(recovery.backup_path).format(&translator)
                    });
                    if loaded.migrated && notice.is_none() {
                        // Malformed current-schema recovery only (no historical migrators).
                        notice = Some(StartupNoticeKind::ProfilesRecovered.format(&translator));
                    }
                    (loaded.catalog, notice)
                }
                Err(error) => {
                    tracing::error!(%error, "failed to load profile catalog");
                    (
                        ProfileCatalog::from_single(default_profile),
                        Some(
                            StartupNoticeKind::ProfilesLoadFailed(error.to_string())
                                .format(&translator),
                        ),
                    )
                }
            };
        tracing::info!(
            elapsed_ms = startup_started.elapsed().as_millis(),
            "profile catalog loaded"
        );

        let profile_env_store = ProfileEnvironmentStore::new(&data_dir);
        let mut profile_env_notice = None;
        if let Some(active) = profile_catalog.active() {
            match profile_env_store.load_or_initialize(active.profile_id.as_uuid(), &settings) {
                Ok(env) => {
                    env.apply_to(&mut settings);
                }
                Err(error) => {
                    tracing::error!(%error, "failed to load profile environment");
                    profile_env_notice = Some(
                        StartupNoticeKind::ProfileEnvFailed(error.to_string()).format(&translator),
                    );
                }
            }
        }

        // Dev/smoke override after bag load so it wins for this process only.
        if let Some(download_directory) = env::var_os("ARIADECK_DOWNLOAD_DIR") {
            settings.download_directory = PathBuf::from(download_directory);
            if let Some(fallback) = settings.categories.iter_mut().find(|c| c.is_fallback) {
                fallback.directory = settings.download_directory.clone();
            } else if settings.categories.is_empty() {
                settings.categories =
                    ariadeck_settings::default_download_categories(&settings.download_directory);
            }
        }

        // Align active catalog entry once (local data_dir + fallback download_dir)
        // and persist a single atomic write instead of two consecutive writes.
        if let Some(entry) = profile_catalog.active_mut() {
            let mut changed = false;
            if entry.download_dir != settings.download_directory {
                entry.download_dir = settings.download_directory.clone();
                changed = true;
            }
            if entry.kind == ProfileKind::LocalManaged && entry.data_dir.is_none() {
                entry.data_dir = Some(data_dir.clone());
                changed = true;
            }
            if changed && let Err(error) = profile_store.save_catalog(&profile_catalog) {
                tracing::warn!(%error, "failed to persist aligned profile catalog on startup");
            }
        }

        let child_process_job = match platform::ChildProcessJob::try_new() {
            Ok(job) => Some(Arc::new(job)),
            Err(error) => {
                tracing::warn!(%error, "could not initialize Windows child-process job object");
                None
            }
        };
        let child_spawn_hook: Option<ariadeck_engine::ChildSpawnHook> =
            child_process_job.as_ref().map(|job| {
                let job = Arc::clone(job);
                Arc::new(move |pid| job.assign_pid(pid)) as ariadeck_engine::ChildSpawnHook
            });

        let initial_snapshot = WorkspaceSnapshot {
            connection: ConnectionView::Connecting,
            ..WorkspaceSnapshot::default()
        };
        let credential_store =
            Arc::new(SystemProxyCredentialStore::default()) as Arc<dyn ProxyCredentialStore>;
        let proxy_reapply_store = settings_store.clone();
        let settings_persistence_available = settings_store.is_some();
        let window_geometry_store = JsonWindowGeometryStore::new(data_dir.join("window.json"));
        let last_saved_geometry = window_geometry_store.load();
        let (persistence_sender, persistence_task, persistence_events) = spawn_persistence_worker(
            runtime.clone(),
            PersistenceWorkerDeps {
                settings_store,
                profile_store,
                profile_env_store,
                window_geometry_store,
                destination_gateway: None,
                sync: None,
                credential_store: credential_store.clone(),
                initial_active_profile_id: profile_catalog.active().map(|p| p.profile_id),
            },
        );
        let settings_view = map_settings(&settings);
        let initial_query = map_ui_preferences_to_query(&settings.ui);
        let platform_capabilities = host_platform_capabilities();
        let workspace = cx.new(|cx| {
            let mut shell = AppShell::new_with_settings(settings_view, window, cx);
            shell.set_platform_capabilities(platform_capabilities, cx);
            shell.set_snapshot(initial_snapshot, cx);
            shell.set_engine_health(EngineHealthView::External, cx);
            shell.set_profiles(map_profile_catalog(&profile_catalog), cx);
            shell.set_cores(map_core_registry(&core_store), cx);
            // B4: what this platform may fetch is fixed at build time, so the
            // panel can describe the offer before anything is scanned.
            shell.set_core_download_offer(map_core_download_offer(), cx);
            shell.restore_list_preferences(initial_query.clone(), cx);
            if let Some(message) = startup_notice {
                shell.set_startup_notice(message, true, cx);
            } else if let Some(message) = profile_notice.take() {
                shell.set_startup_notice(message, true, cx);
            } else if let Some(message) = profile_env_notice.take() {
                shell.set_startup_notice(message, true, cx);
            }
            shell
        });
        let (query_sender, query_receiver) = watch::channel(map_query(&initial_query));
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
                AppShellEvent::SettingsExportRequested(request) => {
                    this.spawn_settings_export(request.clone(), window, cx);
                }
                AppShellEvent::SettingsImportRequested(request) => {
                    this.spawn_settings_import(request.clone(), window, cx);
                }
                AppShellEvent::TrackerListRefreshRequested { request_id } => {
                    this.enqueue_tracker_list_refresh(*request_id, window, cx);
                }
                AppShellEvent::DiagnosticExportRequested(request) => {
                    this.spawn_diagnostic_export(request.clone(), window, cx);
                }
                AppShellEvent::SwitchProfileRequested(request) => {
                    this.handle_switch_profile(request.clone(), window, cx);
                }
                AppShellEvent::SaveProfileCatalogRequested(request) => {
                    this.handle_save_profile_catalog(request.clone(), window, cx);
                }
                AppShellEvent::CoreCommandRequested(request) => {
                    this.handle_core_command(request.clone(), window, cx);
                }
                AppShellEvent::CoreDiscoveryRequested { request_id } => {
                    this.spawn_core_discovery(*request_id, window, cx);
                }
                AppShellEvent::CoreDownloadRequested { request_id } => {
                    this.enqueue_core_download(*request_id, cx);
                }
                AppShellEvent::CheckForUpdatesRequested { request_id } => {
                    this.spawn_update_check(*request_id, window, cx);
                }
                AppShellEvent::DownloadUpdateRequested { request_id } => {
                    this.spawn_update_download(*request_id, window, cx);
                }
                AppShellEvent::InstallUpdateRequested => {
                    this.install_verified_update(cx);
                }
                AppShellEvent::CoreSetupOnboardingDismissed => {
                    this.persist_core_setup_dismissal(cx);
                }
                AppShellEvent::HideToTrayRequested => {
                    this.hide_to_tray(window, cx);
                }
                AppShellEvent::ShowFromTrayRequested => {
                    this.show_from_tray(window, cx);
                }
                AppShellEvent::QuitRequested => {
                    this.quit_application(cx);
                }
                AppShellEvent::OsNotificationRequested { title, body, .. } => {
                    let title = title.clone();
                    let body = body.clone();
                    this.runtime.handle().spawn_blocking(move || {
                        platform::show_os_notification(&title, &body);
                    });
                }
                AppShellEvent::UiPreferencesChanged {
                    filter,
                    sort_key,
                    sort_direction,
                } => {
                    this.persist_ui_preferences(*filter, *sort_key, *sort_direction, cx);
                }
                AppShellEvent::WindowGeometryChanged {
                    x,
                    y,
                    width,
                    height,
                    maximized,
                } => {
                    this.schedule_geometry_save(
                        WindowGeometry {
                            x: *x,
                            y: *y,
                            width: *width,
                            height: *height,
                            maximized: *maximized,
                        },
                        cx,
                    );
                }
                AppShellEvent::CompletionPowerActionRequested(action) => {
                    let action = *action;
                    let app_action = mapping::map_view_completion_power_action_to_app(action);
                    let runtime = this.runtime.handle().clone();
                    tracing::info!(?app_action, "executing completion power action");
                    cx.spawn_in(window, async move |this, cx| {
                        let result = runtime
                            .spawn_blocking(move || platform::execute_power_action(app_action))
                            .await
                            .map_err(|error| format!("power action worker failed: {error}"))
                            .and_then(|res| res);
                        match result {
                            Ok(()) => {
                                if action == CompletionPowerActionView::Shutdown {
                                    this.update(cx, |this, cx| {
                                        this.quit_application(cx);
                                    })
                                    .ok();
                                }
                            }
                            Err(error) => {
                                tracing::error!(%error, "failed to execute completion power action");
                                this.update(cx, |this, cx| {
                                    this.workspace.update(cx, |shell, cx| {
                                        shell.report_power_action_failed(error, cx);
                                    });
                                })
                                .ok();
                            }
                        }
                    })
                    .detach();
                }
            },
        );
        // Intercept the native close control so Minimize-to-tray can keep the
        // process (and managed engine) alive without destroying the window.
        {
            let workspace = workspace.clone();
            window.on_window_should_close(cx, move |_window, cx| {
                workspace.update(cx, |shell, cx| shell.handle_window_close_request(cx))
            });
        }

        spawn_persistence_event_bridge(persistence_events, window, cx);
        let tracker_list_sender = if let Some(store) = proxy_reapply_store.clone() {
            let (req_tx, req_rx) = mpsc::unbounded_channel::<TrackerListRefreshRequest>();
            let (res_tx, res_rx) = mpsc::unbounded_channel::<TrackerListRefreshResult>();
            spawn_tracker_list_refresh_bridge(
                runtime.handle().clone(),
                store,
                persistence_sender.clone(),
                req_rx,
                res_tx,
            );
            spawn_tracker_list_result_bridge(res_rx, window, cx);
            spawn_tracker_list_auto_refresh(runtime.handle().clone(), req_tx.clone());
            Some(req_tx)
        } else {
            None
        };
        let core_download_sender = {
            let (req_tx, req_rx) = mpsc::unbounded_channel::<CoreDownloadRequest>();
            let (res_tx, res_rx) = mpsc::unbounded_channel::<CoreDownloadResult>();
            spawn_core_download_bridge(
                runtime.handle().clone(),
                core_store.clone(),
                req_rx,
                res_tx,
            );
            spawn_core_download_result_bridge(res_rx, window, cx);
            Some(req_tx)
        };

        // Run heavy startup I/O (remote OS keychain lookup, local aria2c spawn,
        // and SQLite history open) off the GPUI main thread so the first frame
        // paints immediately in `Connecting` state.
        {
            let bootstrap_runtime = runtime.clone();
            let bootstrap_data_dir = data_dir.clone();
            let bootstrap_settings = settings.clone();
            let bootstrap_catalog = profile_catalog.clone();
            let bootstrap_store = proxy_reapply_store;
            let bootstrap_credentials = credential_store;
            let bootstrap_started = startup_started;
            cx.spawn_in(window, async move |this, cx| {
                let bootstrap_result = bootstrap_runtime
                    .spawn_blocking({
                        let runtime_for_sync = bootstrap_runtime.clone();
                        move || {
                            create_sync_handle(
                                &runtime_for_sync,
                                &bootstrap_data_dir,
                                &bootstrap_settings,
                                &bootstrap_catalog,
                                child_spawn_hook,
                            )
                        }
                    })
                    .await
                    .map_err(|error| format!("Engine startup worker failed: {error}"))
                    .and_then(|result| result);

                this.update_in(cx, |this, _window, cx| {
                    this.attach_engine_bootstrap(
                        bootstrap_result,
                        bootstrap_store,
                        bootstrap_credentials,
                        query_receiver,
                        bootstrap_started,
                        cx,
                    );
                })
                .ok();
            })
            .detach();
        }

        let platform = PlatformController::new(&settings, last_saved_geometry);
        let launch_queue = LaunchQueue::new(initial_request, instance_requests);

        let root = Self {
            workspace,
            sync: None,
            download_destination_gateway: None,
            task_file_gateway: None,
            local_engine: None,
            _child_process_job: child_process_job,
            runtime,
            shutdown_slot,
            query_sender,
            persistence_sender: Some(persistence_sender),
            persistence_task: Some(persistence_task),
            settings_persistence_available,
            tracker_list_sender,
            core_download_sender,
            settings,
            data_dir: data_dir.clone(),
            profile_catalog,
            core_store,
            updates: UpdateController::default(),
            platform,
            launch_queue,
            _workspace_subscription: workspace_subscription,
        };
        tracing::info!(
            elapsed_ms = startup_started.elapsed().as_millis(),
            "desktop window state constructed"
        );

        let updates_enabled = updates_supported();
        let read_only_install = is_read_only_update_install();
        root.workspace.update(cx, |shell, cx| {
            shell.configure_updates(updates_enabled, read_only_install, cx);
            if updates_enabled {
                shell.request_update_check(cx);
            }
        });

        // Poll tray interactions and single-instance requests on a short UI timer.
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(400))
                    .await;
                let cont = this
                    .update_in(cx, |this, window, cx| {
                        this.poll_platform_surface(window, cx);
                        true
                    })
                    .unwrap_or(false);
                if !cont {
                    break;
                }
            }
        })
        .detach();

        // Probe available disk space on a background blocking thread every 15s
        // so slow SMB/sleeping drives never stall the GPUI render loop.
        {
            let runtime = root.runtime.handle().clone();
            cx.spawn_in(window, async move |this, cx| {
                loop {
                    let dir = match this
                        .read_with(cx, |this, _| this.settings.download_directory.clone())
                    {
                        Ok(dir) => dir,
                        Err(_) => break,
                    };
                    let free = runtime
                        .spawn_blocking(move || platform::available_disk_space(&dir))
                        .await
                        .ok()
                        .flatten();
                    let cont = this
                        .update_in(cx, |this, _window, cx| {
                            this.workspace.update(cx, |shell, cx| {
                                shell.report_disk_space(free, cx);
                            });
                            true
                        })
                        .unwrap_or(false);
                    if !cont {
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_secs(15))
                        .await;
                }
            })
            .detach();
        }

        // B4 first run: offer core setup only when this install has no aria2 it
        // could start, the active profile actually wants a local one, and the
        // user has not already been asked. The probe executes `--version`, so it
        // runs off the first frame rather than blocking the window.
        let wants_local_engine = root
            .profile_catalog
            .active()
            .is_none_or(|profile| profile.kind == ProfileKind::LocalManaged);
        if !root.settings.onboarding.core_setup_dismissed && wants_local_engine {
            let runtime = root.runtime.handle().clone();
            let probe_data_dir = data_dir.clone();
            let profile_executable = root
                .profile_catalog
                .active()
                .and_then(|profile| profile.executable.clone())
                .unwrap_or_default();
            cx.spawn_in(window, async move |this, cx| {
                let missing = runtime
                    .spawn_blocking(move || {
                        local_engine_executable_missing(&probe_data_dir, &profile_executable)
                    })
                    .await
                    .unwrap_or(false);
                if !missing {
                    return;
                }
                tracing::info!("no usable local aria2 found; opening the core setup guide");
                this.update_in(cx, |this, window, cx| {
                    // Same reasoning as D-037/D-038: something that stops to ask
                    // the user overrides start-minimized-to-tray.
                    if this.platform.is_hidden_to_tray() {
                        this.show_from_tray(window, cx);
                    }
                    this.workspace.update(cx, |shell, cx| {
                        shell.open_core_setup_onboarding(window, cx);
                    });
                })
                .ok();
            })
            .detach();
        }

        // Launch items that will ask the user something override start-minimized;
        // ones that will auto-submit do not (D-045 §6).
        let launch_needs_the_window = root
            .launch_queue
            .needs_window_for_confirmation(root.workspace.read(cx).bridge_requires_confirmation());
        if !launch_needs_the_window
            && root.settings.platform.start_minimized_to_tray
            && root.platform.has_tray()
        {
            // Defer hide so the first frame can finish constructing the window.
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                this.update_in(cx, |this, window, cx| {
                    this.hide_to_tray(window, cx);
                })
                .ok();
            })
            .detach();
        } else {
            show_native_window(window);
        }

        root
    }

    fn attach_engine_bootstrap(
        &mut self,
        result: Result<EngineBootstrapOutput, String>,
        proxy_reapply_store: Option<JsonSettingsStore>,
        credential_store: Arc<dyn ProxyCredentialStore>,
        query_receiver: watch::Receiver<TaskListQuery>,
        startup_started: Instant,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok((handle, local_engine, session_recovery)) => {
                tracing::info!(
                    elapsed_ms = startup_started.elapsed().as_millis(),
                    "engine and sync configured asynchronously"
                );
                let local_engine_health = local_engine
                    .as_ref()
                    .map(LocalEngineSupervisor::health_handle);
                let local_download_roots = local_engine.as_ref().map(|_| {
                    LocalDownloadRootRegistry::new(self.settings.download_directory.clone())
                });
                let download_destination_gateway = local_download_roots.as_ref().map(|roots| {
                    Arc::new(LocalDownloadDestinationGateway::with_roots(roots.clone()))
                        as Arc<dyn DownloadDestinationGateway>
                });
                let task_file_gateway = local_download_roots.map(|roots| {
                    Arc::new(LocalTaskFileGateway::with_roots(roots)) as Arc<dyn TaskFileGateway>
                });
                let initial_engine_health = local_engine_health
                    .as_ref()
                    .and_then(LocalEngineHealthHandle::health)
                    .map(map_local_engine_health)
                    .unwrap_or(EngineHealthView::External);

                if let Some(sender) = self.persistence_sender.as_ref() {
                    let _ = sender.send(PersistenceCommand::AttachEngine {
                        sync: handle.clone(),
                        destination_gateway: download_destination_gateway.clone(),
                    });
                }
                spawn_snapshot_bridge(
                    handle.clone(),
                    query_receiver,
                    task_file_gateway.is_some(),
                    cx,
                );
                if let Some(store) = proxy_reapply_store {
                    spawn_proxy_reapply_bridge(
                        self.runtime.handle().clone(),
                        handle.clone(),
                        store,
                        credential_store,
                        cx,
                    );
                }
                if let Some(health) = local_engine_health {
                    spawn_local_engine_health_bridge(health, cx);
                }

                self.sync = Some(handle);
                self.download_destination_gateway = download_destination_gateway;
                self.task_file_gateway = task_file_gateway;
                self.local_engine = local_engine;

                let translator = translator_for_app_language(self.settings.language);
                let recovery_notice = session_recovery.map(|recovery| {
                    StartupNoticeKind::SessionRecovered(recovery).format(&translator)
                });
                self.workspace.update(cx, |shell, cx| {
                    shell.set_engine_health(initial_engine_health, cx);
                    if let Some(message) = recovery_notice {
                        shell.set_startup_notice(message, true, cx);
                    }
                });
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
                self.workspace.update(cx, |shell, cx| {
                    shell.set_snapshot(snapshot, cx);
                });
            }
        }
    }

    fn poll_platform_surface(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.poll_instance_requests(window, cx);
        for action in self.platform.poll_tray_actions() {
            match action {
                TrayAction::Show => self.show_from_tray(window, cx),
                TrayAction::PauseAll => {
                    self.workspace.update(cx, |shell, cx| {
                        shell.request_pause_all_from_tray(cx);
                    });
                }
                TrayAction::ResumeAll => {
                    self.workspace.update(cx, |shell, cx| {
                        shell.request_resume_all_from_tray(cx);
                    });
                }
                TrayAction::Quit => self.quit_application(cx),
            }
        }
        if self.platform.has_tray() {
            let tooltip = self.workspace.read(cx).tray_tooltip();
            self.platform.sync_tray_tooltip(tooltip);
        }

        // Keep-awake management based on active downloads and user preference (D5).
        let has_active_downloads = self.workspace.read(cx).has_active_downloads();
        self.platform.sync_keep_awake(
            self.settings.platform.keep_awake_while_active,
            has_active_downloads,
        );
    }

    fn poll_instance_requests(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let requires_confirmation = self.workspace.read(cx).bridge_requires_confirmation();
        if self.launch_queue.drain_incoming(requires_confirmation) {
            self.show_from_tray(window, cx);
        }
        self.launch_queue
            .dispatch_ready(&self.workspace, window, cx);
    }

    fn hide_to_tray(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.platform.has_tray() {
            // No tray: fall back to quit so the user is not stuck without UI.
            self.quit_application(cx);
            return;
        }
        if self.platform.hide_to_tray(window) {
            // PERF-001: slow live/global/stopped polls while the window is hidden.
            self.set_sync_activity(ActivityMode::Background, cx);
        }
    }

    fn show_from_tray(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.platform.show_from_tray(window, cx) {
            self.set_sync_activity(ActivityMode::Foreground, cx);
        }
    }

    fn set_sync_activity(&self, mode: ActivityMode, cx: &mut Context<Self>) {
        let Some(handle) = self.sync.clone() else {
            return;
        };
        self.runtime.spawn(async move {
            handle.set_activity(mode).await;
        });
        let _ = cx;
    }

    pub(super) fn quit_application(&mut self, cx: &mut Context<Self>) {
        self.platform.cleanup_on_quit();
        cx.quit();
    }

    fn persist_ui_preferences(
        &mut self,
        filter: WorkspaceFilter,
        sort_key: WorkspaceSortKey,
        sort_direction: WorkspaceSortDirection,
        cx: &mut Context<Self>,
    ) {
        let ui = UiPreferences {
            list_filter: match filter {
                WorkspaceFilter::All => ListFilterPreference::All,
                WorkspaceFilter::Active => ListFilterPreference::Active,
                WorkspaceFilter::Waiting => ListFilterPreference::Waiting,
                WorkspaceFilter::Paused => ListFilterPreference::Paused,
                WorkspaceFilter::Completed => ListFilterPreference::Completed,
                WorkspaceFilter::Failed => ListFilterPreference::Failed,
            },
            list_sort_key: match sort_key {
                WorkspaceSortKey::Queue => ListSortKeyPreference::Queue,
                WorkspaceSortKey::Name => ListSortKeyPreference::Name,
                WorkspaceSortKey::Status => ListSortKeyPreference::Status,
                WorkspaceSortKey::Progress => ListSortKeyPreference::Progress,
                WorkspaceSortKey::DownloadSpeed => ListSortKeyPreference::DownloadSpeed,
                WorkspaceSortKey::Size => ListSortKeyPreference::Size,
            },
            list_sort_direction: match sort_direction {
                WorkspaceSortDirection::Ascending => ListSortDirectionPreference::Ascending,
                WorkspaceSortDirection::Descending => ListSortDirectionPreference::Descending,
            },
        };
        if self.settings.ui == ui {
            return;
        }
        let mut next = self.settings.clone();
        next.ui = ui;
        // Persist through the same ordered persistence worker when available so a
        // concurrent proxy/theme save cannot clobber list preferences.
        if self.settings_persistence_available
            && let Some(sender) = self.persistence_sender.as_ref()
        {
            let request = SettingsPersistenceRequest {
                request_id: ariadeck_ui::RequestId::from_u64(0),
                settings: next.clone(),
                previous_settings: self.settings.clone(),
                proxy_password: ProxyPasswordUpdate::Unchanged,
                apply_proxy: false,
                apply_speed_limit: false,
                apply_transfer_policy: false,
                apply_bt_tracker: false,
                active_profile_id: self.profile_catalog.active().map(|p| p.profile_id),
            };
            if sender
                .send(PersistenceCommand::SaveSettings(Box::new(request)))
                .is_ok()
            {
                self.settings = next;
            }
        } else {
            self.settings = next;
        }
        let _ = cx;
    }

    fn schedule_geometry_save(&mut self, geometry: WindowGeometry, cx: &mut Context<Self>) {
        let Some(generation) = self.platform.prepare_geometry_save(geometry) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            this.update(cx, |this, _cx| {
                let Some(geometry) = this.platform.take_ready_geometry(generation) else {
                    return;
                };
                if let Some(sender) = this.persistence_sender.as_ref() {
                    let _ = sender.send(PersistenceCommand::SaveWindowGeometry(geometry));
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn sync_tray_with_settings(&mut self, _cx: &mut Context<Self>) {
        self.platform.sync_tray_with_settings(&self.settings);
    }

    fn spawn_diagnostic_export(
        &self,
        request: DiagnosticExportRequestView,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let snapshot = self
            .workspace
            .read(cx)
            .diagnostic_snapshot(ariadeck_settings::CURRENT_SETTINGS_SCHEMA_VERSION);
        let runtime = self.runtime.handle().clone();
        let destination = PathBuf::from(&request.path);
        cx.spawn_in(window, async move |this, cx| {
            let outcome = match runtime
                .spawn_blocking(move || diagnostics::export_diagnostic_zip(&destination, snapshot))
                .await
            {
                Ok(Ok(())) => DiagnosticExportOutcomeView::Success,
                Ok(Err(error)) => DiagnosticExportOutcomeView::Failure(OperationErrorView {
                    code: "diagnostics.export_failed".into(),
                    summary: error.to_string(),
                    retryable: true,
                }),
                Err(error) => DiagnosticExportOutcomeView::Failure(OperationErrorView {
                    code: "diagnostics.export_failed".into(),
                    summary: format!("Diagnostic export worker failed: {error}"),
                    retryable: true,
                }),
            };
            this.update_in(cx, |this, _window, cx| {
                this.workspace.update(cx, |workspace, cx| {
                    workspace.set_diagnostic_export_result(
                        DiagnosticExportResultView {
                            path: request.path,
                            outcome,
                        },
                        cx,
                    );
                });
            })
            .ok();
        })
        .detach();
    }

    fn spawn_settings_export(
        &self,
        request: SettingsExportRequestView,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let settings = self.settings.clone();
        let destination = PathBuf::from(&request.path);
        let runtime = self.runtime.handle().clone();
        cx.spawn_in(window, async move |this, cx| {
            let outcome = match runtime
                .spawn_blocking(move || {
                    ariadeck_settings::export_settings_to_path(&destination, &settings)
                })
                .await
            {
                Ok(Ok(())) => SettingsExportOutcomeView::Success,
                Ok(Err(error)) => SettingsExportOutcomeView::Failure(OperationErrorView {
                    code: "settings.export_failed".into(),
                    summary: error.to_string(),
                    retryable: true,
                }),
                Err(error) => SettingsExportOutcomeView::Failure(OperationErrorView {
                    code: "settings.export_failed".into(),
                    summary: format!("Settings export worker failed: {error}"),
                    retryable: true,
                }),
            };
            this.update_in(cx, |this, _window, cx| {
                this.workspace.update(cx, |workspace, cx| {
                    workspace.set_settings_export_result(
                        SettingsExportResultView {
                            path: request.path,
                            outcome,
                        },
                        cx,
                    );
                });
            })
            .ok();
        })
        .detach();
    }

    fn spawn_settings_import(
        &self,
        request: SettingsImportRequestView,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.settings.clone();
        let source = PathBuf::from(&request.path);
        let runtime = self.runtime.handle().clone();
        cx.spawn_in(window, async move |this, cx| {
            let outcome = match runtime
                .spawn_blocking(move || {
                    ariadeck_settings::import_settings_from_path(&source, &current)
                })
                .await
            {
                Ok(Ok(settings)) => {
                    SettingsImportOutcomeView::Ready(Box::new(map_settings(&settings)))
                }
                Ok(Err(error)) => SettingsImportOutcomeView::Failure(OperationErrorView {
                    code: "settings.import_failed".into(),
                    summary: error.to_string(),
                    retryable: true,
                }),
                Err(error) => SettingsImportOutcomeView::Failure(OperationErrorView {
                    code: "settings.import_failed".into(),
                    summary: format!("Settings import worker failed: {error}"),
                    retryable: true,
                }),
            };
            this.update_in(cx, |this, _window, cx| {
                this.workspace.update(cx, |workspace, cx| {
                    workspace.set_settings_import_result(
                        SettingsImportResultView {
                            path: request.path,
                            outcome,
                        },
                        cx,
                    );
                });
            })
            .ok();
        })
        .detach();
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
        let apply_transfer_policy = settings.transfer_policy != self.settings.transfer_policy;
        let apply_bt_tracker = settings.tracker_list != self.settings.tracker_list;
        let Some(sender) = self
            .settings_persistence_available
            .then_some(self.persistence_sender.as_ref())
            .flatten()
        else {
            self.deliver_settings_error(
                request,
                "Settings persistence is unavailable for this session.".into(),
                window,
                cx,
            );
            return;
        };
        if sender
            .send(PersistenceCommand::SaveSettings(Box::new(
                SettingsPersistenceRequest {
                    request_id: request.request_id,
                    settings,
                    previous_settings: self.settings.clone(),
                    proxy_password,
                    apply_proxy,
                    apply_speed_limit,
                    apply_transfer_policy,
                    apply_bt_tracker,
                    active_profile_id: self.profile_catalog.active().map(|p| p.profile_id),
                },
            )))
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

    fn enqueue_tracker_list_refresh(
        &self,
        request_id: ariadeck_ui::RequestId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let draft = self.workspace.read(cx).settings().tracker_list.clone();
        let draft = ariadeck_settings::TrackerListSettings {
            enabled: draft.enabled,
            source: match draft.source {
                TrackerListSourceView::Curated => ariadeck_settings::TrackerListSource::Curated,
                TrackerListSourceView::Custom => ariadeck_settings::TrackerListSource::Custom,
            },
            custom_url: trimmed_value(&draft.custom_url),
            auto_refresh: draft.auto_refresh,
            last_refreshed_at: draft.last_refreshed_at,
            list_text: draft.list_text,
        };
        let Some(sender) = &self.tracker_list_sender else {
            self.workspace.update(cx, |workspace, cx| {
                workspace.set_tracker_list_refresh_result(
                    request_id,
                    None,
                    0,
                    Err(OperationErrorView {
                        code: "settings.tracker_refresh_unavailable".into(),
                        summary: "Tracker list refresh is unavailable for this session.".into(),
                        retryable: false,
                    }),
                    false,
                    cx,
                );
            });
            let _ = window;
            return;
        };
        if sender
            .send(TrackerListRefreshRequest {
                request_id,
                auto: false,
                draft: Some(draft),
            })
            .is_err()
        {
            self.workspace.update(cx, |workspace, cx| {
                workspace.set_tracker_list_refresh_result(
                    request_id,
                    None,
                    0,
                    Err(OperationErrorView {
                        code: "settings.tracker_refresh_failed".into(),
                        summary: "The tracker list refresh worker stopped unexpectedly.".into(),
                        retryable: true,
                    }),
                    false,
                    cx,
                );
            });
        }
        let _ = window;
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
        let runtime = self.runtime.handle().clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = match runtime
                .spawn(execute_task_command(
                    sync,
                    task_file_gateway,
                    request.clone(),
                ))
                .await
            {
                Ok(result) => result,
                Err(error) => TaskCommandResultView {
                    request_id: request.request_id,
                    session: request.session,
                    identity: request.identity,
                    command: request.command,
                    outcome: CommandOutcomeView::Failure(OperationErrorView {
                        code: "application.internal".into(),
                        summary: format!("Task command worker failed: {error}"),
                        retryable: true,
                    }),
                },
            };
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
        let runtime = self.runtime.handle().clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = match runtime
                .spawn(execute_batch_task_command(
                    sync,
                    task_file_gateway,
                    request.clone(),
                ))
                .await
            {
                Ok(result) => result,
                Err(error) => BatchTaskCommandResultView {
                    request_id: request.request_id,
                    session: request.session,
                    identities: request.identities,
                    command: request.command,
                    outcome: BatchCommandOutcomeView::Failure {
                        failed: vec![ariadeck_ui::BatchTaskFailureView {
                            identity: None,
                            error: OperationErrorView {
                                code: "application.internal".into(),
                                summary: format!("Batch command worker failed: {error}"),
                                retryable: true,
                            },
                        }],
                    },
                },
            };
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

    fn handle_switch_profile(
        &mut self,
        request: SwitchProfileRequestView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let profile_id = match request.profile_id.parse::<ProfileId>() {
            Ok(id) => id,
            Err(error) => {
                self.deliver_switch_profile_error(
                    &request,
                    format!("Invalid profile id: {error}"),
                    window,
                    cx,
                );
                return;
            }
        };
        let Some(sender) = self.persistence_sender.as_ref() else {
            self.deliver_switch_profile_error(
                &request,
                "The persistence worker is unavailable.".into(),
                window,
                cx,
            );
            return;
        };
        if sender
            .send(PersistenceCommand::SwitchProfile(Box::new(
                ProfileSwitchRequest {
                    request_id: request.request_id,
                    profile_id,
                    profile_id_raw: request.profile_id.clone(),
                    current_catalog: self.profile_catalog.clone(),
                    current_settings: self.settings.clone(),
                },
            )))
            .is_err()
        {
            self.deliver_switch_profile_error(
                &request,
                "The persistence worker stopped unexpectedly.".into(),
                window,
                cx,
            );
        }
    }

    fn deliver_switch_profile_error(
        &self,
        request: &SwitchProfileRequestView,
        summary: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let catalog = map_profile_catalog(&self.profile_catalog);
        self.workspace.update(cx, |shell, cx| {
            shell.set_switch_profile_result(
                SwitchProfileResultView {
                    request_id: request.request_id,
                    profile_id: request.profile_id.clone(),
                    catalog,
                    outcome: SwitchProfileOutcomeView::Failure(OperationErrorView {
                        code: "profile.switch_failed".into(),
                        summary,
                        retryable: true,
                    }),
                },
                cx,
            );
        });
        let _ = window;
    }

    fn handle_save_profile_catalog(
        &mut self,
        request: SaveProfileCatalogRequestView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match map_profile_catalog_request(
            &request.catalog,
            &request.secret_updates,
            &self.profile_catalog,
            &self.data_dir,
            &self.settings,
        ) {
            Ok(mapped) => {
                let Some(sender) = self.persistence_sender.as_ref() else {
                    self.deliver_save_profile_error(
                        &request,
                        "The persistence worker is unavailable.".into(),
                        window,
                        cx,
                    );
                    return;
                };
                if sender
                    .send(PersistenceCommand::SaveProfileCatalog(Box::new(
                        ProfileCatalogSaveJob {
                            request_id: request.request_id,
                            request_catalog: request.catalog.clone(),
                            next_catalog: mapped.catalog,
                            secret_updates: mapped.secret_updates,
                            previous_catalog: self.profile_catalog.clone(),
                            current_settings: self.settings.clone(),
                        },
                    )))
                    .is_err()
                {
                    self.deliver_save_profile_error(
                        &request,
                        "The persistence worker stopped unexpectedly.".into(),
                        window,
                        cx,
                    );
                }
            }
            Err(error) => self.deliver_save_profile_error(&request, error, window, cx),
        }
    }

    fn deliver_save_profile_error(
        &self,
        request: &SaveProfileCatalogRequestView,
        summary: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace.update(cx, |shell, cx| {
            shell.set_save_profile_catalog_result(
                SaveProfileCatalogResultView {
                    request_id: request.request_id,
                    catalog: request.catalog.clone(),
                    outcome: SaveProfileCatalogOutcomeView::Failure(OperationErrorView {
                        code: "profile.save_failed".into(),
                        summary,
                        retryable: true,
                    }),
                },
                Some(window),
                cx,
            );
        });
        let _ = window;
    }

    fn handle_core_command(
        &mut self,
        request: CoreCommandRequestView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let core_store = self.core_store.clone();
        let runtime = self.runtime.handle().clone();
        cx.spawn_in(window, async move |this, cx| {
            let command_for_worker = request.command.clone();
            let (outcome, registry) = match runtime
                .spawn_blocking(move || {
                    let result = execute_core_command_on_store(&core_store, &command_for_worker);
                    let registry = map_core_registry(&core_store);
                    (result, registry)
                })
                .await
            {
                Ok((Ok(()), registry)) => (CoreCommandOutcomeView::Success, registry),
                Ok((Err(summary), registry)) => (
                    CoreCommandOutcomeView::Failure(OperationErrorView {
                        code: "core.command_failed".into(),
                        summary,
                        retryable: true,
                    }),
                    registry,
                ),
                Err(error) => (
                    CoreCommandOutcomeView::Failure(OperationErrorView {
                        code: "core.command_failed".into(),
                        summary: format!("Core command worker failed: {error}"),
                        retryable: true,
                    }),
                    CoreRegistryView::default(),
                ),
            };
            this.update_in(cx, |this, _window, cx| {
                this.workspace.update(cx, |shell, cx| {
                    shell.set_core_command_result(
                        CoreCommandResultView {
                            request_id: request.request_id,
                            command: request.command,
                            registry,
                            outcome,
                        },
                        cx,
                    );
                });
            })
            .ok();
        })
        .detach();
    }

    /// B4: scan the local filesystem for aria2 binaries. No network access.
    ///
    /// Probing spawns one `aria2c --version` per candidate, so the whole scan
    /// runs on a blocking thread rather than the render path.
    fn spawn_core_discovery(
        &self,
        request_id: ariadeck_ui::RequestId,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let runtime = self.runtime.handle().clone();
        let core_store = self.core_store.clone();
        cx.spawn_in(window, async move |this, cx| {
            let discovered = runtime
                .spawn_blocking(move || {
                    let discovered = ariadeck_engine::discover_cores();
                    let registry = map_core_registry(&core_store);
                    map_discovered_cores(discovered, &registry)
                })
                .await
                .unwrap_or_else(|error| {
                    tracing::warn!(%error, "aria2 discovery worker failed");
                    Vec::new()
                });
            this.update_in(cx, |this, _window, cx| {
                this.workspace.update(cx, |workspace, cx| {
                    workspace.set_core_discovery_result(
                        CoreDiscoveryResultView {
                            request_id,
                            discovered,
                        },
                        cx,
                    );
                });
            })
            .ok();
        })
        .detach();
    }

    /// B4: hand an explicit download request to the verified-fetch worker.
    fn enqueue_core_download(
        &mut self,
        request_id: ariadeck_ui::RequestId,
        cx: &mut Context<Self>,
    ) {
        let unavailable = |summary: &str| CoreDownloadResultView {
            request_id,
            registry: CoreRegistryView::default(),
            installed_version: None,
            outcome: CoreCommandOutcomeView::Failure(OperationErrorView {
                code: "core.download_unavailable".into(),
                summary: summary.into(),
                retryable: false,
            }),
        };
        let Some(sender) = self.core_download_sender.as_ref() else {
            self.workspace.update(cx, |workspace, cx| {
                workspace.set_core_download_result(
                    unavailable("The aria2 download worker is not running."),
                    cx,
                );
            });
            return;
        };
        if sender.send(CoreDownloadRequest { request_id }).is_err() {
            tracing::warn!("aria2 download worker is no longer accepting requests");
            self.workspace.update(cx, |workspace, cx| {
                workspace.set_core_download_result(
                    unavailable("The aria2 download worker is no longer accepting requests."),
                    cx,
                );
            });
        }
    }

    /// B4: the first-run guide was closed, so record that the user has been asked.
    ///
    /// Persisted through the same ordered settings worker as every other write so
    /// a concurrent save cannot clobber it (the `persist_ui_preferences` pattern).
    fn persist_core_setup_dismissal(&mut self, cx: &mut Context<Self>) {
        if self.settings.onboarding.core_setup_dismissed {
            return;
        }
        let mut next = self.settings.clone();
        next.onboarding.core_setup_dismissed = true;
        if self.settings_persistence_available
            && let Some(sender) = self.persistence_sender.as_ref()
        {
            let request = SettingsPersistenceRequest {
                request_id: ariadeck_ui::RequestId::from_u64(0),
                settings: next.clone(),
                previous_settings: self.settings.clone(),
                proxy_password: ProxyPasswordUpdate::Unchanged,
                apply_proxy: false,
                apply_speed_limit: false,
                apply_transfer_policy: false,
                apply_bt_tracker: false,
                active_profile_id: self.profile_catalog.active().map(|p| p.profile_id),
            };
            if sender
                .send(PersistenceCommand::SaveSettings(Box::new(request)))
                .is_ok()
            {
                self.settings = next;
            }
        } else {
            self.settings = next;
        }
        let _ = cx;
    }
}

impl Drop for DesktopRoot {
    fn drop(&mut self) {
        let pending_geometry = self.platform.cleanup_on_drop();
        self.tracker_list_sender.take();
        self.core_download_sender.take();
        let persistence_sender = self.persistence_sender.take();
        if let (Some(sender), Some(geometry)) = (persistence_sender.as_ref(), pending_geometry) {
            let _ = sender.send(PersistenceCommand::SaveWindowGeometry(geometry));
        }
        let persistence_task = self.persistence_task.take();
        let sync = self.sync.take();
        let local_engine = self.local_engine.take();
        let cleanup_started = Instant::now();
        // Never capture `Arc<Runtime>` inside a task spawned onto that same
        // runtime (dropping `Runtime` on a Tokio worker thread panics). Instead
        // return the `JoinHandle` to `main()`, which awaits it on the main OS thread.
        let handle = self.runtime.handle().spawn(async move {
            drop(persistence_sender);
            if let Some(task) = persistence_task
                && let Err(error) = task.await
            {
                tracing::warn!(%error, "persistence worker did not stop cleanly");
            }
            if let Some(handle) = sync
                && tokio::time::timeout(Duration::from_secs(2), handle.stop())
                    .await
                    .is_err()
            {
                tracing::warn!("sync coordinator did not stop within 2 seconds");
            }
            if let Some(mut process) = local_engine {
                match tokio::time::timeout(
                    Duration::from_secs(2),
                    request_local_engine_shutdown(&process),
                )
                .await
                {
                    Ok(Ok(())) => process.mark_graceful_shutdown_requested(),
                    Ok(Err(error)) => {
                        tracing::debug!(%error, "local aria2 graceful shutdown request failed")
                    }
                    Err(_) => tracing::warn!("aria2 shutdown request exceeded 2 seconds"),
                }
                match tokio::task::spawn_blocking(move || {
                    process.stop_monitoring();
                    process.shutdown()
                })
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        tracing::warn!(%error, "failed to stop the local aria2 process cleanly")
                    }
                    Err(error) => tracing::warn!(%error, "aria2 shutdown worker failed"),
                }
            }
            tracing::info!(
                elapsed_ms = cleanup_started.elapsed().as_millis(),
                "background shutdown cleanup complete"
            );
        });
        *self
            .shutdown_slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(handle);
    }
}

impl Render for DesktopRoot {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.workspace.clone()
    }
}

#[cfg(test)]
mod tests;
