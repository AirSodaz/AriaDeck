use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use ariadeck_i18n::{FluentArgs, FluentValue};
use gpui::{
    AnyElement, App, ClickEvent, ClipboardItem, Context, Div, ElementId, Entity, ExternalPaths,
    FocusHandle, Focusable, FontFeatures, FontWeight, Hsla, IntoElement, MouseButton,
    MouseDownEvent, PathPromptOptions, Pixels, Point, Render, Role, ScrollHandle, ScrollStrategy,
    SharedString, Stateful, Subscription, Toggled, UniformListScrollHandle, WeakFocusHandle,
    Window, WindowControlArea, div, point, prelude::*, px, relative, uniform_list,
};

use crate::{
    ActivityEntryView, ActivityKindView, AddDownloadAdvancedOptionsView, AddDownloadInputModeView,
    AddDownloadItemResultView, AddDownloadMetadataKindView, AddDownloadMetadataPreviewOutcomeView,
    AddDownloadMetadataPreviewRequestView, AddDownloadMetadataPreviewResultView,
    AddDownloadMetadataPreviewView, AddDownloadModeView, AddDownloadRequestView,
    AddDownloadResultView, AddDownloadSourceView, BatchCommandOutcomeView,
    BatchTaskCommandRequestView, BatchTaskCommandResultView, BatchTaskCommandView,
    BatchTaskFailureView, Button, ButtonStyle, ClearSearch, CloseAddDownload, CloseBatchFailures,
    CloseBehaviorView, CloseSettings, CloseTaskOptions, CloseTaskOutputName, CloseTaskSpeedLimit,
    ColorSchemeView, CommandOutcomeView, ConnectionView, CoreCommandOutcomeView,
    CoreCommandRequestView, CoreCommandResultView, CoreCommandView, CoreRegistryView,
    DiagnosticExportOutcomeView, DiagnosticExportRequestView, DiagnosticExportResultView, Dialog,
    DownloadProxySettingsView, DownloadRowView, EngineHealthView, EngineSessionView,
    FileAllocationView, FileConflictPolicyView, FocusNext, FocusPrevious, FocusSearch,
    GlobalTaskCommandRequestView, GlobalTaskCommandResultView, GlobalTaskCommandView, Icon,
    IconButton, IconName, IconSize, LanguagePreferenceView, LocaleId, MoveTaskDownInQueue,
    MoveTaskToQueueBottom, MoveTaskToQueueTop, MoveTaskUpInQueue, NotificationSettingsView,
    NotificationVolumeView, OpenAddDownload, OpenSettings, OpenTaskDetails, OpenTaskOutputName,
    OpenTaskSpeedLimit, OperationErrorView, PauseSelectedTask, PlatformSettingsView,
    ProfileCatalogView, ProfileEntryView, ProfileKindView, ProfileRpcSecretUpdateView,
    ProxyModeView, ProxyPasswordUpdateView, RemoveSelectedTask, RequestId, ResumeSelectedTask,
    RetrySelectedTask, SaveProfileCatalogOutcomeView, SaveProfileCatalogRequestView,
    SaveProfileCatalogResultView, SaveSettings, SearchInputEvent, SecretStringView, Segment,
    SegmentedControl, SelectAllTasks, SelectNextTask, SelectPreviousTask,
    SettingsExportOutcomeView, SettingsExportRequestView, SettingsExportResultView,
    SettingsImportOutcomeView, SettingsImportRequestView, SettingsImportResultView,
    SettingsSaveOutcomeView, SettingsSaveRequestView, SettingsSaveResultView, SettingsView,
    SpeedLimitSettingsView, SpeedSampleView, StatusIndicator, SubmitAddDownload, SubmitTaskOptions,
    SubmitTaskOutputName, SubmitTaskSpeedLimit, SwitchProfileOutcomeView, SwitchProfileRequestView,
    SwitchProfileResultView, TaskCommandRequestView, TaskCommandResultView, TaskCommandView,
    TaskDetailsOutcomeView, TaskDetailsRequestView, TaskDetailsResultView, TaskDetailsView,
    TaskFileView, TaskIdentity, TaskOpenOutcomeView, TaskOpenRequestView, TaskOpenResultView,
    TaskOpenTargetView, TaskOptionView, TaskPathValidationView, TaskStatusView, TextField,
    TextFieldConfig, Theme, ThemeMode, Toast, ToastKind, Toggle, Tooltip,
    TransferPolicySettingsView, Translator, WorkspaceFilter, WorkspaceQuery, WorkspaceSnapshot,
    WorkspaceSortDirection, WorkspaceSortKey, actions::TEXT_FIELD_KEY_CONTEXT, format_bytes,
    format_eta, format_percent, format_rate, format_share_ratio,
};

#[cfg(test)]
use crate::{TaskPeerView, TaskServerView, TaskTrackerView, TaskUriView};

mod activity;
mod chrome;
mod commands;
mod details;
mod dialogs_add;
mod helpers;
mod profiles;
mod settings;
mod task_list;
#[allow(unused_imports)]
use helpers::*;

const SPEED_CHART_SAMPLES: usize = 120;
const TITLEBAR_HEIGHT: f32 = 52.0;
const TITLEBAR_SIDE_WIDTH: f32 = 240.0;
const TITLEBAR_HORIZONTAL_PADDING: f32 = 12.0;
const SEARCH_MIN_WIDTH: f32 = 300.0;
const SEARCH_WIDTH: f32 = 460.0;
const SIDEBAR_WIDTH: f32 = 208.0;
const COMPACT_SIDEBAR_WIDTH: f32 = 184.0;
const DETAILS_DRAWER_WIDTH: f32 = 380.0;
const WIDE_SHELL_MIN_WIDTH: f32 = 1280.0;
const TASK_ROW_HEIGHT: f32 = 64.0;
const ACTIVITY_HISTORY_LIMIT: usize = 100;
const ACTIVITY_PANEL_WIDTH: f32 = 360.0;
/// Minimum gap between revision-driven details re-fetches while the drawer is open (PERF-001).
const DETAILS_REFRESH_MIN_INTERVAL: Duration = Duration::from_millis(500);

