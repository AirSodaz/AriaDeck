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
    ActivityMode, AddDownloadAdvancedOptions, AddDownloadRequest, AddDownloadSource, AppCommand,
    ApplicationError, ApplicationErrorCode, CommandItem, CommandOutcome, CoordinatorConfig,
    DownloadDestinationFile, DownloadDestinationGateway, DownloadDestinationRequest,
    DownloadProxyConfig, DownloadProxyMode as ApplicationProxyMode, EngineCapabilities,
    FileConflictPolicy, ItemFailure, MoveTaskInQueueRequest, QueueMove, ReconnectPolicy,
    RemoveTasksRequest, SetTaskConnectionPolicyRequest, SetTaskOptionsRequest,
    SetTaskOutputNameRequest, SetTaskSpeedLimitRequest, StoreSnapshot, SyncHandle, TaskFileGateway,
    TaskFileRemovalRequest, TaskListQuery, TaskOpenRequest, TaskOpenTarget, TaskRemovalScope,
    spawn_sync_coordinator,
};
use ariadeck_domain::{
    ByteRate, ConnectionState, DownloadFilter, DownloadSort, DownloadStatus, DownloadTask,
    EnginePath, EngineSession, EngineSessionId, Gid, ProfileId, SessionGeneration, SortDirection,
    SortKey, SpeedLimitConfig, TaskConnectionDetails, TaskConnectionPolicy, TaskDetails,
    TaskIdentity as DomainTaskIdentity, TaskProgress, TaskUriStatus, TransferPolicyConfig,
};
use ariadeck_engine::{
    CoreInstallStatus, CoreSource, CoreStore, ExternalEngineProfile, JsonProfileStore,
    LocalDownloadDestinationGateway, LocalDownloadRootRegistry, LocalEngineHealth,
    LocalEngineHealthHandle, LocalEngineSupervisor, LocalTaskFileGateway, ProfileCatalog,
    ProfileEntry, ProfileKind,
};
use ariadeck_i18n::{LocaleId, Translator};
use ariadeck_rpc::{
    Aria2Client, AuthenticatedTransport, RpcSecret, RpcSyncConnector, WebSocketConfig,
    WebSocketTransport,
};
use ariadeck_settings::{
    AppSettings, CloseBehavior, ColorScheme, DownloadProxyMode, DownloadProxySettings,
    FileAllocationSetting, JsonSettingsStore, JsonWindowGeometryStore, LanguagePreference,
    ListFilterPreference, ListSortDirectionPreference, ListSortKeyPreference, NotificationSettings,
    NotificationVolume, PlatformSettings, ProxyCredentialRef, ProxyCredentialStore,
    SpeedLimitSettings, SystemProxyCredentialStore, TransferPolicySettings, UiPreferences,
    WindowGeometry,
};
use ariadeck_ui::{
    AddDownloadAdvancedOptionsView, AddDownloadItemResultView, AddDownloadMetadataFileView,
    AddDownloadMetadataKindView, AddDownloadMetadataPreviewItemView,
    AddDownloadMetadataPreviewOutcomeView, AddDownloadMetadataPreviewRequestView,
    AddDownloadMetadataPreviewResultView, AddDownloadMetadataPreviewView, AddDownloadModeView,
    AddDownloadRequestView, AddDownloadResultView, AddDownloadSourceView, AppShell, AppShellEvent,
    BatchCommandOutcomeView, BatchTaskCommandRequestView, BatchTaskCommandResultView,
    BatchTaskCommandView, BatchTaskFailureView, CloseBehaviorView, ColorSchemeView,
    CommandOutcomeView, ConnectionView, CoreCommandOutcomeView, CoreCommandRequestView,
    CoreCommandResultView, CoreCommandView, CoreInstallStatusView, CoreInstallationView,
    CoreRegistryView, CoreSourceView, DownloadProxySettingsView, DownloadRowView,
    EngineCapabilitiesView, EngineHealthView, EngineSessionView, FileAllocationView,
    FileConflictPolicyView, GlobalTaskCommandRequestView, GlobalTaskCommandResultView,
    GlobalTaskCommandView, LanguagePreferenceView, NotificationSettingsView,
    NotificationVolumeView, OperationErrorView, PlatformSettingsView, ProfileCatalogView,
    ProfileEntryView, ProfileKindView, ProfileRpcSecretUpdateView, ProxyModeView,
    ProxyPasswordUpdateView, SaveProfileCatalogOutcomeView, SaveProfileCatalogRequestView,
    SaveProfileCatalogResultView, SettingsSaveOutcomeView, SettingsSaveRequestView,
    SettingsSaveResultView, SettingsView, SpeedLimitSettingsView, SpeedSampleView,
    StoppedHistoryView, SwitchProfileOutcomeView, SwitchProfileRequestView,
    SwitchProfileResultView, TaskCommandRequestView, TaskCommandResultView, TaskCommandView,
    TaskCountsView, TaskDetailsOutcomeView, TaskDetailsRequestView, TaskDetailsResultView,
    TaskDetailsView, TaskErrorView, TaskFileView, TaskIdentity, TaskNameStateView,
    TaskOpenOutcomeView, TaskOpenRequestView, TaskOpenResultView, TaskOpenTargetView,
    TaskOptionView, TaskPathValidationView, TaskPeerView, TaskServerView, TaskSourceKindView,
    TaskStatusView, TaskTrackerView, TaskUriStatusView, TaskUriView, TransferPolicySettingsView,
    WorkspaceFilter, WorkspaceQuery, WorkspaceSnapshot, WorkspaceSortDirection, WorkspaceSortKey,
    format_speed_limit_field,
};
use gpui::{AppContext as _, Context, Entity, IntoElement, Render, Subscription, Window};
#[cfg(target_os = "windows")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use secrecy::SecretString;