#[cfg(target_os = "macos")]
const TITLEBAR_BRAND_INSET: f32 = 52.0;
#[cfg(not(target_os = "macos"))]
const TITLEBAR_BRAND_INSET: f32 = 0.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TaskLayoutMode {
    Compact,
    Wide,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DetailsPresentation {
    Inline,
    Overlay,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ShellLayout {
    sidebar_width: f32,
    task_layout: TaskLayoutMode,
    details_presentation: DetailsPresentation,
}

fn shell_layout(viewport_width: f32) -> ShellLayout {
    let wide_shell = viewport_width >= WIDE_SHELL_MIN_WIDTH;
    let sidebar_width = if wide_shell {
        SIDEBAR_WIDTH
    } else {
        COMPACT_SIDEBAR_WIDTH
    };
    let details_presentation = if wide_shell {
        DetailsPresentation::Inline
    } else {
        DetailsPresentation::Overlay
    };
    let task_layout = if wide_shell {
        TaskLayoutMode::Wide
    } else {
        TaskLayoutMode::Compact
    };
    ShellLayout {
        sidebar_width,
        task_layout,
        details_presentation,
    }
}

// Kept as a small compatibility seam for layout tests and callers that only
// need the table mode. The full shell uses `shell_layout` for all dimensions.
#[cfg(test)]
fn task_layout_mode(viewport_width: f32) -> TaskLayoutMode {
    shell_layout(viewport_width).task_layout
}

fn centered_search_bounds(viewport_width: f32) -> (f32, f32) {
    let available_width =
        (viewport_width - 2.0 * (TITLEBAR_SIDE_WIDTH + TITLEBAR_HORIZONTAL_PADDING)).max(0.0);
    let width = available_width
        .min(SEARCH_WIDTH)
        .max(SEARCH_MIN_WIDTH.min(available_width));
    let left = (viewport_width - width) / 2.0;
    (left, left + width)
}

#[derive(Clone, Debug, PartialEq)]
pub enum AppShellEvent {
    QueryChanged(WorkspaceQuery),
    RetryRequested,
    /// Request the next stopped-result page when history is incomplete.
    LoadMoreStoppedRequested,
    AddDownloadRequested(AddDownloadRequestView),
    AddDownloadMetadataPreviewRequested(AddDownloadMetadataPreviewRequestView),
    TaskCommandRequested(TaskCommandRequestView),
    GlobalTaskCommandRequested(GlobalTaskCommandRequestView),
    BatchTaskCommandRequested(BatchTaskCommandRequestView),
    TaskDetailsRequested(TaskDetailsRequestView),
    TaskOpenRequested(TaskOpenRequestView),
    SettingsSaveRequested(SettingsSaveRequestView),
    SettingsExportRequested(SettingsExportRequestView),
    SettingsImportRequested(SettingsImportRequestView),
    DiagnosticExportRequested(DiagnosticExportRequestView),
    SwitchProfileRequested(SwitchProfileRequestView),
    SaveProfileCatalogRequested(SaveProfileCatalogRequestView),
    CoreCommandRequested(CoreCommandRequestView),
    /// Hide the main window while keeping the process and managed engine alive.
    HideToTrayRequested,
    /// Bring the main window back from tray/minimized state.
    ShowFromTrayRequested,
    /// Fully quit AriaDeck (stops a managed engine on drop).
    QuitRequested,
    /// Emit an OS-native notification when preference-gated (PLAT-001).
    OsNotificationRequested {
        title: String,
        body: String,
        is_error: bool,
    },
    /// Persist list filter/sort preferences without rewriting other settings (UI-001).
    UiPreferencesChanged {
        filter: WorkspaceFilter,
        sort_key: WorkspaceSortKey,
        sort_direction: WorkspaceSortDirection,
    },
    /// Debounced main-window geometry for session restore (UI-001).
    WindowGeometryChanged {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        maximized: bool,
    },
}

struct PendingAddDownload {
    request_id: RequestId,
    session: EngineSessionView,
}

struct PendingMetadataPreview {
    request_id: RequestId,
    paths: Vec<PathBuf>,
}

#[derive(Default)]
struct AddDownloadDialog {
    open: bool,
    input_mode: AddDownloadInputModeView,
    mode: AddDownloadModeView,
    file_conflict: FileConflictPolicyView,
    /// Selected download category id (C1); None uses settings default / global dir.
    category_id: Option<String>,
    /// Collapsed by default so the common path stays simple (ADD-005).
    advanced_open: bool,
    metadata_files: Vec<AddDownloadMetadataPreviewView>,
    active_metadata_file: Option<usize>,
    preview_pending: Option<PendingMetadataPreview>,
    previous_focus: Option<WeakFocusHandle>,
    pending: Option<PendingAddDownload>,
    error: Option<OperationErrorView>,
    results: Vec<AddDownloadItemResultView>,
    updating_input_from_result: bool,
}

struct PendingTaskCommand {
    request_id: RequestId,
    session: EngineSessionView,
    identity: TaskIdentity,
    command: TaskCommandView,
}

struct PendingGlobalTaskCommand {
    request_id: RequestId,
    session: EngineSessionView,
    command: GlobalTaskCommandView,
}

struct PendingBatchTaskCommand {
    request_id: RequestId,
    session: EngineSessionView,
    identities: Vec<TaskIdentity>,
    command: BatchTaskCommandView,
}

enum TaskDetailsLoadState {
    Loading,
    Ready { details: Box<TaskDetailsView> },
    Failed { error: OperationErrorView },
    Stale,
}

struct PendingTaskDetails {
    request_id: RequestId,
    source_revision: u64,
}

struct PendingTaskOpen {
    request_id: RequestId,
    target: TaskOpenTargetView,
}

enum TaskDetailsPresentation {
    Loading,
    Ready(Box<TaskDetailsView>),
    Failed(String),
    Stale,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum TaskDetailsTab {
    #[default]
    Info,
    Files,
    Network,
    Options,
}

struct TaskDetailsDrawer {
    identity: TaskIdentity,
    overview: DownloadRowView,
    session: EngineSessionView,
    state: TaskDetailsLoadState,
    pending: Option<PendingTaskDetails>,
    open_pending: Option<PendingTaskOpen>,
    tab: TaskDetailsTab,
    file_scroll: UniformListScrollHandle,
    rendered_file_range: Range<usize>,
    /// Last time a ready→ready details RPC was emitted (coalesce rapid revisions).
    last_ready_refresh_at: Option<Instant>,
    /// True when a revision-driven refresh was deferred by the min interval.
    refresh_coalesced: bool,
}

struct StatusNotice {
    id: u64,
    message: String,
    is_error: bool,
    /// When false, this is command feedback that Quiet mode still shows.
    #[allow(dead_code)]
    automatic: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum AppPage {
    #[default]
    Downloads,
    Settings,
}

#[derive(Default)]
pub(crate) struct SettingsPage {
    previous_focus: Option<WeakFocusHandle>,
    /// Currently visible navigation category in the two-pane layout.
    active_category: SettingsCategory,
    draft_color_scheme: ColorSchemeView,
    draft_language: LanguagePreferenceView,
    draft_proxy_mode: ProxyModeView,
    draft_check_certificate: bool,
    draft_file_allocation: FileAllocationView,
    draft_check_integrity: bool,
    draft_notification_volume: NotificationVolumeView,
    draft_notify_on_completion: bool,
    draft_notify_on_error: bool,
    draft_notify_on_engine_events: bool,
    draft_os_notifications: bool,
    draft_notify_on_low_disk: bool,
    draft_close_behavior: CloseBehaviorView,
    draft_show_tray_icon: bool,
    draft_start_minimized_to_tray: bool,
    /// Working copy of download categories (C1); saved with Directory/General.
    draft_categories: Vec<crate::DownloadCategoryView>,
    draft_default_category_id: Option<String>,
    clear_proxy_password: bool,
    /// Profile id currently open in the inline editor (Settings → Profiles).
    editing_profile_id: Option<String>,
    draft_profile_kind: ProfileKindView,
    /// Pending remote RPC secret mutations (keyed by draft profile_id).
    profile_secret_updates: std::collections::HashMap<String, ProfileRpcSecretUpdateView>,
    /// Pending profile delete confirmation.
    pending_profile_delete: Option<PendingProfileDelete>,
    clear_profile_rpc_secret: bool,
    error: Option<OperationErrorView>,
}

pub(crate) struct PendingProfileDelete {
    profile_id: String,
    name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PathPickTarget {
    DownloadDirectory,
    CoreExecutable,
    ProfileExecutable,
    ProfileDownloadDirectory,
    /// Browse path for a draft category by index.
    CategoryDirectory {
        index: usize,
    },
}

/// Top-level navigation categories for the settings two-pane layout.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum SettingsCategory {
    /// Appearance (theme) + Downloads (default directory).
    #[default]
    General,
    /// Multi-profile catalog management.
    Profiles,
    /// Managed aria2 core registry.
    Engine,
    /// Download proxy configuration.
    Network,
    /// Speed limits + transfer policy.
    Transfers,
    /// Notification volume and categories.
    Notifications,
    /// Window close behavior and system tray.
    System,
    /// Application and connected engine information (read-only).
    About,
}

impl SettingsCategory {
    const ALL: [Self; 8] = [
        Self::General,
        Self::Profiles,
        Self::Engine,
        Self::Network,
        Self::Transfers,
        Self::Notifications,
        Self::System,
        Self::About,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Profiles => "Profiles",
            Self::Engine => "Engine",
            Self::Network => "Network",
            Self::Transfers => "Transfers",
            Self::Notifications => "Notifications",
            Self::System => "System",
            Self::About => "About",
        }
    }

    const fn message_key(self) -> &'static str {
        match self {
            Self::General => "settings-nav-general",
            Self::Profiles => "settings-nav-profiles",
            Self::Engine => "settings-nav-engine",
            Self::Network => "settings-nav-network",
            Self::Transfers => "settings-nav-transfers",
            Self::Notifications => "settings-nav-notifications",
            Self::System => "settings-nav-system",
            Self::About => "settings-nav-about",
        }
    }

    const fn icon(self) -> crate::IconName {
        match self {
            Self::General => crate::IconName::Sun,
            Self::Profiles => crate::IconName::List,
            Self::Engine => crate::IconName::Activity,
            Self::Network => crate::IconName::Wifi,
            Self::Transfers => crate::IconName::ArrowDown,
            Self::Notifications => crate::IconName::Info,
            Self::System => crate::IconName::Settings,
            Self::About => crate::IconName::CircleHelp,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SettingsSaveSource {
    Theme,
    Language,
    Directory,
    Proxy,
    SpeedLimit,
    TransferPolicy,
    /// Combined speed limits + transfer policy (Settings → Transfers footer).
    Transfers,
    Notifications,
    Platform,
    Import,
}

pub(crate) struct PendingSettingsSave {
    request_id: RequestId,
    settings: SettingsView,
    source: SettingsSaveSource,
}

struct RemoveConfirmation {
    identities: Vec<TaskIdentity>,
    display_name: String,
    has_live_tasks: bool,
    has_terminal_tasks: bool,
    delete_files: bool,
    previous_focus: Option<WeakFocusHandle>,
}

struct TaskOutputNameDialog {
    identity: TaskIdentity,
    display_name: String,
    active: bool,
    previous_focus: Option<WeakFocusHandle>,
    error: Option<OperationErrorView>,
}

struct TaskSpeedLimitDialog {
    identity: TaskIdentity,
    display_name: String,
    previous_focus: Option<WeakFocusHandle>,
    error: Option<OperationErrorView>,
}

struct TaskOptionsDialog {
    identity: TaskIdentity,
    display_name: String,
    supports_seed_rules: bool,
    previous_focus: Option<WeakFocusHandle>,
    error: Option<OperationErrorView>,
}

/// Right-click menu for one focused task row (D-024).
struct TextFieldContextMenuState {
    field: Entity<TextField>,
    position: Point<Pixels>,
}

struct TaskContextMenu {
    identity: TaskIdentity,
    position: Point<Pixels>,
}

struct BatchFailureDetails {
    command: BatchTaskCommandView,
    failures: Vec<BatchTaskFailureView>,
    previous_focus: Option<WeakFocusHandle>,
}

/// Settings-page TextField entities, grouped for navigation and theme updates.
pub(crate) struct SettingsInputs {
    pub(crate) directory: Entity<TextField>,
    pub(crate) core_path: Entity<TextField>,
    pub(crate) profile_name: Entity<TextField>,
    pub(crate) profile_executable: Entity<TextField>,
    pub(crate) profile_endpoint: Entity<TextField>,
    pub(crate) profile_download: Entity<TextField>,
    pub(crate) profile_secret: Entity<TextField>,
    pub(crate) all_proxy: Entity<TextField>,
    pub(crate) http_proxy: Entity<TextField>,
    pub(crate) https_proxy: Entity<TextField>,
    pub(crate) ftp_proxy: Entity<TextField>,
    pub(crate) no_proxy: Entity<TextField>,
    pub(crate) proxy_username: Entity<TextField>,
    pub(crate) proxy_password: Entity<TextField>,
    pub(crate) download_limit: Entity<TextField>,
    pub(crate) upload_limit: Entity<TextField>,
    pub(crate) max_concurrent: Entity<TextField>,
    pub(crate) max_connection: Entity<TextField>,
    pub(crate) split: Entity<TextField>,
    pub(crate) min_split_size: Entity<TextField>,
}

impl SettingsInputs {
    pub(crate) fn all(&self) -> [&Entity<TextField>; 20] {
        [
            &self.directory,
            &self.core_path,
            &self.profile_name,
            &self.profile_executable,
            &self.profile_endpoint,
            &self.profile_download,
            &self.profile_secret,
            &self.all_proxy,
            &self.http_proxy,
            &self.https_proxy,
            &self.ftp_proxy,
            &self.no_proxy,
            &self.proxy_username,
            &self.proxy_password,
            &self.download_limit,
            &self.upload_limit,
            &self.max_concurrent,
            &self.max_connection,
            &self.split,
            &self.min_split_size,
        ]
    }
}

/// Add-download advanced option TextField entities.
pub(crate) struct AddInputs {
    pub(crate) referer: Entity<TextField>,
    pub(crate) user_agent: Entity<TextField>,
    pub(crate) headers: Entity<TextField>,
    pub(crate) cookie: Entity<TextField>,
    pub(crate) http_user: Entity<TextField>,
    pub(crate) http_passwd: Entity<TextField>,
    pub(crate) checksum: Entity<TextField>,
}

impl AddInputs {
    pub(crate) fn all(&self) -> [&Entity<TextField>; 7] {
        [
            &self.referer,
            &self.user_agent,
            &self.headers,
            &self.cookie,
            &self.http_user,
            &self.http_passwd,
            &self.checksum,
        ]
    }
}

/// Per-task dialog TextField entities.
pub(crate) struct TaskInputs {
    pub(crate) download_limit: Entity<TextField>,
    pub(crate) upload_limit: Entity<TextField>,
    pub(crate) seed_ratio: Entity<TextField>,
    pub(crate) seed_time: Entity<TextField>,
}

impl TaskInputs {
    pub(crate) fn all(&self) -> [&Entity<TextField>; 4] {
        [
            &self.download_limit,
            &self.upload_limit,
            &self.seed_ratio,
            &self.seed_time,
        ]
    }
}

pub struct AppShell {
    theme: Theme,
    /// Active Fluent translator for the resolved display language.
    translator: Translator,
    settings: SettingsView,
    profiles: ProfileCatalogView,
    cores: CoreRegistryView,
    page: AppPage,
    engine_health: EngineHealthView,
    snapshot: WorkspaceSnapshot,
    query: WorkspaceQuery,
    selected: Option<TaskIdentity>,
    selected_tasks: HashSet<TaskIdentity>,
    range_anchor: Option<TaskIdentity>,
    search_input: Entity<TextField>,
    add_input: Entity<TextField>,
    add_inputs: AddInputs,
    output_name_input: Entity<TextField>,
    settings_inputs: SettingsInputs,
    add_dialog: AddDownloadDialog,
    add_dialog_focus: FocusHandle,
    add_cancel_focus: FocusHandle,
    add_submit_focus: FocusHandle,
    settings_page: SettingsPage,
    pending_settings_save: Option<PendingSettingsSave>,
    pending_task_command: Option<PendingTaskCommand>,
    pending_global_task_command: Option<PendingGlobalTaskCommand>,
    pending_batch_command: Option<PendingBatchTaskCommand>,
    pending_load_more_stopped: bool,
    batch_failure_details: Option<BatchFailureDetails>,
    batch_failure_dialog_focus: FocusHandle,
    batch_failure_close_focus: FocusHandle,
    output_name_dialog: Option<TaskOutputNameDialog>,
    output_name_dialog_focus: FocusHandle,
    output_name_cancel_focus: FocusHandle,
    output_name_submit_focus: FocusHandle,
    task_inputs: TaskInputs,
    task_speed_limit_dialog: Option<TaskSpeedLimitDialog>,
    task_speed_limit_dialog_focus: FocusHandle,
    task_speed_limit_cancel_focus: FocusHandle,
    task_speed_limit_submit_focus: FocusHandle,
    task_options_dialog: Option<TaskOptionsDialog>,
    task_options_dialog_focus: FocusHandle,
    task_options_cancel_focus: FocusHandle,
    task_options_submit_focus: FocusHandle,
    details_drawer: Option<TaskDetailsDrawer>,
    remove_confirmation: Option<RemoveConfirmation>,
    remove_dialog_focus: FocusHandle,
    remove_cancel_focus: FocusHandle,
    remove_submit_focus: FocusHandle,
    speed_popover_open: bool,
    speed_popover_previous_focus: Option<WeakFocusHandle>,
    sort_popover_open: bool,
    context_menu: Option<TaskContextMenu>,
    text_field_context_menu: Option<TextFieldContextMenuState>,
    status_notice: Option<StatusNotice>,
    next_notice_id: u64,
    activity_log: Vec<ActivityEntryView>,
    next_activity_id: u64,
    activity_panel_open: bool,
    /// Suppresses repeated low-disk warnings until free space recovers.
    low_disk_active: bool,
    known_task_status: HashMap<TaskIdentity, TaskStatusView>,
    next_request_id: u64,
    list_scroll: UniformListScrollHandle,
    settings_scroll: ScrollHandle,
    metadata_file_scroll: UniformListScrollHandle,
    focus_handle: FocusHandle,
    rendered_range: Range<usize>,
    _search_subscription: Subscription,
    _add_subscription: Subscription,
    _add_advanced_subscriptions: [Subscription; 7],
    _output_name_subscription: Subscription,
    _task_speed_limit_subscriptions: [Subscription; 2],
    _task_options_subscriptions: [Subscription; 2],
    _settings_subscriptions: Vec<Subscription>,
    _window_bounds_subscription: Subscription,
}

impl gpui::EventEmitter<AppShellEvent> for AppShell {}

impl AppShell {
    #[must_use]
    pub fn new(theme: Theme, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let color_scheme = match theme.mode {
            ThemeMode::System => ColorSchemeView::System,
            ThemeMode::Light => ColorSchemeView::Light,
            ThemeMode::Dark => ColorSchemeView::Dark,
        };
        Self::new_inner(
            theme,
            SettingsView {
                color_scheme,
                download_directory: String::new(),
                ..SettingsView::default()
            },
            window,
            cx,
        )
    }

    #[must_use]
    pub fn new_with_settings(
        settings: SettingsView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_inner(
            resolve_theme(settings.color_scheme, window),
            settings,
            window,
            cx,
        )
    }

    fn new_inner(
        theme: Theme,
        settings: SettingsView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search_input = cx.new(|cx| TextField::new("Search downloads or GID", theme, cx));
        let search_subscription = cx.subscribe(
            &search_input,
            |this: &mut Self, input, event: &SearchInputEvent, cx| {
                this.handle_text_field_event(input, event, cx);
                if let SearchInputEvent::TextChanged { text, .. } = event
                    && this.query.search != *text
                {
                    this.query.search.clone_from(text);
                    this.sort_popover_open = false;
                    this.clear_task_selection();
                    this.emit_query(cx);
                }
            },
        );
        let add_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "add-download-uri".into(),
                    key_context: "AddDownloadInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Download URLs or magnet links".into(),
                    placeholder: "Paste one or more URLs".into(),
                    leading_icon: Some(IconName::Link),
                    clearable: true,
                    allow_newlines: true,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let add_subscription = cx.subscribe(
            &add_input,
            |this: &mut Self, input, event: &SearchInputEvent, cx| {
                this.handle_text_field_event(input, event, cx);
                if let SearchInputEvent::TextChanged { .. } = event
                    && this.add_dialog.open
                    && this.add_dialog.pending.is_none()
                {
                    if this.add_dialog.updating_input_from_result {
                        this.add_dialog.updating_input_from_result = false;
                        return;
                    }
                    let changed = this.add_dialog.error.take().is_some()
                        || !this.add_dialog.results.is_empty();
                    this.add_dialog.results.clear();
                    if changed {
                        cx.notify();
                    }
                }
            },
        );
        let add_referer_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "add-download-referer".into(),
                    key_context: "AddDownloadAdvancedInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Download referer".into(),
                    placeholder: "https://example.test/page".into(),
                    leading_icon: Some(IconName::Link),
                    clearable: true,
                    allow_newlines: false,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let add_user_agent_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "add-download-user-agent".into(),
                    key_context: "AddDownloadAdvancedInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Download user agent".into(),
                    placeholder: "Optional User-Agent".into(),
                    leading_icon: Some(IconName::Info),
                    clearable: true,
                    allow_newlines: false,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let add_headers_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "add-download-headers".into(),
                    key_context: "AddDownloadAdvancedInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Custom download headers".into(),
                    placeholder: "One Name: value header per line".into(),
                    leading_icon: Some(IconName::List),
                    clearable: true,
                    allow_newlines: true,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let add_cookie_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "add-download-cookie".into(),
                    key_context: "AddDownloadAdvancedInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Download cookie".into(),
                    placeholder: "session=…".into(),
                    leading_icon: Some(IconName::Info),
                    clearable: true,
                    allow_newlines: false,
                    secure: true,
                },
                theme,
                cx,
            )
        });
        let add_http_user_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "add-download-http-user".into(),
                    key_context: "AddDownloadAdvancedInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "HTTP authentication username".into(),
                    placeholder: "username".into(),
                    leading_icon: Some(IconName::Pencil),
                    clearable: true,
                    allow_newlines: false,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let add_http_passwd_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "add-download-http-passwd".into(),
                    key_context: "AddDownloadAdvancedInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "HTTP authentication password".into(),
                    placeholder: "password".into(),
                    leading_icon: Some(IconName::CircleAlert),
                    clearable: true,
                    allow_newlines: false,
                    secure: true,
                },
                theme,
                cx,
            )
        });
        let add_checksum_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "add-download-checksum".into(),
                    key_context: "AddDownloadAdvancedInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Download checksum".into(),
                    placeholder: "sha-256=…".into(),
                    leading_icon: Some(IconName::ScanSearch),
                    clearable: true,
                    allow_newlines: false,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let add_advanced_subscriptions = [
            &add_referer_input,
            &add_user_agent_input,
            &add_headers_input,
            &add_cookie_input,
            &add_http_user_input,
            &add_http_passwd_input,
            &add_checksum_input,
        ]
        .map(|input| {
            cx.subscribe(
                input,
                |this: &mut Self, input, event: &SearchInputEvent, cx| {
                    this.handle_text_field_event(input, event, cx);
                    let SearchInputEvent::TextChanged { .. } = event else {
                        return;
                    };
                    if this.add_dialog.open
                        && this.add_dialog.pending.is_none()
                        && this.add_dialog.error.take().is_some()
                    {
                        cx.notify();
                    }
                },
            )
        });
        let output_name_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "task-output-name".into(),
                    key_context: "OutputNameInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Task output filename".into(),
                    placeholder: "archive.iso".into(),
                    leading_icon: Some(IconName::Pencil),
                    clearable: true,
                    allow_newlines: false,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let output_name_subscription = cx.subscribe(
            &output_name_input,
            |this: &mut Self, input, event: &SearchInputEvent, cx| {
                this.handle_text_field_event(input, event, cx);
                let SearchInputEvent::TextChanged { .. } = event else {
                    return;
                };
                if let Some(dialog) = &mut this.output_name_dialog
                    && this.pending_task_command.is_none()
                    && dialog.error.take().is_some()
                {
                    cx.notify();
                }
            },
        );
        let task_download_limit_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "task-download-limit".into(),
                    key_context: "TaskSpeedLimitInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Task maximum download speed".into(),
                    placeholder: "Unlimited (e.g. 2M, 512K)".into(),
                    leading_icon: Some(IconName::ArrowDown),
                    clearable: true,
                    allow_newlines: false,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let task_upload_limit_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "task-upload-limit".into(),
                    key_context: "TaskSpeedLimitInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Task maximum upload speed".into(),
                    placeholder: "Unlimited (e.g. 1M, 256K)".into(),
                    leading_icon: Some(IconName::ArrowUp),
                    clearable: true,
                    allow_newlines: false,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let task_speed_limit_subscriptions = [&task_download_limit_input, &task_upload_limit_input]
            .map(|input| {
                cx.subscribe(
                    input,
                    |this: &mut Self, input, event: &SearchInputEvent, cx| {
                        this.handle_text_field_event(input, event, cx);
                        let SearchInputEvent::TextChanged { .. } = event else {
                            return;
                        };
                        if let Some(dialog) = &mut this.task_speed_limit_dialog
                            && this.pending_task_command.is_none()
                            && dialog.error.take().is_some()
                        {
                            cx.notify();
                        }
                    },
                )
            });
        let task_seed_ratio_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "task-seed-ratio".into(),
                    key_context: "TaskOptionsInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "BitTorrent seed ratio".into(),
                    placeholder: "e.g. 1.0 or 0 to disable".into(),
                    leading_icon: Some(IconName::Activity),
                    clearable: true,
                    allow_newlines: false,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let task_seed_time_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "task-seed-time".into(),
                    key_context: "TaskOptionsInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "BitTorrent seed time in minutes".into(),
                    placeholder: "Minutes (e.g. 60)".into(),
                    leading_icon: Some(IconName::Clock3),
                    clearable: true,
                    allow_newlines: false,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let task_options_subscriptions =
            [&task_seed_ratio_input, &task_seed_time_input].map(|input| {
                cx.subscribe(
                    input,
                    |this: &mut Self, input, event: &SearchInputEvent, cx| {
                        this.handle_text_field_event(input, event, cx);
                        let SearchInputEvent::TextChanged { .. } = event else {
                            return;
                        };
                        if let Some(dialog) = &mut this.task_options_dialog
                            && this.pending_task_command.is_none()
                            && dialog.error.take().is_some()
                        {
                            cx.notify();
                        }
                    },
                )
            });
        let settings_directory_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "settings-download-directory".into(),
                    key_context: "SettingsDirectoryInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Default download directory".into(),
                    placeholder: "D:\\Downloads".into(),
                    leading_icon: Some(IconName::FolderDown),
                    clearable: true,
                    allow_newlines: false,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let settings_core_path_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "settings-core-path".into(),
                    key_context: "SettingsCorePathInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Path to aria2c executable to import or link".into(),
                    placeholder: if cfg!(windows) {
                        "C:\\path\\to\\aria2c.exe"
                    } else {
                        "/usr/bin/aria2c"
                    }
                    .into(),
                    leading_icon: Some(IconName::FolderDown),
                    clearable: true,
                    allow_newlines: false,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let settings_profile_name_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "settings-profile-name".into(),
                    key_context: "SettingsProfileNameInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Profile display name".into(),
                    placeholder: "Local aria2".into(),
                    leading_icon: Some(IconName::Pencil),
                    clearable: true,
                    allow_newlines: false,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let settings_profile_executable_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "settings-profile-executable".into(),
                    key_context: "SettingsProfileExecutableInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Optional pinned aria2c path for this local profile"
                        .into(),
                    placeholder: "Leave empty to use the active managed core".into(),
                    leading_icon: Some(IconName::FolderDown),
                    clearable: true,
                    allow_newlines: false,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let settings_profile_endpoint_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "settings-profile-endpoint".into(),
                    key_context: "SettingsProfileEndpointInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Remote aria2 WebSocket endpoint".into(),
                    placeholder: "wss://host:6800/jsonrpc".into(),
                    leading_icon: Some(IconName::Link),
                    clearable: true,
                    allow_newlines: false,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let settings_profile_download_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "settings-profile-download".into(),
                    key_context: "SettingsProfileDownloadInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Default download directory for this profile".into(),
                    placeholder: "D:\\Downloads".into(),
                    leading_icon: Some(IconName::FolderDown),
                    clearable: true,
                    allow_newlines: false,
                    secure: false,
                },
                theme,
                cx,
            )
        });
        let settings_profile_secret_input = cx.new(|cx| {
            TextField::new_with_config(
                TextFieldConfig {
                    element_id: "settings-profile-secret".into(),
                    key_context: "SettingsProfileSecretInput".into(),
                    role: Role::TextInput,
                    accessibility_label: "Remote aria2 RPC secret".into(),
                    placeholder: "Leave blank to keep the saved secret".into(),
                    leading_icon: Some(IconName::Wifi),
                    clearable: true,
                    allow_newlines: false,
                    secure: true,
                },
                theme,
                cx,
            )
        });
        let settings_all_proxy_input = cx.new(|cx| {
            TextField::new_with_config(
                settings_input_config(
                    "settings-all-proxy",
                    "All-protocol proxy",
                    "http://proxy.example:8080",
                    Some(IconName::Wifi),
                    false,
                ),
                theme,
                cx,
            )
        });
        let settings_http_proxy_input = cx.new(|cx| {
            TextField::new_with_config(
                settings_input_config(
                    "settings-http-proxy",
                    "HTTP proxy",
                    "http://proxy.example:8080",
                    Some(IconName::Link),
                    false,
                ),
                theme,
                cx,
            )
        });
        let settings_https_proxy_input = cx.new(|cx| {
            TextField::new_with_config(
                settings_input_config(
                    "settings-https-proxy",
                    "HTTPS proxy",
                    "http://proxy.example:8080",
                    Some(IconName::Link),
                    false,
                ),
                theme,
                cx,
            )
        });
        let settings_ftp_proxy_input = cx.new(|cx| {
            TextField::new_with_config(
                settings_input_config(
                    "settings-ftp-proxy",
                    "FTP proxy",
                    "http://proxy.example:8080",
                    Some(IconName::Link),
                    false,
                ),
                theme,
                cx,
            )
        });
        let settings_no_proxy_input = cx.new(|cx| {
            TextField::new_with_config(
                settings_input_config(
                    "settings-no-proxy",
                    "Hosts that bypass the proxy",
                    "localhost, 127.0.0.1, 10.0.0.0/8",
                    Some(IconName::WifiOff),
                    false,
                ),
                theme,
                cx,
            )
        });
        let settings_proxy_username_input = cx.new(|cx| {
            TextField::new_with_config(
                settings_input_config(
                    "settings-proxy-username",
                    "Proxy username",
                    "Optional username",
                    None,
                    false,
                ),
                theme,
                cx,
            )
        });
        let settings_proxy_password_input = cx.new(|cx| {
            TextField::new_with_config(
                settings_input_config(
                    "settings-proxy-password",
                    "Proxy password",
                    "Optional password or replacement",
                    None,
                    true,
                ),
                theme,
                cx,
            )
        });
        let settings_download_limit_input = cx.new(|cx| {
            TextField::new_with_config(
                settings_input_config(
                    "settings-download-limit",
                    "Maximum download speed",
                    "Unlimited (e.g. 2M, 512K)",
                    Some(IconName::ArrowDown),
                    false,
                ),
                theme,
                cx,
            )
        });
        let settings_upload_limit_input = cx.new(|cx| {
            TextField::new_with_config(
                settings_input_config(
                    "settings-upload-limit",
                    "Maximum upload speed",
                    "Unlimited (e.g. 1M, 256K)",
                    Some(IconName::ArrowUp),
                    false,
                ),
                theme,
                cx,
            )
        });
        let settings_max_concurrent_input = cx.new(|cx| {
            TextField::new_with_config(
                settings_input_config(
                    "settings-max-concurrent",
                    "Maximum concurrent downloads",
                    "5",
                    None,
                    false,
                ),
                theme,
                cx,
            )
        });
        let settings_max_connection_input = cx.new(|cx| {
            TextField::new_with_config(
                settings_input_config(
                    "settings-max-connection",
                    "Maximum connections per server",
                    "1-16",
                    None,
                    false,
                ),
                theme,
                cx,
            )
        });
        let settings_split_input = cx.new(|cx| {
            TextField::new_with_config(
                settings_input_config("settings-split", "Split count", "5", None, false),
                theme,
                cx,
            )
        });
        let settings_min_split_size_input = cx.new(|cx| {
            TextField::new_with_config(
                settings_input_config(
                    "settings-min-split-size",
                    "Minimum split size",
                    "20M",
                    None,
                    false,
                ),
                theme,
                cx,
            )
        });
        let mut settings_subscriptions = [
            &settings_directory_input,
            &settings_core_path_input,
            &settings_profile_name_input,
            &settings_profile_executable_input,
            &settings_profile_endpoint_input,
            &settings_profile_download_input,
            &settings_profile_secret_input,
            &settings_all_proxy_input,
            &settings_http_proxy_input,
            &settings_https_proxy_input,
            &settings_ftp_proxy_input,
            &settings_no_proxy_input,
            &settings_proxy_username_input,
            &settings_download_limit_input,
            &settings_upload_limit_input,
            &settings_max_concurrent_input,
            &settings_max_connection_input,
            &settings_split_input,
            &settings_min_split_size_input,
        ]
        .into_iter()
        .map(|input| {
            cx.subscribe(
                input,
                |this: &mut Self, input, event: &SearchInputEvent, cx| {
                    this.handle_text_field_event(input, event, cx);
                    let SearchInputEvent::TextChanged { .. } = event else {
                        return;
                    };
                    if this.page == AppPage::Settings
                        && this.pending_settings_save.is_none()
                        && this.settings_page.error.take().is_some()
                    {
                        cx.notify();
                    }
                },
            )
        })
        .collect::<Vec<_>>();
        settings_subscriptions.push(cx.subscribe(
            &settings_proxy_password_input,
            |this: &mut Self, input, event: &SearchInputEvent, cx| {
                this.handle_text_field_event(input, event, cx);
                let SearchInputEvent::TextChanged { .. } = event else {
                    return;
                };
                let changed = this.settings_page.clear_proxy_password
                    || (this.page == AppPage::Settings
                        && this.pending_settings_save.is_none()
                        && this.settings_page.error.is_some());
                this.settings_page.clear_proxy_password = false;
                this.settings_page.error = None;
                if changed {
                    cx.notify();
                }
            },
        ));
        let window_bounds_subscription = cx.observe_window_bounds(window, |this, window, cx| {
            this.emit_window_geometry(window, cx);
            cx.notify();
        });
        let _window_appearance_subscription =
            cx.observe_window_appearance(window, |this, window, cx| {
                if this.settings.color_scheme == ColorSchemeView::System {
                    let theme = resolve_theme(ColorSchemeView::System, window);
                    if this.theme.mode != theme.mode || this.theme.colors != theme.colors {
                        this.theme = theme;
                        this.apply_theme_to_text_fields(cx);
                        cx.notify();
                    }
                }
            });
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        Self {
            theme,
            translator: translator_for_language(settings.language),
            settings,
            profiles: ProfileCatalogView::default(),
            cores: CoreRegistryView::default(),
            page: AppPage::Downloads,
            engine_health: EngineHealthView::External,
            snapshot: WorkspaceSnapshot::default(),
            query: WorkspaceQuery::default(),
            selected: None,
            selected_tasks: HashSet::new(),
            range_anchor: None,
            search_input,
            add_input,
            add_inputs: AddInputs {
                referer: add_referer_input,
                user_agent: add_user_agent_input,
                headers: add_headers_input,
                cookie: add_cookie_input,
                http_user: add_http_user_input,
                http_passwd: add_http_passwd_input,
                checksum: add_checksum_input,
            },
            output_name_input,
            settings_inputs: SettingsInputs {
                directory: settings_directory_input,
                core_path: settings_core_path_input,
                profile_name: settings_profile_name_input,
                profile_executable: settings_profile_executable_input,
                profile_endpoint: settings_profile_endpoint_input,
                profile_download: settings_profile_download_input,
                profile_secret: settings_profile_secret_input,
                all_proxy: settings_all_proxy_input,
                http_proxy: settings_http_proxy_input,
                https_proxy: settings_https_proxy_input,
                ftp_proxy: settings_ftp_proxy_input,
                no_proxy: settings_no_proxy_input,
                proxy_username: settings_proxy_username_input,
                proxy_password: settings_proxy_password_input,
                download_limit: settings_download_limit_input,
                upload_limit: settings_upload_limit_input,
                max_concurrent: settings_max_concurrent_input,
                max_connection: settings_max_connection_input,
                split: settings_split_input,
                min_split_size: settings_min_split_size_input,
            },
            add_dialog: AddDownloadDialog::default(),
            add_dialog_focus: cx.focus_handle(),
            add_cancel_focus: cx.focus_handle().tab_stop(true),
            add_submit_focus: cx.focus_handle().tab_stop(true),
            settings_page: SettingsPage::default(),
            pending_settings_save: None,
            pending_task_command: None,
            pending_global_task_command: None,
            pending_batch_command: None,
            pending_load_more_stopped: false,
            batch_failure_details: None,
            batch_failure_dialog_focus: cx.focus_handle(),
            batch_failure_close_focus: cx.focus_handle().tab_stop(true),
            output_name_dialog: None,
            output_name_dialog_focus: cx.focus_handle(),
            output_name_cancel_focus: cx.focus_handle().tab_stop(true),
            output_name_submit_focus: cx.focus_handle().tab_stop(true),
            task_inputs: TaskInputs {
                download_limit: task_download_limit_input,
                upload_limit: task_upload_limit_input,
                seed_ratio: task_seed_ratio_input,
                seed_time: task_seed_time_input,
            },
            task_speed_limit_dialog: None,
            task_speed_limit_dialog_focus: cx.focus_handle(),
            task_speed_limit_cancel_focus: cx.focus_handle().tab_stop(true),
            task_speed_limit_submit_focus: cx.focus_handle().tab_stop(true),

            task_options_dialog: None,
            task_options_dialog_focus: cx.focus_handle(),
            task_options_cancel_focus: cx.focus_handle().tab_stop(true),
            task_options_submit_focus: cx.focus_handle().tab_stop(true),
            details_drawer: None,
            remove_confirmation: None,
            remove_dialog_focus: cx.focus_handle(),
            remove_cancel_focus: cx.focus_handle().tab_stop(true),
            remove_submit_focus: cx.focus_handle().tab_stop(true),
            speed_popover_open: false,
            speed_popover_previous_focus: None,
            sort_popover_open: false,
            context_menu: None,
            text_field_context_menu: None,
            status_notice: None,
            next_notice_id: 1,
            activity_log: Vec::new(),
            next_activity_id: 1,
            activity_panel_open: false,
            low_disk_active: false,
            known_task_status: HashMap::new(),
            next_request_id: 1,
            list_scroll: UniformListScrollHandle::new(),
            settings_scroll: ScrollHandle::new(),
            metadata_file_scroll: UniformListScrollHandle::new(),
            focus_handle,
            rendered_range: 0..0,
            _search_subscription: search_subscription,
            _add_subscription: add_subscription,
            _add_advanced_subscriptions: add_advanced_subscriptions,
            _output_name_subscription: output_name_subscription,
            _task_speed_limit_subscriptions: task_speed_limit_subscriptions,
            _task_options_subscriptions: task_options_subscriptions,
            _settings_subscriptions: settings_subscriptions,
            _window_bounds_subscription: window_bounds_subscription,
        }
    }

    pub fn set_snapshot(&mut self, snapshot: WorkspaceSnapshot, cx: &mut Context<Self>) {
        // PERF-001: when session identity is stable *and* the task list content is
        // unchanged, only connection/rates/history/counts may move — skip selection
        // migration, status transition notices, and details rewrite. Task equality is
        // required so status/gid successor updates never take this path even if a
        // producer forgets to bump `source_revision`.
        if self.snapshot.source_revision == snapshot.source_revision
            && self.snapshot.generation == snapshot.generation
            && self.snapshot.profile_id == snapshot.profile_id
            && self.snapshot.session_id == snapshot.session_id
            && self.snapshot.tasks == snapshot.tasks
        {
            let light_changed = self.snapshot.connection != snapshot.connection
                || self.snapshot.stale != snapshot.stale
                || self.snapshot.download_rate != snapshot.download_rate
                || self.snapshot.upload_rate != snapshot.upload_rate
                || self.snapshot.speed_history != snapshot.speed_history
                || self.snapshot.counts != snapshot.counts
                || self.snapshot.stopped_history != snapshot.stopped_history
                || self.snapshot.capabilities != snapshot.capabilities
                || self.snapshot.local_path_actions_available
                    != snapshot.local_path_actions_available;
            if !light_changed {
                return;
            }
            self.snapshot.connection = snapshot.connection;
            self.snapshot.stale = snapshot.stale;
            self.snapshot.download_rate = snapshot.download_rate;
            self.snapshot.upload_rate = snapshot.upload_rate;
            self.snapshot.speed_history = snapshot.speed_history;
            self.snapshot.counts = snapshot.counts;
            self.snapshot.stopped_history = snapshot.stopped_history;
            self.snapshot.capabilities = snapshot.capabilities;
            self.snapshot.local_path_actions_available = snapshot.local_path_actions_available;
            cx.notify();
            return;
        }
        let previous_session = self.snapshot.engine_session();
        let previous_commands_available = self.snapshot.commands_available();
        let next_session = snapshot.engine_session();
        let session_changed = previous_session != next_session;
        let profile_changed = self
            .selected
            .as_ref()
            .is_some_and(|selected| selected.profile_id != snapshot.profile_id);
        let selected_before = self.selected.clone();
        let selected_successor = (!profile_changed)
            .then(|| {
                selected_before
                    .as_ref()
                    .and_then(|selected| successor_task(&self.snapshot, &snapshot, selected))
            })
            .flatten();
        let selection_migrations = if profile_changed {
            Vec::new()
        } else {
            self.selected_tasks
                .iter()
                .filter_map(|identity| {
                    successor_task(&self.snapshot, &snapshot, identity)
                        .map(|successor| (identity.clone(), successor.identity))
                })
                .collect::<Vec<_>>()
        };
        let anchor_successor = (!profile_changed)
            .then(|| {
                self.range_anchor
                    .as_ref()
                    .and_then(|anchor| successor_task(&self.snapshot, &snapshot, anchor))
            })
            .flatten();
        let drawer_successor = (!profile_changed)
            .then(|| {
                self.details_drawer
                    .as_ref()
                    .and_then(|drawer| successor_task(&self.snapshot, &snapshot, &drawer.identity))
            })
            .flatten();
        let details_revision_advanced = self.details_drawer.as_ref().is_some_and(|drawer| {
            snapshot
                .tasks
                .iter()
                .find(|task| task.identity == drawer.identity)
                .is_some_and(|task| task.revision > drawer.overview.revision)
        });

        if profile_changed {
            self.selected = None;
            self.selected_tasks.clear();
            self.range_anchor = None;
            self.details_drawer = None;
            self.batch_failure_details = None;
        }

        if session_changed {
            if self.add_dialog.pending.take().is_some() {
                self.add_dialog.error = Some(stale_session_error());
            }
            if let Some(pending) = self.pending_task_command.take() {
                if matches!(&pending.command, TaskCommandView::SetOutputName { .. }) {
                    if let Some(dialog) = &mut self.output_name_dialog {
                        dialog.error = Some(stale_session_error());
                    }
                } else {
                    self.show_notice(
                        "The engine session changed before the command completed. Its outcome was not replayed.",
                        true,
                        cx,
                    );
                }
            }
            if self.pending_batch_command.take().is_some() {
                self.show_notice(
                    "The engine session changed before the batch command completed. Its outcome was not replayed.",
                    true,
                    cx,
                );
            }
            if self.pending_load_more_stopped {
                self.pending_load_more_stopped = false;
            }
            if let (Some(drawer), Some(session)) = (&mut self.details_drawer, &next_session) {
                drawer.session = session.clone();
                drawer.state = TaskDetailsLoadState::Stale;
                drawer.pending = None;
            }
        }

        let previous_snapshot = std::mem::replace(&mut self.snapshot, snapshot);
        let followed_task = selected_successor.is_some() || drawer_successor.is_some();
        self.observe_task_status_transitions(&previous_snapshot, cx);

        for (previous, successor) in selection_migrations {
            self.selected_tasks.remove(&previous);
            self.selected_tasks.insert(successor);
        }

        if let Some(successor) = selected_successor {
            self.selected = Some(successor.identity.clone());
        }
        if let Some(successor) = anchor_successor {
            self.range_anchor = Some(successor.identity);
        }
        if let (Some(drawer), Some(successor)) = (&mut self.details_drawer, drawer_successor) {
            drawer.identity = successor.identity.clone();
            drawer.overview = successor;
            drawer.state = TaskDetailsLoadState::Stale;
            drawer.pending = None;
        }

        if self.selected.as_ref().is_none_or(|selected| {
            !self
                .snapshot
                .tasks
                .iter()
                .any(|task| &task.identity == selected)
        }) && let Some(visible_selected) = self
            .snapshot
            .tasks
            .iter()
            .find(|task| self.selected_tasks.contains(&task.identity))
        {
            self.selected = Some(visible_selected.identity.clone());
        }

        if let Some(drawer) = &mut self.details_drawer {
            if let Some(task) = self
                .snapshot
                .tasks
                .iter()
                .find(|task| task.identity == drawer.identity)
            {
                let left_active_state = drawer.overview.status.uses_active_connections()
                    && !task.status.uses_active_connections();
                drawer.overview = task.clone();
                if left_active_state
                    && let TaskDetailsLoadState::Ready { details } = &mut drawer.state
                {
                    details.peers.clear();
                    details.servers.clear();
                }
            }
            if !self.snapshot.commands_available() {
                drawer.state = TaskDetailsLoadState::Stale;
                drawer.pending = None;
            }
        }

        let force_details_refresh =
            followed_task || session_changed || !previous_commands_available;
        let should_refresh_details = self.details_drawer.is_some()
            && self.snapshot.commands_available()
            && (force_details_refresh || details_revision_advanced);
        if should_refresh_details {
            // Revision-only refreshes are rate-limited; session/follow always run.
            self.request_current_details(!force_details_refresh && details_revision_advanced, cx);
        }
        cx.notify();
    }

    pub fn set_engine_health(&mut self, health: EngineHealthView, cx: &mut Context<Self>) {
        if self.engine_health == health {
            return;
        }
        self.engine_health = health;
        match &self.engine_health {
            EngineHealthView::Running { restarts } if *restarts > 0 => {
                let message = format!(
                    "Local aria2 recovered after {restarts} restart attempt{}.",
                    if *restarts == 1 { "" } else { "s" }
                );
                self.record_activity(ActivityKindView::Engine, message.clone(), None, None, 1, cx);
                self.show_automatic_notice(message, false, true, cx);
            }
            EngineHealthView::Failed { summary } => {
                let message = format!("Local aria2 could not be restarted: {summary}");
                self.record_activity(ActivityKindView::Engine, message.clone(), None, None, 1, cx);
                self.show_automatic_notice(message, true, true, cx);
            }
            _ => cx.notify(),
        }
    }

    pub fn set_add_download_result(
        &mut self,
        result: AddDownloadResultView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let matches_pending = self.add_dialog.pending.as_ref().is_some_and(|pending| {
            pending.request_id == result.request_id && pending.session == result.session
        });
        if !matches_pending {
            return;
        }

        self.add_dialog.pending = None;
        if result.items.is_empty() {
            self.add_dialog.error = Some(OperationErrorView {
                code: "application.internal".into(),
                summary: "The add request returned no item results.".into(),
                retryable: false,
            });
            cx.notify();
            return;
        }

        let accepted = result
            .items
            .iter()
            .flat_map(|item| match &item.outcome {
                CommandOutcomeView::Success { tasks } => tasks.clone(),
                CommandOutcomeView::Failure(_) => Vec::new(),
            })
            .collect::<Vec<_>>();
        let failed_count = result
            .items
            .iter()
            .filter(|item| matches!(item.outcome, CommandOutcomeView::Failure(_)))
            .count();
        let all_succeeded = failed_count == 0;
        let existing_duplicates = result
            .items
            .iter()
            .filter_map(|item| item.existing_task.clone())
            .collect::<Vec<_>>();
        if !accepted.is_empty() {
            self.selected_tasks = accepted.iter().cloned().collect();
            self.selected = accepted.first().cloned();
            self.range_anchor = self.selected.clone();
        } else if !existing_duplicates.is_empty() {
            self.selected_tasks = existing_duplicates.iter().cloned().collect();
            self.selected = existing_duplicates.first().cloned();
            self.range_anchor = self.selected.clone();
        }

        if all_succeeded {
            self.add_input
                .update(cx, |input, cx| input.set_text("", cx));
            self.add_dialog.metadata_files.clear();
            self.add_dialog.active_metadata_file = None;
            self.show_notice(
                format!(
                    "{} download{} accepted by aria2.",
                    accepted.len(),
                    if accepted.len() == 1 { "" } else { "s" }
                ),
                false,
                cx,
            );
            self.close_add_download(window, cx);
            return;
        }

        if accepted.is_empty() && existing_duplicates.len() == result.items.len() {
            self.show_notice(
                format!(
                    "{} download{} already in the transfer list.",
                    existing_duplicates.len(),
                    if existing_duplicates.len() == 1 {
                        " is"
                    } else {
                        "s are"
                    }
                ),
                false,
                cx,
            );
            self.close_add_download(window, cx);
            return;
        }

        let retryable_sources = result
            .items
            .iter()
            .filter_map(|item| match &item.outcome {
                CommandOutcomeView::Failure(error)
                    if error.retryable && !error.outcome_unknown() =>
                {
                    Some(item.sources.clone())
                }
                CommandOutcomeView::Success { .. } | CommandOutcomeView::Failure(_) => None,
            })
            .flatten()
            .collect::<Vec<_>>();
        let retryable_uris = retryable_sources
            .iter()
            .filter_map(|source| match source {
                AddDownloadSourceView::Uri { uri, .. } => Some(uri.as_str()),
                AddDownloadSourceView::MetadataFile { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let retryable_metadata_paths = retryable_sources
            .iter()
            .filter_map(|source| match source {
                AddDownloadSourceView::MetadataFile { path, .. } => Some(metadata_path_key(path)),
                AddDownloadSourceView::Uri { .. } => None,
            })
            .collect::<HashSet<_>>();
        self.add_dialog
            .metadata_files
            .retain(|preview| retryable_metadata_paths.contains(&metadata_path_key(&preview.path)));
        self.add_dialog.active_metadata_file =
            (!self.add_dialog.metadata_files.is_empty()).then_some(0);
        self.add_dialog.updating_input_from_result =
            self.add_input.read(cx).text() != retryable_uris;
        self.add_input
            .update(cx, |input, cx| input.set_text(retryable_uris, cx));
        self.add_dialog.results = result.items;
        self.show_notice(
            format!(
                "{} accepted, {failed_count} need attention.",
                accepted.len()
            ),
            true,
            cx,
        );
        cx.notify();
    }

    pub fn set_add_download_metadata_preview_result(
        &mut self,
        result: AddDownloadMetadataPreviewResultView,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.add_dialog.preview_pending.as_ref() else {
            return;
        };
        if pending.request_id != result.request_id
            || pending.paths.len() != result.items.len()
            || !pending
                .paths
                .iter()
                .zip(&result.items)
                .all(|(path, item)| path == &item.path)
        {
            return;
        }

        self.add_dialog.preview_pending = None;
        let previous_error = self.add_dialog.error.take();
        let mut failures = Vec::new();
        for item in result.items {
            match item.outcome {
                AddDownloadMetadataPreviewOutcomeView::Ready(preview) => {
                    let key = metadata_path_key(&preview.path);
                    if self
                        .add_dialog
                        .metadata_files
                        .iter()
                        .all(|known| metadata_path_key(&known.path) != key)
                    {
                        self.add_dialog.metadata_files.push(preview);
                    }
                }
                AddDownloadMetadataPreviewOutcomeView::Failed(error) => {
                    failures.push(format!("{}: {}", item.path.display(), error.summary));
                }
            }
        }
        if self.add_dialog.active_metadata_file.is_none()
            && !self.add_dialog.metadata_files.is_empty()
        {
            self.add_dialog.active_metadata_file = Some(0);
        }
        self.add_dialog.error = match (previous_error, failures.is_empty()) {
            (None, true) => None,
            (Some(error), true) => Some(error),
            (previous, false) => Some(OperationErrorView {
                code: "validation.invalid_metadata".into(),
                summary: previous.map_or_else(
                    || failures.join(" "),
                    |error| format!("{} {}", error.summary, failures.join(" ")),
                ),
                retryable: false,
            }),
        };
        self.add_dialog.results.clear();
        cx.notify();
    }

    pub fn set_task_command_result(
        &mut self,
        result: TaskCommandResultView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let matches_pending = self.pending_task_command.as_ref().is_some_and(|pending| {
            pending.request_id == result.request_id
                && pending.session == result.session
                && pending.identity == result.identity
                && pending.command == result.command
        });
        if !matches_pending {
            return;
        }

        self.pending_task_command = None;
        match result.outcome {
            CommandOutcomeView::Success { tasks } => {
                self.show_notice(self.t(result.command.success_message_key()), false, cx);
                match result.command {
                    TaskCommandView::RemoveTask
                    | TaskCommandView::ForceRemoveTask
                    | TaskCommandView::RemoveTaskAndFiles => {
                        self.selected_tasks.remove(&result.identity);
                        self.range_anchor = None;
                        self.selected = None;
                        self.details_drawer = None;
                        self.context_menu = None;
                        // Recovery path: files stay on disk or in Trash (see
                        // success_label). Restore list focus for continued work.
                        window.focus(&self.focus_handle, cx);
                    }
                    TaskCommandView::Retry => {
                        self.selected_tasks.remove(&result.identity);
                        if let Some(identity) = tasks.into_iter().next() {
                            self.selected_tasks.insert(identity.clone());
                            self.selected = Some(identity);
                        }
                        self.range_anchor = self.selected.clone();
                        self.details_drawer = None;
                    }
                    TaskCommandView::SetOutputName { .. } => {
                        self.close_task_output_name(window, cx);
                    }
                    TaskCommandView::SetSpeedLimit { .. } => {
                        self.close_task_speed_limit(window, cx);
                    }
                    TaskCommandView::SetConnectionPolicy { .. } => {}
                    TaskCommandView::SetOptions { .. } => {
                        self.close_task_options(window, cx);
                    }
                    TaskCommandView::Pause
                    | TaskCommandView::ForcePause
                    | TaskCommandView::Resume
                    | TaskCommandView::MoveToQueueTop
                    | TaskCommandView::MoveUpInQueue
                    | TaskCommandView::MoveDownInQueue
                    | TaskCommandView::MoveToQueueBottom => {}
                }
            }
            CommandOutcomeView::Failure(mut error) => {
                if error.outcome_unknown() {
                    error.summary = format!(
                        "Command outcome is unknown; AriaDeck will not retry it automatically. {}",
                        error.summary
                    );
                }
                if matches!(result.command, TaskCommandView::SetOutputName { .. }) {
                    if let Some(dialog) = &mut self.output_name_dialog {
                        dialog.error = Some(error);
                    } else {
                        self.show_notice(self.te(&error), true, cx);
                    }
                } else if matches!(result.command, TaskCommandView::SetSpeedLimit { .. }) {
                    if let Some(dialog) = &mut self.task_speed_limit_dialog {
                        dialog.error = Some(error);
                    } else {
                        self.show_notice(self.te(&error), true, cx);
                    }
                } else if matches!(result.command, TaskCommandView::SetOptions { .. }) {
                    if let Some(dialog) = &mut self.task_options_dialog {
                        dialog.error = Some(error);
                    } else {
                        self.show_notice(self.te(&error), true, cx);
                    }
                } else {
                    self.show_notice(self.te(&error), true, cx);
                }
            }
        }
        cx.notify();
    }

    pub fn set_global_task_command_result(
        &mut self,
        result: GlobalTaskCommandResultView,
        cx: &mut Context<Self>,
    ) {
        let matches_pending = self
            .pending_global_task_command
            .as_ref()
            .is_some_and(|pending| {
                pending.request_id == result.request_id
                    && pending.session == result.session
                    && pending.command == result.command
            });
        if !matches_pending {
            return;
        }

        self.pending_global_task_command = None;
        match result.outcome {
            CommandOutcomeView::Success { .. } => {
                self.show_notice(self.t(result.command.success_message_key()), false, cx);
            }
            CommandOutcomeView::Failure(mut error) => {
                if error.outcome_unknown() {
                    error.summary = format!(
                        "Command outcome is unknown; AriaDeck will not retry it automatically. {}",
                        error.summary
                    );
                }
                self.show_notice(self.te(&error), true, cx);
            }
        }
        cx.notify();
    }

    pub fn set_batch_task_command_result(
        &mut self,
        result: BatchTaskCommandResultView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let matches_pending = self.pending_batch_command.as_ref().is_some_and(|pending| {
            pending.request_id == result.request_id
                && pending.session == result.session
                && pending.identities == result.identities
                && pending.command == result.command
        });
        if !matches_pending {
            return;
        }

        self.pending_batch_command = None;
        match result.outcome {
            BatchCommandOutcomeView::Success { succeeded } => {
                self.apply_batch_selection_result(
                    &result.identities,
                    result.command,
                    &succeeded,
                    &[],
                );
                let summary = if result.command == BatchTaskCommandView::Retry {
                    format!(
                        "Created {} new retry task{}; failed results were kept.",
                        succeeded.len(),
                        if succeeded.len() == 1 { "" } else { "s" }
                    )
                } else {
                    format!(
                        "{} completed for {} task{}.",
                        result.command.label(),
                        succeeded.len(),
                        if succeeded.len() == 1 { "" } else { "s" }
                    )
                };
                self.show_notice(summary, false, cx);
            }
            BatchCommandOutcomeView::PartialSuccess { succeeded, failed } => {
                let failure_details = failed.clone();
                self.apply_batch_selection_result(
                    &result.identities,
                    result.command,
                    &succeeded,
                    &failed,
                );
                let summary = if result.command == BatchTaskCommandView::Retry {
                    format!(
                        "Retry created {} new task{}; {} failed. Original failed results were kept and unresolved items remain selected.",
                        succeeded.len(),
                        if succeeded.len() == 1 { "" } else { "s" },
                        failed.len()
                    )
                } else {
                    format!(
                        "{}: {} succeeded, {} failed. Failed tasks remain selected.",
                        result.command.label(),
                        succeeded.len(),
                        failed.len()
                    )
                };
                self.show_notice(summary, true, cx);
                self.open_batch_failure_details(result.command, failure_details, window, cx);
            }
            BatchCommandOutcomeView::Failure { failed } => {
                let failure_details = failed.clone();
                self.apply_batch_selection_result(&result.identities, result.command, &[], &failed);
                let detail = failed
                    .first()
                    .map(|failure| self.te(&failure.error))
                    .unwrap_or_else(|| self.t("error-command-no-result"));
                let count = failed.len().max(result.identities.len());
                self.show_notice(
                    self.t_args(
                        "notice-batch-command-failed",
                        &[
                            (
                                "count",
                                FluentValue::from(i64::try_from(count).unwrap_or(i64::MAX)),
                            ),
                            ("detail", FluentValue::from(detail)),
                        ],
                    ),
                    true,
                    cx,
                );
                self.open_batch_failure_details(result.command, failure_details, window, cx);
            }
        }
        cx.notify();
    }

    fn open_batch_failure_details(
        &mut self,
        command: BatchTaskCommandView,
        failures: Vec<BatchTaskFailureView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.batch_failure_details = Some(BatchFailureDetails {
            command,
            failures,
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
        });
        cx.defer_in(window, |this, window, cx| {
            if this.batch_failure_details.is_some() {
                window.focus(&this.batch_failure_close_focus, cx);
            }
        });
    }

    fn close_batch_failure_details_action(
        &mut self,
        _: &CloseBatchFailures,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_batch_failure_details(window, cx);
    }

    fn close_batch_failure_details(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(details) = self.batch_failure_details.take() else {
            return;
        };
        if let Some(focus) = details.previous_focus.and_then(|focus| focus.upgrade()) {
            window.focus(&focus, cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    fn apply_batch_selection_result(
        &mut self,
        requested: &[TaskIdentity],
        command: BatchTaskCommandView,
        succeeded: &[TaskIdentity],
        failed: &[BatchTaskFailureView],
    ) {
        let requested = requested.iter().cloned().collect::<HashSet<_>>();
        let failed_identities = failed
            .iter()
            .filter_map(|failure| failure.identity.clone())
            .collect::<HashSet<_>>();
        let has_global_failure = failed.iter().any(|failure| failure.identity.is_none());
        if !has_global_failure {
            self.selected_tasks.retain(|identity| {
                !requested.contains(identity) || failed_identities.contains(identity)
            });
        }
        if command == BatchTaskCommandView::Retry {
            self.selected_tasks.extend(succeeded.iter().cloned());
        }

        if matches!(
            command,
            BatchTaskCommandView::Retry
                | BatchTaskCommandView::RemoveTask
                | BatchTaskCommandView::ForceRemoveTask
                | BatchTaskCommandView::RemoveTaskAndFiles
        ) && self.selected.as_ref().is_some_and(|identity| {
            requested.contains(identity) && !failed_identities.contains(identity)
        }) {
            self.selected = self.selected_tasks.iter().next().cloned();
            self.details_drawer = None;
            self.context_menu = None;
        }
        if self.selected_tasks.is_empty() {
            self.range_anchor = None;
        }
    }

    pub fn set_task_details_result(
        &mut self,
        result: TaskDetailsResultView,
        cx: &mut Context<Self>,
    ) {
        let commands_available = self.snapshot.commands_available();
        let mut refresh_failure = None;
        let request_again = {
            let Some(drawer) = &mut self.details_drawer else {
                return;
            };
            let Some(pending) = drawer.pending.as_ref() else {
                return;
            };
            if pending.request_id != result.request_id
                || drawer.session != result.session
                || drawer.identity != result.identity
            {
                return;
            }

            let pending = drawer
                .pending
                .take()
                .expect("matched pending details request");
            let background_refresh = matches!(drawer.state, TaskDetailsLoadState::Ready { .. });
            match result.outcome {
                TaskDetailsOutcomeView::Ready(details) => {
                    drawer.state = TaskDetailsLoadState::Ready { details };
                }
                TaskDetailsOutcomeView::Failed(error) if background_refresh => {
                    refresh_failure = Some(error);
                }
                TaskDetailsOutcomeView::Failed(error) => {
                    drawer.state = TaskDetailsLoadState::Failed { error };
                }
            }
            commands_available && drawer.overview.revision > pending.source_revision
        };

        if request_again {
            // Catch-up after a completed request is not rate-limited so the drawer
            // can converge to the latest overview revision (D-017).
            self.request_current_details(false, cx);
        } else if let Some(error) = refresh_failure {
            let summary = self.te(&error);
            self.show_notice(
                self.t_args(
                    "notice-refresh-details",
                    &[("summary", FluentValue::from(summary))],
                ),
                true,
                cx,
            );
        } else if self
            .details_drawer
            .as_ref()
            .is_some_and(|drawer| drawer.refresh_coalesced)
        {
            self.request_current_details(true, cx);
            cx.notify();
        } else {
            cx.notify();
        }
    }

    pub fn set_task_open_result(&mut self, result: TaskOpenResultView, cx: &mut Context<Self>) {
        let Some(drawer) = &mut self.details_drawer else {
            return;
        };
        let Some(pending) = drawer.open_pending.as_ref() else {
            return;
        };
        if pending.request_id != result.request_id
            || pending.target != result.target
            || drawer.session != result.session
            || drawer.identity != result.identity
        {
            return;
        }
        drawer.open_pending = None;
        match result.outcome {
            TaskOpenOutcomeView::Success => self.show_notice(
                match result.target {
                    TaskOpenTargetView::Download => "Opened the downloaded item.",
                    TaskOpenTargetView::Folder => "Opened the download folder.",
                },
                false,
                cx,
            ),
            TaskOpenOutcomeView::Failure(error) => {
                let summary = self.te(&error);
                self.show_notice(
                    self.t_args(
                        "notice-open-path-failed",
                        &[("summary", FluentValue::from(summary))],
                    ),
                    true,
                    cx,
                );
            }
        }
    }

    pub fn set_startup_notice(&mut self, message: String, is_error: bool, cx: &mut Context<Self>) {
        self.show_notice(message, is_error, cx);
    }

    /// Translate a Fluent message id with the active catalog.
    #[must_use]
    pub(crate) fn t(&self, id: &str) -> String {
        self.translator.t(id)
    }

    /// Translate a Fluent message id with named arguments.
    #[must_use]
    pub(crate) fn t_args(&self, id: &str, args: &[(&str, FluentValue<'_>)]) -> String {
        let mut fluent_args = FluentArgs::new();
        for (name, value) in args {
            fluent_args.set(*name, value.clone());
        }
        self.translator.t_args(id, Some(&fluent_args))
    }

    /// Translate a Fluent message id with the conventional numeric `n` argument.
    #[must_use]
    pub(crate) fn t_count(&self, id: &str, n: u64) -> String {
        self.translator.t_count(id, n)
    }

    #[must_use]
    pub(crate) fn te(&self, error: &OperationErrorView) -> String {
        error.localized_summary(&self.translator)
    }

    #[must_use]
    #[allow(dead_code)]
    pub(crate) fn translator(&self) -> &Translator {
        &self.translator
    }

    pub(crate) fn set_language_runtime(&mut self, language: LanguagePreferenceView) {
        self.translator = translator_for_language(language);
    }

    pub fn settings(&self) -> &SettingsView {
        &self.settings
    }

    /// Build the privacy-safe projection used by the desktop diagnostic export.
    #[must_use]
    pub fn diagnostic_snapshot(
        &self,
        settings_schema_version: u32,
    ) -> ariadeck_domain::DiagnosticSnapshot {
        let active_profile = self.profiles.active();
        let capability_count = [
            self.snapshot.capabilities.force_pause,
            self.snapshot.capabilities.force_pause_all,
            self.snapshot.capabilities.force_remove,
            self.snapshot.capabilities.queue_positioning,
            self.snapshot.capabilities.change_option,
            self.snapshot.capabilities.change_global_option,
            self.snapshot.capabilities.get_peers,
            self.snapshot.capabilities.get_servers,
            self.snapshot.capabilities.multicall,
        ]
        .into_iter()
        .filter(|supported| *supported)
        .count() as u32;
        ariadeck_domain::DiagnosticSnapshot {
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            engine_version: (!self.snapshot.capabilities.version.trim().is_empty())
                .then(|| self.snapshot.capabilities.version.clone()),
            settings_schema_version: Some(settings_schema_version),
            connection_state: self.snapshot.connection.label().to_owned(),
            redacted_rpc_endpoint: active_profile
                .and_then(|profile| {
                    (!profile.endpoint.trim().is_empty()).then(|| profile.endpoint.clone())
                })
                .map(|endpoint| ariadeck_domain::redact_endpoint_url(&endpoint)),
            profile_kind: active_profile.map(|profile| profile.kind.label().to_owned()),
            task_count: Some(u32::try_from(self.snapshot.counts.all).unwrap_or(u32::MAX)),
            capability_count: Some(capability_count),
        }
    }

    pub fn set_diagnostic_export_result(
        &mut self,
        result: DiagnosticExportResultView,
        cx: &mut Context<Self>,
    ) {
        match result.outcome {
            DiagnosticExportOutcomeView::Success => {
                self.show_notice(
                    self.t_args(
                        "notice-diagnostics-exported",
                        &[("path", FluentValue::from(result.path))],
                    ),
                    false,
                    cx,
                );
            }
            DiagnosticExportOutcomeView::Failure(error) => {
                self.show_notice(self.te(&error), true, cx);
            }
        }
    }

    #[must_use]
    pub fn query(&self) -> WorkspaceQuery {
        self.query.clone()
    }

    #[must_use]
    pub fn selected_identity(&self) -> Option<&TaskIdentity> {
        self.selected.as_ref()
    }

    #[must_use]
    pub fn selected_task_count(&self) -> usize {
        self.selected_tasks.len()
    }

    #[must_use]
    pub fn visible_selected_task_count(&self) -> usize {
        self.snapshot
            .tasks
            .iter()
            .filter(|task| self.selected_tasks.contains(&task.identity))
            .count()
    }

    #[must_use]
    pub fn rendered_range(&self) -> Range<usize> {
        self.rendered_range.clone()
    }

    /// Restore list filter/sort from persisted preferences without re-emitting a save.
    pub fn restore_list_preferences(&mut self, query: WorkspaceQuery, cx: &mut Context<Self>) {
        self.query = WorkspaceQuery {
            filter: query.filter,
            // Search is intentionally never restored across restarts (D-031).
            search: String::new(),
            sort_key: query.sort_key,
            sort_direction: query.sort_direction,
            category_id: query.category_id,
        };
        cx.notify();
    }

    fn emit_query(&self, cx: &mut Context<Self>) {
        cx.emit(AppShellEvent::QueryChanged(self.query.clone()));
        cx.emit(AppShellEvent::UiPreferencesChanged {
            filter: self.query.filter,
            sort_key: self.query.sort_key,
            sort_direction: self.query.sort_direction,
        });
        cx.notify();
    }

    fn emit_window_geometry(&self, window: &Window, cx: &mut Context<Self>) {
        let bounds = window.bounds();
        cx.emit(AppShellEvent::WindowGeometryChanged {
            x: f32::from(bounds.origin.x),
            y: f32::from(bounds.origin.y),
            width: f32::from(bounds.size.width),
            height: f32::from(bounds.size.height),
            maximized: window.is_maximized(),
        });
    }

    fn set_filter(&mut self, filter: WorkspaceFilter, window: &mut Window, cx: &mut Context<Self>) {
        let query_changed = self.query.filter != filter;
        self.page = AppPage::Downloads;
        self.speed_popover_open = false;
        if query_changed {
            self.query.filter = filter;
            self.clear_task_selection();
        }
        self.list_scroll
            .scroll_to_item_strict(0, ScrollStrategy::Top);
        window.focus(&self.focus_handle, cx);
        if query_changed {
            self.emit_query(cx);
        } else {
            cx.notify();
        }
    }

    pub(crate) fn set_category_filter(
        &mut self,
        category_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query_changed = self.query.category_id != category_id;
        self.page = AppPage::Downloads;
        self.speed_popover_open = false;
        if query_changed {
            self.query.category_id = category_id;
            self.clear_task_selection();
        }
        self.list_scroll
            .scroll_to_item_strict(0, ScrollStrategy::Top);
        window.focus(&self.focus_handle, cx);
        if query_changed {
            self.emit_query(cx);
        } else {
            cx.notify();
        }
    }

    fn toggle_sort_popover(&mut self, cx: &mut Context<Self>) {
        self.sort_popover_open = !self.sort_popover_open;
        cx.notify();
    }

    fn close_sort_popover(&mut self, cx: &mut Context<Self>) {
        if self.sort_popover_open {
            self.sort_popover_open = false;
            cx.notify();
        }
    }

    /// D-014: changing the sort key or direction only changes the current
    /// AriaDeck query and preserves identity-based selection; it never writes a
    /// new engine priority.
    fn set_sort_key(&mut self, key: WorkspaceSortKey, cx: &mut Context<Self>) {
        if self.query.sort_key != key {
            self.query.sort_key = key;
            self.list_scroll
                .scroll_to_item_strict(0, ScrollStrategy::Top);
            self.emit_query(cx);
        }
        self.sort_popover_open = false;
        cx.notify();
    }

    fn set_sort_direction(&mut self, direction: WorkspaceSortDirection, cx: &mut Context<Self>) {
        if self.query.sort_direction != direction {
            self.query.sort_direction = direction;
            self.list_scroll
                .scroll_to_item_strict(0, ScrollStrategy::Top);
            self.emit_query(cx);
        }
        cx.notify();
    }

    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.page = AppPage::Downloads;
        self.speed_popover_open = false;
        self.sort_popover_open = false;
        window.focus(&self.search_input.focus_handle(cx), cx);
        cx.notify();
    }

    fn clear_search(&mut self, _: &ClearSearch, window: &mut Window, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            cx.notify();
        } else if self.activity_panel_open {
            self.close_activity_panel(window, cx);
        } else if self.sort_popover_open {
            self.close_sort_popover(cx);
        } else if self.speed_popover_open {
            self.close_speed_popover(window, cx);
        } else if self.output_name_dialog.is_some() {
            self.close_task_output_name(window, cx);
        } else if self.task_speed_limit_dialog.is_some() {
            self.close_task_speed_limit(window, cx);
        } else if self.task_options_dialog.is_some() {
            self.close_task_options(window, cx);
        } else if self.remove_confirmation.is_some() {
            self.close_remove_confirmation(window, cx);
        } else if self.page == AppPage::Settings {
            self.close_settings(window, cx);
        } else if self.details_drawer.is_some() {
            // Overlay and inline drawers both dismiss before clearing search/selection.
            self.close_task_details(window, cx);
        } else if !self.search_input.read(cx).text().is_empty() {
            self.search_input
                .update(cx, |input, cx| input.set_text("", cx));
        } else if !self.selected_tasks.is_empty() {
            self.clear_task_selection();
            cx.notify();
        } else {
            window.focus(&self.focus_handle, cx);
        }
    }

    fn select_next(&mut self, _: &SelectNextTask, window: &mut Window, cx: &mut Context<Self>) {
        if self.snapshot.tasks.is_empty() {
            return;
        }
        let next = match self.selected_index() {
            Some(current) => (current + 1).min(self.snapshot.tasks.len() - 1),
            None => 0,
        };
        self.select_at(next, window, cx);
    }

    fn select_previous(
        &mut self,
        _: &SelectPreviousTask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.snapshot.tasks.is_empty() {
            return;
        }
        let previous = match self.selected_index() {
            Some(current) => current.saturating_sub(1),
            None => self.snapshot.tasks.len() - 1,
        };
        self.select_at(previous, window, cx);
    }

    fn select_at(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.select_at_with_modifiers(index, false, false, window, cx);
    }

    fn select_at_with_modifiers(
        &mut self,
        index: usize,
        extend_range: bool,
        toggle: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(task) = self.snapshot.tasks.get(index) else {
            return;
        };
        let task = task.clone();
        if extend_range {
            let anchor_index = self
                .range_anchor
                .as_ref()
                .and_then(|anchor| {
                    self.snapshot
                        .tasks
                        .iter()
                        .position(|candidate| &candidate.identity == anchor)
                })
                .or_else(|| self.selected_index())
                .unwrap_or(index);
            let (start, end) = if anchor_index <= index {
                (anchor_index, index)
            } else {
                (index, anchor_index)
            };
            self.selected_tasks = self.snapshot.tasks[start..=end]
                .iter()
                .map(|task| task.identity.clone())
                .collect();
            if self.range_anchor.is_none() {
                self.range_anchor = self
                    .snapshot
                    .tasks
                    .get(anchor_index)
                    .map(|task| task.identity.clone());
            }
        } else if toggle {
            if !self.selected_tasks.remove(&task.identity) {
                self.selected_tasks.insert(task.identity.clone());
            }
            self.range_anchor = Some(task.identity.clone());
        } else {
            self.selected_tasks.clear();
            self.selected_tasks.insert(task.identity.clone());
            self.range_anchor = Some(task.identity.clone());
        }
        self.selected = Some(task.identity.clone());
        self.list_scroll
            .scroll_to_item(index, ScrollStrategy::Nearest);
        if self.details_drawer.is_some() {
            self.open_details_for(task, cx);
        }
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn select_all_tasks(
        &mut self,
        _: &SelectAllTasks,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Downloads
            || self.add_dialog.open
            || self.output_name_dialog.is_some()
            || self.remove_confirmation.is_some()
            || self.batch_failure_details.is_some()
            || self.snapshot.tasks.is_empty()
        {
            return;
        }
        self.selected_tasks
            .extend(self.snapshot.tasks.iter().map(|task| task.identity.clone()));
        if self.selected_index().is_none() {
            self.selected = self
                .snapshot
                .tasks
                .first()
                .map(|task| task.identity.clone());
        }
        self.range_anchor = self.selected.clone();
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn toggle_select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.snapshot.tasks.is_empty() {
            return;
        }
        let all_selected = self
            .snapshot
            .tasks
            .iter()
            .all(|task| self.selected_tasks.contains(&task.identity));
        if all_selected {
            let visible = self
                .snapshot
                .tasks
                .iter()
                .map(|task| task.identity.clone())
                .collect::<HashSet<_>>();
            self.selected_tasks
                .retain(|identity| !visible.contains(identity));
            self.range_anchor = None;
        } else {
            self.selected_tasks
                .extend(self.snapshot.tasks.iter().map(|task| task.identity.clone()));
            self.range_anchor = self.selected.clone().or_else(|| {
                self.snapshot
                    .tasks
                    .first()
                    .map(|task| task.identity.clone())
            });
            if self.selected.is_none() {
                self.selected = self.range_anchor.clone();
            }
        }
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        self.snapshot
            .tasks
            .iter()
            .position(|task| &task.identity == selected)
    }

    fn clear_task_selection(&mut self) {
        self.selected_tasks.clear();
        self.range_anchor = None;
    }

    fn apply_theme_to_text_fields(&mut self, cx: &mut Context<Self>) {
        let theme = self.theme;
        for input in self.all_text_fields() {
            input.update(cx, |input, cx| input.set_theme(theme, cx));
        }
    }

    fn handle_text_field_event(
        &mut self,
        field: Entity<TextField>,
        event: &SearchInputEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            SearchInputEvent::TextChanged { .. } => {}
            SearchInputEvent::ContextMenuRequested { position } => {
                self.open_text_field_context_menu(field, *position, cx);
            }
        }
    }

    fn open_text_field_context_menu(
        &mut self,
        field: Entity<TextField>,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = None;
        self.sort_popover_open = false;
        self.speed_popover_open = false;
        self.text_field_context_menu = Some(TextFieldContextMenuState { field, position });
        cx.notify();
    }

    fn close_text_field_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.text_field_context_menu.take().is_some() {
            cx.notify();
        }
    }

    fn activate_text_field_context_action(
        &mut self,
        action: TextFieldContextAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.text_field_context_menu.take() else {
            return;
        };
        menu.field.update(cx, |field, cx| match action {
            TextFieldContextAction::Cut => field.context_cut(window, cx),
            TextFieldContextAction::Copy => field.context_copy(window, cx),
            TextFieldContextAction::Paste => field.context_paste(window, cx),
            TextFieldContextAction::SelectAll => field.context_select_all(window, cx),
        });
        cx.notify();
    }

    fn all_text_fields(&self) -> [&Entity<TextField>; 34] {
        let add = self.add_inputs.all();
        let settings = self.settings_inputs.all();
        let task = self.task_inputs.all();
        [
            &self.search_input,
            &self.add_input,
            add[0],
            add[1],
            add[2],
            add[3],
            add[4],
            add[5],
            add[6],
            &self.output_name_input,
            settings[0],
            settings[1],
            settings[2],
            settings[3],
            settings[4],
            settings[5],
            settings[6],
            settings[7],
            settings[8],
            settings[9],
            settings[10],
            settings[11],
            settings[12],
            settings[13],
            settings[14],
            settings[15],
            settings[16],
            settings[17],
            settings[18],
            settings[19],
            task[0],
            task[1],
            task[2],
            task[3],
        ]
    }

    fn focus_next(&mut self, _: &FocusNext, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next(cx);
        if self.add_dialog.open && !self.add_dialog_focus.contains_focused(window, cx) {
            window.focus(&self.add_input.focus_handle(cx), cx);
        } else if self.output_name_dialog.is_some()
            && !self.output_name_dialog_focus.contains_focused(window, cx)
        {
            window.focus(&self.output_name_input.focus_handle(cx), cx);
        } else if self.remove_confirmation.is_some()
            && !self.remove_dialog_focus.contains_focused(window, cx)
        {
            window.focus(&self.remove_cancel_focus, cx);
        } else if self.batch_failure_details.is_some()
            && !self.batch_failure_dialog_focus.contains_focused(window, cx)
        {
            window.focus(&self.batch_failure_close_focus, cx);
        } else if self.task_speed_limit_dialog.is_some()
            && !self
                .task_speed_limit_dialog_focus
                .contains_focused(window, cx)
        {
            window.focus(&self.task_inputs.download_limit.focus_handle(cx), cx);
        } else if self.task_options_dialog.is_some()
            && !self.task_options_dialog_focus.contains_focused(window, cx)
        {
            window.focus(&self.task_inputs.seed_ratio.focus_handle(cx), cx);
        }
    }

    fn focus_previous(&mut self, _: &FocusPrevious, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_prev(cx);
        if self.add_dialog.open && !self.add_dialog_focus.contains_focused(window, cx) {
            window.focus(&self.add_submit_focus, cx);
        } else if self.output_name_dialog.is_some()
            && !self.output_name_dialog_focus.contains_focused(window, cx)
        {
            window.focus(&self.output_name_submit_focus, cx);
        } else if self.remove_confirmation.is_some()
            && !self.remove_dialog_focus.contains_focused(window, cx)
        {
            window.focus(&self.remove_submit_focus, cx);
        } else if self.batch_failure_details.is_some()
            && !self.batch_failure_dialog_focus.contains_focused(window, cx)
        {
            window.focus(&self.batch_failure_close_focus, cx);
        } else if self.task_speed_limit_dialog.is_some()
            && !self
                .task_speed_limit_dialog_focus
                .contains_focused(window, cx)
        {
            window.focus(&self.task_speed_limit_submit_focus, cx);
        } else if self.task_options_dialog.is_some()
            && !self.task_options_dialog_focus.contains_focused(window, cx)
        {
            window.focus(&self.task_options_submit_focus, cx);
        }
    }

    pub fn handle_window_close_request(&mut self, cx: &mut Context<Self>) -> bool {
        match self.settings.platform.close_behavior {
            CloseBehaviorView::MinimizeToTray if self.settings.platform.show_tray_icon => {
                cx.emit(AppShellEvent::HideToTrayRequested);
                false
            }
            _ => {
                cx.emit(AppShellEvent::QuitRequested);
                true
            }
        }
    }

    pub fn request_show_from_tray(&mut self, cx: &mut Context<Self>) {
        cx.emit(AppShellEvent::ShowFromTrayRequested);
    }

    pub fn request_quit(&mut self, cx: &mut Context<Self>) {
        cx.emit(AppShellEvent::QuitRequested);
    }

    pub fn request_pause_all_from_tray(&mut self, cx: &mut Context<Self>) {
        self.begin_global_task_command(GlobalTaskCommandView::PauseAll, cx);
    }

    pub fn request_resume_all_from_tray(&mut self, cx: &mut Context<Self>) {
        self.begin_global_task_command(GlobalTaskCommandView::ResumeAll, cx);
    }

    #[must_use]
    pub fn tray_tooltip(&self) -> String {
        self.snapshot.tray_tooltip()
    }

    /// Surface a low-disk warning once per free-space recovery (PLAT-001).
    pub fn report_disk_space(&mut self, available_bytes: Option<u64>, cx: &mut Context<Self>) {
        let prefs = self.settings.notifications;
        if !prefs.notify_on_low_disk {
            self.low_disk_active = false;
            return;
        }
        let Some(available) = available_bytes else {
            return;
        };
        if available < prefs.low_disk_threshold_bytes {
            if self.low_disk_active {
                return;
            }
            self.low_disk_active = true;
            let threshold = format_bytes(prefs.low_disk_threshold_bytes);
            let free = format_bytes(available);
            let message = format!(
                "Low disk space: {free} free (threshold {threshold}). New downloads may fail."
            );
            self.record_activity(ActivityKindView::Error, message.clone(), None, None, 1, cx);
            self.show_automatic_notice(message, true, false, cx);
        } else if self.low_disk_active {
            self.low_disk_active = false;
        }
    }

    fn request_retry(&mut self, cx: &mut Context<Self>) {
        cx.emit(AppShellEvent::RetryRequested);
    }

    fn request_load_more_stopped(&mut self, cx: &mut Context<Self>) {
        if self.pending_load_more_stopped
            || !self.snapshot.connection.is_connected()
            || self.snapshot.stale
            || !self.snapshot.stopped_history.can_load_more
        {
            return;
        }
        self.pending_load_more_stopped = true;
        cx.emit(AppShellEvent::LoadMoreStoppedRequested);
        cx.notify();
    }

    pub fn set_load_more_stopped_result(
        &mut self,
        success: bool,
        message: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if !self.pending_load_more_stopped {
            return;
        }
        self.pending_load_more_stopped = false;
        if let Some(message) = message {
            self.show_notice(message, !success, cx);
        }
        cx.notify();
    }

    fn allocate_request_id(&mut self) -> RequestId {
        let request_id = RequestId::from_u64(self.next_request_id);
        self.next_request_id = self.next_request_id.checked_add(1).unwrap_or(1);
        request_id
    }
}

impl Focusable for AppShell {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for AppShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = self.theme.colors;
        let metadata_drop_enabled =
            self.add_dialog.pending.is_none() && self.add_dialog.preview_pending.is_none();
        let shell_layout = shell_layout(f32::from(window.viewport_size().width));
        div()
            .id("download-workspace")
            .key_context("DownloadWorkspace")
            .role(Role::Application)
            .aria_label(self.t("workspace-aria"))
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::select_all_tasks))
            .on_action(cx.listener(Self::clear_search))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::open_add_download))
            .on_action(cx.listener(Self::close_add_download_action))
            .on_action(cx.listener(Self::submit_add_download_action))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::close_settings_action))
            .on_action(cx.listener(Self::save_settings_action))
            .on_action(cx.listener(Self::open_task_details_action))
            .on_action(cx.listener(Self::pause_selected))
            .on_action(cx.listener(Self::resume_selected))
            .on_action(cx.listener(Self::retry_selected))
            .on_action(cx.listener(Self::move_selected_to_queue_top))
            .on_action(cx.listener(Self::move_selected_up_in_queue))
            .on_action(cx.listener(Self::move_selected_down_in_queue))
            .on_action(cx.listener(Self::move_selected_to_queue_bottom))
            .on_action(cx.listener(Self::open_task_output_name_action))
            .on_action(cx.listener(Self::close_task_output_name_action))
            .on_action(cx.listener(Self::submit_task_output_name_action))
            .on_action(cx.listener(Self::open_task_speed_limit_action))
            .on_action(cx.listener(Self::close_task_speed_limit_action))
            .on_action(cx.listener(Self::submit_task_speed_limit_action))
            .on_action(cx.listener(Self::close_task_options_action))
            .on_action(cx.listener(Self::submit_task_options_action))
            .on_action(cx.listener(Self::close_batch_failure_details_action))
            .on_action(cx.listener(Self::remove_selected))
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .can_drop(move |value, _window, _cx| {
                value.downcast_ref::<ExternalPaths>().is_some_and(|paths| {
                    can_accept_metadata_drop(metadata_drop_enabled, paths.paths())
                })
            })
            .on_drop::<ExternalPaths>(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                let _ = this.open_metadata_paths(paths.paths().to_vec(), window, cx);
            }))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.text_primary)
            .child(self.render_header(window, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.render_sidebar(shell_layout.sidebar_width, cx))
                    .child(match self.page {
                        AppPage::Downloads => self
                            .render_main(
                                shell_layout.task_layout,
                                shell_layout.details_presentation,
                                cx,
                            )
                            .into_any_element(),
                        AppPage::Settings => self.render_settings_page(cx).into_any_element(),
                    }),
            )
            .child(self.render_status_bar(cx))
            .when(self.add_dialog.open, |element| {
                element.child(self.render_add_download_dialog(cx))
            })
            .when(self.output_name_dialog.is_some(), |element| {
                element.child(self.render_task_output_name_dialog(cx))
            })
            .when(self.task_speed_limit_dialog.is_some(), |element| {
                element.child(self.render_task_speed_limit_dialog(cx))
            })
            .when(self.task_options_dialog.is_some(), |element| {
                element.child(self.render_task_options_dialog(cx))
            })
            .when(self.remove_confirmation.is_some(), |element| {
                element.child(self.render_remove_confirmation(cx))
            })
            .when(self.speed_popover_open, |element| {
                element.child(self.render_speed_popover(cx))
            })
            .when(self.activity_panel_open, |element| {
                element.child(self.render_activity_panel(cx))
            })
            .when(self.context_menu.is_some(), |element| {
                element.child(self.render_task_context_menu(cx))
            })
            .when(self.text_field_context_menu.is_some(), |element| {
                element.child(self.render_text_field_context_menu(cx))
            })
            .when(
                self.sort_popover_open && self.page == AppPage::Downloads,
                |element| element.child(self.render_sort_popover(cx)),
            )
            .when(self.status_notice.is_some(), |element| {
                element.child(self.render_toast(cx))
            })
            .when(self.batch_failure_details.is_some(), |element| {
                element.child(self.render_batch_failure_details(cx))
            })
    }
}

#[cfg(test)]
mod tests;

fn translator_for_language(language: LanguagePreferenceView) -> Translator {
    let locale = match language {
        LanguagePreferenceView::System => LocaleId::from_system_env(),
        LanguagePreferenceView::English => LocaleId::En,
        LanguagePreferenceView::ChineseSimplified => LocaleId::ZhCn,
    };
    Translator::new(locale)
}