use crate::platform::{self, SystemTray, TrayAction};
use tokio::{
    runtime::Runtime,
    sync::{mpsc, watch},
    task::JoinHandle,
};
use url::Url;

use crate::metadata::parse_metadata;

mod engine_setup;
mod mapping;
mod ops;
mod settings_bridge;

// Re-export so DesktopRoot methods and tests keep short names.
#[allow(unused_imports)]
pub(crate) use engine_setup::*;
#[allow(unused_imports)]
pub(crate) use mapping::*;
#[allow(unused_imports)]
pub(crate) use ops::*;
#[allow(unused_imports)]
pub(crate) use settings_bridge::*;

/// Process-wide flag: true while AriaDeck is intentionally hidden to tray with
/// no open window. Prevents `on_window_closed` from quitting the app.
pub(crate) static TRAY_SESSION_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

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
    data_dir: PathBuf,
    profile_store: JsonProfileStore,
    profile_catalog: ProfileCatalog,
    core_store: CoreStore,
    tray: Option<SystemTray>,
    window_hidden_to_tray: bool,
    window_geometry_store: JsonWindowGeometryStore,
    last_saved_geometry: Option<WindowGeometry>,
    pending_geometry: Option<WindowGeometry>,
    geometry_save_generation: u64,
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
}

#[derive(Clone)]
pub(crate) enum ProxyPasswordUpdate {
    Unchanged,
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
        default_data_dir()
    }

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
        let (profile_catalog, mut profile_notice) = match profile_store
            .load_catalog_or_recover(&default_profile)
        {
            Ok(loaded) => {
                let mut notice = loaded.recovery.map(|recovery| {
                    format!(
                        "Invalid profile catalog was reset; the original was preserved at {}.",
                        recovery.backup_path.display()
                    )
                });
                if loaded.migrated && notice.is_none() {
                    notice = Some("Profile catalog was upgraded to multi-profile format.".into());
                }
                // Keep active local profile download dir aligned with settings.
                if let Some(active) = loaded.catalog.active().cloned() {
                    if active.kind == ProfileKind::LocalManaged {
                        let mut catalog = loaded.catalog;
                        if let Some(entry) = catalog.active_mut() {
                            entry.download_dir = settings.download_directory.clone();
                            if entry.data_dir.is_none() {
                                entry.data_dir = Some(data_dir.clone());
                            }
                            // Leave empty executable as managed-core opt-in;
                            // resolve_local_executable handles spawn-time path.
                        }
                        let _ = profile_store.save_catalog(&catalog);
                        (catalog, notice)
                    } else {
                        (loaded.catalog, notice)
                    }
                } else {
                    (loaded.catalog, notice)
                }
            }
            Err(error) => {
                tracing::error!(%error, "failed to load profile catalog");
                (
                    ProfileCatalog::from_single(default_profile),
                    Some(format!("Profile catalog could not be loaded: {error}")),
                )
            }
        };

        let (sync, local_engine, mut initial_snapshot, engine_startup_notice) =
            match create_sync_handle(&runtime, &data_dir, &settings, &profile_catalog) {
                Ok((handle, local_engine, engine_notice)) => {
                    let snapshot = WorkspaceSnapshot {
                        connection: ConnectionView::Connecting,
                        ..WorkspaceSnapshot::default()
                    };
                    (Some(handle), local_engine, snapshot, engine_notice)
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
                    (None, None, snapshot, None)
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
        let initial_query = map_ui_preferences_to_query(&settings.ui);
        let workspace = cx.new(|cx| {
            let mut shell = AppShell::new_with_settings(settings_view, window, cx);
            shell.set_snapshot(initial_snapshot, cx);
            shell.set_engine_health(initial_engine_health, cx);
            shell.set_profiles(map_profile_catalog(&profile_catalog), cx);
            shell.set_cores(map_core_registry(&core_store), cx);
            shell.restore_list_preferences(initial_query.clone(), cx);
            if let Some(message) = startup_notice {
                shell.set_startup_notice(message, true, cx);
            } else if let Some(message) = profile_notice.take() {
                shell.set_startup_notice(message, true, cx);
            } else if let Some(message) = engine_startup_notice {
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
                AppShellEvent::SwitchProfileRequested(request) => {
                    this.handle_switch_profile(request.clone(), window, cx);
                }
                AppShellEvent::SaveProfileCatalogRequested(request) => {
                    this.handle_save_profile_catalog(request.clone(), window, cx);
                }
                AppShellEvent::CoreCommandRequested(request) => {
                    this.handle_core_command(request.clone(), window, cx);
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
                    platform::show_os_notification(title, body);
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
            },
        );
        let window_geometry_store = JsonWindowGeometryStore::new(data_dir.join("window.json"));
        let last_saved_geometry = window_geometry_store.load();

        // Intercept the native close control so Minimize-to-tray can keep the
        // process (and managed engine) alive without destroying the window.
        {
            let workspace = workspace.clone();
            window.on_window_should_close(cx, move |_window, cx| {
                workspace.update(cx, |shell, cx| shell.handle_window_close_request(cx))
            });
        }

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

        let tray = if settings.platform.show_tray_icon {
            match tray_from_settings(&settings) {
                Ok(tray) => Some(tray),
                Err(error) => {
                    tracing::warn!(%error, "system tray icon could not be created");
                    None
                }
            }
        } else {
            None
        };

        let root = Self {
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
            data_dir: data_dir.clone(),
            profile_store,
            profile_catalog,
            core_store,
            tray,
            window_hidden_to_tray: false,
            window_geometry_store,
            last_saved_geometry,
            pending_geometry: None,
            geometry_save_generation: 0,
            _workspace_subscription: workspace_subscription,
        };

        // Poll tray interactions and free-space checks on a short UI timer.
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

        if root.settings.platform.start_minimized_to_tray && root.tray.is_some() {
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
        }

        root
    }

    fn poll_platform_surface(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tray_actions = self
            .tray
            .as_ref()
            .map(|tray| tray.poll_actions())
            .unwrap_or_default();
        for action in tray_actions {
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
        if let Some(tray) = self.tray.as_ref() {
            let tooltip = self.workspace.read(cx).tray_tooltip();
            tray.set_tooltip(&tooltip);
        }

        // Low-disk check against the configured download directory.
        let free = platform::available_disk_space(&self.settings.download_directory);
        self.workspace.update(cx, |shell, cx| {
            shell.report_disk_space(free, cx);
        });
    }

    fn hide_to_tray(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tray.is_none() {
            // No tray: fall back to quit so the user is not stuck without UI.
            self.quit_application(cx);
            return;
        }
        if hide_native_window(window) {
            self.window_hidden_to_tray = true;
            TRAY_SESSION_ACTIVE.store(true, std::sync::atomic::Ordering::SeqCst);
            if let Some(tray) = self.tray.as_ref() {
                tray.set_visible(true);
            }
            // PERF-001: slow live/global/stopped polls while the window is hidden.
            self.set_sync_activity(ActivityMode::Background, cx);
        }
    }

    fn show_from_tray(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if show_native_window(window) {
            self.window_hidden_to_tray = false;
            TRAY_SESSION_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
            window.activate_window();
            cx.activate(true);
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

    fn quit_application(&mut self, cx: &mut Context<Self>) {
        TRAY_SESSION_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
        self.window_hidden_to_tray = false;
        // Drop tray first so Windows removes the icon immediately.
        self.tray.take();
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
        // Persist through the same ordered settings worker when available so a
        // concurrent proxy/theme save cannot clobber list preferences.
        if let Some(sender) = self.settings_sender.as_ref() {
            let request = SettingsPersistenceRequest {
                request_id: ariadeck_ui::RequestId::from_u64(0),
                settings: next.clone(),
                previous_settings: self.settings.clone(),
                proxy_password: ProxyPasswordUpdate::Unchanged,
                apply_proxy: false,
                apply_speed_limit: false,
                apply_transfer_policy: false,
            };
            if sender.send(request).is_ok() {
                self.settings = next;
            }
        } else {
            self.settings = next;
        }
        let _ = cx;
    }

    fn schedule_geometry_save(&mut self, geometry: WindowGeometry, cx: &mut Context<Self>) {
        let geometry = geometry.sanitized();
        if self.last_saved_geometry == Some(geometry) {
            self.pending_geometry = None;
            return;
        }
        self.pending_geometry = Some(geometry);
        self.geometry_save_generation = self.geometry_save_generation.wrapping_add(1);
        let generation = self.geometry_save_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            this.update(cx, |this, _cx| {
                if this.geometry_save_generation != generation {
                    return;
                }
                let Some(geometry) = this.pending_geometry.take() else {
                    return;
                };
                if this.last_saved_geometry == Some(geometry) {
                    return;
                }
                if let Err(error) = this.window_geometry_store.save(geometry) {
                    tracing::debug!(%error, "failed to persist window geometry");
                    return;
                }
                this.last_saved_geometry = Some(geometry);
            })
            .ok();
        })
        .detach();
    }

    fn sync_tray_with_settings(&mut self, cx: &mut Context<Self>) {
        let want_tray = self.settings.platform.show_tray_icon;
        match (want_tray, self.tray.is_some()) {
            (true, false) => match tray_from_settings(&self.settings) {
                Ok(tray) => self.tray = Some(tray),
                Err(error) => tracing::warn!(%error, "system tray icon could not be created"),
            },
            (false, true) => {
                self.tray.take();
                if self.window_hidden_to_tray {
                    // Cannot stay hidden without a tray affordance.
                    TRAY_SESSION_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
                    self.window_hidden_to_tray = false;
                }
            }
            (true, true) => {
                if let Some(tray) = self.tray.as_ref() {
                    tray.set_visible(true);
                }
            }
            (false, false) => {}
        }
        let _ = cx;
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
                apply_transfer_policy,
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
        if let Err(error) = self.profile_catalog.set_active(profile_id) {
            self.deliver_switch_profile_error(&request, error.to_string(), window, cx);
            return;
        }
        if let Err(error) = self.profile_store.save_catalog(&self.profile_catalog) {
            self.deliver_switch_profile_error(
                &request,
                format!("Failed to save active profile: {error}"),
                window,
                cx,
            );
            return;
        }
        let catalog = map_profile_catalog(&self.profile_catalog);
        let name = catalog
            .active()
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| "selected profile".into());
        self.workspace.update(cx, |shell, cx| {
            shell.set_switch_profile_result(
                SwitchProfileResultView {
                    request_id: request.request_id,
                    profile_id: request.profile_id.clone(),
                    catalog,
                    outcome: SwitchProfileOutcomeView::Success,
                },
                cx,
            );
            shell.set_startup_notice(
                format!(
                    "Active profile set to {name}. Restart AriaDeck to connect with the new profile."
                ),
                false,
                cx,
            );
        });
        let _ = window;
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
            Ok(catalog) => {
                if let Err(error) = apply_profile_secret_updates(
                    &catalog,
                    &request.catalog,
                    &request.secret_updates,
                    &self.profile_catalog,
                ) {
                    self.deliver_save_profile_error(&request, error, window, cx);
                    return;
                }
                if let Err(error) = self.profile_store.save_catalog(&catalog) {
                    self.deliver_save_profile_error(
                        &request,
                        format!("Failed to save profiles: {error}"),
                        window,
                        cx,
                    );
                    return;
                }
                // Drop secrets for profiles removed from the catalog.
                cleanup_removed_profile_secrets(&self.profile_catalog, &catalog);
                self.profile_catalog = catalog;
                let view = map_profile_catalog(&self.profile_catalog);
                self.workspace.update(cx, |shell, cx| {
                    shell.set_save_profile_catalog_result(
                        SaveProfileCatalogResultView {
                            request_id: request.request_id,
                            catalog: view,
                            outcome: SaveProfileCatalogOutcomeView::Success,
                        },
                        cx,
                    );
                });
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
        let result = self.execute_core_command(&request.command);
        match result {
            Ok(()) => {
                let registry = map_core_registry(&self.core_store);
                // Profiles that opt into the managed core keep an empty executable
                // path so Activate-core does not rewrite them to a pinned binary.
                self.workspace.update(cx, |shell, cx| {
                    shell.set_core_command_result(
                        CoreCommandResultView {
                            request_id: request.request_id,
                            command: request.command.clone(),
                            registry,
                            outcome: CoreCommandOutcomeView::Success,
                        },
                        cx,
                    );
                });
            }
            Err(summary) => {
                let registry = map_core_registry(&self.core_store);
                self.workspace.update(cx, |shell, cx| {
                    shell.set_core_command_result(
                        CoreCommandResultView {
                            request_id: request.request_id,
                            command: request.command.clone(),
                            registry,
                            outcome: CoreCommandOutcomeView::Failure(OperationErrorView {
                                code: "core.command_failed".into(),
                                summary,
                                retryable: true,
                            }),
                        },
                        cx,
                    );
                });
            }
        }
        let _ = window;
    }

    fn execute_core_command(&self, command: &CoreCommandView) -> Result<(), String> {
        match command {
            CoreCommandView::Import { path } => self
                .core_store
                .import_executable(PathBuf::from(path))
                .map(|_| ())
                .map_err(|error| error.to_string()),
            CoreCommandView::Link { path } => self
                .core_store
                .link_executable(PathBuf::from(path))
                .map(|_| ())
                .map_err(|error| error.to_string()),
            CoreCommandView::Verify { core_id } => {
                let id = core_id
                    .parse()
                    .map_err(|error| format!("Invalid core id: {error}"))?;
                self.core_store
                    .verify(id)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }
            CoreCommandView::Activate { core_id } => {
                let id = core_id
                    .parse()
                    .map_err(|error| format!("Invalid core id: {error}"))?;
                self.core_store
                    .activate(id)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }
            CoreCommandView::Rollback => self
                .core_store
                .rollback_to_last_working()
                .map(|_| ())
                .map_err(|error| error.to_string()),
            CoreCommandView::Remove { core_id } => {
                let id = core_id
                    .parse()
                    .map_err(|error| format!("Invalid core id: {error}"))?;
                self.core_store
                    .remove(id)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }
        }
    }
}

impl Drop for DesktopRoot {
    fn drop(&mut self) {
        TRAY_SESSION_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
        self.window_hidden_to_tray = false;
        self.tray.take();
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

#[cfg(test)]
mod tests;

fn tray_from_settings(settings: &AppSettings) -> Result<SystemTray, String> {
    let locale = match settings.language {
        LanguagePreference::System => LocaleId::from_system_env(),
        LanguagePreference::En => LocaleId::En,
        LanguagePreference::ZhCn => LocaleId::ZhCn,
    };
    let translator = Translator::new(locale);
    SystemTray::try_new_with_labels(
        &translator.t("tray-show"),
        &translator.t("tray-pause-all"),
        &translator.t("tray-resume-all"),
        &translator.t("tray-quit"),
        &translator.t("app-name"),
    )
}
