use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use gpui::{
    AnyElement, App, ClickEvent, ClipboardItem, Context, Div, Entity, ExternalPaths, FocusHandle,
    Focusable, FontFeatures, FontWeight, Hsla, IntoElement, MouseButton, MouseDownEvent,
    PathPromptOptions, Pixels, Point, Render, Role, ScrollHandle, ScrollStrategy, SharedString,
    Stateful, Subscription, Toggled, UniformListScrollHandle, WeakFocusHandle, Window,
    WindowControlArea, div, point, prelude::*, px, relative, uniform_list,
};

use crate::{
    ActivityEntryView, ActivityKindView, AddDownloadAdvancedOptionsView, AddDownloadInputModeView,
    AddDownloadItemResultView, AddDownloadMetadataKindView, AddDownloadMetadataPreviewOutcomeView,
    AddDownloadMetadataPreviewRequestView, AddDownloadMetadataPreviewResultView,
    AddDownloadMetadataPreviewView, AddDownloadModeView, AddDownloadRequestView,
    AddDownloadResultView, AddDownloadSourceView, BatchCommandOutcomeView,
    BatchTaskCommandRequestView, BatchTaskCommandResultView, BatchTaskCommandView,
    BatchTaskFailureView, Button, ButtonStyle, ClearSearch, CloseAddDownload, CloseBatchFailures,
    CloseSettings, CloseTaskOutputName, CloseTaskSpeedLimit, ColorSchemeView, CommandOutcomeView,
    ConnectionView, CoreCommandOutcomeView, CoreCommandRequestView, CoreCommandResultView,
    CoreCommandView, CoreRegistryView, Dialog, DownloadProxySettingsView, DownloadRowView,
    EngineHealthView, EngineSessionView, FileAllocationView, FileConflictPolicyView, FocusNext,
    FocusPrevious, FocusSearch, GlobalTaskCommandRequestView, GlobalTaskCommandResultView,
    GlobalTaskCommandView, Icon, IconButton, IconName, IconSize, MoveTaskDownInQueue,
    MoveTaskToQueueBottom, MoveTaskToQueueTop, MoveTaskUpInQueue, NotificationSettingsView,
    NotificationVolumeView, OpenAddDownload, OpenSettings, OpenTaskDetails, OpenTaskOutputName,
    OpenTaskSpeedLimit, OperationErrorView, PauseSelectedTask, ProfileCatalogView,
    ProfileEntryView, ProfileKindView, ProfileRpcSecretUpdateView, ProxyModeView,
    ProxyPasswordUpdateView, RemoveSelectedTask, RequestId, ResumeSelectedTask, RetrySelectedTask,
    SaveProfileCatalogOutcomeView, SaveProfileCatalogRequestView, SaveProfileCatalogResultView,
    SaveSettings, SearchInputEvent, SecretStringView, Segment, SegmentedControl, SelectAllTasks,
    SelectNextTask, SelectPreviousTask, SettingsSaveOutcomeView, SettingsSaveRequestView,
    SettingsSaveResultView, SettingsView, SpeedLimitSettingsView, SpeedSampleView, StatusIndicator,
    SubmitAddDownload, SubmitTaskOutputName, SubmitTaskSpeedLimit, SwitchProfileOutcomeView,
    SwitchProfileRequestView, SwitchProfileResultView, TaskCommandRequestView,
    TaskCommandResultView, TaskCommandView, TaskDetailsOutcomeView, TaskDetailsRequestView,
    TaskDetailsResultView, TaskDetailsView, TaskFileView, TaskIdentity, TaskOpenOutcomeView,
    TaskOpenRequestView, TaskOpenResultView, TaskOpenTargetView, TaskOptionView,
    TaskPathValidationView, TaskPeerView, TaskServerView, TaskStatusView, TaskTrackerView,
    TaskUriView, TextField, TextFieldConfig, Theme, ThemeMode, Toast, ToastKind, Tooltip,
    TransferPolicySettingsView, WorkspaceFilter, WorkspaceQuery, WorkspaceSnapshot,
    WorkspaceSortDirection, WorkspaceSortKey, actions::TEXT_FIELD_KEY_CONTEXT, format_bytes,
    format_eta, format_percent, format_rate, format_share_ratio,
};

const SPEED_CHART_SAMPLES: usize = 120;
const TITLEBAR_HEIGHT: f32 = 56.0;
const TITLEBAR_SIDE_WIDTH: f32 = 240.0;
const TITLEBAR_HORIZONTAL_PADDING: f32 = 12.0;
const SEARCH_WIDTH: f32 = 460.0;
const SIDEBAR_WIDTH: f32 = 208.0;
const DETAILS_DRAWER_WIDTH: f32 = 360.0;
const TASK_LAYOUT_WIDE_MIN_WIDTH: f32 = 820.0;
const TASK_ROW_HEIGHT: f32 = 68.0;
const ACTIVITY_HISTORY_LIMIT: usize = 100;
const ACTIVITY_PANEL_WIDTH: f32 = 360.0;

#[cfg(target_os = "macos")]
const TITLEBAR_BRAND_INSET: f32 = 52.0;
#[cfg(not(target_os = "macos"))]
const TITLEBAR_BRAND_INSET: f32 = 0.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TaskLayoutMode {
    Compact,
    Wide,
}

fn task_layout_mode(viewport_width: f32, details_open: bool) -> TaskLayoutMode {
    let details_width = if details_open {
        DETAILS_DRAWER_WIDTH
    } else {
        0.0
    };
    let main_width = viewport_width - SIDEBAR_WIDTH - details_width;
    if main_width >= TASK_LAYOUT_WIDE_MIN_WIDTH {
        TaskLayoutMode::Wide
    } else {
        TaskLayoutMode::Compact
    }
}

fn centered_search_bounds(viewport_width: f32) -> (f32, f32) {
    let available_width =
        (viewport_width - 2.0 * (TITLEBAR_SIDE_WIDTH + TITLEBAR_HORIZONTAL_PADDING)).max(0.0);
    let width = available_width.min(SEARCH_WIDTH);
    let left = (viewport_width - width) / 2.0;
    (left, left + width)
}

#[derive(Clone, Debug, Eq, PartialEq)]
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
    SwitchProfileRequested(SwitchProfileRequestView),
    SaveProfileCatalogRequested(SaveProfileCatalogRequestView),
    CoreCommandRequested(CoreCommandRequestView),
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
struct SettingsPage {
    previous_focus: Option<WeakFocusHandle>,
    draft_color_scheme: ColorSchemeView,
    draft_proxy_mode: ProxyModeView,
    draft_file_allocation: FileAllocationView,
    draft_check_integrity: bool,
    draft_notification_volume: NotificationVolumeView,
    draft_notify_on_completion: bool,
    draft_notify_on_error: bool,
    draft_notify_on_engine_events: bool,
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

struct PendingProfileDelete {
    profile_id: String,
    name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettingsSaveSource {
    Theme,
    Directory,
    Proxy,
    SpeedLimit,
    TransferPolicy,
    Notifications,
}

struct PendingSettingsSave {
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
struct TaskContextMenu {
    identity: TaskIdentity,
    position: Point<Pixels>,
}

struct BatchFailureDetails {
    command: BatchTaskCommandView,
    failures: Vec<BatchTaskFailureView>,
    previous_focus: Option<WeakFocusHandle>,
}

pub struct AppShell {
    theme: Theme,
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
    add_referer_input: Entity<TextField>,
    add_user_agent_input: Entity<TextField>,
    add_headers_input: Entity<TextField>,
    add_cookie_input: Entity<TextField>,
    add_http_user_input: Entity<TextField>,
    add_http_passwd_input: Entity<TextField>,
    add_checksum_input: Entity<TextField>,
    output_name_input: Entity<TextField>,
    settings_directory_input: Entity<TextField>,
    settings_core_path_input: Entity<TextField>,
    settings_profile_name_input: Entity<TextField>,
    settings_profile_executable_input: Entity<TextField>,
    settings_profile_endpoint_input: Entity<TextField>,
    settings_profile_download_input: Entity<TextField>,
    settings_profile_secret_input: Entity<TextField>,
    settings_all_proxy_input: Entity<TextField>,
    settings_http_proxy_input: Entity<TextField>,
    settings_https_proxy_input: Entity<TextField>,
    settings_ftp_proxy_input: Entity<TextField>,
    settings_no_proxy_input: Entity<TextField>,
    settings_proxy_username_input: Entity<TextField>,
    settings_proxy_password_input: Entity<TextField>,
    settings_download_limit_input: Entity<TextField>,
    settings_upload_limit_input: Entity<TextField>,
    settings_max_concurrent_input: Entity<TextField>,
    settings_max_connection_input: Entity<TextField>,
    settings_split_input: Entity<TextField>,
    settings_min_split_size_input: Entity<TextField>,
    add_dialog: AddDownloadDialog,
    add_dialog_focus: FocusHandle,
    add_cancel_focus: FocusHandle,
    add_submit_focus: FocusHandle,
    settings_page: SettingsPage,
    settings_save_focus: FocusHandle,
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
    task_download_limit_input: Entity<TextField>,
    task_upload_limit_input: Entity<TextField>,
    task_speed_limit_dialog: Option<TaskSpeedLimitDialog>,
    task_speed_limit_dialog_focus: FocusHandle,
    task_speed_limit_cancel_focus: FocusHandle,
    task_speed_limit_submit_focus: FocusHandle,
    task_seed_ratio_input: Entity<TextField>,
    task_seed_time_input: Entity<TextField>,
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
    status_notice: Option<StatusNotice>,
    next_notice_id: u64,
    activity_log: Vec<ActivityEntryView>,
    next_activity_id: u64,
    activity_panel_open: bool,
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
            ThemeMode::Light => ColorSchemeView::Light,
            ThemeMode::Dark | ThemeMode::System => ColorSchemeView::Dark,
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
            theme_for_scheme(settings.color_scheme),
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
            |this: &mut Self, _input, event: &SearchInputEvent, cx| {
                if this.query.search != event.text {
                    this.query.search.clone_from(&event.text);
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
            |this: &mut Self, _input, _event: &SearchInputEvent, cx| {
                if this.add_dialog.open && this.add_dialog.pending.is_none() {
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
                |this: &mut Self, _input, _event: &SearchInputEvent, cx| {
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
            |this: &mut Self, _input, _event: &SearchInputEvent, cx| {
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
                    |this: &mut Self, _input, _event: &SearchInputEvent, cx| {
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
                    |this: &mut Self, _input, _event: &SearchInputEvent, cx| {
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
                |this: &mut Self, _input, _event: &SearchInputEvent, cx| {
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
            |this: &mut Self, _input, _event: &SearchInputEvent, cx| {
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
        let window_bounds_subscription = cx.observe_window_bounds(window, |_, _, cx| {
            cx.notify();
        });
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        Self {
            theme,
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
            add_referer_input,
            add_user_agent_input,
            add_headers_input,
            add_cookie_input,
            add_http_user_input,
            add_http_passwd_input,
            add_checksum_input,
            output_name_input,
            settings_directory_input,
            settings_core_path_input,
            settings_profile_name_input,
            settings_profile_executable_input,
            settings_profile_endpoint_input,
            settings_profile_download_input,
            settings_profile_secret_input,
            settings_all_proxy_input,
            settings_http_proxy_input,
            settings_https_proxy_input,
            settings_ftp_proxy_input,
            settings_no_proxy_input,
            settings_proxy_username_input,
            settings_proxy_password_input,
            settings_download_limit_input,
            settings_upload_limit_input,
            settings_max_concurrent_input,
            settings_max_connection_input,
            settings_split_input,
            settings_min_split_size_input,
            add_dialog: AddDownloadDialog::default(),
            add_dialog_focus: cx.focus_handle(),
            add_cancel_focus: cx.focus_handle().tab_stop(true),
            add_submit_focus: cx.focus_handle().tab_stop(true),
            settings_page: SettingsPage::default(),
            settings_save_focus: cx.focus_handle().tab_stop(true),
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
            task_download_limit_input,
            task_upload_limit_input,
            task_speed_limit_dialog: None,
            task_speed_limit_dialog_focus: cx.focus_handle(),
            task_speed_limit_cancel_focus: cx.focus_handle().tab_stop(true),
            task_speed_limit_submit_focus: cx.focus_handle().tab_stop(true),
            task_seed_ratio_input,
            task_seed_time_input,
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
            status_notice: None,
            next_notice_id: 1,
            activity_log: Vec::new(),
            next_activity_id: 1,
            activity_panel_open: false,
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

        let should_refresh_details = self.details_drawer.is_some()
            && self.snapshot.commands_available()
            && (followed_task
                || session_changed
                || !previous_commands_available
                || details_revision_advanced);
        if should_refresh_details {
            self.request_current_details(cx);
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
                self.show_notice(result.command.success_label(), false, cx);
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
                        self.show_notice(error.summary, true, cx);
                    }
                } else if matches!(result.command, TaskCommandView::SetSpeedLimit { .. }) {
                    if let Some(dialog) = &mut self.task_speed_limit_dialog {
                        dialog.error = Some(error);
                    } else {
                        self.show_notice(error.summary, true, cx);
                    }
                } else if matches!(result.command, TaskCommandView::SetOptions { .. }) {
                    if let Some(dialog) = &mut self.task_options_dialog {
                        dialog.error = Some(error);
                    } else {
                        self.show_notice(error.summary, true, cx);
                    }
                } else {
                    self.show_notice(error.summary, true, cx);
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
                self.show_notice(result.command.success_label(), false, cx);
            }
            CommandOutcomeView::Failure(mut error) => {
                if error.outcome_unknown() {
                    error.summary = format!(
                        "Command outcome is unknown; AriaDeck will not retry it automatically. {}",
                        error.summary
                    );
                }
                self.show_notice(error.summary, true, cx);
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
                    .map(|failure| failure.error.summary.as_str())
                    .unwrap_or("The batch command returned no item results.");
                self.show_notice(
                    format!(
                        "{} failed for {} task{}. {detail}",
                        result.command.label(),
                        failed.len().max(result.identities.len()),
                        if failed.len().max(result.identities.len()) == 1 {
                            ""
                        } else {
                            "s"
                        }
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
                    refresh_failure = Some(error.summary);
                }
                TaskDetailsOutcomeView::Failed(error) => {
                    drawer.state = TaskDetailsLoadState::Failed { error };
                }
            }
            commands_available && drawer.overview.revision > pending.source_revision
        };

        if request_again {
            self.request_current_details(cx);
        } else if let Some(summary) = refresh_failure {
            self.show_notice(
                format!("Could not refresh task details: {summary}"),
                true,
                cx,
            );
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
                self.show_notice(
                    format!("Could not open the task path: {}", error.summary),
                    true,
                    cx,
                );
            }
        }
    }

    pub fn set_settings_save_result(
        &mut self,
        result: SettingsSaveResultView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending_settings_save.as_ref() else {
            return;
        };
        if pending.request_id != result.request_id || pending.settings != result.settings {
            return;
        }
        let source = pending.source;
        self.pending_settings_save = None;

        match result.outcome {
            SettingsSaveOutcomeView::Success => {
                self.apply_settings(result.settings, cx);
                self.settings_page.error = None;
                let message = match source {
                    SettingsSaveSource::Theme => "Appearance updated.",
                    SettingsSaveSource::Directory => "Download directory saved.",
                    SettingsSaveSource::Proxy => {
                        self.settings_proxy_password_input
                            .update(cx, |input, cx| input.set_text("", cx));
                        self.settings_page.clear_proxy_password = false;
                        "Download proxy settings saved."
                    }
                    SettingsSaveSource::SpeedLimit => {
                        // Reflect the normalized (compact) form back into the fields
                        // so a saved "2097152" re-renders as "2M".
                        let speed_limits = self.settings.speed_limits.clone();
                        self.settings_download_limit_input.update(cx, |input, cx| {
                            input.set_text(speed_limits.download_limit.clone(), cx);
                        });
                        self.settings_upload_limit_input.update(cx, |input, cx| {
                            input.set_text(speed_limits.upload_limit.clone(), cx);
                        });
                        "Speed limits saved."
                    }
                    SettingsSaveSource::TransferPolicy => {
                        let policy = self.settings.transfer_policy.clone();
                        self.settings_max_concurrent_input.update(cx, |input, cx| {
                            input.set_text(policy.max_concurrent_downloads.clone(), cx);
                        });
                        self.settings_max_connection_input.update(cx, |input, cx| {
                            input.set_text(policy.max_connection_per_server.clone(), cx);
                        });
                        self.settings_split_input.update(cx, |input, cx| {
                            input.set_text(policy.split.clone(), cx);
                        });
                        self.settings_min_split_size_input.update(cx, |input, cx| {
                            input.set_text(policy.min_split_size.clone(), cx);
                        });
                        "Transfer policy saved."
                    }
                    SettingsSaveSource::Notifications => "Notification preferences saved.",
                };
                self.show_notice(message, false, cx);
            }
            SettingsSaveOutcomeView::Failure(error) => {
                self.settings_page.error = Some(error);
                cx.notify();
            }
        }
        let _ = window;
    }

    pub fn set_startup_notice(&mut self, message: String, is_error: bool, cx: &mut Context<Self>) {
        self.show_notice(message, is_error, cx);
    }

    #[must_use]
    pub fn profiles(&self) -> &ProfileCatalogView {
        &self.profiles
    }

    pub fn set_profiles(&mut self, profiles: ProfileCatalogView, cx: &mut Context<Self>) {
        self.profiles = profiles;
        cx.notify();
    }

    pub fn request_switch_profile(&mut self, profile_id: String, cx: &mut Context<Self>) {
        let profile_id = profile_id.trim().to_owned();
        if profile_id.is_empty() {
            self.show_notice("Select a profile to activate.", true, cx);
            return;
        }
        if profile_id == self.profiles.active_profile_id {
            self.show_notice("That profile is already active.", false, cx);
            return;
        }
        if !self
            .profiles
            .profiles
            .iter()
            .any(|profile| profile.profile_id == profile_id)
        {
            self.show_notice(
                "The selected profile is no longer in the catalog.",
                true,
                cx,
            );
            return;
        }
        let request_id = self.allocate_request_id();
        self.show_notice("Switching profile...", false, cx);
        cx.emit(AppShellEvent::SwitchProfileRequested(
            SwitchProfileRequestView {
                request_id,
                profile_id,
            },
        ));
        cx.notify();
    }

    pub fn set_switch_profile_result(
        &mut self,
        result: SwitchProfileResultView,
        cx: &mut Context<Self>,
    ) {
        match result.outcome {
            SwitchProfileOutcomeView::Success => {
                self.profiles = result.catalog;
                self.show_notice(
                    "Active profile updated. Restart AriaDeck to reconnect.",
                    false,
                    cx,
                );
            }
            SwitchProfileOutcomeView::Failure(error) => {
                self.show_notice(error.summary, true, cx);
            }
        }
        cx.notify();
    }

    pub fn request_save_profile_catalog(
        &mut self,
        catalog: ProfileCatalogView,
        cx: &mut Context<Self>,
    ) {
        if catalog.profiles.is_empty() {
            self.show_notice("At least one profile is required.", true, cx);
            return;
        }
        if !catalog
            .profiles
            .iter()
            .any(|profile| profile.profile_id == catalog.active_profile_id)
        {
            self.show_notice("The active profile must exist in the catalog.", true, cx);
            return;
        }
        let request_id = self.allocate_request_id();
        let secret_updates = self.settings_page.profile_secret_updates.clone();
        self.show_notice("Saving profiles...", false, cx);
        cx.emit(AppShellEvent::SaveProfileCatalogRequested(
            SaveProfileCatalogRequestView {
                request_id,
                catalog,
                secret_updates,
            },
        ));
        cx.notify();
    }

    pub fn set_save_profile_catalog_result(
        &mut self,
        result: SaveProfileCatalogResultView,
        cx: &mut Context<Self>,
    ) {
        match result.outcome {
            SaveProfileCatalogOutcomeView::Success => {
                self.profiles = result.catalog;
                self.settings_page.profile_secret_updates.clear();
                self.settings_page.clear_profile_rpc_secret = false;
                self.settings_profile_secret_input.update(cx, |input, cx| {
                    input.set_text(String::new(), cx);
                });
                self.show_notice("Profiles saved.", false, cx);
            }
            SaveProfileCatalogOutcomeView::Failure(error) => {
                self.show_notice(error.summary, true, cx);
            }
        }
        cx.notify();
    }

    fn add_draft_local_profile(&mut self, cx: &mut Context<Self>) {
        let id = format!("draft-local-{}", self.allocate_request_id().get());
        let download_dir = self.settings.download_directory.clone();
        let name = format!("Local {}", self.profiles.profiles.len() + 1);
        self.profiles.profiles.push(ProfileEntryView {
            profile_id: id.clone(),
            name,
            kind: ProfileKindView::LocalManaged,
            // Empty = use Settings → Engine active managed core at spawn.
            executable: String::new(),
            download_dir,
            endpoint: String::new(),
            has_secret: false,
        });
        self.open_profile_editor(id, cx);
        self.show_notice(
            "Local profile draft added (uses managed core). Edit fields, Apply, then Save profiles.",
            false,
            cx,
        );
    }

    fn add_draft_remote_profile(&mut self, cx: &mut Context<Self>) {
        let id = format!("draft-remote-{}", self.allocate_request_id().get());
        let download_dir = self.settings.download_directory.clone();
        let name = format!("Remote {}", self.profiles.profiles.len() + 1);
        self.profiles.profiles.push(ProfileEntryView {
            profile_id: id.clone(),
            name,
            kind: ProfileKindView::RemoteRpc,
            executable: String::new(),
            download_dir,
            endpoint: "wss://127.0.0.1:6800/jsonrpc".into(),
            has_secret: false,
        });
        self.open_profile_editor(id, cx);
        self.show_notice(
            "Remote profile draft added. Set the endpoint, Apply, then Save profiles.",
            false,
            cx,
        );
    }

    fn open_profile_editor(&mut self, profile_id: String, cx: &mut Context<Self>) {
        let Some(profile) = self
            .profiles
            .profiles
            .iter()
            .find(|profile| profile.profile_id == profile_id)
            .cloned()
        else {
            self.show_notice("That profile is no longer in the catalog.", true, cx);
            return;
        };
        self.settings_page.editing_profile_id = Some(profile.profile_id);
        self.settings_page.draft_profile_kind = profile.kind;
        self.settings_profile_name_input.update(cx, |input, cx| {
            input.set_text(profile.name, cx);
        });
        self.settings_profile_executable_input
            .update(cx, |input, cx| {
                input.set_text(profile.executable, cx);
            });
        self.settings_profile_endpoint_input
            .update(cx, |input, cx| {
                input.set_text(profile.endpoint, cx);
            });
        self.settings_profile_download_input
            .update(cx, |input, cx| {
                input.set_text(profile.download_dir, cx);
            });
        self.settings_profile_secret_input.update(cx, |input, cx| {
            input.set_text(String::new(), cx);
        });
        self.settings_page.clear_profile_rpc_secret = false;
        cx.notify();
    }

    fn close_profile_editor(&mut self, cx: &mut Context<Self>) {
        self.settings_page.editing_profile_id = None;
        self.settings_page.clear_profile_rpc_secret = false;
        self.settings_profile_secret_input.update(cx, |input, cx| {
            input.set_text(String::new(), cx);
        });
        cx.notify();
    }

    fn apply_profile_editor(&mut self, cx: &mut Context<Self>) {
        let Some(profile_id) = self.settings_page.editing_profile_id.clone() else {
            self.show_notice("No profile is open for editing.", true, cx);
            return;
        };
        let name = self
            .settings_profile_name_input
            .read(cx)
            .text()
            .trim()
            .to_owned();
        if name.is_empty() {
            self.show_notice("Profile name cannot be empty.", true, cx);
            return;
        }
        let kind = self.settings_page.draft_profile_kind;
        let executable = self
            .settings_profile_executable_input
            .read(cx)
            .text()
            .trim()
            .to_owned();
        let endpoint = self
            .settings_profile_endpoint_input
            .read(cx)
            .text()
            .trim()
            .to_owned();
        let download_dir = self
            .settings_profile_download_input
            .read(cx)
            .text()
            .trim()
            .to_owned();
        if kind == ProfileKindView::RemoteRpc && endpoint.is_empty() {
            self.show_notice("Remote profiles need a ws/wss endpoint.", true, cx);
            return;
        }
        let Some(profile) = self
            .profiles
            .profiles
            .iter_mut()
            .find(|profile| profile.profile_id == profile_id)
        else {
            self.show_notice("That profile is no longer in the catalog.", true, cx);
            self.settings_page.editing_profile_id = None;
            cx.notify();
            return;
        };
        profile.name = name;
        profile.kind = kind;
        profile.executable = if kind == ProfileKindView::LocalManaged {
            executable
        } else {
            String::new()
        };
        profile.endpoint = if kind == ProfileKindView::RemoteRpc {
            endpoint
        } else {
            String::new()
        };
        profile.download_dir = if download_dir.is_empty() {
            self.settings.download_directory.clone()
        } else {
            download_dir
        };
        if kind == ProfileKindView::RemoteRpc {
            let secret_text = self
                .settings_profile_secret_input
                .read(cx)
                .text()
                .trim()
                .to_owned();
            let update = if self.settings_page.clear_profile_rpc_secret {
                ProfileRpcSecretUpdateView::Clear
            } else if !secret_text.is_empty() {
                ProfileRpcSecretUpdateView::Set(SecretStringView::new(secret_text))
            } else {
                ProfileRpcSecretUpdateView::Unchanged
            };
            match &update {
                ProfileRpcSecretUpdateView::Unchanged => {}
                ProfileRpcSecretUpdateView::Clear => {
                    profile.has_secret = false;
                    self.settings_page
                        .profile_secret_updates
                        .insert(profile_id.clone(), update);
                }
                ProfileRpcSecretUpdateView::Set(_) => {
                    profile.has_secret = true;
                    self.settings_page
                        .profile_secret_updates
                        .insert(profile_id.clone(), update);
                }
            }
        } else {
            profile.has_secret = false;
            self.settings_page
                .profile_secret_updates
                .remove(&profile_id);
        }
        self.settings_page.editing_profile_id = None;
        self.settings_page.clear_profile_rpc_secret = false;
        self.settings_profile_secret_input.update(cx, |input, cx| {
            input.set_text(String::new(), cx);
        });
        self.show_notice(
            "Profile updated in the draft catalog. Click Save profiles to persist.",
            false,
            cx,
        );
        cx.notify();
    }

    fn request_remove_profile(&mut self, profile_id: String, cx: &mut Context<Self>) {
        if self.profiles.profiles.len() <= 1 {
            self.show_notice("At least one profile must remain.", true, cx);
            return;
        }
        let Some(profile) = self
            .profiles
            .profiles
            .iter()
            .find(|profile| profile.profile_id == profile_id)
        else {
            self.show_notice("That profile is no longer in the catalog.", true, cx);
            return;
        };
        self.settings_page.pending_profile_delete = Some(PendingProfileDelete {
            profile_id: profile.profile_id.clone(),
            name: profile.name.clone(),
        });
        cx.notify();
    }

    fn cancel_remove_profile(&mut self, cx: &mut Context<Self>) {
        self.settings_page.pending_profile_delete = None;
        cx.notify();
    }

    fn confirm_remove_profile(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.settings_page.pending_profile_delete.take() else {
            return;
        };
        self.remove_profile(pending.profile_id, cx);
    }

    fn remove_profile(&mut self, profile_id: String, cx: &mut Context<Self>) {
        if self.profiles.profiles.len() <= 1 {
            self.show_notice("At least one profile must remain.", true, cx);
            return;
        }
        let Some(index) = self
            .profiles
            .profiles
            .iter()
            .position(|profile| profile.profile_id == profile_id)
        else {
            self.show_notice("That profile is no longer in the catalog.", true, cx);
            return;
        };
        let removed = self.profiles.profiles.remove(index);
        self.settings_page
            .profile_secret_updates
            .remove(&profile_id);
        if self.settings_page.editing_profile_id.as_deref() == Some(profile_id.as_str()) {
            self.settings_page.editing_profile_id = None;
        }
        if self.profiles.active_profile_id == profile_id {
            self.profiles.active_profile_id = self
                .profiles
                .profiles
                .first()
                .map(|profile| profile.profile_id.clone())
                .unwrap_or_default();
        }
        let catalog = self.profiles.clone();
        self.show_notice(
            format!("Removed “{}”. Saving catalog…", removed.name),
            false,
            cx,
        );
        self.request_save_profile_catalog(catalog, cx);
    }

    fn toggle_clear_profile_rpc_secret(&mut self, cx: &mut Context<Self>) {
        let clear = !self.settings_page.clear_profile_rpc_secret;
        if clear {
            self.settings_profile_secret_input.update(cx, |input, cx| {
                input.set_text(String::new(), cx);
            });
        }
        self.settings_page.clear_profile_rpc_secret = clear;
        cx.notify();
    }

    fn select_profile_editor_kind(&mut self, kind: ProfileKindView, cx: &mut Context<Self>) {
        self.settings_page.draft_profile_kind = kind;
        if kind == ProfileKindView::LocalManaged {
            self.settings_page.clear_profile_rpc_secret = false;
            self.settings_profile_secret_input.update(cx, |input, cx| {
                input.set_text(String::new(), cx);
            });
        }
        cx.notify();
    }

    pub fn set_cores(&mut self, cores: CoreRegistryView, cx: &mut Context<Self>) {
        self.cores = cores;
        cx.notify();
    }

    pub fn request_core_command(&mut self, command: CoreCommandView, cx: &mut Context<Self>) {
        let request_id = self.allocate_request_id();
        let notice = match &command {
            CoreCommandView::Import { .. } => "Importing aria2 core...",
            CoreCommandView::Link { .. } => "Linking aria2 core...",
            CoreCommandView::Verify { .. } => "Verifying aria2 core...",
            CoreCommandView::Activate { .. } => "Activating aria2 core...",
            CoreCommandView::Rollback => "Rolling back to last working core...",
            CoreCommandView::Remove { .. } => "Removing aria2 core...",
        };
        self.show_notice(notice, false, cx);
        cx.emit(AppShellEvent::CoreCommandRequested(
            CoreCommandRequestView {
                request_id,
                command,
            },
        ));
        cx.notify();
    }

    pub fn set_core_command_result(
        &mut self,
        result: CoreCommandResultView,
        cx: &mut Context<Self>,
    ) {
        match result.outcome {
            CoreCommandOutcomeView::Success => {
                self.cores = result.registry;
                let message = match result.command {
                    CoreCommandView::Import { .. } => {
                        "aria2 core imported. Activate it, then restart AriaDeck to use it."
                    }
                    CoreCommandView::Link { .. } => {
                        "aria2 core linked. Activate it, then restart AriaDeck to use it."
                    }
                    CoreCommandView::Verify { .. } => "aria2 core verified.",
                    CoreCommandView::Activate { .. } => {
                        "Active aria2 core updated. Restart AriaDeck to start that version."
                    }
                    CoreCommandView::Rollback => {
                        "Rolled back to the last working core. Restart AriaDeck to apply."
                    }
                    CoreCommandView::Remove { .. } => "aria2 core removed.",
                };
                self.show_notice(message, false, cx);
            }
            CoreCommandOutcomeView::Failure(error) => {
                self.show_notice(error.summary, true, cx);
            }
        }
        cx.notify();
    }

    fn request_import_core_from_input(&mut self, cx: &mut Context<Self>) {
        let path = self
            .settings_core_path_input
            .read(cx)
            .text()
            .trim()
            .to_owned();
        if path.is_empty() {
            self.show_notice("Enter a path to an aria2c executable first.", true, cx);
            return;
        }
        self.request_core_command(CoreCommandView::Import { path }, cx);
    }

    fn request_link_core_from_input(&mut self, cx: &mut Context<Self>) {
        let path = self
            .settings_core_path_input
            .read(cx)
            .text()
            .trim()
            .to_owned();
        if path.is_empty() {
            self.show_notice("Enter a path to an aria2c executable first.", true, cx);
            return;
        }
        self.request_core_command(CoreCommandView::Link { path }, cx);
    }

    #[must_use]
    pub fn settings(&self) -> &SettingsView {
        &self.settings
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

    fn emit_query(&self, cx: &mut Context<Self>) {
        cx.emit(AppShellEvent::QueryChanged(self.query.clone()));
        cx.notify();
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
        } else if !self.search_input.read(cx).text().is_empty() {
            self.search_input
                .update(cx, |input, cx| input.set_text("", cx));
        } else if !self.selected_tasks.is_empty() {
            self.clear_task_selection();
            cx.notify();
        } else if self.details_drawer.take().is_some() {
            window.focus(&self.focus_handle, cx);
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

    fn apply_settings(&mut self, settings: SettingsView, cx: &mut Context<Self>) {
        self.theme = theme_for_scheme(settings.color_scheme);
        self.settings = settings.clone();
        self.settings_page.draft_color_scheme = settings.color_scheme;
        self.settings_page.draft_file_allocation = settings.transfer_policy.file_allocation;
        self.settings_page.draft_check_integrity = settings.transfer_policy.check_integrity;
        self.settings_page.draft_notification_volume = settings.notifications.volume;
        self.settings_page.draft_notify_on_completion = settings.notifications.notify_on_completion;
        self.settings_page.draft_notify_on_error = settings.notifications.notify_on_error;
        self.settings_page.draft_notify_on_engine_events =
            settings.notifications.notify_on_engine_events;
        self.search_input
            .update(cx, |input, cx| input.set_theme(self.theme, cx));
        self.add_input
            .update(cx, |input, cx| input.set_theme(self.theme, cx));
        for input in [
            &self.add_referer_input,
            &self.add_user_agent_input,
            &self.add_headers_input,
            &self.add_cookie_input,
            &self.add_http_user_input,
            &self.add_http_passwd_input,
            &self.add_checksum_input,
        ] {
            input.update(cx, |input, cx| input.set_theme(self.theme, cx));
        }
        self.output_name_input
            .update(cx, |input, cx| input.set_theme(self.theme, cx));
        self.settings_directory_input
            .update(cx, |input, cx| input.set_theme(self.theme, cx));
        for input in [
            &self.settings_all_proxy_input,
            &self.settings_http_proxy_input,
            &self.settings_https_proxy_input,
            &self.settings_ftp_proxy_input,
            &self.settings_no_proxy_input,
            &self.settings_proxy_username_input,
            &self.settings_proxy_password_input,
            &self.settings_download_limit_input,
            &self.settings_upload_limit_input,
            &self.settings_max_concurrent_input,
            &self.settings_max_connection_input,
            &self.settings_split_input,
            &self.settings_min_split_size_input,
        ] {
            input.update(cx, |input, cx| input.set_theme(self.theme, cx));
        }
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
        }
    }

    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == AppPage::Settings {
            window.focus(&self.settings_directory_input.focus_handle(cx), cx);
            return;
        }
        if self.add_dialog.open
            || self.output_name_dialog.is_some()
            || self.remove_confirmation.is_some()
            || self.batch_failure_details.is_some()
        {
            return;
        }
        let download_directory = self.settings.download_directory.clone();
        self.settings_directory_input
            .update(cx, |input, cx| input.set_text(download_directory, cx));
        let proxy = self.settings.download_proxy.clone();
        self.settings_all_proxy_input.update(cx, |input, cx| {
            input.set_text(proxy.all_proxy.clone(), cx);
        });
        self.settings_http_proxy_input.update(cx, |input, cx| {
            input.set_text(proxy.http_proxy.clone(), cx);
        });
        self.settings_https_proxy_input.update(cx, |input, cx| {
            input.set_text(proxy.https_proxy.clone(), cx);
        });
        self.settings_ftp_proxy_input.update(cx, |input, cx| {
            input.set_text(proxy.ftp_proxy.clone(), cx);
        });
        self.settings_no_proxy_input.update(cx, |input, cx| {
            input.set_text(proxy.no_proxy.join(", "), cx);
        });
        self.settings_proxy_username_input.update(cx, |input, cx| {
            input.set_text(proxy.username.clone(), cx);
        });
        self.settings_proxy_password_input
            .update(cx, |input, cx| input.set_text("", cx));
        let speed_limits = self.settings.speed_limits.clone();
        self.settings_download_limit_input.update(cx, |input, cx| {
            input.set_text(speed_limits.download_limit.clone(), cx);
        });
        self.settings_upload_limit_input.update(cx, |input, cx| {
            input.set_text(speed_limits.upload_limit.clone(), cx);
        });
        let transfer_policy = self.settings.transfer_policy.clone();
        self.settings_max_concurrent_input.update(cx, |input, cx| {
            input.set_text(transfer_policy.max_concurrent_downloads.clone(), cx);
        });
        self.settings_max_connection_input.update(cx, |input, cx| {
            input.set_text(transfer_policy.max_connection_per_server.clone(), cx);
        });
        self.settings_split_input.update(cx, |input, cx| {
            input.set_text(transfer_policy.split.clone(), cx);
        });
        self.settings_min_split_size_input.update(cx, |input, cx| {
            input.set_text(transfer_policy.min_split_size.clone(), cx);
        });
        self.page = AppPage::Settings;
        self.details_drawer = None;
        self.speed_popover_open = false;
        self.activity_panel_open = false;
        self.settings_page = SettingsPage {
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
            draft_color_scheme: self.settings.color_scheme,
            draft_proxy_mode: proxy.mode,
            draft_file_allocation: transfer_policy.file_allocation,
            draft_check_integrity: transfer_policy.check_integrity,
            draft_notification_volume: self.settings.notifications.volume,
            draft_notify_on_completion: self.settings.notifications.notify_on_completion,
            draft_notify_on_error: self.settings.notifications.notify_on_error,
            draft_notify_on_engine_events: self.settings.notifications.notify_on_engine_events,
            clear_proxy_password: false,
            editing_profile_id: None,
            draft_profile_kind: ProfileKindView::LocalManaged,
            profile_secret_updates: std::collections::HashMap::new(),
            pending_profile_delete: None,
            clear_profile_rpc_secret: false,
            error: None,
        };
        cx.notify();
    }

    fn close_settings_action(
        &mut self,
        _: &CloseSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_settings(window, cx);
    }

    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings {
            return;
        }
        let previous_focus = self.settings_page.previous_focus.take();
        self.page = AppPage::Downloads;
        if let Some(focus) = previous_focus.and_then(|focus| focus.upgrade()) {
            window.focus(&focus, cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    fn save_settings_action(
        &mut self,
        _: &SaveSettings,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_settings(cx);
    }

    fn submit_settings(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        let download_directory = self
            .settings_directory_input
            .read(cx)
            .text()
            .trim()
            .to_owned();
        if download_directory.is_empty() {
            self.settings_page.error = Some(OperationErrorView {
                code: "settings.invalid_download_directory".into(),
                summary: "Choose a non-empty download directory.".into(),
                retryable: false,
            });
            cx.notify();
            return;
        }
        let mut settings = self.settings.clone();
        settings.download_directory = download_directory;
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Directory,
            cx,
        );
    }

    fn select_color_scheme(&mut self, scheme: ColorSchemeView, cx: &mut Context<Self>) {
        if self.pending_settings_save.is_some() || scheme == self.settings.color_scheme {
            return;
        }
        self.settings_page.draft_color_scheme = scheme;
        let mut settings = self.settings.clone();
        settings.color_scheme = scheme;
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Theme,
            cx,
        );
    }

    fn select_proxy_mode(&mut self, mode: ProxyModeView, cx: &mut Context<Self>) {
        if self.pending_settings_save.is_some() || mode == self.settings_page.draft_proxy_mode {
            return;
        }
        self.settings_page.draft_proxy_mode = mode;
        self.settings_page.error = None;
        cx.notify();
    }

    fn clear_saved_proxy_password(&mut self, cx: &mut Context<Self>) {
        if self.pending_settings_save.is_some() || !self.settings.download_proxy.has_password {
            return;
        }
        let clear = !self.settings_page.clear_proxy_password;
        if clear {
            self.settings_proxy_password_input
                .update(cx, |input, cx| input.set_text("", cx));
        }
        self.settings_page.clear_proxy_password = clear;
        self.settings_page.error = None;
        cx.notify();
    }

    fn submit_proxy_settings(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        let mut settings = self.settings.clone();
        let password = self
            .settings_proxy_password_input
            .read(cx)
            .text()
            .to_owned();
        let password_update = if self.settings_page.clear_proxy_password {
            ProxyPasswordUpdateView::Clear
        } else if password.is_empty() {
            ProxyPasswordUpdateView::Unchanged
        } else {
            ProxyPasswordUpdateView::Set(SecretStringView::new(password))
        };
        settings.download_proxy = DownloadProxySettingsView {
            mode: self.settings_page.draft_proxy_mode,
            all_proxy: self.settings_all_proxy_input.read(cx).text().trim().into(),
            http_proxy: self.settings_http_proxy_input.read(cx).text().trim().into(),
            https_proxy: self
                .settings_https_proxy_input
                .read(cx)
                .text()
                .trim()
                .into(),
            ftp_proxy: self.settings_ftp_proxy_input.read(cx).text().trim().into(),
            no_proxy: self
                .settings_no_proxy_input
                .read(cx)
                .text()
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
            username: self
                .settings_proxy_username_input
                .read(cx)
                .text()
                .trim()
                .into(),
            has_password: match &password_update {
                ProxyPasswordUpdateView::Unchanged => self.settings.download_proxy.has_password,
                ProxyPasswordUpdateView::Clear => false,
                ProxyPasswordUpdateView::Set(_) => true,
            },
        };
        self.request_settings_save(settings, password_update, SettingsSaveSource::Proxy, cx);
    }

    fn submit_speed_limits(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        let download_limit = self
            .settings_download_limit_input
            .read(cx)
            .text()
            .trim()
            .to_owned();
        let upload_limit = self
            .settings_upload_limit_input
            .read(cx)
            .text()
            .trim()
            .to_owned();
        let draft = SpeedLimitSettingsView {
            download_limit,
            upload_limit,
        };
        if !draft.is_valid() {
            self.settings_page.error = Some(OperationErrorView {
                code: "settings.invalid_speed_limit".into(),
                summary: "Enter a speed as bytes/second or a K/M/G value, or leave it blank for unlimited.".into(),
                retryable: false,
            });
            cx.notify();
            return;
        }
        let mut settings = self.settings.clone();
        settings.speed_limits = draft;
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::SpeedLimit,
            cx,
        );
    }

    fn submit_transfer_policy(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        let draft = TransferPolicySettingsView {
            max_concurrent_downloads: self
                .settings_max_concurrent_input
                .read(cx)
                .text()
                .trim()
                .into(),
            max_connection_per_server: self
                .settings_max_connection_input
                .read(cx)
                .text()
                .trim()
                .into(),
            split: self.settings_split_input.read(cx).text().trim().into(),
            min_split_size: self
                .settings_min_split_size_input
                .read(cx)
                .text()
                .trim()
                .into(),
            file_allocation: self.settings_page.draft_file_allocation,
            check_integrity: self.settings_page.draft_check_integrity,
        };
        if !draft.is_valid() {
            self.settings_page.error = Some(OperationErrorView {
                code: "settings.invalid_transfer_policy".into(),
                summary: "Enter positive integers for concurrent downloads, connections (1-16), and split, plus a positive min-split size (for example 1M).".into(),
                retryable: false,
            });
            cx.notify();
            return;
        }
        let mut settings = self.settings.clone();
        settings.transfer_policy = draft;
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::TransferPolicy,
            cx,
        );
    }

    fn select_file_allocation(&mut self, method: FileAllocationView, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        if self.settings_page.draft_file_allocation == method {
            return;
        }
        self.settings_page.draft_file_allocation = method;
        self.settings_page.error = None;
        cx.notify();
    }

    fn toggle_check_integrity(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        self.settings_page.draft_check_integrity = !self.settings_page.draft_check_integrity;
        self.settings_page.error = None;
        cx.notify();
    }

    fn submit_notifications(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        let draft = NotificationSettingsView {
            volume: self.settings_page.draft_notification_volume,
            notify_on_completion: self.settings_page.draft_notify_on_completion,
            notify_on_error: self.settings_page.draft_notify_on_error,
            notify_on_engine_events: self.settings_page.draft_notify_on_engine_events,
        };
        if draft == self.settings.notifications {
            return;
        }
        let mut settings = self.settings.clone();
        settings.notifications = draft;
        self.request_settings_save(
            settings,
            ProxyPasswordUpdateView::Unchanged,
            SettingsSaveSource::Notifications,
            cx,
        );
    }

    fn select_notification_volume(
        &mut self,
        volume: NotificationVolumeView,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        if self.settings_page.draft_notification_volume == volume {
            return;
        }
        self.settings_page.draft_notification_volume = volume;
        self.settings_page.error = None;
        cx.notify();
    }

    fn toggle_notify_on_completion(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        self.settings_page.draft_notify_on_completion =
            !self.settings_page.draft_notify_on_completion;
        self.settings_page.error = None;
        cx.notify();
    }

    fn toggle_notify_on_error(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        self.settings_page.draft_notify_on_error = !self.settings_page.draft_notify_on_error;
        self.settings_page.error = None;
        cx.notify();
    }

    fn toggle_notify_on_engine_events(&mut self, cx: &mut Context<Self>) {
        if self.page != AppPage::Settings || self.pending_settings_save.is_some() {
            return;
        }
        self.settings_page.draft_notify_on_engine_events =
            !self.settings_page.draft_notify_on_engine_events;
        self.settings_page.error = None;
        cx.notify();
    }

    fn request_settings_save(
        &mut self,
        settings: SettingsView,
        proxy_password: ProxyPasswordUpdateView,
        source: SettingsSaveSource,
        cx: &mut Context<Self>,
    ) {
        if self.pending_settings_save.is_some() {
            return;
        }
        let request_id = self.allocate_request_id();
        self.pending_settings_save = Some(PendingSettingsSave {
            request_id,
            settings: settings.clone(),
            source,
        });
        self.settings_page.error = None;
        cx.emit(AppShellEvent::SettingsSaveRequested(
            SettingsSaveRequestView {
                request_id,
                settings,
                proxy_password,
            },
        ));
        cx.notify();
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

    fn show_notice(&mut self, message: impl Into<String>, is_error: bool, cx: &mut Context<Self>) {
        // Command/action feedback always surfaces unless Silent is selected.
        self.show_notice_inner(message, is_error, false, cx);
    }

    fn show_automatic_notice(
        &mut self,
        message: impl Into<String>,
        is_error: bool,
        engine_event: bool,
        cx: &mut Context<Self>,
    ) {
        let prefs = self.settings.notifications;
        if prefs.volume == NotificationVolumeView::Silent {
            return;
        }
        if prefs.volume == NotificationVolumeView::Quiet {
            return;
        }
        if engine_event && !prefs.notify_on_engine_events {
            return;
        }
        if !engine_event && is_error && !prefs.notify_on_error {
            return;
        }
        if !engine_event && !is_error && !prefs.notify_on_completion {
            return;
        }
        self.show_notice_inner(message, is_error, true, cx);
    }

    fn show_notice_inner(
        &mut self,
        message: impl Into<String>,
        is_error: bool,
        automatic: bool,
        cx: &mut Context<Self>,
    ) {
        if self.settings.notifications.volume == NotificationVolumeView::Silent {
            // Silent still records history but suppresses every toast surface.
            return;
        }
        if automatic && self.settings.notifications.volume == NotificationVolumeView::Quiet {
            return;
        }
        let id = self.next_notice_id;
        self.next_notice_id = self.next_notice_id.checked_add(1).unwrap_or(1);
        self.status_notice = Some(StatusNotice {
            id,
            message: message.into(),
            is_error,
            automatic,
        });
        cx.notify();
        if !is_error {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(3)).await;
                this.update(cx, |this, cx| {
                    this.expire_notice(id, cx);
                })
                .ok();
            })
            .detach();
        }
    }

    fn record_activity(
        &mut self,
        kind: ActivityKindView,
        summary: impl Into<String>,
        detail: Option<String>,
        task: Option<TaskIdentity>,
        count: u32,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_activity_id;
        self.next_activity_id = self.next_activity_id.checked_add(1).unwrap_or(1);
        self.activity_log.insert(
            0,
            ActivityEntryView {
                id,
                kind,
                summary: summary.into(),
                detail,
                task,
                count: count.max(1),
            },
        );
        if self.activity_log.len() > ACTIVITY_HISTORY_LIMIT {
            self.activity_log.truncate(ACTIVITY_HISTORY_LIMIT);
        }
        cx.notify();
    }

    fn observe_task_status_transitions(
        &mut self,
        previous: &WorkspaceSnapshot,
        cx: &mut Context<Self>,
    ) {
        // First connected snapshot after connect/session change only seeds the map.
        let seed_only = previous.profile_id != self.snapshot.profile_id
            || previous.session_id != self.snapshot.session_id
            || previous.generation != self.snapshot.generation
            || self.known_task_status.is_empty();

        let mut completed: Vec<(TaskIdentity, String)> = Vec::new();
        let mut failed: Vec<(TaskIdentity, String, Option<String>)> = Vec::new();

        for task in &self.snapshot.tasks {
            let previous_status = self.known_task_status.get(&task.identity).copied();
            self.known_task_status
                .insert(task.identity.clone(), task.status);
            if seed_only {
                continue;
            }
            let Some(previous_status) = previous_status else {
                // First sighting of a task is not a transition event.
                continue;
            };
            if previous_status == task.status || previous_status.is_terminal() {
                continue;
            }
            match task.status {
                TaskStatusView::Complete => {
                    completed.push((task.identity.clone(), task_display_name(task)));
                }
                TaskStatusView::Failed => {
                    let detail = task.error.as_ref().map(|error| {
                        if let Some(details) = error.details.as_ref() {
                            format!("{} ({details})", error.summary)
                        } else {
                            error.summary.clone()
                        }
                    });
                    failed.push((task.identity.clone(), task_display_name(task), detail));
                }
                _ => {}
            }
        }

        // Drop identities that left the loaded workspace to bound memory.
        self.known_task_status.retain(|identity, _| {
            self.snapshot
                .tasks
                .iter()
                .any(|task| &task.identity == identity)
        });

        if seed_only {
            return;
        }

        if !completed.is_empty() {
            let count = completed.len() as u32;
            let summary = if count == 1 {
                format!("{} finished downloading.", completed[0].1)
            } else {
                format!("{count} downloads finished.")
            };
            let detail = if count == 1 {
                None
            } else {
                let listed = completed
                    .iter()
                    .take(5)
                    .map(|(_, name)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                Some(if count > 5 {
                    format!("{listed}, …")
                } else {
                    listed
                })
            };
            let task = (count == 1).then(|| completed[0].0.clone());
            self.record_activity(
                ActivityKindView::Completion,
                summary.clone(),
                detail,
                task,
                count,
                cx,
            );
            self.show_automatic_notice(summary, false, false, cx);
        }

        if !failed.is_empty() {
            let count = failed.len() as u32;
            let summary = if count == 1 {
                format!("{} failed.", failed[0].1)
            } else {
                format!("{count} downloads failed.")
            };
            let detail = if count == 1 {
                failed[0].2.clone()
            } else {
                let listed = failed
                    .iter()
                    .take(5)
                    .map(|(_, name, _)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                Some(if count > 5 {
                    format!("{listed}, …")
                } else {
                    listed
                })
            };
            let task = (count == 1).then(|| failed[0].0.clone());
            self.record_activity(
                ActivityKindView::Error,
                summary.clone(),
                detail,
                task,
                count,
                cx,
            );
            self.show_automatic_notice(summary, true, false, cx);
        }
    }

    fn toggle_activity_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.activity_panel_open {
            self.close_activity_panel(window, cx);
            return;
        }
        self.speed_popover_open = false;
        self.sort_popover_open = false;
        self.context_menu = None;
        self.activity_panel_open = true;
        cx.notify();
        let _ = window;
    }

    fn close_activity_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.activity_panel_open {
            return;
        }
        self.activity_panel_open = false;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn clear_activity_log(&mut self, cx: &mut Context<Self>) {
        if self.activity_log.is_empty() {
            return;
        }
        self.activity_log.clear();
        cx.notify();
    }

    fn expire_notice(&mut self, id: u64, cx: &mut Context<Self>) {
        if self
            .status_notice
            .as_ref()
            .is_some_and(|notice| notice.id == id && !notice.is_error)
        {
            self.status_notice = None;
            cx.notify();
        }
    }

    fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        if self.status_notice.take().is_some() {
            cx.notify();
        }
    }

    fn allocate_request_id(&mut self) -> RequestId {
        let request_id = RequestId::from_u64(self.next_request_id);
        self.next_request_id = self.next_request_id.checked_add(1).unwrap_or(1);
        request_id
    }

    fn open_add_download(
        &mut self,
        _: &OpenAddDownload,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.page = AppPage::Downloads;
        self.speed_popover_open = false;
        if self.add_dialog.open {
            window.focus(&self.add_input.focus_handle(cx), cx);
            return;
        }
        if self.output_name_dialog.is_some()
            || self.remove_confirmation.is_some()
            || self.batch_failure_details.is_some()
        {
            return;
        }
        if self.pending_task_command.is_some() || self.pending_batch_command.is_some() {
            return;
        }
        if !self.snapshot.commands_available() {
            self.show_notice(
                "Connect and finish synchronization before adding a download.",
                true,
                cx,
            );
            return;
        }

        self.add_input
            .update(cx, |input, cx| input.set_text("", cx));
        for input in [
            &self.add_referer_input,
            &self.add_user_agent_input,
            &self.add_headers_input,
            &self.add_cookie_input,
            &self.add_http_user_input,
            &self.add_http_passwd_input,
            &self.add_checksum_input,
        ] {
            input.update(cx, |input, cx| input.set_text("", cx));
        }
        self.add_dialog = AddDownloadDialog {
            open: true,
            input_mode: AddDownloadInputModeView::Links,
            mode: AddDownloadModeView::SeparateTasks,
            file_conflict: FileConflictPolicyView::AutoRename,
            advanced_open: false,
            metadata_files: Vec::new(),
            active_metadata_file: None,
            preview_pending: None,
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
            pending: None,
            error: None,
            results: Vec::new(),
            updating_input_from_result: false,
        };
        cx.notify();
        cx.defer_in(window, |this, window, cx| {
            if this.add_dialog.open && this.add_dialog.input_mode == AddDownloadInputModeView::Links
            {
                window.focus(&this.add_input.focus_handle(cx), cx);
            }
        });
    }

    fn close_add_download_action(
        &mut self,
        _: &CloseAddDownload,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_add_download(window, cx);
    }

    fn close_add_download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.add_dialog.open
            || self.add_dialog.pending.is_some()
            || self.add_dialog.preview_pending.is_some()
        {
            return;
        }
        let restore_focus = self.add_dialog_focus.contains_focused(window, cx)
            || self.add_input.focus_handle(cx).is_focused(window);
        let previous_focus = self.add_dialog.previous_focus.take();
        self.add_dialog = AddDownloadDialog::default();
        if restore_focus {
            if let Some(focus) = previous_focus.and_then(|focus| focus.upgrade()) {
                window.focus(&focus, cx);
            } else {
                window.focus(&self.focus_handle, cx);
            }
        }
        cx.notify();
    }

    fn submit_add_download_action(
        &mut self,
        _: &SubmitAddDownload,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_add_download(cx);
    }

    fn submit_add_download(&mut self, cx: &mut Context<Self>) {
        if !self.add_dialog.open
            || self.add_dialog.pending.is_some()
            || self.add_dialog.preview_pending.is_some()
        {
            return;
        }
        let sources = match self.add_dialog.input_mode {
            AddDownloadInputModeView::Links => {
                parse_add_download_sources(self.add_input.read(cx).text())
            }
            AddDownloadInputModeView::MetadataFiles => self
                .add_dialog
                .metadata_files
                .iter()
                .map(|preview| AddDownloadSourceView::MetadataFile {
                    path: preview.path.clone(),
                    kind: preview.kind,
                    content_sha256: preview.content_sha256.clone(),
                    info_hash: preview.info_hash.clone(),
                    selected_file_indices: preview.selected_file_indices.clone(),
                })
                .collect(),
        };
        if sources.is_empty() {
            self.add_dialog.error = Some(OperationErrorView {
                code: "validation.invalid_request".into(),
                summary: match self.add_dialog.input_mode {
                    AddDownloadInputModeView::Links => {
                        "Enter at least one URL or magnet link.".into()
                    }
                    AddDownloadInputModeView::MetadataFiles => {
                        "Choose at least one Torrent or Metalink file.".into()
                    }
                },
                retryable: false,
            });
            cx.notify();
            return;
        }
        if let Some(preview) = self
            .add_dialog
            .metadata_files
            .iter()
            .find(|preview| preview.selected_file_indices.is_empty())
        {
            self.add_dialog.error = Some(OperationErrorView {
                code: "validation.invalid_request".into(),
                summary: format!("Select at least one file from {}.", preview.path.display()),
                retryable: false,
            });
            cx.notify();
            return;
        }
        let required_bytes = if self.add_dialog.input_mode
            == AddDownloadInputModeView::MetadataFiles
        {
            match selected_metadata_known_bytes(&self.add_dialog.metadata_files) {
                Some(bytes) => Some(bytes),
                None => {
                    self.add_dialog.error = Some(OperationErrorView {
                        code: "validation.invalid_request".into(),
                        summary: "Selected metadata file sizes exceed the supported range.".into(),
                        retryable: false,
                    });
                    cx.notify();
                    return;
                }
            }
        } else {
            None
        };
        let Some(session) = self
            .snapshot
            .commands_available()
            .then(|| self.snapshot.engine_session())
            .flatten()
        else {
            self.add_dialog.error = Some(stale_session_error());
            cx.notify();
            return;
        };

        let request_id = self.allocate_request_id();
        self.add_dialog.pending = Some(PendingAddDownload {
            request_id,
            session: session.clone(),
        });
        self.add_dialog.error = None;
        let advanced = if self.add_dialog.input_mode == AddDownloadInputModeView::Links {
            self.collect_add_advanced_options(cx)
        } else {
            AddDownloadAdvancedOptionsView::default()
        };
        cx.emit(AppShellEvent::AddDownloadRequested(
            AddDownloadRequestView {
                request_id,
                session,
                sources,
                mode: if self.add_dialog.input_mode == AddDownloadInputModeView::Links {
                    self.add_dialog.mode
                } else {
                    AddDownloadModeView::SeparateTasks
                },
                destination: (!self.settings.download_directory.is_empty())
                    .then(|| self.settings.download_directory.clone()),
                required_bytes,
                file_conflict: if self.add_dialog.input_mode == AddDownloadInputModeView::Links {
                    self.add_dialog.file_conflict
                } else {
                    FileConflictPolicyView::Reject
                },
                advanced,
            },
        ));
        cx.notify();
    }

    fn set_add_download_mode(&mut self, mode: AddDownloadModeView, cx: &mut Context<Self>) {
        if self.add_dialog.pending.is_some()
            || self.add_dialog.preview_pending.is_some()
            || self.add_dialog.mode == mode
        {
            return;
        }
        self.add_dialog.mode = mode;
        self.add_dialog.error = None;
        self.add_dialog.results.clear();
        cx.notify();
    }

    fn set_add_input_mode(&mut self, mode: AddDownloadInputModeView, cx: &mut Context<Self>) {
        if self.add_dialog.pending.is_some()
            || self.add_dialog.preview_pending.is_some()
            || self.add_dialog.input_mode == mode
        {
            return;
        }
        self.add_dialog.input_mode = mode;
        self.add_dialog.error = None;
        self.add_dialog.results.clear();
        cx.notify();
    }

    fn choose_metadata_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.add_dialog.open
            || self.add_dialog.pending.is_some()
            || self.add_dialog.preview_pending.is_some()
        {
            return;
        }
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Choose Torrent or Metalink files".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let selected = selected.await;
            let _ = this.update_in(cx, |this, window, cx| match selected {
                Ok(Ok(Some(paths))) => this.add_metadata_paths(paths, window, cx),
                Ok(Ok(None)) => {}
                Ok(Err(error)) => {
                    this.set_add_dialog_error(format!("File picker failed: {error}"), cx);
                }
                Err(error) => {
                    this.set_add_dialog_error(
                        format!("File picker closed unexpectedly: {error}"),
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    fn add_metadata_paths(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.add_dialog.open {
            self.open_add_download(&OpenAddDownload, window, cx);
        }
        if !self.add_dialog.open
            || self.add_dialog.pending.is_some()
            || self.add_dialog.preview_pending.is_some()
        {
            return;
        }

        let mut known = self
            .add_dialog
            .metadata_files
            .iter()
            .map(|preview| metadata_path_key(&preview.path))
            .collect::<HashSet<_>>();
        let mut invalid = Vec::new();
        let mut accepted = Vec::new();
        for path in paths {
            if metadata_kind_from_path(&path).is_none() {
                invalid.push(path);
                continue;
            }
            if known.insert(metadata_path_key(&path)) {
                accepted.push(path);
            }
        }
        self.add_dialog.input_mode = AddDownloadInputModeView::MetadataFiles;
        self.add_dialog.mode = AddDownloadModeView::SeparateTasks;
        self.add_dialog.file_conflict = FileConflictPolicyView::Reject;
        self.add_dialog.results.clear();
        self.add_dialog.error = if invalid.is_empty() {
            None
        } else {
            Some(OperationErrorView {
                code: "validation.unsupported_metadata_file".into(),
                summary: format!(
                    "Skipped {} file{}; supported extensions are .torrent, .metalink, and .meta4.",
                    invalid.len(),
                    if invalid.len() == 1 { "" } else { "s" }
                ),
                retryable: false,
            })
        };
        if !accepted.is_empty() {
            let request_id = self.allocate_request_id();
            self.add_dialog.preview_pending = Some(PendingMetadataPreview {
                request_id,
                paths: accepted.clone(),
            });
            cx.emit(AppShellEvent::AddDownloadMetadataPreviewRequested(
                AddDownloadMetadataPreviewRequestView {
                    request_id,
                    paths: accepted,
                },
            ));
        }
        cx.notify();
    }

    fn remove_metadata_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.add_dialog.pending.is_some()
            || self.add_dialog.preview_pending.is_some()
            || index >= self.add_dialog.metadata_files.len()
        {
            return;
        }
        self.add_dialog.metadata_files.remove(index);
        self.add_dialog.active_metadata_file = if self.add_dialog.metadata_files.is_empty() {
            None
        } else {
            Some(
                self.add_dialog
                    .active_metadata_file
                    .unwrap_or_default()
                    .min(self.add_dialog.metadata_files.len() - 1),
            )
        };
        self.add_dialog.error = None;
        self.add_dialog.results.clear();
        cx.notify();
    }

    fn select_metadata_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.add_dialog.pending.is_none()
            && self.add_dialog.preview_pending.is_none()
            && index < self.add_dialog.metadata_files.len()
            && self.add_dialog.active_metadata_file != Some(index)
        {
            self.add_dialog.active_metadata_file = Some(index);
            cx.notify();
        }
    }

    fn toggle_metadata_file_entry(
        &mut self,
        preview_index: usize,
        file_index: u32,
        cx: &mut Context<Self>,
    ) {
        if self.add_dialog.pending.is_some() || self.add_dialog.preview_pending.is_some() {
            return;
        }
        let Some(preview) = self.add_dialog.metadata_files.get_mut(preview_index) else {
            return;
        };
        match preview.selected_file_indices.binary_search(&file_index) {
            Ok(position) => {
                preview.selected_file_indices.remove(position);
            }
            Err(position) if preview.files.iter().any(|file| file.index == file_index) => {
                preview.selected_file_indices.insert(position, file_index);
            }
            Err(_) => return,
        }
        self.add_dialog.error = None;
        self.add_dialog.results.clear();
        cx.notify();
    }

    fn toggle_all_metadata_file_entries(&mut self, preview_index: usize, cx: &mut Context<Self>) {
        if self.add_dialog.pending.is_some() || self.add_dialog.preview_pending.is_some() {
            return;
        }
        let Some(preview) = self.add_dialog.metadata_files.get_mut(preview_index) else {
            return;
        };
        if preview.selected_file_indices.len() == preview.files.len() {
            preview.selected_file_indices.clear();
        } else {
            preview.selected_file_indices = preview.files.iter().map(|file| file.index).collect();
        }
        self.add_dialog.error = None;
        self.add_dialog.results.clear();
        cx.notify();
    }

    fn set_add_dialog_error(&mut self, summary: String, cx: &mut Context<Self>) {
        if self.add_dialog.open {
            self.add_dialog.error = Some(OperationErrorView {
                code: "application.filesystem".into(),
                summary,
                retryable: true,
            });
            cx.notify();
        }
    }

    fn toggle_add_advanced(&mut self, cx: &mut Context<Self>) {
        if self.add_dialog.pending.is_some() || self.add_dialog.preview_pending.is_some() {
            return;
        }
        self.add_dialog.advanced_open = !self.add_dialog.advanced_open;
        cx.notify();
    }

    fn collect_add_advanced_options(&self, cx: &App) -> AddDownloadAdvancedOptionsView {
        let cookie = self.add_cookie_input.read(cx).text().trim().to_owned();
        let http_passwd = self.add_http_passwd_input.read(cx).text();
        AddDownloadAdvancedOptionsView {
            referer: self.add_referer_input.read(cx).text().trim().to_owned(),
            user_agent: self.add_user_agent_input.read(cx).text().trim().to_owned(),
            headers: self.add_headers_input.read(cx).text().to_owned(),
            cookie: (!cookie.is_empty()).then(|| SecretStringView::new(cookie)),
            http_user: self.add_http_user_input.read(cx).text().trim().to_owned(),
            http_passwd: (!http_passwd.is_empty()).then(|| SecretStringView::new(http_passwd)),
            checksum: self.add_checksum_input.read(cx).text().trim().to_owned(),
        }
    }

    fn set_file_conflict_policy(&mut self, policy: FileConflictPolicyView, cx: &mut Context<Self>) {
        if self.add_dialog.pending.is_some() || self.add_dialog.file_conflict == policy {
            return;
        }
        self.add_dialog.file_conflict = policy;
        self.add_dialog.error = None;
        self.add_dialog.results.clear();
        cx.notify();
    }

    fn open_task_details_action(
        &mut self,
        _: &OpenTaskDetails,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(task) = self.selected_task_view() else {
            self.show_notice("Select a visible task to open its details.", true, cx);
            return;
        };
        self.open_details_for(task, cx);
    }

    fn open_details_for(&mut self, task: DownloadRowView, cx: &mut Context<Self>) {
        let session = self
            .snapshot
            .engine_session()
            .unwrap_or_else(|| EngineSessionView {
                profile_id: task.identity.profile_id.clone(),
                session_id: String::new(),
                generation: self.snapshot.generation,
            });
        self.details_drawer = Some(TaskDetailsDrawer {
            identity: task.identity.clone(),
            overview: task,
            session,
            state: TaskDetailsLoadState::Stale,
            pending: None,
            open_pending: None,
            tab: TaskDetailsTab::Info,
            file_scroll: UniformListScrollHandle::new(),
            rendered_file_range: 0..0,
        });
        if self.snapshot.commands_available() {
            self.request_current_details(cx);
        }
        cx.notify();
    }

    fn request_current_details(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.snapshot.engine_session() else {
            return;
        };
        let Some((identity, source_revision, active, is_bittorrent)) =
            self.details_drawer.as_ref().and_then(|drawer| {
                drawer.pending.is_none().then(|| {
                    (
                        drawer.identity.clone(),
                        drawer.overview.revision,
                        drawer.overview.status.uses_active_connections(),
                        matches!(
                            drawer.overview.source_kind,
                            crate::TaskSourceKindView::Magnet
                                | crate::TaskSourceKindView::BitTorrent
                        ) || drawer.overview.status == TaskStatusView::Seeding,
                    )
                })
            })
        else {
            return;
        };
        if identity.profile_id != session.profile_id || !self.snapshot.commands_available() {
            return;
        }

        let request_id = self.allocate_request_id();
        if let Some(drawer) = &mut self.details_drawer {
            drawer.session = session.clone();
            if !matches!(drawer.state, TaskDetailsLoadState::Ready { .. }) {
                drawer.state = TaskDetailsLoadState::Loading;
            }
            drawer.pending = Some(PendingTaskDetails {
                request_id,
                source_revision,
            });
        }
        cx.emit(AppShellEvent::TaskDetailsRequested(
            TaskDetailsRequestView {
                request_id,
                session,
                identity,
                active,
                is_bittorrent,
            },
        ));
        cx.notify();
    }

    fn request_task_open(&mut self, target: TaskOpenTargetView, cx: &mut Context<Self>) {
        if !self.snapshot.commands_available() || !self.snapshot.local_path_actions_available {
            self.show_notice(
                "Opening task paths is available only for the managed local engine.",
                true,
                cx,
            );
            return;
        }
        let Some(session) = self.snapshot.engine_session() else {
            return;
        };
        let Some(identity) = self.details_drawer.as_ref().and_then(|drawer| {
            drawer
                .open_pending
                .is_none()
                .then(|| drawer.identity.clone())
        }) else {
            return;
        };
        if identity.profile_id != session.profile_id {
            return;
        }
        let request_id = self.allocate_request_id();
        if let Some(drawer) = &mut self.details_drawer {
            drawer.open_pending = Some(PendingTaskOpen { request_id, target });
        }
        cx.emit(AppShellEvent::TaskOpenRequested(TaskOpenRequestView {
            request_id,
            session,
            identity,
            target,
        }));
        cx.notify();
    }

    fn close_task_details(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.details_drawer.take().is_some() {
            window.focus(&self.focus_handle, cx);
            cx.notify();
        }
    }

    fn pause_selected(
        &mut self,
        _: &PauseSelectedTask,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.visible_selected_task_count() > 1 {
            self.begin_batch_task_command(BatchTaskCommandView::Pause, cx);
        } else {
            self.begin_task_command(TaskCommandView::Pause, cx);
        }
    }

    fn resume_selected(
        &mut self,
        _: &ResumeSelectedTask,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.visible_selected_task_count() > 1 {
            self.begin_batch_task_command(BatchTaskCommandView::Resume, cx);
        } else {
            self.begin_task_command(TaskCommandView::Resume, cx);
        }
    }

    fn retry_selected(
        &mut self,
        _: &RetrySelectedTask,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.visible_selected_task_count() > 1 {
            self.begin_batch_task_command(BatchTaskCommandView::Retry, cx);
        } else {
            self.begin_task_command(TaskCommandView::Retry, cx);
        }
    }

    fn open_task_output_name_action(
        &mut self,
        _: &OpenTaskOutputName,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_task_output_name(window, cx);
    }

    fn open_task_output_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.output_name_dialog.is_some() {
            window.focus(&self.output_name_input.focus_handle(cx), cx);
            return;
        }
        if self.add_dialog.open
            || self.remove_confirmation.is_some()
            || self.batch_failure_details.is_some()
            || self.pending_task_command.is_some()
            || self.pending_batch_command.is_some()
        {
            return;
        }
        let Some(task) = self.selected_task_view() else {
            self.show_notice("Select a visible task first.", true, cx);
            return;
        };
        if !task.can_set_output_name() || !self.snapshot.commands_available() {
            self.show_notice(
                "Output names can be changed only for non-terminal direct URI tasks.",
                true,
                cx,
            );
            return;
        }

        let initial_name = if task.name_state.is_resolving() {
            String::new()
        } else {
            task.display_name.clone()
        };
        self.output_name_input
            .update(cx, |input, cx| input.set_text(initial_name, cx));
        self.output_name_dialog = Some(TaskOutputNameDialog {
            identity: task.identity.clone(),
            display_name: task_display_name(&task),
            active: task.status.uses_active_connections(),
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
            error: None,
        });
        cx.notify();
        cx.defer_in(window, |this, window, cx| {
            if this.output_name_dialog.is_some() {
                window.focus(&this.output_name_input.focus_handle(cx), cx);
            }
        });
    }

    fn close_task_output_name_action(
        &mut self,
        _: &CloseTaskOutputName,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_task_output_name(window, cx);
    }

    fn close_task_output_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.output_name_dialog.is_none()
            || self.pending_task_command.as_ref().is_some_and(|pending| {
                matches!(&pending.command, TaskCommandView::SetOutputName { .. })
            })
        {
            return;
        }
        let previous_focus = self
            .output_name_dialog
            .take()
            .and_then(|dialog| dialog.previous_focus)
            .and_then(|focus| focus.upgrade());
        if let Some(focus) = previous_focus {
            window.focus(&focus, cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    fn submit_task_output_name_action(
        &mut self,
        _: &SubmitTaskOutputName,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_task_output_name(cx);
    }

    fn submit_task_output_name(&mut self, cx: &mut Context<Self>) {
        if self.pending_task_command.is_some() {
            return;
        }
        let Some(identity) = self
            .output_name_dialog
            .as_ref()
            .map(|dialog| dialog.identity.clone())
        else {
            return;
        };
        let output_name = self.output_name_input.read(cx).text().trim().to_owned();
        if let Some(summary) = output_name_validation_error(&output_name) {
            if let Some(dialog) = &mut self.output_name_dialog {
                dialog.error = Some(OperationErrorView {
                    code: "validation.invalid_output_name".into(),
                    summary: summary.into(),
                    retryable: false,
                });
            }
            cx.notify();
            return;
        }
        let current_task = self
            .snapshot
            .tasks
            .iter()
            .find(|task| task.identity == identity);
        if self.selected.as_ref() != Some(&identity)
            || current_task.is_none_or(|task| !task.can_set_output_name())
        {
            if let Some(dialog) = &mut self.output_name_dialog {
                dialog.error = Some(OperationErrorView {
                    code: "command.task_changed".into(),
                    summary: "The task changed. Close this dialog and review its current state."
                        .into(),
                    retryable: false,
                });
            }
            cx.notify();
            return;
        }
        self.begin_task_command(TaskCommandView::SetOutputName { output_name }, cx);
    }

    fn open_task_speed_limit_action(
        &mut self,
        _: &OpenTaskSpeedLimit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_task_speed_limit(window, cx);
    }

    fn open_task_speed_limit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.task_speed_limit_dialog.is_some() {
            window.focus(&self.task_download_limit_input.focus_handle(cx), cx);
            return;
        }
        if self.add_dialog.open
            || self.output_name_dialog.is_some()
            || self.remove_confirmation.is_some()
            || self.batch_failure_details.is_some()
            || self.pending_task_command.is_some()
            || self.pending_batch_command.is_some()
        {
            return;
        }
        let Some(task) = self.selected_task_view() else {
            self.show_notice("Select a visible task first.", true, cx);
            return;
        };
        if !task.status.can_set_speed_limit() || !self.snapshot.commands_available() {
            self.show_notice(
                "Speed limits can be set only for a task that is still downloading.",
                true,
                cx,
            );
            return;
        }
        // The list projection does not carry per-task limits (that is DETAIL-001's
        // getOption surface), so the fields start blank and set a fresh value.
        self.task_download_limit_input
            .update(cx, |input, cx| input.set_text("", cx));
        self.task_upload_limit_input
            .update(cx, |input, cx| input.set_text("", cx));
        self.task_speed_limit_dialog = Some(TaskSpeedLimitDialog {
            identity: task.identity.clone(),
            display_name: task_display_name(&task),
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
            error: None,
        });
        cx.notify();
        cx.defer_in(window, |this, window, cx| {
            if this.task_speed_limit_dialog.is_some() {
                window.focus(&this.task_download_limit_input.focus_handle(cx), cx);
            }
        });
    }

    fn close_task_speed_limit_action(
        &mut self,
        _: &CloseTaskSpeedLimit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_task_speed_limit(window, cx);
    }

    fn close_task_speed_limit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.task_speed_limit_dialog.is_none()
            || self.pending_task_command.as_ref().is_some_and(|pending| {
                matches!(&pending.command, TaskCommandView::SetSpeedLimit { .. })
            })
        {
            return;
        }
        let previous_focus = self
            .task_speed_limit_dialog
            .take()
            .and_then(|dialog| dialog.previous_focus)
            .and_then(|focus| focus.upgrade());
        if let Some(focus) = previous_focus {
            window.focus(&focus, cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    fn submit_task_speed_limit_action(
        &mut self,
        _: &SubmitTaskSpeedLimit,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_task_speed_limit(cx);
    }

    fn submit_task_speed_limit(&mut self, cx: &mut Context<Self>) {
        if self.pending_task_command.is_some() {
            return;
        }
        let Some(identity) = self
            .task_speed_limit_dialog
            .as_ref()
            .map(|dialog| dialog.identity.clone())
        else {
            return;
        };
        let draft = SpeedLimitSettingsView {
            download_limit: self.task_download_limit_input.read(cx).text().trim().into(),
            upload_limit: self.task_upload_limit_input.read(cx).text().trim().into(),
        };
        let (Some(download_limit), Some(upload_limit)) =
            (draft.parse_download_limit(), draft.parse_upload_limit())
        else {
            if let Some(dialog) = &mut self.task_speed_limit_dialog {
                dialog.error = Some(OperationErrorView {
                    code: "validation.invalid_speed_limit".into(),
                    summary: "Enter a speed as bytes/second or a K/M/G value, or leave it blank for unlimited."
                        .into(),
                    retryable: false,
                });
            }
            cx.notify();
            return;
        };
        let current_task = self
            .snapshot
            .tasks
            .iter()
            .find(|task| task.identity == identity);
        if self.selected.as_ref() != Some(&identity)
            || current_task.is_none_or(|task| !task.status.can_set_speed_limit())
        {
            if let Some(dialog) = &mut self.task_speed_limit_dialog {
                dialog.error = Some(OperationErrorView {
                    code: "command.task_changed".into(),
                    summary: "The task changed. Close this dialog and review its current state."
                        .into(),
                    retryable: false,
                });
            }
            cx.notify();
            return;
        }
        self.begin_task_command(
            TaskCommandView::SetSpeedLimit {
                download_limit,
                upload_limit,
            },
            cx,
        );
    }

    fn open_task_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.task_options_dialog.is_some() {
            window.focus(&self.task_seed_ratio_input.focus_handle(cx), cx);
            return;
        }
        if self.add_dialog.open
            || self.output_name_dialog.is_some()
            || self.task_speed_limit_dialog.is_some()
            || self.remove_confirmation.is_some()
            || self.batch_failure_details.is_some()
            || self.pending_task_command.is_some()
            || self.pending_batch_command.is_some()
        {
            return;
        }
        let Some(task) = self.selected_task_view() else {
            self.show_notice("Select a visible task first.", true, cx);
            return;
        };
        if !task.status.can_set_speed_limit() || !self.snapshot.commands_available() {
            self.show_notice(
                "Task options can be changed only while the download is still live.",
                true,
                cx,
            );
            return;
        }
        let supports_seed_rules = matches!(
            task.source_kind,
            crate::TaskSourceKindView::Magnet | crate::TaskSourceKindView::BitTorrent
        ) || task.status == TaskStatusView::Seeding;
        // Prefill from the open details drawer options projection when present.
        let (seed_ratio, seed_time) = self
            .details_drawer
            .as_ref()
            .and_then(|drawer| match &drawer.state {
                TaskDetailsLoadState::Ready { details } => Some(details),
                _ => None,
            })
            .map(|details| {
                let value = |key: &str| {
                    details
                        .options
                        .iter()
                        .find(|option| option.key.eq_ignore_ascii_case(key))
                        .map(|option| option.value.clone())
                        .unwrap_or_default()
                };
                (value("seed-ratio"), value("seed-time"))
            })
            .unwrap_or_default();
        self.task_seed_ratio_input.update(cx, |input, cx| {
            input.set_text(
                if supports_seed_rules {
                    seed_ratio
                } else {
                    String::new()
                },
                cx,
            );
        });
        self.task_seed_time_input.update(cx, |input, cx| {
            input.set_text(
                if supports_seed_rules {
                    seed_time
                } else {
                    String::new()
                },
                cx,
            );
        });
        self.task_options_dialog = Some(TaskOptionsDialog {
            identity: task.identity.clone(),
            display_name: task_display_name(&task),
            supports_seed_rules,
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
            error: None,
        });
        cx.notify();
        cx.defer_in(window, |this, window, cx| {
            if this.task_options_dialog.is_some() {
                window.focus(&this.task_seed_ratio_input.focus_handle(cx), cx);
            }
        });
    }

    fn close_task_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.task_options_dialog.is_none()
            || self.pending_task_command.as_ref().is_some_and(|pending| {
                matches!(&pending.command, TaskCommandView::SetOptions { .. })
            })
        {
            return;
        }
        let previous_focus = self
            .task_options_dialog
            .take()
            .and_then(|dialog| dialog.previous_focus)
            .and_then(|focus| focus.upgrade());
        if let Some(focus) = previous_focus {
            window.focus(&focus, cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    fn submit_task_options(&mut self, cx: &mut Context<Self>) {
        if self.pending_task_command.is_some() {
            return;
        }
        let Some(dialog) = self.task_options_dialog.as_ref() else {
            return;
        };
        let identity = dialog.identity.clone();
        let supports_seed_rules = dialog.supports_seed_rules;
        let seed_ratio_raw = self.task_seed_ratio_input.read(cx).text().trim().to_owned();
        let seed_time_raw = self.task_seed_time_input.read(cx).text().trim().to_owned();
        let mut seed_ratio = None;
        let mut seed_time_minutes = None;
        if !supports_seed_rules {
            if let Some(dialog) = &mut self.task_options_dialog {
                dialog.error = Some(OperationErrorView {
                    code: "command.unsupported".into(),
                    summary: "Seed rules apply only to BitTorrent tasks.".into(),
                    retryable: false,
                });
            }
            cx.notify();
            return;
        }
        if !seed_ratio_raw.is_empty() {
            match seed_ratio_raw.parse::<f64>() {
                Ok(value) if value.is_finite() && value >= 0.0 => {
                    seed_ratio = Some(seed_ratio_raw.clone());
                }
                _ => {
                    if let Some(dialog) = &mut self.task_options_dialog {
                        dialog.error = Some(OperationErrorView {
                            code: "validation.invalid_seed_ratio".into(),
                            summary: "Seed ratio must be a number greater than or equal to 0."
                                .into(),
                            retryable: false,
                        });
                    }
                    cx.notify();
                    return;
                }
            }
        }
        if !seed_time_raw.is_empty() {
            match seed_time_raw.parse::<u64>() {
                Ok(_) => seed_time_minutes = Some(seed_time_raw.clone()),
                Err(_) => {
                    if let Some(dialog) = &mut self.task_options_dialog {
                        dialog.error = Some(OperationErrorView {
                            code: "validation.invalid_seed_time".into(),
                            summary: "Seed time must be a whole number of minutes.".into(),
                            retryable: false,
                        });
                    }
                    cx.notify();
                    return;
                }
            }
        }
        if seed_ratio.is_none() && seed_time_minutes.is_none() {
            if let Some(dialog) = &mut self.task_options_dialog {
                dialog.error = Some(OperationErrorView {
                    code: "validation.empty_task_options".into(),
                    summary: "Enter a seed ratio and/or seed time to apply.".into(),
                    retryable: false,
                });
            }
            cx.notify();
            return;
        }
        let current_task = self
            .snapshot
            .tasks
            .iter()
            .find(|task| task.identity == identity);
        if self.selected.as_ref() != Some(&identity)
            || current_task.is_none_or(|task| !task.status.can_set_speed_limit())
        {
            if let Some(dialog) = &mut self.task_options_dialog {
                dialog.error = Some(OperationErrorView {
                    code: "command.task_changed".into(),
                    summary: "The task changed. Close this dialog and review its current state."
                        .into(),
                    retryable: false,
                });
            }
            cx.notify();
            return;
        }
        self.begin_task_command(
            TaskCommandView::SetOptions {
                seed_ratio,
                seed_time_minutes,
                selected_file_indices: None,
            },
            cx,
        );
    }

    fn remove_selected(
        &mut self,
        _: &RemoveSelectedTask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_remove_selected(window, cx);
    }

    fn move_selected_to_queue_top(
        &mut self,
        _: &MoveTaskToQueueTop,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_task_context_menu(cx);
        self.begin_task_command(TaskCommandView::MoveToQueueTop, cx);
    }

    fn move_selected_up_in_queue(
        &mut self,
        _: &MoveTaskUpInQueue,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_task_context_menu(cx);
        self.begin_task_command(TaskCommandView::MoveUpInQueue, cx);
    }

    fn move_selected_down_in_queue(
        &mut self,
        _: &MoveTaskDownInQueue,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_task_context_menu(cx);
        self.begin_task_command(TaskCommandView::MoveDownInQueue, cx);
    }

    fn move_selected_to_queue_bottom(
        &mut self,
        _: &MoveTaskToQueueBottom,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_task_context_menu(cx);
        self.begin_task_command(TaskCommandView::MoveToQueueBottom, cx);
    }

    fn open_task_context_menu(
        &mut self,
        index: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != AppPage::Downloads
            || self.add_dialog.open
            || self.output_name_dialog.is_some()
            || self.task_speed_limit_dialog.is_some()
            || self.task_options_dialog.is_some()
            || self.remove_confirmation.is_some()
            || self.batch_failure_details.is_some()
        {
            return;
        }
        let Some(task) = self.snapshot.tasks.get(index).cloned() else {
            return;
        };
        // Right-click focuses the row without clearing a multi-selection that
        // already includes it (qBittorrent/Motrix parity).
        if !self.selected_tasks.contains(&task.identity) {
            self.select_at_with_modifiers(index, false, false, window, cx);
        } else {
            self.selected = Some(task.identity.clone());
        }
        self.sort_popover_open = false;
        self.speed_popover_open = false;
        self.context_menu = Some(TaskContextMenu {
            identity: task.identity,
            position,
        });
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn close_task_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            cx.notify();
        }
    }

    /// Prefer the right-clicked menu identity so multi-selection still has a
    /// single authoritative target for copy/open/details actions.
    fn context_menu_task_view(&self) -> Option<DownloadRowView> {
        if let Some(menu) = self.context_menu.as_ref()
            && let Some(task) = self
                .snapshot
                .tasks
                .iter()
                .find(|task| task.identity == menu.identity)
        {
            return Some(task.clone());
        }
        self.selected_task_view()
            .or_else(|| self.command_target_task_view())
    }

    fn copy_task_source(&mut self, task: &DownloadRowView, cx: &mut Context<Self>) {
        let source = task
            .primary_source
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or("");
        if source.is_empty() {
            self.show_notice("This task has no copyable source.", true, cx);
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(source.to_owned()));
        self.show_notice("Source copied to the clipboard.", false, cx);
    }

    fn copy_task_gid(&mut self, task: &DownloadRowView, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(task.identity.gid.clone()));
        self.show_notice("Task GID copied to the clipboard.", false, cx);
    }

    /// Open a local path for the command-target task without requiring the
    /// details drawer (used by the task context menu).
    fn request_task_open_for_selection(
        &mut self,
        target: TaskOpenTargetView,
        cx: &mut Context<Self>,
    ) {
        if !self.snapshot.commands_available() || !self.snapshot.local_path_actions_available {
            self.show_notice(
                "Opening task paths is available only for the managed local engine.",
                true,
                cx,
            );
            return;
        }
        let Some(session) = self.snapshot.engine_session() else {
            return;
        };
        let Some(task) = self.context_menu_task_view() else {
            self.show_notice("Select a visible task first.", true, cx);
            return;
        };
        if task.identity.profile_id != session.profile_id {
            return;
        }
        let request_id = self.allocate_request_id();
        cx.emit(AppShellEvent::TaskOpenRequested(TaskOpenRequestView {
            request_id,
            session,
            identity: task.identity,
            target,
        }));
        self.show_notice("Opening local path...", false, cx);
        cx.notify();
    }

    /// Queue reordering is authoritative only when the visible query is the
    /// full, unsearched, ascending queue order (D-014 Scope rule). aria2's
    /// queue is global across active/waiting/paused tasks, so relative movement
    /// inside a filtered, searched, reversed, or value-sorted projection would
    /// imply a position that is not authoritative.
    fn queue_reordering_available(&self) -> bool {
        self.snapshot.capabilities.queue_positioning
            && self.query.filter == WorkspaceFilter::All
            && self.query.search.trim().is_empty()
            && self.query.sort_key == WorkspaceSortKey::Queue
            && self.query.sort_direction == WorkspaceSortDirection::Ascending
    }

    fn begin_task_command(&mut self, command: TaskCommandView, cx: &mut Context<Self>) {
        if self.pending_task_command.is_some()
            || self.pending_global_task_command.is_some()
            || self.pending_batch_command.is_some()
            || self.batch_failure_details.is_some()
        {
            return;
        }
        let Some(task) = self.command_target_task_view() else {
            self.show_notice("Select a visible task first.", true, cx);
            return;
        };
        let capability_block = match command {
            TaskCommandView::ForcePause if !self.snapshot.capabilities.force_pause => {
                Some(self.snapshot.capabilities.unsupported_force_pause_message())
            }
            TaskCommandView::ForceRemoveTask if !self.snapshot.capabilities.force_remove => Some(
                self.snapshot
                    .capabilities
                    .unsupported_force_remove_message(),
            ),
            TaskCommandView::MoveToQueueTop
            | TaskCommandView::MoveUpInQueue
            | TaskCommandView::MoveDownInQueue
            | TaskCommandView::MoveToQueueBottom
                if !self.snapshot.capabilities.queue_positioning =>
            {
                Some(self.snapshot.capabilities.unsupported_queue_message())
            }
            TaskCommandView::SetSpeedLimit { .. }
            | TaskCommandView::SetConnectionPolicy { .. }
            | TaskCommandView::SetOptions { .. }
                if !self.snapshot.capabilities.change_option =>
            {
                Some(
                    self.snapshot
                        .capabilities
                        .unsupported_change_option_message(),
                )
            }
            _ => None,
        };
        if let Some(message) = capability_block {
            self.show_notice(message, true, cx);
            return;
        }
        let allowed = match command {
            TaskCommandView::Pause | TaskCommandView::ForcePause => task.status.can_pause(),
            TaskCommandView::Resume => task.status.can_resume(),
            TaskCommandView::MoveToQueueTop
            | TaskCommandView::MoveUpInQueue
            | TaskCommandView::MoveDownInQueue
            | TaskCommandView::MoveToQueueBottom => {
                task.status.can_move_in_queue() && self.queue_reordering_available()
            }
            TaskCommandView::Retry => task.status.can_retry(),
            TaskCommandView::SetOutputName { .. } => task.can_set_output_name(),
            TaskCommandView::SetSpeedLimit { .. } => task.status.can_set_speed_limit(),
            TaskCommandView::SetConnectionPolicy { .. } => task.status.can_set_connection_policy(),
            TaskCommandView::SetOptions { .. } => task.status.can_set_speed_limit(),
            TaskCommandView::RemoveTask
            | TaskCommandView::ForceRemoveTask
            | TaskCommandView::RemoveTaskAndFiles => task.status.can_remove(),
        };
        if !allowed {
            self.show_notice(
                format!(
                    "{} is not available while the task is {}.",
                    task_command_label(&command),
                    task.status.label().to_lowercase()
                ),
                true,
                cx,
            );
            return;
        }
        let Some(session) = self
            .snapshot
            .commands_available()
            .then(|| self.snapshot.engine_session())
            .flatten()
        else {
            self.show_notice("The engine is not ready for commands.", true, cx);
            return;
        };

        let request_id = self.allocate_request_id();
        let identity = task.identity;
        self.pending_task_command = Some(PendingTaskCommand {
            request_id,
            session: session.clone(),
            identity: identity.clone(),
            command: command.clone(),
        });
        self.show_notice(command.progress_label(), false, cx);
        cx.emit(AppShellEvent::TaskCommandRequested(
            TaskCommandRequestView {
                request_id,
                session,
                identity,
                command,
            },
        ));
        cx.notify();
    }

    fn begin_global_task_command(
        &mut self,
        command: GlobalTaskCommandView,
        cx: &mut Context<Self>,
    ) {
        if self.pending_task_command.is_some()
            || self.pending_global_task_command.is_some()
            || self.pending_batch_command.is_some()
            || self.batch_failure_details.is_some()
        {
            return;
        }
        let Some(session) = self
            .snapshot
            .commands_available()
            .then(|| self.snapshot.engine_session())
            .flatten()
        else {
            self.show_notice("The engine is not ready for commands.", true, cx);
            return;
        };
        if matches!(command, GlobalTaskCommandView::ForcePauseAll)
            && !self.snapshot.capabilities.force_pause_all
        {
            self.show_notice(
                self.snapshot
                    .capabilities
                    .unsupported_force_pause_all_message(),
                true,
                cx,
            );
            return;
        }
        let request_id = self.allocate_request_id();
        self.pending_global_task_command = Some(PendingGlobalTaskCommand {
            request_id,
            session: session.clone(),
            command,
        });
        self.show_notice(command.progress_label(), false, cx);
        cx.emit(AppShellEvent::GlobalTaskCommandRequested(
            GlobalTaskCommandRequestView {
                request_id,
                session,
                command,
            },
        ));
        cx.notify();
    }

    fn begin_batch_task_command(&mut self, command: BatchTaskCommandView, cx: &mut Context<Self>) {
        if self.pending_task_command.is_some()
            || self.pending_global_task_command.is_some()
            || self.pending_batch_command.is_some()
            || self.batch_failure_details.is_some()
        {
            return;
        }
        let identities = self
            .snapshot
            .tasks
            .iter()
            .filter(|task| self.selected_tasks.contains(&task.identity))
            .map(|task| task.identity.clone())
            .collect::<Vec<_>>();
        if identities.len() < 2 {
            self.show_notice(
                "Select at least two visible tasks for a batch action.",
                true,
                cx,
            );
            return;
        }
        let Some(session) = self
            .snapshot
            .commands_available()
            .then(|| self.snapshot.engine_session())
            .flatten()
        else {
            self.show_notice("The engine is not ready for commands.", true, cx);
            return;
        };
        let capability_block = match command {
            BatchTaskCommandView::ForcePause if !self.snapshot.capabilities.force_pause => {
                Some(self.snapshot.capabilities.unsupported_force_pause_message())
            }
            BatchTaskCommandView::ForceRemoveTask if !self.snapshot.capabilities.force_remove => {
                Some(
                    self.snapshot
                        .capabilities
                        .unsupported_force_remove_message(),
                )
            }
            _ => None,
        };
        if let Some(message) = capability_block {
            self.show_notice(message, true, cx);
            return;
        }
        let request_id = self.allocate_request_id();
        self.pending_batch_command = Some(PendingBatchTaskCommand {
            request_id,
            session: session.clone(),
            identities: identities.clone(),
            command,
        });
        self.show_notice(command.progress_label(), false, cx);
        cx.emit(AppShellEvent::BatchTaskCommandRequested(
            BatchTaskCommandRequestView {
                request_id,
                session,
                identities,
                command,
            },
        ));
        cx.notify();
    }

    fn confirm_remove_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.remove_confirmation.is_some()
            || self.output_name_dialog.is_some()
            || self.batch_failure_details.is_some()
            || self.pending_task_command.is_some()
            || self.pending_batch_command.is_some()
        {
            return;
        }
        let visible_selected_count = self.visible_selected_task_count();
        if visible_selected_count > 1 {
            let selected_tasks = self
                .snapshot
                .tasks
                .iter()
                .filter(|task| self.selected_tasks.contains(&task.identity))
                .collect::<Vec<_>>();
            let identities = selected_tasks
                .iter()
                .map(|task| task.identity.clone())
                .collect::<Vec<_>>();
            if identities.len() > 1 && self.snapshot.commands_available() {
                self.remove_confirmation = Some(RemoveConfirmation {
                    display_name: format!("{} selected tasks", identities.len()),
                    identities,
                    has_live_tasks: selected_tasks.iter().any(|task| !task.status.is_terminal()),
                    has_terminal_tasks: selected_tasks.iter().any(|task| task.status.is_terminal()),
                    delete_files: false,
                    previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
                });
                cx.notify();
                cx.defer_in(window, |this, window, cx| {
                    if this.remove_confirmation.is_some() {
                        window.focus(&this.remove_cancel_focus, cx);
                    }
                });
            }
            return;
        }
        let Some(task) = self.command_target_task_view() else {
            if !self.selected_tasks.is_empty() {
                self.show_notice(
                    "Selected tasks are outside the current result. Clear the hidden selection or change the query.",
                    true,
                    cx,
                );
            }
            return;
        };
        if !task.status.can_remove() || !self.snapshot.commands_available() {
            self.show_notice(
                "The selected task cannot be removed in the current engine state.",
                true,
                cx,
            );
            return;
        }

        let display_name = task_display_name(&task);
        self.remove_confirmation = Some(RemoveConfirmation {
            identities: vec![task.identity],
            display_name,
            has_live_tasks: !task.status.is_terminal(),
            has_terminal_tasks: task.status.is_terminal(),
            delete_files: false,
            previous_focus: window.focused(cx).map(|focus| focus.downgrade()),
        });
        cx.notify();
        cx.defer_in(window, |this, window, cx| {
            if this.remove_confirmation.is_some() {
                window.focus(&this.remove_cancel_focus, cx);
            }
        });
    }

    fn close_remove_confirmation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(confirmation) = self.remove_confirmation.take() else {
            return;
        };
        if let Some(focus) = confirmation
            .previous_focus
            .and_then(|focus| focus.upgrade())
        {
            window.focus(&focus, cx);
        } else {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    fn submit_remove_confirmation(&mut self, cx: &mut Context<Self>) {
        let Some(confirmation) = self.remove_confirmation.take() else {
            return;
        };
        let selection_matches = if confirmation.identities.len() > 1 {
            confirmation
                .identities
                .iter()
                .all(|identity| self.selected_tasks.contains(identity))
        } else {
            confirmation
                .identities
                .first()
                .is_some_and(|identity| self.selected.as_ref() == Some(identity))
        };
        if !selection_matches {
            self.show_notice(
                "The task selection changed. Review it before removing tasks.",
                true,
                cx,
            );
            return;
        }
        if confirmation.identities.len() > 1 {
            self.begin_batch_task_command(
                if confirmation.delete_files {
                    BatchTaskCommandView::RemoveTaskAndFiles
                } else {
                    BatchTaskCommandView::RemoveTask
                },
                cx,
            );
        } else {
            self.begin_task_command(
                if confirmation.delete_files {
                    TaskCommandView::RemoveTaskAndFiles
                } else {
                    TaskCommandView::RemoveTask
                },
                cx,
            );
        }
    }

    fn toggle_remove_files(&mut self, cx: &mut Context<Self>) {
        if matches!(self.engine_health, EngineHealthView::External) {
            return;
        }
        if let Some(confirmation) = &mut self.remove_confirmation {
            confirmation.delete_files = !confirmation.delete_files;
            cx.notify();
        }
    }

    fn toggle_speed_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.speed_popover_open {
            self.close_speed_popover(window, cx);
            return;
        }
        self.speed_popover_previous_focus = window.focused(cx).map(|focus| focus.downgrade());
        self.speed_popover_open = true;
        cx.notify();
    }

    fn close_speed_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.speed_popover_open {
            return;
        }
        self.speed_popover_open = false;
        if let Some(focus) = self
            .speed_popover_previous_focus
            .take()
            .and_then(|focus| focus.upgrade())
        {
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    fn selected_task_view(&self) -> Option<DownloadRowView> {
        let selected = self.selected.as_ref()?;
        self.snapshot
            .tasks
            .iter()
            .find(|task| &task.identity == selected)
            .cloned()
            .or_else(|| {
                self.details_drawer
                    .as_ref()
                    .filter(|drawer| &drawer.identity == selected)
                    .map(|drawer| drawer.overview.clone())
            })
    }

    fn command_target_task_view(&self) -> Option<DownloadRowView> {
        let mut visible_selected = self
            .snapshot
            .tasks
            .iter()
            .filter(|task| self.selected_tasks.contains(&task.identity));
        let first = visible_selected.next();
        if first.is_some() && visible_selected.next().is_none() {
            return first.cloned();
        }
        if first.is_none() && !self.selected_tasks.is_empty() {
            return None;
        }
        self.selected_task_view()
    }

    fn render_header(&mut self, _window: &Window, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let (search_left, search_right) =
            centered_search_bounds(f32::from(_window.viewport_size().width));
        let search_width = search_right - search_left;
        let brand = div()
            .w(px(TITLEBAR_SIDE_WIDTH))
            .flex_none()
            .flex()
            .items_center()
            .h_full()
            .gap_2()
            .pl(px(TITLEBAR_BRAND_INSET))
            .window_control_area(WindowControlArea::Drag)
            .child(
                Icon::new(IconName::Download)
                    .size(IconSize::Medium)
                    .color(colors.accent),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("AriaDeck"),
            );
        // On Windows the caption strip must reach the physical right edge, so
        // horizontal padding is applied only on the left (and non-Windows keeps
        // symmetric padding for the Add action cluster).
        let header = div()
            .h(px(TITLEBAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .pl_3()
            .border_b_1()
            .border_color(colors.border)
            .bg(colors.toolbar_surface);
        #[cfg(not(target_os = "windows"))]
        let header = header.pr_3();
        header
            .child(brand)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .child(titlebar_drag_region())
                    .child(
                        div()
                            .w(px(search_width))
                            .flex_none()
                            .child(self.search_input.clone()),
                    )
                    .child(titlebar_drag_region()),
            )
            .child({
                // Keep chrome actions (Add) padded; Windows caption buttons are
                // rendered outside this inset so Close can sit flush to the edge.
                let actions = div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .when(cfg!(target_os = "windows"), |element| element.pl_2().pr_2())
                    .when(!cfg!(target_os = "windows"), |element| {
                        element.w(px(TITLEBAR_SIDE_WIDTH))
                    })
                    .child(self.render_add_button(cx));
                #[cfg(target_os = "windows")]
                {
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_end()
                        .child(actions)
                        .child(self.render_window_controls(_window))
                }
                #[cfg(not(target_os = "windows"))]
                {
                    actions
                }
            })
    }

    #[cfg(target_os = "windows")]
    fn render_window_controls(&self, window: &Window) -> Div {
        let colors = self.theme.colors;
        let maximized = window.is_maximized();
        div()
            .h(px(TITLEBAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .children(
                [
                    WindowControlKind::Minimize,
                    WindowControlKind::Maximize,
                    WindowControlKind::Close,
                ]
                .map(|kind| {
                    let control = window_control_config(kind, maximized);
                    window_control_button(
                        control.id,
                        control.icon,
                        control.label,
                        control.area,
                        colors,
                        control.danger,
                    )
                }),
            )
    }

    fn render_add_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = self.theme.colors;
        let enabled = self.snapshot.commands_available() && !self.add_dialog.open;
        Button::new("open-add-download", "Add")
            .icon(IconName::Plus)
            .aria_label(if enabled {
                "Add a URL or magnet download"
            } else {
                "Add download unavailable"
            })
            .tooltip(Tooltip::new("Add download").meta("Ctrl/Cmd+N"))
            .style(ButtonStyle::Primary)
            .disabled(!enabled)
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_add_download(&OpenAddDownload, window, cx);
            }))
            .render(colors)
    }

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let mut filters = Vec::with_capacity(WorkspaceFilter::ALL.len());
        for filter in WorkspaceFilter::ALL {
            let count = filter.count(self.snapshot.counts);
            let selected = self.page == AppPage::Downloads && self.query.filter == filter;
            let icon = filter_icon(filter);
            filters.push(
                div()
                    .id(SharedString::from(format!(
                        "sidebar-filter-{}",
                        filter.key()
                    )))
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label(format!("{}, {count} tasks", filter.label()))
                    .h(px(38.0))
                    .w_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .rounded_md()
                    .text_xs()
                    .text_color(if selected {
                        colors.accent
                    } else {
                        colors.text_secondary
                    })
                    .when(selected, |element| {
                        element.bg(with_alpha(colors.accent, 0.09))
                    })
                    .when(!selected, |element| {
                        element.hover(|style| style.bg(colors.surface_hover))
                    })
                    .focus_visible(|style| style.border_1().border_color(colors.focus_ring))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_filter(filter, window, cx);
                    }))
                    .child(Icon::new(icon).size(IconSize::Small).color(if selected {
                        colors.accent
                    } else {
                        colors.text_muted
                    }))
                    .child(div().flex_1().child(filter.short_label()))
                    .child(
                        div()
                            .h(px(22.0))
                            .min_w(px(22.0))
                            .px_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(if selected {
                                with_alpha(colors.accent, 0.12)
                            } else {
                                colors.surface_active
                            })
                            .font_features(tabular_numbers())
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if selected {
                                colors.accent
                            } else {
                                colors.text_muted
                            })
                            .child(count.to_string()),
                    ),
            );
        }

        let active_profile_name = self
            .profiles
            .active()
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| "No profile".into());
        let active_profile_kind = self
            .profiles
            .active()
            .map(|profile| profile.kind.label())
            .unwrap_or("—");
        let profile_count = self.profiles.profiles.len();

        div()
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .flex()
            .flex_col()
            .justify_between()
            .border_r_1()
            .border_color(colors.border)
            .bg(colors.surface)
            .p_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .id("active-profile-banner")
                            .role(Role::Status)
                            .aria_label(format!(
                                "Active profile {active_profile_name}, {active_profile_kind}"
                            ))
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(colors.surface_active)
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.text_primary)
                                    .child(active_profile_name),
                            )
                            .child(div().text_xs().text_color(colors.text_muted).child(format!(
                                "{active_profile_kind} · {profile_count} profile{}",
                                if profile_count == 1 { "" } else { "s" }
                            ))),
                    )
                    .child(div().flex().flex_col().gap_1().children(filters)),
            )
            .child(
                div()
                    .id("open-settings")
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label("Open application settings")
                    .h(px(38.0))
                    .w_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .rounded_md()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if self.page == AppPage::Settings {
                        colors.accent
                    } else {
                        colors.text_secondary
                    })
                    .when(self.page == AppPage::Settings, |element| {
                        element.bg(with_alpha(colors.accent, 0.09))
                    })
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .focus_visible(|style| style.border_1().border_color(colors.focus_ring))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_settings(&OpenSettings, window, cx);
                    }))
                    .child(Icon::new(IconName::Settings).size(IconSize::Small).color(
                        if self.page == AppPage::Settings {
                            colors.accent
                        } else {
                            colors.text_muted
                        },
                    ))
                    .child("Settings"),
            )
    }

    fn render_speed_chart(&self) -> Stateful<Div> {
        let colors = self.theme.colors;
        let visible = speed_chart_window(&self.snapshot.speed_history);
        let max_rate = visible
            .iter()
            .map(|sample| sample.download_rate.max(sample.upload_rate))
            .max()
            .unwrap_or(0);
        let scale = max_rate.max(1) as f32;
        let mut columns = Vec::with_capacity(SPEED_CHART_SAMPLES);
        columns.extend(
            (visible.len()..SPEED_CHART_SAMPLES).map(|_| speed_chart_column(0.0, 0.0, colors)),
        );
        columns.extend(visible.iter().map(|sample| {
            speed_chart_column(
                sample.download_rate as f32 / scale,
                sample.upload_rate as f32 / scale,
                colors,
            )
        }));

        div()
            .id("speed-history-chart")
            .role(Role::Group)
            .aria_label(format!(
                "Transfer speed for the last minute, current download {}, current upload {}, peak {}",
                format_rate(self.snapshot.download_rate),
                format_rate(self.snapshot.upload_rate),
                format_rate(max_rate)
            ))
            .h(px(144.0))
            .w(px(280.0))
            .flex_none()
            .flex()
            .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_md()
            .border_1()
            .border_color(colors.border_strong)
            .bg(colors.elevated_surface)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.text_secondary)
                            .child("Last minute"),
                    )
                    .child(
                        div()
                            .font_features(tabular_numbers())
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(format_rate(max_rate)),
                    ),
            )
            .child(
                div()
                    .h(px(58.0))
                    .w_full()
                    .flex_none()
                    .flex()
                    .items_end()
                    .children(columns),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(speed_chart_legend("Down", colors.progress_download, colors))
                    .child(speed_chart_legend("Up", colors.progress_upload, colors)),
            )
    }

    fn render_task_header(&mut self, layout: TaskLayoutMode, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let selected_count = self.visible_selected_task_count();
        let selection_state = if selected_count == 0 {
            Toggled::False
        } else if selected_count == self.snapshot.tasks.len() {
            Toggled::True
        } else {
            Toggled::Mixed
        };
        let selection_icon = match selection_state {
            Toggled::False => IconName::Square,
            Toggled::True => IconName::SquareCheckBig,
            Toggled::Mixed => IconName::SquareMinus,
        };
        let header = div()
            .h(px(36.0))
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .border_b_1()
            .border_color(colors.border)
            .bg(colors.toolbar_surface)
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(colors.text_muted)
            .child(
                div()
                    .id("select-all-tasks")
                    .role(Role::CheckBox)
                    .aria_label(match selection_state {
                        Toggled::True => "Clear selection",
                        Toggled::False | Toggled::Mixed => "Select all visible tasks",
                    })
                    .aria_toggled(selection_state)
                    .size(px(20.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_select_all(window, cx);
                    }))
                    .child(Icon::new(selection_icon).size(IconSize::Small).color(
                        if selected_count == 0 {
                            colors.text_muted
                        } else {
                            colors.accent
                        },
                    )),
            )
            .child(div().w(px(32.0)).flex_none());

        match layout {
            TaskLayoutMode::Wide => header
                .child(div().flex_1().min_w_0().child("Name"))
                .child(div().w(px(132.0)).flex_none().child("Progress / ratio"))
                .child(div().w(px(88.0)).flex_none().child("Down / up"))
                .child(div().w(px(124.0)).flex_none().child("Size"))
                .child(div().w(px(72.0)).flex_none().child("ETA / seed"))
                .child(div().w(px(86.0)).flex_none().text_center().child("Status")),
            TaskLayoutMode::Compact => header
                .child(div().flex_1().min_w_0().child("Task"))
                .child(div().w(px(112.0)).flex_none().child("Progress"))
                .child(div().w(px(78.0)).flex_none().text_center().child("Status")),
        }
    }

    fn render_main(&mut self, layout: TaskLayoutMode, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let task_count = self.snapshot.tasks.len();
        let selected_count = self.visible_selected_task_count();
        let hidden_selected_count = self.selected_tasks.len().saturating_sub(selected_count);
        let content =
            if task_count == 0 {
                self.render_empty_state(cx)
            } else {
                div()
                    .id("download-task-list")
                    .role(Role::List)
                    .aria_label(format!("Downloads, {task_count} visible tasks"))
                    .size_full()
                    .child(
                        uniform_list(
                            "download-tasks",
                            task_count,
                            cx.processor(move |this, range: Range<usize>, _window, cx| {
                                this.rendered_range = range.clone();
                                range
                                    .filter_map(|index| {
                                        this.snapshot.tasks.get(index).cloned().map(|task| {
                                            this.render_task_row(index, task, layout, cx)
                                        })
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .track_scroll(&self.list_scroll)
                        .h_full()
                        .w_full(),
                    )
                    .into_any_element()
            };

        let center = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(colors.background)
            .child(
                div()
                    .h(px(52.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .bg(colors.toolbar_surface)
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap_2()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.query.filter.label()),
                            )
                            .child(
                                div()
                                    .font_features(tabular_numbers())
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(format!("{task_count} visible")),
                            )
                            .when(selected_count > 0 || hidden_selected_count > 0, |element| {
                                element.child(
                                    div()
                                        .font_features(tabular_numbers())
                                        .text_xs()
                                        .text_color(colors.text_secondary)
                                        .child(if hidden_selected_count > 0 {
                                            format!(
                                                "{selected_count} selected, {hidden_selected_count} hidden"
                                            )
                                        } else {
                                            format!("{selected_count} selected")
                                        }),
                                )
                            })
                            .child(self.render_list_controls(cx)),
                    )
                    .child(self.render_task_toolbar(cx)),
            )
            .child(self.render_task_header(layout, cx))
            .child(div().flex_1().min_h_0().child(content));

        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .child(center)
            .when(self.details_drawer.is_some(), |element| {
                element.child(self.render_task_details_drawer(cx))
            })
    }

    fn render_status_bar(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let connection_color = connection_color(&self.snapshot.connection, colors);
        let connection_label = match &self.snapshot.connection {
            ConnectionView::Reconnecting { attempt } => format!("Reconnecting · {attempt}"),
            connection => connection.label().to_owned(),
        };
        let status_button = div()
            .id("connection-status")
            .role(if self.snapshot.connection.can_retry() {
                Role::Button
            } else {
                Role::Status
            })
            .aria_label(if self.snapshot.connection.can_retry() {
                "Retry aria2 connection".to_owned()
            } else {
                format!("Connection status: {connection_label}")
            })
            .h_full()
            .px_2()
            .flex()
            .items_center()
            .gap_1()
            .text_xs()
            .text_color(colors.text_muted)
            .child(StatusIndicator::new(connection_color))
            .child(connection_label)
            .when(self.snapshot.connection.can_retry(), |element| {
                element
                    .focusable()
                    .tab_stop(true)
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .on_click(cx.listener(|this, _, _, cx| this.request_retry(cx)))
            });

        div()
            .h(px(28.0))
            .flex_none()
            .flex()
            .items_center()
            .border_t_1()
            .border_color(colors.border)
            .bg(colors.toolbar_surface)
            .child(status_button)
            .child(
                div()
                    .id("engine-status")
                    .role(Role::Status)
                    .aria_label(self.engine_health.label())
                    .h_full()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(StatusIndicator::new(engine_health_color(
                        &self.engine_health,
                        colors,
                    )))
                    .child(self.engine_health.label()),
            )
            .when(self.snapshot.stale, |element| {
                element.child(
                    div()
                        .id("stale-status")
                        .role(Role::Status)
                        .h_full()
                        .px_2()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .text_color(colors.warning)
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .size(IconSize::XSmall)
                                .color(colors.warning),
                        )
                        .child("Last known data"),
                )
            })
            .when_some(
                self.snapshot.stopped_history.summary_label(),
                |element, label| {
                    let can_load = self.snapshot.stopped_history.can_load_more
                        && self.snapshot.connection.is_connected()
                        && !self.snapshot.stale;
                    let pending = self.pending_load_more_stopped;
                    element.child(
                        div()
                            .id("stopped-history-status")
                            .role(if can_load { Role::Button } else { Role::Status })
                            .aria_label(if can_load {
                                format!("{label}. Load more stopped results.")
                            } else {
                                label.clone()
                            })
                            .h_full()
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(label)
                            .when(can_load, |element| {
                                element
                                    .focusable()
                                    .tab_stop(true)
                                    .cursor_pointer()
                                    .hover(|style| style.bg(colors.surface_hover))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.request_load_more_stopped(cx);
                                    }))
                                    .child(div().text_color(colors.information).child(if pending {
                                        "Loading..."
                                    } else {
                                        "Load more"
                                    }))
                            }),
                    )
                },
            )
            .child({
                let activity_count = self.activity_log.len();
                let activity_label = if activity_count == 0 {
                    "Activity history".to_owned()
                } else {
                    format!("Activity history, {activity_count} recent events")
                };
                div()
                    .id("activity-status")
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label(activity_label)
                    .aria_expanded(self.activity_panel_open)
                    .ml_auto()
                    .h_full()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_xs()
                    .text_color(if self.activity_panel_open {
                        colors.text_primary
                    } else {
                        colors.text_muted
                    })
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .focus_visible(|style| style.bg(colors.surface_active))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_activity_panel(window, cx);
                    }))
                    .child(Icon::new(IconName::Activity).size(IconSize::XSmall).color(
                        if self.activity_panel_open {
                            colors.information
                        } else {
                            colors.text_muted
                        },
                    ))
                    .child(if activity_count == 0 {
                        "Activity".to_owned()
                    } else {
                        format!("Activity · {activity_count}")
                    })
            })
            .child(
                div()
                    .id("transfer-status")
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label(format!(
                        "Transfer speed, download {}, upload {}; show last minute chart",
                        format_rate(self.snapshot.download_rate),
                        format_rate(self.snapshot.upload_rate)
                    ))
                    .aria_expanded(self.speed_popover_open)
                    .h_full()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .font_features(tabular_numbers())
                    .text_xs()
                    .text_color(colors.text_muted)
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .focus_visible(|style| style.bg(colors.surface_active))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_speed_popover(window, cx);
                    }))
                    .child(
                        Icon::new(IconName::ArrowDown)
                            .size(IconSize::XSmall)
                            .color(colors.progress_download),
                    )
                    .child(format_rate(self.snapshot.download_rate))
                    .child(
                        Icon::new(IconName::ArrowUp)
                            .size(IconSize::XSmall)
                            .color(colors.progress_upload),
                    )
                    .child(format_rate(self.snapshot.upload_rate)),
            )
    }

    fn render_speed_popover(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = self.theme.colors;
        div()
            .id("speed-popover-layer")
            .absolute()
            .inset_0()
            .occlude()
            .on_click(cx.listener(|this, _, window, cx| {
                this.close_speed_popover(window, cx);
            }))
            .child(
                div()
                    .id("speed-popover")
                    .absolute()
                    .right(px(8.0))
                    .bottom(px(32.0))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .bg(colors.elevated_surface)
                    .child(self.render_speed_chart()),
            )
    }

    fn render_task_context_menu(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(menu) = self.context_menu.as_ref() else {
            return div().into_any_element();
        };
        let identity = menu.identity.clone();
        let position = menu.position;
        let Some(task) = self
            .snapshot
            .tasks
            .iter()
            .find(|task| task.identity == identity)
            .cloned()
        else {
            return div().into_any_element();
        };
        let idle = self.pending_task_command.is_none()
            && self.pending_batch_command.is_none()
            && self.remove_confirmation.is_none()
            && self.output_name_dialog.is_none()
            && self.task_speed_limit_dialog.is_none()
            && self.task_options_dialog.is_none();
        let commands_available = self.snapshot.commands_available() && idle;
        let path_actions = commands_available && self.snapshot.local_path_actions_available;
        let can_pause = commands_available && task.status.can_pause();
        let can_force_pause = can_pause && self.snapshot.capabilities.force_pause;
        let can_resume = commands_available && task.status.can_resume();
        let can_retry = commands_available && task.status.can_retry();
        let can_remove = commands_available && task.status.can_remove();
        let can_queue = commands_available
            && task.status.can_move_in_queue()
            && self.queue_reordering_available();
        let can_output = commands_available && task.can_set_output_name();
        let can_speed = commands_available
            && task.status.can_set_speed_limit()
            && self.snapshot.capabilities.change_option;
        let has_source = task
            .primary_source
            .as_deref()
            .is_some_and(|value| !value.is_empty());

        let mut entries: Vec<(
            ContextMenuAction,
            &'static str,
            Option<&'static str>,
            bool,
            bool,
        )> = vec![
            (
                ContextMenuAction::Details,
                "Details",
                Some("Enter"),
                true,
                false,
            ),
            (
                ContextMenuAction::OpenDownload,
                "Open download",
                None,
                path_actions,
                false,
            ),
            (
                ContextMenuAction::OpenFolder,
                "Open folder",
                None,
                path_actions,
                false,
            ),
            (
                ContextMenuAction::CopySource,
                "Copy source",
                None,
                has_source,
                false,
            ),
            (ContextMenuAction::CopyGid, "Copy GID", None, true, false),
        ];
        if task.status.can_pause() {
            entries.push((
                ContextMenuAction::Pause,
                "Pause",
                Some("Cmd+Shift+P"),
                can_pause,
                false,
            ));
            entries.push((
                ContextMenuAction::ForcePause,
                "Force pause",
                None,
                can_force_pause,
                false,
            ));
        }
        if task.status.can_resume() {
            entries.push((
                ContextMenuAction::Resume,
                "Resume",
                Some("Cmd+Shift+R"),
                can_resume,
                false,
            ));
        }
        if task.status.can_retry() {
            entries.push((
                ContextMenuAction::Retry,
                "Retry",
                Some("Cmd+Alt+R"),
                can_retry,
                false,
            ));
        }
        if task.status.can_move_in_queue() {
            entries.push((
                ContextMenuAction::MoveTop,
                "Move to top",
                Some("Cmd+Shift+Home"),
                can_queue,
                false,
            ));
            entries.push((
                ContextMenuAction::MoveUp,
                "Move up",
                Some("Cmd+Shift+Up"),
                can_queue,
                false,
            ));
            entries.push((
                ContextMenuAction::MoveDown,
                "Move down",
                Some("Cmd+Shift+Down"),
                can_queue,
                false,
            ));
            entries.push((
                ContextMenuAction::MoveBottom,
                "Move to bottom",
                Some("Cmd+Shift+End"),
                can_queue,
                false,
            ));
        }
        if task.can_set_output_name() {
            entries.push((
                ContextMenuAction::OutputName,
                "Change output name",
                Some("F2"),
                can_output,
                false,
            ));
        }
        if task.status.can_set_speed_limit() {
            entries.push((
                ContextMenuAction::SpeedLimit,
                "Set speed limits",
                None,
                can_speed,
                false,
            ));
            entries.push((
                ContextMenuAction::TaskOptions,
                "Edit task options",
                None,
                can_speed,
                false,
            ));
        }
        entries.push((
            ContextMenuAction::Remove,
            "Remove",
            Some("Delete"),
            can_remove,
            true,
        ));

        let left = f32::from(position.x).max(8.0);
        let top = f32::from(position.y).max(8.0);
        let display_name = task_display_name(&task);

        div()
            .id("task-context-menu-layer")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_task_context_menu(cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| this.close_task_context_menu(cx)),
            )
            .child(
                div()
                    .id("task-context-menu")
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(260.0))
                    .py_1()
                    .px_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.elevated_surface)
                    .shadow_md()
                    .role(Role::Menu)
                    .aria_label(format!("Actions for {display_name}"))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                    .flex()
                    .flex_col()
                    .children(entries.into_iter().map(
                        |(action, label, shortcut, enabled, destructive)| {
                            context_menu_item(
                                action,
                                label,
                                shortcut,
                                enabled,
                                destructive,
                                colors,
                                cx,
                            )
                        },
                    )),
            )
            .into_any_element()
    }

    fn activate_context_menu_action(
        &mut self,
        action: ContextMenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Capture the right-clicked task before closing so multi-select still
        // has a single authoritative target for details/copy/open.
        let menu_task = self.context_menu_task_view();
        self.close_task_context_menu(cx);
        match action {
            ContextMenuAction::Details => {
                if let Some(task) = menu_task.or_else(|| self.selected_task_view()) {
                    self.open_details_for(task, cx);
                }
            }
            ContextMenuAction::OpenDownload => {
                self.request_task_open_for_selection(TaskOpenTargetView::Download, cx);
            }
            ContextMenuAction::OpenFolder => {
                self.request_task_open_for_selection(TaskOpenTargetView::Folder, cx);
            }
            ContextMenuAction::CopySource => {
                if let Some(task) = menu_task.or_else(|| self.selected_task_view()) {
                    self.copy_task_source(&task, cx);
                }
            }
            ContextMenuAction::CopyGid => {
                if let Some(task) = menu_task.or_else(|| self.selected_task_view()) {
                    self.copy_task_gid(&task, cx);
                }
            }
            ContextMenuAction::Pause => {
                self.begin_task_command(TaskCommandView::Pause, cx);
            }
            ContextMenuAction::ForcePause => {
                self.begin_task_command(TaskCommandView::ForcePause, cx);
            }
            ContextMenuAction::Resume => {
                self.begin_task_command(TaskCommandView::Resume, cx);
            }
            ContextMenuAction::Retry => {
                self.begin_task_command(TaskCommandView::Retry, cx);
            }
            ContextMenuAction::MoveTop => {
                self.begin_task_command(TaskCommandView::MoveToQueueTop, cx);
            }
            ContextMenuAction::MoveUp => {
                self.begin_task_command(TaskCommandView::MoveUpInQueue, cx);
            }
            ContextMenuAction::MoveDown => {
                self.begin_task_command(TaskCommandView::MoveDownInQueue, cx);
            }
            ContextMenuAction::MoveBottom => {
                self.begin_task_command(TaskCommandView::MoveToQueueBottom, cx);
            }
            ContextMenuAction::OutputName => self.open_task_output_name(window, cx),
            ContextMenuAction::SpeedLimit => self.open_task_speed_limit(window, cx),
            ContextMenuAction::TaskOptions => self.open_task_options(window, cx),
            ContextMenuAction::Remove => self.confirm_remove_selected(window, cx),
        }
    }

    /// Sort menu and engine-wide pause-all/resume-all controls (D-014).
    fn render_list_controls(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let idle = self.pending_task_command.is_none()
            && self.pending_global_task_command.is_none()
            && self.pending_batch_command.is_none()
            && self.remove_confirmation.is_none();
        let commands_available = self.snapshot.commands_available() && idle;
        let pending_global = self
            .pending_global_task_command
            .as_ref()
            .map(|pending| pending.command);
        let sort_label = self.query.sort_key.label();

        div()
            .ml_2()
            .flex()
            .items_center()
            .gap_1()
            .child(
                IconButton::new("pause-all-action", IconName::Pause)
                    .aria_label("Pause all tasks")
                    .style(ButtonStyle::Ghost)
                    .disabled(!commands_available)
                    .loading(pending_global == Some(GlobalTaskCommandView::PauseAll))
                    .tooltip(Tooltip::new("Pause all"))
                    .render(colors)
                    .when(commands_available, |button| {
                        button.on_click(cx.listener(|this, _, _, cx| {
                            this.begin_global_task_command(GlobalTaskCommandView::PauseAll, cx);
                        }))
                    }),
            )
            .child(
                IconButton::new("force-pause-all-action", IconName::Square)
                    .aria_label("Force pause all tasks")
                    .style(ButtonStyle::Ghost)
                    .disabled(!commands_available || !self.snapshot.capabilities.force_pause_all)
                    .loading(pending_global == Some(GlobalTaskCommandView::ForcePauseAll))
                    .tooltip(Tooltip::new(
                        if self.snapshot.capabilities.force_pause_all {
                            "Force pause all"
                        } else {
                            self.snapshot
                                .capabilities
                                .unsupported_force_pause_all_message()
                        },
                    ))
                    .render(colors)
                    .when(
                        commands_available && self.snapshot.capabilities.force_pause_all,
                        |button| {
                            button.on_click(cx.listener(|this, _, _, cx| {
                                this.begin_global_task_command(
                                    GlobalTaskCommandView::ForcePauseAll,
                                    cx,
                                );
                            }))
                        },
                    ),
            )
            .child(
                IconButton::new("resume-all-action", IconName::Play)
                    .aria_label("Resume all tasks")
                    .style(ButtonStyle::Ghost)
                    .disabled(!commands_available)
                    .loading(pending_global == Some(GlobalTaskCommandView::ResumeAll))
                    .tooltip(Tooltip::new("Resume all"))
                    .render(colors)
                    .when(commands_available, |button| {
                        button.on_click(cx.listener(|this, _, _, cx| {
                            this.begin_global_task_command(GlobalTaskCommandView::ResumeAll, cx);
                        }))
                    }),
            )
            .child(
                div()
                    .id("sort-menu-trigger")
                    .focusable()
                    .tab_stop(true)
                    .role(Role::Button)
                    .aria_label(format!("Sort by {sort_label}"))
                    .aria_expanded(self.sort_popover_open)
                    .h(px(28.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded_md()
                    .text_xs()
                    .text_color(colors.text_secondary)
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .focus_visible(|style| style.border_1().border_color(colors.focus_ring))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_sort_popover(cx);
                    }))
                    .child(
                        Icon::new(IconName::ArrowUpDown)
                            .size(IconSize::Small)
                            .color(colors.text_muted),
                    )
                    .child(sort_label),
            )
    }

    fn render_sort_popover(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = self.theme.colors;
        let current_key = self.query.sort_key;
        let current_direction = self.query.sort_direction;

        let mut menu = div()
            .id("sort-menu")
            .absolute()
            .right(px(12.0))
            .top(px(96.0))
            .w(px(220.0))
            .on_click(|_, _, cx| cx.stop_propagation())
            .bg(colors.elevated_surface)
            .border_1()
            .border_color(colors.border)
            .rounded_lg()
            .p_1()
            .flex()
            .flex_col()
            .gap_px()
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(colors.text_muted)
                    .child("Sort by"),
            );

        for key in WorkspaceSortKey::ALL {
            let selected = key == current_key;
            menu = menu.child(
                div()
                    .id(SharedString::from(format!("sort-key-{}", key.key())))
                    .role(Role::Button)
                    .aria_label(format!("Sort by {}", key.label()))
                    .h(px(32.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .text_xs()
                    .text_color(if selected {
                        colors.accent
                    } else {
                        colors.text_secondary
                    })
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_sort_key(key, cx);
                    }))
                    .child(div().w(px(16.0)).flex_none().when(selected, |element| {
                        element.child(
                            Icon::new(IconName::Check)
                                .size(IconSize::Small)
                                .color(colors.accent),
                        )
                    }))
                    .child(div().flex_1().child(key.label())),
            );
        }

        menu = menu.child(
            div()
                .mt_1()
                .pt_1()
                .border_t_1()
                .border_color(colors.border)
                .flex()
                .flex_col()
                .gap_px(),
        );
        for direction in [
            WorkspaceSortDirection::Ascending,
            WorkspaceSortDirection::Descending,
        ] {
            let selected = direction == current_direction;
            let icon = match direction {
                WorkspaceSortDirection::Ascending => IconName::ArrowUp,
                WorkspaceSortDirection::Descending => IconName::ArrowDown,
            };
            menu = menu.child(
                div()
                    .id(SharedString::from(match direction {
                        WorkspaceSortDirection::Ascending => "sort-direction-ascending",
                        WorkspaceSortDirection::Descending => "sort-direction-descending",
                    }))
                    .role(Role::Button)
                    .aria_label(format!("{} order", direction.label()))
                    .aria_toggled(if selected {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .h(px(32.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .text_xs()
                    .text_color(if selected {
                        colors.accent
                    } else {
                        colors.text_secondary
                    })
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_sort_direction(direction, cx);
                    }))
                    .child(div().w(px(16.0)).flex_none().child(
                        Icon::new(icon).size(IconSize::Small).color(if selected {
                            colors.accent
                        } else {
                            colors.text_muted
                        }),
                    ))
                    .child(div().flex_1().child(direction.label())),
            );
        }

        div()
            .id("sort-popover-layer")
            .absolute()
            .inset_0()
            .occlude()
            .on_click(cx.listener(|this, _, _, cx| {
                this.close_sort_popover(cx);
            }))
            .child(menu)
    }

    fn render_task_toolbar(&mut self, cx: &mut Context<Self>) -> Div {
        let visible_selected_count = self.visible_selected_task_count();
        if visible_selected_count > 1 {
            return self.render_batch_task_toolbar(cx);
        }
        if !self.selected_tasks.is_empty() && self.selected_tasks.len() > visible_selected_count {
            return self.render_hidden_selection_toolbar(cx);
        }
        let colors = self.theme.colors;
        let Some(task) = self.selected_task_view() else {
            return div();
        };
        let idle = self.pending_task_command.is_none()
            && self.pending_batch_command.is_none()
            && self.remove_confirmation.is_none()
            && self.output_name_dialog.is_none();
        let pending_command = self
            .pending_task_command
            .as_ref()
            .map(|pending| pending.command.clone());
        let commands_available = self.snapshot.commands_available() && idle;
        let details_enabled = self.snapshot.commands_available();
        let pause_enabled = commands_available && task.status.can_pause();
        let force_pause_enabled = pause_enabled && self.snapshot.capabilities.force_pause;
        let resume_enabled = commands_available && task.status.can_resume();
        let retry_enabled = commands_available && task.status.can_retry();
        let remove_enabled = commands_available && task.status.can_remove();

        div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                toolbar_icon_button(
                    "task-details-action",
                    IconName::PanelRight,
                    "Details",
                    ToolbarButtonState::from_flags(details_enabled, false),
                    false,
                    Some("Enter"),
                    colors,
                )
                .when(details_enabled, |button| {
                    button.on_click(cx.listener(|this, _, _window, cx| {
                        if let Some(task) = this.selected_task_view() {
                            this.open_details_for(task, cx);
                        }
                    }))
                }),
            )
            .when(task.status.can_pause(), |element| {
                element
                    .child(
                        toolbar_icon_button(
                            "pause-task-action",
                            IconName::Pause,
                            "Pause",
                            ToolbarButtonState::from_flags(
                                pause_enabled,
                                pending_command == Some(TaskCommandView::Pause),
                            ),
                            false,
                            Some("Cmd+Shift+P"),
                            colors,
                        )
                        .when(pause_enabled, |button| {
                            button.on_click(cx.listener(|this, _, _window, cx| {
                                this.begin_task_command(TaskCommandView::Pause, cx);
                            }))
                        }),
                    )
                    .child(
                        toolbar_icon_button(
                            "force-pause-task-action",
                            IconName::Square,
                            if self.snapshot.capabilities.force_pause {
                                "Force pause"
                            } else {
                                self.snapshot.capabilities.unsupported_force_pause_message()
                            },
                            ToolbarButtonState::from_flags(
                                force_pause_enabled,
                                pending_command == Some(TaskCommandView::ForcePause),
                            ),
                            false,
                            None,
                            colors,
                        )
                        .when(force_pause_enabled, |button| {
                            button.on_click(cx.listener(|this, _, _window, cx| {
                                this.begin_task_command(TaskCommandView::ForcePause, cx);
                            }))
                        }),
                    )
            })
            .when(task.status.can_resume(), |element| {
                element.child(
                    toolbar_icon_button(
                        "resume-task-action",
                        IconName::Play,
                        "Resume",
                        ToolbarButtonState::from_flags(
                            resume_enabled,
                            pending_command == Some(TaskCommandView::Resume),
                        ),
                        false,
                        Some("Cmd+Shift+R"),
                        colors,
                    )
                    .when(resume_enabled, |button| {
                        button.on_click(cx.listener(|this, _, _window, cx| {
                            this.begin_task_command(TaskCommandView::Resume, cx);
                        }))
                    }),
                )
            })
            .when(
                task.status.can_move_in_queue() && self.queue_reordering_available(),
                |element| {
                    let queue_enabled = commands_available;
                    element.children([
                        queue_move_button(
                            "queue-move-top-action",
                            IconName::ChevronsUp,
                            "Move to top",
                            TaskCommandView::MoveToQueueTop,
                            queue_enabled,
                            pending_command.as_ref(),
                            Some("Cmd+Shift+Home"),
                            colors,
                            cx,
                        ),
                        queue_move_button(
                            "queue-move-up-action",
                            IconName::ChevronUp,
                            "Move up",
                            TaskCommandView::MoveUpInQueue,
                            queue_enabled,
                            pending_command.as_ref(),
                            Some("Cmd+Shift+Up"),
                            colors,
                            cx,
                        ),
                        queue_move_button(
                            "queue-move-down-action",
                            IconName::ChevronDown,
                            "Move down",
                            TaskCommandView::MoveDownInQueue,
                            queue_enabled,
                            pending_command.as_ref(),
                            Some("Cmd+Shift+Down"),
                            colors,
                            cx,
                        ),
                        queue_move_button(
                            "queue-move-bottom-action",
                            IconName::ChevronsDown,
                            "Move to bottom",
                            TaskCommandView::MoveToQueueBottom,
                            queue_enabled,
                            pending_command.as_ref(),
                            Some("Cmd+Shift+End"),
                            colors,
                            cx,
                        ),
                    ])
                },
            )
            .when(task.status.can_retry(), |element| {
                element.child(
                    toolbar_icon_button(
                        "retry-task-action",
                        IconName::RotateCcw,
                        "Retry",
                        ToolbarButtonState::from_flags(
                            retry_enabled,
                            pending_command == Some(TaskCommandView::Retry),
                        ),
                        false,
                        Some("Cmd+Alt+R"),
                        colors,
                    )
                    .when(retry_enabled, |button| {
                        button.on_click(cx.listener(|this, _, _window, cx| {
                            this.begin_task_command(TaskCommandView::Retry, cx);
                        }))
                    }),
                )
            })
            .when(task.can_set_output_name(), |element| {
                element.child(
                    toolbar_icon_button(
                        "task-output-name-action",
                        IconName::Pencil,
                        "Change output name",
                        ToolbarButtonState::from_flags(commands_available, false),
                        false,
                        Some("F2"),
                        colors,
                    )
                    .when(commands_available, |button| {
                        button.on_click(cx.listener(|this, _, window, cx| {
                            this.open_task_output_name(window, cx);
                        }))
                    }),
                )
            })
            .when(task.status.can_set_speed_limit(), |element| {
                element
                    .child(
                        toolbar_icon_button(
                            "task-speed-limit-action",
                            IconName::ArrowUpDown,
                            "Set speed limits",
                            ToolbarButtonState::from_flags(commands_available, false),
                            false,
                            None,
                            colors,
                        )
                        .when(commands_available, |button| {
                            button.on_click(cx.listener(|this, _, window, cx| {
                                this.open_task_speed_limit(window, cx);
                            }))
                        }),
                    )
                    .child(
                        toolbar_icon_button(
                            "task-options-action",
                            IconName::Settings,
                            "Edit task options",
                            ToolbarButtonState::from_flags(commands_available, false),
                            false,
                            None,
                            colors,
                        )
                        .when(commands_available, |button| {
                            button.on_click(cx.listener(|this, _, window, cx| {
                                this.open_task_options(window, cx);
                            }))
                        }),
                    )
            })
            .child(
                toolbar_icon_button(
                    "remove-task-action",
                    IconName::Trash2,
                    "Remove",
                    ToolbarButtonState::from_flags(
                        remove_enabled,
                        matches!(
                            pending_command,
                            Some(
                                TaskCommandView::RemoveTask
                                    | TaskCommandView::ForceRemoveTask
                                    | TaskCommandView::RemoveTaskAndFiles
                            )
                        ),
                    ),
                    true,
                    Some("Delete"),
                    colors,
                )
                .when(remove_enabled, |button| {
                    button.on_click(cx.listener(|this, _, window, cx| {
                        this.confirm_remove_selected(window, cx);
                    }))
                }),
            )
    }

    fn render_batch_task_toolbar(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let selected = self
            .snapshot
            .tasks
            .iter()
            .filter(|task| self.selected_tasks.contains(&task.identity))
            .collect::<Vec<_>>();
        let idle = self.pending_task_command.is_none()
            && self.pending_batch_command.is_none()
            && self.remove_confirmation.is_none();
        let commands_available = self.snapshot.commands_available() && idle;
        let can_pause = selected.iter().any(|task| task.status.can_pause());
        let can_force_pause = can_pause && self.snapshot.capabilities.force_pause;
        let can_resume = selected.iter().any(|task| task.status.can_resume());
        let can_retry = selected.iter().any(|task| task.status.can_retry());
        let can_remove = selected.iter().any(|task| task.status.can_remove());
        let pending = self
            .pending_batch_command
            .as_ref()
            .map(|pending| pending.command);

        div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .mr_2()
                    .font_features(tabular_numbers())
                    .text_xs()
                    .text_color(colors.text_secondary)
                    .child(format!("{} selected", selected.len())),
            )
            .child(
                toolbar_icon_button(
                    "batch-pause-action",
                    IconName::Pause,
                    "Pause selected",
                    ToolbarButtonState::from_flags(
                        commands_available && can_pause,
                        pending == Some(BatchTaskCommandView::Pause),
                    ),
                    false,
                    Some("Cmd+Shift+P"),
                    colors,
                )
                .when(commands_available && can_pause, |button| {
                    button.on_click(cx.listener(|this, _, _, cx| {
                        this.begin_batch_task_command(BatchTaskCommandView::Pause, cx);
                    }))
                }),
            )
            .child(
                toolbar_icon_button(
                    "batch-force-pause-action",
                    IconName::Square,
                    if self.snapshot.capabilities.force_pause {
                        "Force pause selected"
                    } else {
                        self.snapshot.capabilities.unsupported_force_pause_message()
                    },
                    ToolbarButtonState::from_flags(
                        commands_available && can_force_pause,
                        pending == Some(BatchTaskCommandView::ForcePause),
                    ),
                    false,
                    None,
                    colors,
                )
                .when(commands_available && can_force_pause, |button| {
                    button.on_click(cx.listener(|this, _, _, cx| {
                        this.begin_batch_task_command(BatchTaskCommandView::ForcePause, cx);
                    }))
                }),
            )
            .child(
                toolbar_icon_button(
                    "batch-resume-action",
                    IconName::Play,
                    "Resume selected",
                    ToolbarButtonState::from_flags(
                        commands_available && can_resume,
                        pending == Some(BatchTaskCommandView::Resume),
                    ),
                    false,
                    Some("Cmd+Shift+R"),
                    colors,
                )
                .when(commands_available && can_resume, |button| {
                    button.on_click(cx.listener(|this, _, _, cx| {
                        this.begin_batch_task_command(BatchTaskCommandView::Resume, cx);
                    }))
                }),
            )
            .child(
                toolbar_icon_button(
                    "batch-retry-action",
                    IconName::RotateCcw,
                    "Retry selected",
                    ToolbarButtonState::from_flags(
                        commands_available && can_retry,
                        pending == Some(BatchTaskCommandView::Retry),
                    ),
                    false,
                    Some("Cmd+Alt+R"),
                    colors,
                )
                .when(commands_available && can_retry, |button| {
                    button.on_click(cx.listener(|this, _, _, cx| {
                        this.begin_batch_task_command(BatchTaskCommandView::Retry, cx);
                    }))
                }),
            )
            .child(
                toolbar_icon_button(
                    "batch-remove-action",
                    IconName::Trash2,
                    "Remove selected",
                    ToolbarButtonState::from_flags(
                        commands_available && can_remove,
                        matches!(
                            pending,
                            Some(
                                BatchTaskCommandView::RemoveTask
                                    | BatchTaskCommandView::ForceRemoveTask
                                    | BatchTaskCommandView::RemoveTaskAndFiles
                            )
                        ),
                    ),
                    true,
                    Some("Delete"),
                    colors,
                )
                .when(commands_available && can_remove, |button| {
                    button.on_click(cx.listener(|this, _, window, cx| {
                        this.confirm_remove_selected(window, cx);
                    }))
                }),
            )
            .child(
                toolbar_icon_button(
                    "clear-task-selection",
                    IconName::X,
                    "Clear selection",
                    ToolbarButtonState::from_flags(idle, false),
                    false,
                    Some("Escape"),
                    colors,
                )
                .when(idle, |button| {
                    button.on_click(cx.listener(|this, _, _, cx| {
                        this.clear_task_selection();
                        cx.notify();
                    }))
                }),
            )
    }

    fn render_hidden_selection_toolbar(&mut self, cx: &mut Context<Self>) -> Div {
        let colors = self.theme.colors;
        let visible = self.visible_selected_task_count();
        let hidden = self.selected_tasks.len().saturating_sub(visible);
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .font_features(tabular_numbers())
                    .text_xs()
                    .text_color(colors.text_secondary)
                    .child(format!("{visible} visible, {hidden} hidden selected")),
            )
            .child(
                toolbar_icon_button(
                    "clear-hidden-task-selection",
                    IconName::X,
                    "Clear selection",
                    ToolbarButtonState::Enabled,
                    false,
                    Some("Escape"),
                    colors,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.clear_task_selection();
                    cx.notify();
                })),
            )
    }

    fn render_task_details_drawer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(drawer) = self.details_drawer.as_ref() else {
            return div().into_any_element();
        };
        let identity = drawer.identity.clone();
        let overview = drawer.overview.clone();
        let selected_tab = drawer.tab;
        let display_name = task_display_name(&overview);
        let overview_progress = overview.progress_basis_points();
        let path_actions_available =
            self.snapshot.commands_available() && self.snapshot.local_path_actions_available;
        let path_open_pending = drawer.open_pending.is_some();
        let presentation = match &drawer.state {
            TaskDetailsLoadState::Loading => TaskDetailsPresentation::Loading,
            TaskDetailsLoadState::Ready { details } => {
                TaskDetailsPresentation::Ready(details.clone())
            }
            TaskDetailsLoadState::Failed { error } => {
                TaskDetailsPresentation::Failed(error.summary.clone())
            }
            TaskDetailsLoadState::Stale => TaskDetailsPresentation::Stale,
        };

        let body = match presentation {
            TaskDetailsPresentation::Loading => drawer_message(
                "Loading task details",
                "Requesting file metadata from this aria2 session.",
                colors,
            ),
            TaskDetailsPresentation::Failed(summary) => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .px_5()
                .text_center()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.danger)
                        .child("Could not load task details"),
                )
                .child(div().text_xs().text_color(colors.text_muted).child(summary))
                .child(
                    toolbar_icon_button(
                        "retry-task-details",
                        IconName::RotateCcw,
                        "Retry",
                        ToolbarButtonState::Enabled,
                        false,
                        None,
                        colors,
                    )
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.request_current_details(cx);
                    })),
                )
                .into_any_element(),
            TaskDetailsPresentation::Stale => div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .px_5()
                .text_center()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.warning)
                        .child("Details are stale"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.text_muted)
                        .child("Reconnect to refresh files for the current engine session."),
                )
                .when(self.snapshot.commands_available(), |element| {
                    element.child(
                        toolbar_icon_button(
                            "refresh-task-details",
                            IconName::RefreshCw,
                            "Refresh",
                            ToolbarButtonState::Enabled,
                            false,
                            None,
                            colors,
                        )
                        .on_click(cx.listener(|this, _, _window, cx| {
                            this.request_current_details(cx);
                        })),
                    )
                })
                .into_any_element(),
            TaskDetailsPresentation::Ready(details) => {
                let TaskDetailsView {
                    directory,
                    primary_source,
                    output_path,
                    path_validation,
                    info_hash,
                    piece_length,
                    piece_count,
                    trackers,
                    uris,
                    servers,
                    peers,
                    options,
                    files,
                } = *details;
                let file_count = files.len();
                let gid = identity.gid.clone();
                let is_bittorrent = matches!(
                    overview.source_kind,
                    crate::TaskSourceKindView::Magnet | crate::TaskSourceKindView::BitTorrent
                ) || overview.status == TaskStatusView::Seeding;
                let seed_stop_rules = format_seed_stop_rules(&options);
                let path_validation_label = match path_validation {
                    TaskPathValidationView::Unavailable => {
                        "Unavailable for an external or remote engine profile.".into()
                    }
                    TaskPathValidationView::Valid {
                        existing_files,
                        missing_paths,
                    } => format!(
                        "Validated locally: {existing_files} existing, {missing_paths} missing."
                    ),
                    TaskPathValidationView::Warning(error) => error.summary,
                };
                let shell = cx.entity().downgrade();
                let tabs = SegmentedControl::new(
                    "task-details-tabs",
                    [
                        Segment::new("Info"),
                        Segment::new("Files"),
                        Segment::new("Network"),
                        Segment::new("Options"),
                    ],
                    match selected_tab {
                        TaskDetailsTab::Info => 0,
                        TaskDetailsTab::Files => 1,
                        TaskDetailsTab::Network => 2,
                        TaskDetailsTab::Options => 3,
                    },
                    self.theme,
                )
                .on_select(move |index, _window, cx| {
                    let tab = match index {
                        1 => TaskDetailsTab::Files,
                        2 => TaskDetailsTab::Network,
                        3 => TaskDetailsTab::Options,
                        _ => TaskDetailsTab::Info,
                    };
                    shell
                        .update(cx, |shell, cx| {
                            if let Some(drawer) = &mut shell.details_drawer {
                                drawer.tab = tab;
                                cx.notify();
                            }
                        })
                        .ok();
                });

                let content = match selected_tab {
                    TaskDetailsTab::Info => div()
                        .id("task-details-info-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p_4()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(detail_line_with_action(
                            "GID",
                            gid.clone(),
                            IconButton::new("copy-task-gid", IconName::Copy)
                                .aria_label("Copy task GID")
                                .tooltip(Tooltip::new("Copy GID"))
                                .on_click({
                                    let gid = gid.clone();
                                    cx.listener(move |this, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            gid.clone(),
                                        ));
                                        this.show_notice("GID copied.", false, cx);
                                    })
                                })
                                .render(colors),
                            colors,
                        ))
                        .child(detail_line(
                            "Source type",
                            overview.source_kind.label(),
                            colors,
                        ))
                        .when_some(
                            primary_source
                                .as_deref()
                                .or(overview.primary_source.as_deref()),
                            |element, source| element.child(detail_line("Source", source, colors)),
                        )
                        .child(detail_line(
                            "Directory",
                            directory.as_deref().unwrap_or("Not reported"),
                            colors,
                        ))
                        .when_some(output_path.as_deref(), |element, path| {
                            element.child(detail_line("Output", path, colors))
                        })
                        .child(detail_line(
                            "Local path check",
                            path_validation_label,
                            colors,
                        ))
                        .when_some(overview.error.as_ref(), |element, error| {
                            element
                                .child(detail_line("Failure", error.summary.clone(), colors))
                                .when_some(error.details.as_deref(), |element, details| {
                                    element.child(detail_line("aria2 details", details, colors))
                                })
                        })
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .pt_2()
                                .child(
                                    toolbar_icon_button(
                                        "open-task-download",
                                        IconName::Download,
                                        "Open download",
                                        if path_actions_available && !path_open_pending {
                                            ToolbarButtonState::Enabled
                                        } else {
                                            ToolbarButtonState::Disabled
                                        },
                                        false,
                                        None,
                                        colors,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.request_task_open(
                                                TaskOpenTargetView::Download,
                                                cx,
                                            );
                                        },
                                    )),
                                )
                                .child(
                                    toolbar_icon_button(
                                        "open-task-folder",
                                        IconName::FolderDown,
                                        "Open folder",
                                        if path_actions_available && !path_open_pending {
                                            ToolbarButtonState::Enabled
                                        } else {
                                            ToolbarButtonState::Disabled
                                        },
                                        false,
                                        None,
                                        colors,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.request_task_open(TaskOpenTargetView::Folder, cx);
                                        },
                                    )),
                                ),
                        )
                        .when_some(info_hash.as_deref(), |element, hash| {
                            element.child(detail_line("Info hash", hash, colors))
                        })
                        .when(piece_length.is_some() || piece_count.is_some(), |element| {
                            element.child(detail_line(
                                "Pieces",
                                format!(
                                    "{} x {}",
                                    piece_count
                                        .map_or_else(|| "?".into(), |value| value.to_string()),
                                    piece_length.map_or_else(|| "unknown".into(), format_bytes)
                                ),
                                colors,
                            ))
                        })
                        .when(is_bittorrent, |element| {
                            element.child(detail_line(
                                "Effective seed limits",
                                seed_stop_rules,
                                colors,
                            ))
                        })
                        .into_any_element(),
                    TaskDetailsTab::Files => {
                        if file_count == 0 {
                            drawer_message(
                                "No files reported",
                                "aria2 did not return any file entries for this task.",
                                colors,
                            )
                        } else {
                            let list_id =
                                SharedString::from(format!("task-files:{}", identity.gid));
                            div()
                                .id(list_id.clone())
                                .role(Role::List)
                                .aria_label(format!("Task files, {file_count} items"))
                                .flex_1()
                                .min_h_0()
                                .child(
                                    uniform_list(
                                        list_id,
                                        file_count,
                                        cx.processor(
                                            move |this, range: Range<usize>, _window, _cx| {
                                                let colors = this.theme.colors;
                                                let Some(drawer) = &mut this.details_drawer else {
                                                    return Vec::new();
                                                };
                                                drawer.rendered_file_range = range.clone();
                                                let TaskDetailsLoadState::Ready { details } =
                                                    &drawer.state
                                                else {
                                                    return Vec::new();
                                                };
                                                let gid = drawer.identity.gid.clone();
                                                range
                                                    .filter_map(|index| {
                                                        details.files.get(index).cloned().map(
                                                            |file| {
                                                                render_file_row(
                                                                    &gid, index, file, file_count,
                                                                    colors,
                                                                )
                                                            },
                                                        )
                                                    })
                                                    .collect::<Vec<_>>()
                                            },
                                        ),
                                    )
                                    .track_scroll(
                                        &self
                                            .details_drawer
                                            .as_ref()
                                            .expect("details drawer exists while rendering files")
                                            .file_scroll,
                                    )
                                    .size_full(),
                                )
                                .into_any_element()
                        }
                    }
                    TaskDetailsTab::Network => div()
                        .id("task-details-network-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p_4()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(detail_collection_section(
                            "Sources and mirrors",
                            "No source URIs reported.",
                            uris.into_iter()
                                .map(|source| render_task_uri(source, colors))
                                .collect(),
                            colors,
                        ))
                        .child(detail_collection_section(
                            "Trackers",
                            "No BitTorrent trackers reported.",
                            trackers
                                .into_iter()
                                .map(|tracker| render_task_tracker(tracker, colors))
                                .collect(),
                            colors,
                        ))
                        .child(detail_collection_section(
                            "Servers",
                            "No active HTTP, HTTPS, or FTP servers.",
                            servers
                                .into_iter()
                                .map(|server| render_task_server(server, colors))
                                .collect(),
                            colors,
                        ))
                        .child(detail_collection_section(
                            "Peers",
                            "No active BitTorrent peers.",
                            peers
                                .into_iter()
                                .map(|peer| render_task_peer(peer, colors))
                                .collect(),
                            colors,
                        ))
                        .into_any_element(),
                    TaskDetailsTab::Options => detail_collection_section(
                        "Read-only task options",
                        "No task-specific options reported.",
                        options
                            .into_iter()
                            .map(|option| render_task_option(option, colors))
                            .collect(),
                        colors,
                    )
                    .id("task-details-options-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .into_any_element(),
                };

                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex_none()
                            .p_3()
                            .border_b_1()
                            .border_color(colors.border)
                            .bg(colors.toolbar_surface)
                            .child(tabs),
                    )
                    .child(content)
                    .into_any_element()
            }
        };

        div()
            .id("task-details-drawer")
            .role(Role::Complementary)
            .aria_label(format!("Task details for {display_name}"))
            .w(px(DETAILS_DRAWER_WIDTH))
            .flex_none()
            .min_h_0()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(colors.border)
            .bg(colors.surface)
            .child(
                div()
                    .h(px(68.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .size(px(36.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .border_1()
                            .border_color(colors.border)
                            .bg(colors.elevated_surface)
                            .child(
                                Icon::new(task_status_icon(overview.status))
                                    .size(IconSize::Small)
                                    .color(task_status_color(overview.status, colors)),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(display_name),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(overview.status.label())
                                    .child(format_percent(overview_progress)),
                            ),
                    )
                    .child(
                        toolbar_icon_button(
                            "close-task-details",
                            IconName::X,
                            "Close details",
                            ToolbarButtonState::Enabled,
                            false,
                            None,
                            colors,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_task_details(window, cx);
                        })),
                    ),
            )
            .child(task_overview_summary(&overview, colors))
            .child(body)
            .into_any_element()
    }

    fn render_task_row(
        &mut self,
        index: usize,
        task: DownloadRowView,
        layout: TaskLayoutMode,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let colors = self.theme.colors;
        let focused = self.selected.as_ref() == Some(&task.identity);
        let selected = self.selected_tasks.contains(&task.identity);
        let stable_id = SharedString::from(format!(
            "task-row:{}:{}",
            task.identity.profile_id, task.identity.gid
        ));
        let task_count = self.snapshot.tasks.len();
        let basis_points = task.progress_basis_points();
        let progress = f32::from(basis_points.unwrap_or(0)) / 10_000.0;
        let seeding = task.status == TaskStatusView::Seeding;
        let share_ratio = format_share_ratio(task.share_ratio_milli());
        let observed_seeding = format_eta(task.observed_seeding_seconds);
        let status_color = task_status_color(task.status, colors);
        let display_name = task_display_name(&task);
        let size_label = if task.total_bytes == 0 {
            format_bytes(task.completed_bytes)
        } else {
            format!(
                "{} / {}",
                format_bytes(task.completed_bytes),
                format_bytes(task.total_bytes)
            )
        };
        let task_error_label = task.error.as_ref().map(|error| {
            error.code.map_or_else(
                || error.summary.clone(),
                |code| format!("Error {code}: {}", error.summary),
            )
        });
        let mut aria_label = if seeding {
            format!(
                "{}, Seeding, share ratio {}, uploaded {}, upload speed {}, observed seeding time {} in this session",
                display_name.as_str(),
                share_ratio,
                format_bytes(task.uploaded_bytes),
                format_rate(task.upload_rate),
                observed_seeding
            )
        } else {
            format!(
                "{}, {}, {}, download speed {}, ETA {}",
                display_name.as_str(),
                task.status.label(),
                format_percent(basis_points),
                format_rate(task.download_rate),
                format_eta(task.eta_seconds)
            )
        };
        if let Some(error) = &task_error_label {
            aria_label.push_str(", ");
            aria_label.push_str(error);
        }
        let wide_secondary_label = task_error_label
            .clone()
            .unwrap_or_else(|| format!("GID {}", task.identity.gid));
        let compact_secondary_label = task_error_label.clone().unwrap_or_else(|| {
            if seeding {
                format!(
                    "Uploaded {} · Up {} · {}",
                    format_bytes(task.uploaded_bytes),
                    format_rate(task.upload_rate),
                    observed_seeding
                )
            } else {
                format!(
                    "{size_label} · {} · {}",
                    format_rate(task.download_rate),
                    format_eta(task.eta_seconds)
                )
            }
        });
        let secondary_color = if task_error_label.is_some() {
            colors.danger
        } else {
            colors.text_muted
        };
        let progress_label = if seeding {
            format!("Ratio {share_ratio}")
        } else {
            format_percent(basis_points)
        };
        let rate_label = if seeding {
            format!("Up {}", format_rate(task.upload_rate))
        } else {
            format_rate(task.download_rate)
        };
        let eta_label = if seeding {
            observed_seeding
        } else {
            format_eta(task.eta_seconds)
        };
        let status_badge = task_status_badge(task.status, colors);
        let row = div()
            .id(stable_id)
            .role(Role::ListItem)
            .aria_label(aria_label)
            .aria_selected(selected)
            .aria_position_in_set(index + 1)
            .aria_size_of_set(task_count)
            .when(focused, |row| row.aria_active_descendant())
            .h(px(TASK_ROW_HEIGHT))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .border_b_1()
            .border_color(colors.border)
            .bg(if selected {
                with_alpha(colors.accent, 0.07)
            } else {
                colors.background
            })
            .when(focused, |row| {
                row.border_1().border_color(with_alpha(colors.accent, 0.72))
            })
            .hover(|style| style.bg(colors.surface_hover))
            .cursor_pointer()
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if event.is_right_click() {
                    this.open_task_context_menu(index, event.position(), window, cx);
                    return;
                }
                if !event.standard_click() {
                    return;
                }
                let modifiers = event.modifiers();
                this.select_at_with_modifiers(
                    index,
                    modifiers.shift,
                    modifiers.secondary(),
                    window,
                    cx,
                );
            }))
            .child(
                div()
                    .id(SharedString::from(format!(
                        "task-select:{}:{}",
                        task.identity.profile_id, task.identity.gid
                    )))
                    .role(Role::CheckBox)
                    .aria_label(if selected {
                        format!("Deselect {display_name}")
                    } else {
                        format!("Select {display_name}")
                    })
                    .aria_toggled(if selected {
                        Toggled::True
                    } else {
                        Toggled::False
                    })
                    .size(px(20.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_hover))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.select_at_with_modifiers(index, false, true, window, cx);
                    }))
                    .child(
                        Icon::new(if selected {
                            IconName::SquareCheckBig
                        } else {
                            IconName::Square
                        })
                        .size(IconSize::Small)
                        .color(if selected {
                            colors.accent
                        } else {
                            colors.text_muted
                        }),
                    ),
            )
            .child(
                div()
                    .size(px(32.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.elevated_surface)
                    .child(
                        Icon::new(task_status_icon(task.status))
                            .size(IconSize::Small)
                            .color(status_color),
                    ),
            );

        match layout {
            TaskLayoutMode::Wide => row
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .truncate()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(display_name),
                        )
                        .child(
                            div()
                                .truncate()
                                .font_features(tabular_numbers())
                                .text_xs()
                                .text_color(secondary_color)
                                .child(wide_secondary_label),
                        ),
                )
                .child(
                    div()
                        .w(px(132.0))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .font_features(tabular_numbers())
                        .text_xs()
                        .text_color(colors.text_secondary)
                        .child(progress_label)
                        .child(task_progress_bar(progress, task.status, colors)),
                )
                .child(task_table_value(88.0, rate_label, colors))
                .child(task_table_value(124.0, size_label, colors))
                .child(task_table_value(72.0, eta_label, colors))
                .child(
                    div()
                        .w(px(86.0))
                        .flex_none()
                        .flex()
                        .justify_center()
                        .child(status_badge),
                ),
            TaskLayoutMode::Compact => row
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .truncate()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(display_name),
                        )
                        .child(
                            div()
                                .truncate()
                                .font_features(tabular_numbers())
                                .text_xs()
                                .text_color(secondary_color)
                                .child(compact_secondary_label),
                        ),
                )
                .child(
                    div()
                        .w(px(112.0))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .font_features(tabular_numbers())
                        .text_xs()
                        .text_color(colors.text_secondary)
                        .child(progress_label)
                        .child(task_progress_bar(progress, task.status, colors)),
                )
                .child(
                    div()
                        .w(px(78.0))
                        .flex_none()
                        .flex()
                        .justify_center()
                        .child(status_badge),
                ),
        }
    }

    fn render_add_download_dialog(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let add_pending = self.add_dialog.pending.is_some();
        let preview_pending = self.add_dialog.preview_pending.is_some();
        let pending = add_pending || preview_pending;
        let error = self.add_dialog.error.clone();
        let sources = parse_add_download_sources(self.add_input.read(cx).text());
        let input_mode = self.add_dialog.input_mode;
        let mode = self.add_dialog.mode;
        let file_conflict = self.add_dialog.file_conflict;
        let shell = cx.entity().downgrade();
        let input_shell = shell.clone();
        let conflict_shell = shell.clone();
        let input_mode_control = SegmentedControl::new(
            "add-download-input-mode",
            [
                Segment::new(AddDownloadInputModeView::Links.label()),
                Segment::new(AddDownloadInputModeView::MetadataFiles.label()),
            ],
            usize::from(input_mode == AddDownloadInputModeView::MetadataFiles),
            self.theme,
        )
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let mode = if index == 0 {
                AddDownloadInputModeView::Links
            } else {
                AddDownloadInputModeView::MetadataFiles
            };
            input_shell
                .update(cx, |shell, cx| shell.set_add_input_mode(mode, cx))
                .ok();
        });
        let mode_control = SegmentedControl::new(
            "add-download-mode",
            [
                Segment::new(AddDownloadModeView::SeparateTasks.label()),
                Segment::new(AddDownloadModeView::Mirrors.label()),
            ],
            usize::from(mode == AddDownloadModeView::Mirrors),
            self.theme,
        )
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let mode = if index == 0 {
                AddDownloadModeView::SeparateTasks
            } else {
                AddDownloadModeView::Mirrors
            };
            shell
                .update(cx, |shell, cx| shell.set_add_download_mode(mode, cx))
                .ok();
        });
        let conflict_control = SegmentedControl::new(
            "add-download-file-conflict",
            [
                Segment::new(FileConflictPolicyView::AutoRename.label()),
                Segment::new(FileConflictPolicyView::Reject.label()),
                Segment::new(FileConflictPolicyView::Overwrite.label()),
            ],
            match file_conflict {
                FileConflictPolicyView::AutoRename => 0,
                FileConflictPolicyView::Reject => 1,
                FileConflictPolicyView::Overwrite => 2,
            },
            self.theme,
        )
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let policy = match index {
                0 => FileConflictPolicyView::AutoRename,
                1 => FileConflictPolicyView::Reject,
                _ => FileConflictPolicyView::Overwrite,
            };
            conflict_shell
                .update(cx, |shell, cx| {
                    shell.set_file_conflict_policy(policy, cx);
                })
                .ok();
        });
        let content = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(input_mode_control)
            .child(match input_mode {
                AddDownloadInputModeView::Links => div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.text_secondary)
                            .child("URL or magnet link"),
                    )
                    .child(self.add_input.clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(div().text_xs().text_color(colors.text_muted).child(
                                if sources.is_empty() {
                                    "No sources detected".to_owned()
                                } else {
                                    format!(
                                        "{} source{} detected",
                                        sources.len(),
                                        if sources.len() == 1 { "" } else { "s" }
                                    )
                                },
                            ))
                            .child(mode_control),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(colors.text_secondary)
                                    .child("If file exists"),
                            )
                            .child(conflict_control),
                    )
                    .when(
                        file_conflict == FileConflictPolicyView::Overwrite,
                        |element| {
                            element.child(
                                div()
                                    .id("add-download-overwrite-warning")
                                    .role(Role::Alert)
                                    .text_xs()
                                    .text_color(colors.danger)
                                    .child("Existing destination files may be replaced."),
                            )
                        },
                    )
                    .child(self.render_add_advanced_section(pending, colors, cx))
                    .into_any_element(),
                AddDownloadInputModeView::MetadataFiles => {
                    self.render_metadata_file_picker(pending, preview_pending, cx)
                }
            })
            .when(!self.add_dialog.results.is_empty(), |element| {
                element.child(self.render_add_result_list(colors))
            })
            .when_some(error, |element, error| {
                element.child(
                    div()
                        .id("add-download-error")
                        .role(Role::Alert)
                        .aria_label(error.summary.clone())
                        .text_xs()
                        .text_color(colors.danger)
                        .child(error.summary),
                )
            });

        Dialog::new("add-download-dialog", "Add download", self.theme)
            .key_context("AddDownloadDialog")
            .track_focus(self.add_dialog_focus.clone())
            .width(if input_mode == AddDownloadInputModeView::MetadataFiles {
                720.0
            } else if self.add_dialog.advanced_open {
                640.0
            } else {
                560.0
            })
            .child(content)
            .action(
                Button::new("cancel-add-download", "Cancel")
                    .aria_label("Cancel adding a download")
                    .style(ButtonStyle::Secondary)
                    .disabled(pending)
                    .track_focus(self.add_cancel_focus.clone())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_add_download(window, cx);
                    }))
                    .render(colors),
            )
            .action(
                Button::new("submit-add-download", "Add")
                    .aria_label(if add_pending {
                        "Adding download"
                    } else {
                        "Add download"
                    })
                    .style(ButtonStyle::Primary)
                    .disabled(preview_pending)
                    .loading(add_pending)
                    .track_focus(self.add_submit_focus.clone())
                    .on_click(cx.listener(|this, _, _window, cx| {
                        this.submit_add_download(cx);
                    }))
                    .render(colors),
            )
            .into_any_element()
    }

    fn render_add_advanced_section(
        &mut self,
        pending: bool,
        colors: crate::ThemeColors,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = self.add_dialog.advanced_open;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .id("add-download-advanced-toggle")
                    .role(Role::Button)
                    .aria_label(if open {
                        "Hide advanced download options"
                    } else {
                        "Show advanced download options"
                    })
                    .aria_expanded(open)
                    .focusable()
                    .tab_stop(true)
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.elevated_surface)
                    .px_3()
                    .py_2()
                    .hover(|style| style.bg(colors.surface_hover))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_add_advanced(cx)))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.text_secondary)
                            .child("Advanced options"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(if open { "Hide" } else { "Show" }),
                    ),
            )
            .when(open, |element| {
                element
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(
                                "Applies only to direct URL downloads. Cookies and HTTP passwords stay out of task rows and logs.",
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(
                                settings_labeled_input(
                                    "Referer",
                                    self.add_referer_input.clone(),
                                    colors,
                                )
                                .flex_1()
                                .min_w_0(),
                            )
                            .child(
                                settings_labeled_input(
                                    "User-Agent",
                                    self.add_user_agent_input.clone(),
                                    colors,
                                )
                                .flex_1()
                                .min_w_0(),
                            ),
                    )
                    .child(settings_labeled_input(
                        "Custom headers",
                        self.add_headers_input.clone(),
                        colors,
                    ))
                    .child(settings_labeled_input(
                        "Cookie",
                        self.add_cookie_input.clone(),
                        colors,
                    ))
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(
                                settings_labeled_input(
                                    "HTTP username",
                                    self.add_http_user_input.clone(),
                                    colors,
                                )
                                .flex_1()
                                .min_w_0(),
                            )
                            .child(
                                settings_labeled_input(
                                    "HTTP password",
                                    self.add_http_passwd_input.clone(),
                                    colors,
                                )
                                .flex_1()
                                .min_w_0(),
                            ),
                    )
                    .child(settings_labeled_input(
                        "Checksum",
                        self.add_checksum_input.clone(),
                        colors,
                    ))
                    .when(pending, |element| {
                        // Keep the section visible while submitting, but inputs stay
                        // disabled through the dialog pending state of TextField focus.
                        element
                    })
            })
    }

    fn render_metadata_file_picker(
        &mut self,
        pending: bool,
        preview_pending: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = self.theme.colors;
        let rows =
            self.add_dialog
                .metadata_files
                .iter()
                .enumerate()
                .map(|(index, preview)| {
                    let active = self.add_dialog.active_metadata_file == Some(index);
                    let name = preview.path.file_name().map_or_else(
                        || preview.path.display().to_string(),
                        |name| name.to_string_lossy().into(),
                    );
                    let full_path = preview.path.display().to_string();
                    let kind = preview.kind;
                    let selected = preview.selected_file_indices.len();
                    let total = preview.files.len();
                    div()
                        .id(SharedString::from(format!("metadata-file-{index}")))
                        .role(Role::ListItem)
                        .aria_label(format!(
                            "{} {}, {selected} of {total} files selected",
                            kind.label(),
                            name
                        ))
                        .h(px(48.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .border_b_1()
                        .border_color(if active { colors.accent } else { colors.border })
                        .bg(if active {
                            colors.surface_active
                        } else {
                            colors.surface
                        })
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_metadata_file(index, cx);
                        }))
                        .child(Icon::new(IconName::Download).size(IconSize::Small).color(
                            if active {
                                colors.accent
                            } else {
                                colors.text_muted
                            },
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(div().truncate().text_sm().child(name))
                                .child(
                                    div()
                                        .truncate()
                                        .text_xs()
                                        .text_color(colors.text_muted)
                                        .child(format!(
                                            "{} · {selected}/{total} files · {full_path}",
                                            kind.label()
                                        )),
                                ),
                        )
                        .child(
                            IconButton::new(
                                SharedString::from(format!("remove-metadata-file-{index}")),
                                IconName::X,
                            )
                            .aria_label(format!("Remove {} file", kind.label()))
                            .disabled(pending)
                            .tooltip(Tooltip::new("Remove file"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_metadata_file(index, cx);
                            }))
                            .render(colors),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>();
        let count = self.add_dialog.metadata_files.len();
        let active_index = self.add_dialog.active_metadata_file;
        let active_summary = active_index
            .and_then(|index| self.add_dialog.metadata_files.get(index))
            .map(metadata_selection_summary);
        let active_file_count = active_index
            .and_then(|index| self.add_dialog.metadata_files.get(index))
            .map_or(0, |preview| preview.files.len());
        let active_selection_state = active_index
            .and_then(|index| self.add_dialog.metadata_files.get(index))
            .map_or(Toggled::False, |preview| {
                if preview.selected_file_indices.is_empty() {
                    Toggled::False
                } else if preview.selected_file_indices.len() == preview.files.len() {
                    Toggled::True
                } else {
                    Toggled::Mixed
                }
            });
        let active_selection_icon = match active_selection_state {
            Toggled::False => IconName::Square,
            Toggled::True => IconName::SquareCheckBig,
            Toggled::Mixed => IconName::SquareMinus,
        };
        let file_list = active_index.map(|preview_index| {
            let list_id = SharedString::from(format!("metadata-preview-files-{preview_index}"));
            div()
                .h(px(220.0))
                .min_h_0()
                .child(
                    uniform_list(
                        list_id.clone(),
                        active_file_count,
                        cx.processor(move |this, range: Range<usize>, _window, cx| {
                            let colors = this.theme.colors;
                            let Some(preview) = this.add_dialog.metadata_files.get(preview_index)
                            else {
                                return Vec::new();
                            };
                            range
                                .filter_map(|position| {
                                    let file = preview.files.get(position)?.clone();
                                    let selected = preview
                                        .selected_file_indices
                                        .binary_search(&file.index)
                                        .is_ok();
                                    let file_index = file.index;
                                    Some(
                                        div()
                                            .id(SharedString::from(format!(
                                                "metadata-preview-file:{preview_index}:{file_index}"
                                            )))
                                            .role(Role::CheckBox)
                                            .aria_position_in_set(position + 1)
                                            .aria_size_of_set(active_file_count)
                                            .aria_toggled(if selected {
                                                Toggled::True
                                            } else {
                                                Toggled::False
                                            })
                                            .aria_label(format!(
                                                "File {file_index}, {}, {}",
                                                file.path,
                                                file.length.map_or_else(
                                                    || "unknown size".into(),
                                                    format_bytes
                                                )
                                            ))
                                            .h(px(40.0))
                                            .w_full()
                                            .flex_none()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .px_3()
                                            .border_b_1()
                                            .border_color(colors.border)
                                            .cursor_pointer()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.toggle_metadata_file_entry(
                                                    preview_index,
                                                    file_index,
                                                    cx,
                                                );
                                            }))
                                            .child(
                                                Icon::new(if selected {
                                                    IconName::SquareCheckBig
                                                } else {
                                                    IconName::Square
                                                })
                                                .size(IconSize::Small)
                                                .color(if selected {
                                                    colors.accent
                                                } else {
                                                    colors.text_muted
                                                }),
                                            )
                                            .child(
                                                div()
                                                    .w(px(34.0))
                                                    .flex_none()
                                                    .font_features(tabular_numbers())
                                                    .text_xs()
                                                    .text_color(colors.text_muted)
                                                    .child(file_index.to_string()),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_xs()
                                                    .text_color(colors.text_secondary)
                                                    .child(file.path),
                                            )
                                            .child(
                                                div()
                                                    .w(px(84.0))
                                                    .flex_none()
                                                    .text_right()
                                                    .font_features(tabular_numbers())
                                                    .text_xs()
                                                    .text_color(colors.text_muted)
                                                    .child(file.length.map_or_else(
                                                        || "Unknown".into(),
                                                        format_bytes,
                                                    )),
                                            )
                                            .into_any_element(),
                                    )
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&self.metadata_file_scroll)
                    .size_full(),
                )
                .border_1()
                .border_color(colors.border)
                .rounded_md()
                .into_any_element()
        });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .id("metadata-file-drop-target")
                    .role(Role::Group)
                    .aria_label("Torrent and Metalink file drop target")
                    .min_h(px(82.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .p_3()
                    .border_1()
                    .border_color(colors.border)
                    .rounded_md()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Icon::new(IconName::Inbox)
                                    .size(IconSize::Medium)
                                    .color(colors.text_muted),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child("Torrent / Metalink files")
                                    .child(div().text_xs().text_color(colors.text_muted).child(
                                        if preview_pending {
                                            "Reading metadata...".to_owned()
                                        } else {
                                            format!(
                                                "{count} source{} ready",
                                                if count == 1 { "" } else { "s" }
                                            )
                                        },
                                    )),
                            ),
                    )
                    .child(
                        Button::new("choose-metadata-files", "Choose files")
                            .icon(IconName::FolderDown)
                            .aria_label("Choose Torrent or Metalink files")
                            .style(ButtonStyle::Secondary)
                            .disabled(pending)
                            .loading(preview_pending)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_metadata_files(window, cx);
                            }))
                            .render(colors),
                    ),
            )
            .when(!rows.is_empty(), |element| {
                element.child(
                    div()
                        .id("metadata-file-list")
                        .role(Role::List)
                        .aria_label("Selected Torrent and Metalink files")
                        .max_h(px(112.0))
                        .border_1()
                        .border_color(colors.border)
                        .rounded_md()
                        .children(rows),
                )
            })
            .when_some(active_summary, |element, summary| {
                element
                    .child(
                        div()
                            .h(px(36.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .id("toggle-all-metadata-files")
                                    .role(Role::CheckBox)
                                    .aria_toggled(active_selection_state)
                                    .aria_label(match active_selection_state {
                                        Toggled::True => "Clear file selection",
                                        Toggled::False | Toggled::Mixed => "Select all files",
                                    })
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(index) = active_index {
                                            this.toggle_all_metadata_file_entries(index, cx);
                                        }
                                    }))
                                    .child(
                                        Icon::new(active_selection_icon)
                                            .size(IconSize::Small)
                                            .color(colors.accent),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(colors.text_secondary)
                                            .child("Files"),
                                    ),
                            )
                            .child(
                                div()
                                    .font_features(tabular_numbers())
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(summary),
                            ),
                    )
                    .when_some(file_list, |element, list| element.child(list))
            })
            .into_any_element()
    }

    fn render_add_result_list(&self, colors: crate::ThemeColors) -> Stateful<Div> {
        let rows = self
            .add_dialog
            .results
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let source_label = item
                    .sources
                    .iter()
                    .map(AddDownloadSourceView::label)
                    .collect::<Vec<_>>()
                    .join("  |  ");
                let (icon, label, color) = match &item.outcome {
                    CommandOutcomeView::Success { tasks } => (
                        IconName::CircleCheck,
                        match tasks.as_slice() {
                            [] => "Accepted".to_owned(),
                            [task] => format!("Accepted · GID {}", task.gid),
                            tasks => format!("Accepted · {} tasks", tasks.len()),
                        },
                        colors.success,
                    ),
                    CommandOutcomeView::Failure(error) if error.outcome_unknown() => (
                        IconName::TriangleAlert,
                        format!("Outcome unknown · {}", error.summary),
                        colors.warning,
                    ),
                    CommandOutcomeView::Failure(error) => {
                        (IconName::CircleAlert, error.summary.clone(), colors.danger)
                    }
                };
                div()
                    .id(SharedString::from(format!("add-result-{index}")))
                    .role(Role::ListItem)
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(Icon::new(icon).size(IconSize::XSmall).color(color))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().truncate().text_xs().child(source_label))
                            .child(div().text_xs().text_color(color).child(label)),
                    )
            })
            .collect::<Vec<_>>();
        div()
            .id("add-download-results")
            .role(Role::List)
            .aria_label("Add download results")
            .max_h(px(220.0))
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(colors.border)
            .rounded_md()
            .children(rows)
    }

    fn render_settings_page(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let colors = self.theme.colors;
        let pending = self.pending_settings_save.is_some();
        let directory_saving = self
            .pending_settings_save
            .as_ref()
            .is_some_and(|pending| pending.source == SettingsSaveSource::Directory);
        let proxy_saving = self
            .pending_settings_save
            .as_ref()
            .is_some_and(|pending| pending.source == SettingsSaveSource::Proxy);
        let error = self.settings_page.error.clone();
        let draft_scheme = self.settings_page.draft_color_scheme;
        let directory_dirty = self.settings_directory_input.read(cx).text().trim()
            != self.settings.download_directory;
        let password_changed = !self
            .settings_proxy_password_input
            .read(cx)
            .text()
            .is_empty();
        let password_cleared = self.settings_page.clear_proxy_password;
        let proxy_has_password = if password_changed {
            true
        } else if password_cleared {
            false
        } else {
            self.settings.download_proxy.has_password
        };
        let proxy_draft = DownloadProxySettingsView {
            mode: self.settings_page.draft_proxy_mode,
            all_proxy: self.settings_all_proxy_input.read(cx).text().trim().into(),
            http_proxy: self.settings_http_proxy_input.read(cx).text().trim().into(),
            https_proxy: self
                .settings_https_proxy_input
                .read(cx)
                .text()
                .trim()
                .into(),
            ftp_proxy: self.settings_ftp_proxy_input.read(cx).text().trim().into(),
            no_proxy: self
                .settings_no_proxy_input
                .read(cx)
                .text()
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
            username: self
                .settings_proxy_username_input
                .read(cx)
                .text()
                .trim()
                .into(),
            has_password: proxy_has_password,
        };
        let proxy_dirty =
            proxy_draft != self.settings.download_proxy || password_changed || password_cleared;
        let speed_limit_saving = self
            .pending_settings_save
            .as_ref()
            .is_some_and(|pending| pending.source == SettingsSaveSource::SpeedLimit);
        let speed_limit_draft = SpeedLimitSettingsView {
            download_limit: self
                .settings_download_limit_input
                .read(cx)
                .text()
                .trim()
                .into(),
            upload_limit: self
                .settings_upload_limit_input
                .read(cx)
                .text()
                .trim()
                .into(),
        };
        let speed_limit_dirty = speed_limit_draft != self.settings.speed_limits;
        let speed_limit_valid = speed_limit_draft.is_valid();
        let transfer_policy_saving = self
            .pending_settings_save
            .as_ref()
            .is_some_and(|pending| pending.source == SettingsSaveSource::TransferPolicy);
        let transfer_policy_draft = TransferPolicySettingsView {
            max_concurrent_downloads: self
                .settings_max_concurrent_input
                .read(cx)
                .text()
                .trim()
                .into(),
            max_connection_per_server: self
                .settings_max_connection_input
                .read(cx)
                .text()
                .trim()
                .into(),
            split: self.settings_split_input.read(cx).text().trim().into(),
            min_split_size: self
                .settings_min_split_size_input
                .read(cx)
                .text()
                .trim()
                .into(),
            file_allocation: self.settings_page.draft_file_allocation,
            check_integrity: self.settings_page.draft_check_integrity,
        };
        let transfer_policy_dirty = transfer_policy_draft != self.settings.transfer_policy;
        let transfer_policy_valid = transfer_policy_draft.is_valid();
        let notifications_saving = self
            .pending_settings_save
            .as_ref()
            .is_some_and(|pending| pending.source == SettingsSaveSource::Notifications);
        let notifications_draft = NotificationSettingsView {
            volume: self.settings_page.draft_notification_volume,
            notify_on_completion: self.settings_page.draft_notify_on_completion,
            notify_on_error: self.settings_page.draft_notify_on_error,
            notify_on_engine_events: self.settings_page.draft_notify_on_engine_events,
        };
        let notifications_dirty = notifications_draft != self.settings.notifications;
        let volume_selected = NotificationVolumeView::all()
            .iter()
            .position(|volume| *volume == self.settings_page.draft_notification_volume)
            .unwrap_or(0);
        let allocation_selected = FileAllocationView::all()
            .iter()
            .position(|method| *method == self.settings_page.draft_file_allocation)
            .unwrap_or(1);
        let manual_proxy = proxy_draft.mode == ProxyModeView::Manual;
        let selected_scheme = usize::from(draft_scheme == ColorSchemeView::Dark);
        let shell = cx.entity().downgrade();
        let scheme_control = SegmentedControl::new(
            "settings-theme",
            [
                Segment::new("Light").icon(IconName::Sun),
                Segment::new("Dark").icon(IconName::Moon),
            ],
            selected_scheme,
            self.theme,
        )
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let scheme = if index == 0 {
                ColorSchemeView::Light
            } else {
                ColorSchemeView::Dark
            };
            shell
                .update(cx, |shell, cx| shell.select_color_scheme(scheme, cx))
                .ok();
        });
        let proxy_shell = cx.entity().downgrade();
        let proxy_mode_control = SegmentedControl::new(
            "settings-proxy-mode",
            [Segment::new("Disabled"), Segment::new("Manual")],
            usize::from(manual_proxy),
            self.theme,
        )
        .disabled(pending)
        .on_select(move |index, _window, cx| {
            let mode = if index == 0 {
                ProxyModeView::Disabled
            } else {
                ProxyModeView::Manual
            };
            proxy_shell
                .update(cx, |shell, cx| shell.select_proxy_mode(mode, cx))
                .ok();
        });
        let password_button_label = if password_cleared {
            "Keep saved proxy password"
        } else {
            "Clear saved proxy password"
        };
        let password_button_icon = if password_cleared {
            IconName::RotateCcw
        } else {
            IconName::Trash2
        };

        div()
            .id("settings-page")
            .key_context("SettingsPage")
            .role(Role::Main)
            .aria_label("Application settings")
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background)
            .child(
                div()
                    .h(px(44.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_4()
                    .bg(colors.toolbar_surface)
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Settings"),
                    ),
            )
            .child(
                div()
                    .id("settings-scroll-shell")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .id("settings-scroll")
                            .flex_1()
                            .min_h_0()
                            .px_6()
                            .py_5()
                            .overflow_y_scroll()
                            .track_scroll(&self.settings_scroll)
                            .child(
                    div()
                        .max_w(px(720.0))
                        .flex()
                        .flex_col()
                        .gap_8()
                        .child({
                            let profiles = self.profiles.clone();
                            let active_id = profiles.active_profile_id.clone();
                            let profiles_count = profiles.profiles.len();
                            settings_section(
                                "Profiles",
                                "A profile is a connection context: local profiles own a session directory and spawn aria2; remote profiles only connect over RPC. The Engine section below chooses which aria2 binary local profiles use by default (leave Executable empty). Activate switches the active profile immediately; restart AriaDeck to reconnect.",
                                colors,
                            )
                            .child(
                                div()
                                    .mt_3()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .children(profiles.profiles.into_iter().map(|profile| {
                                        let is_active = profile.profile_id == active_id;
                                        let profile_id = profile.profile_id.clone();
                                        let switch_id = profile_id.clone();
                                        let edit_id = profile_id.clone();
                                        let remove_id = profile_id.clone();
                                        let can_remove = profiles_count > 1;
                                        let summary = match profile.kind {
                                            ProfileKindView::LocalManaged => {
                                                if profile.executable.is_empty() {
                                                    "Local · uses managed core".to_owned()
                                                } else {
                                                    format!("Local · pinned {}", profile.executable)
                                                }
                                            }
                                            ProfileKindView::RemoteRpc => {
                                                let endpoint = if profile.endpoint.is_empty() {
                                                    "no endpoint".to_owned()
                                                } else {
                                                    profile.endpoint.clone()
                                                };
                                                if profile.has_secret {
                                                    format!("Remote · {endpoint} · secret saved")
                                                } else {
                                                    format!("Remote · {endpoint}")
                                                }
                                            }
                                        };
                                        div()
                                            .id(SharedString::from(format!(
                                                "profile-row-{}",
                                                profile.profile_id
                                            )))
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .px_3()
                                            .py_2()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(if is_active {
                                                colors.accent
                                            } else {
                                                colors.border
                                            })
                                            .bg(if is_active {
                                                with_alpha(colors.accent, 0.08)
                                            } else {
                                                colors.surface
                                            })
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_0p5()
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(colors.text_primary)
                                                            .child(profile.name.clone()),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(colors.text_muted)
                                                            .child(summary),
                                                    ),
                                            )
                                            .child(
                                                Button::new(
                                                    SharedString::from(format!(
                                                        "activate-profile-{}",
                                                        profile_id
                                                    )),
                                                    if is_active { "Active" } else { "Activate" },
                                                )
                                                .aria_label(if is_active {
                                                    format!("{} is active", profile.name)
                                                } else {
                                                    format!("Activate {}", profile.name)
                                                })
                                                .style(if is_active {
                                                    ButtonStyle::Secondary
                                                } else {
                                                    ButtonStyle::Primary
                                                })
                                                .disabled(is_active)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.request_switch_profile(
                                                        switch_id.clone(),
                                                        cx,
                                                    );
                                                }))
                                                .render(colors),
                                            )
                                            .child(
                                                Button::new(
                                                    SharedString::from(format!(
                                                        "edit-profile-{}",
                                                        edit_id
                                                    )),
                                                    "Edit",
                                                )
                                                .aria_label(format!("Edit {}", profile.name))
                                                .style(ButtonStyle::Secondary)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.open_profile_editor(edit_id.clone(), cx);
                                                }))
                                                .render(colors),
                                            )
                                            .child(
                                                Button::new(
                                                    SharedString::from(format!(
                                                        "remove-profile-{}",
                                                        remove_id
                                                    )),
                                                    "Delete",
                                                )
                                                .aria_label(format!("Delete {}", profile.name))
                                                .style(ButtonStyle::Secondary)
                                                .disabled(!can_remove)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.request_remove_profile(
                                                        remove_id.clone(),
                                                        cx,
                                                    );
                                                }))
                                                .render(colors),
                                            )
                                    })),
                            )
                            .when_some(
                                self.settings_page.editing_profile_id.clone(),
                                |section, editing_id| {
                                    let kind = self.settings_page.draft_profile_kind;
                                    let is_local = kind == ProfileKindView::LocalManaged;
                                    let kind_shell = cx.entity().downgrade();
                                    let kind_control = SegmentedControl::new(
                                        "settings-profile-kind",
                                        [Segment::new("Local"), Segment::new("Remote")],
                                        usize::from(!is_local),
                                        self.theme,
                                    )
                                    .on_select(move |index, _window, cx| {
                                        let kind = if index == 0 {
                                            ProfileKindView::LocalManaged
                                        } else {
                                            ProfileKindView::RemoteRpc
                                        };
                                        kind_shell
                                            .update(cx, |shell, cx| {
                                                shell.select_profile_editor_kind(kind, cx);
                                            })
                                            .ok();
                                    });
                                    section.child(
                                        div()
                                            .mt_3()
                                            .flex()
                                            .flex_col()
                                            .gap_3()
                                            .px_3()
                                            .py_3()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(colors.border)
                                            .bg(colors.surface)
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(colors.text_primary)
                                                    .child(format!("Edit profile ({editing_id})")),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(colors.text_muted)
                                                    .child(
                                                        "Apply writes the draft catalog only. Save profiles persists to disk.",
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(colors.text_muted)
                                                            .child("Name"),
                                                    )
                                                    .child(self.settings_profile_name_input.clone()),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(colors.text_muted)
                                                            .child("Kind"),
                                                    )
                                                    .child(kind_control),
                                            )
                                            .child(if is_local {
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(colors.text_muted)
                                                            .child(
                                                                "Executable (optional — empty uses managed core)",
                                                            ),
                                                    )
                                                    .child(
                                                        self.settings_profile_executable_input
                                                            .clone(),
                                                    )
                                                    .into_any_element()
                                            } else {
                                                let has_secret = self
                                                    .profiles
                                                    .profiles
                                                    .iter()
                                                    .find(|profile| {
                                                        profile.profile_id == editing_id
                                                    })
                                                    .is_some_and(|profile| profile.has_secret);
                                                let secret_cleared =
                                                    self.settings_page.clear_profile_rpc_secret;
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .flex_col()
                                                            .gap_1()
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(colors.text_muted)
                                                                    .child("Endpoint (ws/wss)"),
                                                            )
                                                            .child(
                                                                self.settings_profile_endpoint_input
                                                                    .clone(),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .flex_col()
                                                            .gap_1()
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(colors.text_muted)
                                                                    .child(if secret_cleared {
                                                                        "RPC secret (will clear on Apply)"
                                                                            .to_owned()
                                                                    } else if has_secret {
                                                                        "RPC secret (saved — enter a new value to replace)"
                                                                            .to_owned()
                                                                    } else {
                                                                        "RPC secret (optional)"
                                                                            .to_owned()
                                                                    }),
                                                            )
                                                            .child(
                                                                self.settings_profile_secret_input
                                                                    .clone(),
                                                            ),
                                                    )
                                                    .child(
                                                        Button::new(
                                                            "toggle-clear-profile-secret",
                                                            if secret_cleared {
                                                                "Keep saved secret"
                                                            } else if has_secret {
                                                                "Clear saved secret"
                                                            } else {
                                                                "No saved secret"
                                                            },
                                                        )
                                                        .aria_label(
                                                            "Toggle clearing the saved RPC secret",
                                                        )
                                                        .style(ButtonStyle::Secondary)
                                                        .disabled(!has_secret && !secret_cleared)
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.toggle_clear_profile_rpc_secret(cx);
                                                        }))
                                                        .render(colors),
                                                    )
                                                    .into_any_element()
                                            })
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(colors.text_muted)
                                                            .child("Download directory"),
                                                    )
                                                    .child(
                                                        self.settings_profile_download_input.clone(),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_wrap()
                                                    .gap_2()
                                                    .child(
                                                        Button::new(
                                                            "apply-profile-editor",
                                                            "Apply changes",
                                                        )
                                                        .aria_label("Apply profile editor changes")
                                                        .style(ButtonStyle::Primary)
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.apply_profile_editor(cx);
                                                        }))
                                                        .render(colors),
                                                    )
                                                    .child(
                                                        Button::new(
                                                            "cancel-profile-editor",
                                                            "Cancel",
                                                        )
                                                        .aria_label("Cancel profile editor")
                                                        .style(ButtonStyle::Secondary)
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.close_profile_editor(cx);
                                                        }))
                                                        .render(colors),
                                                    ),
                                            ),
                                    )
                                },
                            )
                            .child(
                                div()
                                    .mt_3()
                                    .flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        Button::new("add-local-profile", "Add local profile")
                                            .aria_label("Add a local managed aria2 profile")
                                            .style(ButtonStyle::Secondary)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.add_draft_local_profile(cx);
                                            }))
                                            .render(colors),
                                    )
                                    .child(
                                        Button::new("add-remote-profile", "Add remote profile")
                                            .aria_label("Add a remote RPC aria2 profile")
                                            .style(ButtonStyle::Secondary)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.add_draft_remote_profile(cx);
                                            }))
                                            .render(colors),
                                    )
                                    .child(
                                        Button::new("save-profile-catalog", "Save profiles")
                                            .aria_label("Save the profile catalog")
                                            .style(ButtonStyle::Primary)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                let catalog = this.profiles.clone();
                                                this.request_save_profile_catalog(catalog, cx);
                                            }))
                                            .render(colors),
                                    ),
                            )
                            .when_some(
                                self.settings_page.pending_profile_delete.as_ref().map(
                                    |pending| (pending.profile_id.clone(), pending.name.clone()),
                                ),
                                |section, (delete_id, delete_name)| {
                                    let is_active = delete_id
                                        == self.profiles.active_profile_id;
                                    section.child(
                                        div()
                                            .mt_3()
                                            .flex()
                                            .flex_col()
                                            .gap_2()
                                            .px_3()
                                            .py_3()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(colors.danger)
                                            .bg(with_alpha(colors.danger, 0.08))
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(colors.text_primary)
                                                    .child(format!(
                                                        "Delete profile “{delete_name}”?"
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(colors.text_muted)
                                                    .child(if is_active {
                                                        "This is the active profile. Another profile will become active. Local session data is not deleted from disk."
                                                            .to_owned()
                                                    } else {
                                                        "Local session data is not deleted from disk. This saves the catalog immediately."
                                                            .to_owned()
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_wrap()
                                                    .gap_2()
                                                    .child(
                                                        Button::new(
                                                            "confirm-delete-profile",
                                                            "Delete profile",
                                                        )
                                                        .aria_label(format!(
                                                            "Confirm delete {delete_name}"
                                                        ))
                                                        .style(ButtonStyle::Primary)
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.confirm_remove_profile(cx);
                                                        }))
                                                        .render(colors),
                                                    )
                                                    .child(
                                                        Button::new(
                                                            "cancel-delete-profile",
                                                            "Cancel",
                                                        )
                                                        .aria_label("Cancel profile delete")
                                                        .style(ButtonStyle::Secondary)
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.cancel_remove_profile(cx);
                                                        }))
                                                        .render(colors),
                                                    ),
                                            ),
                                    )
                                },
                            )
                        })
                        .child({
                            let cores = self.cores.clone();
                            let can_rollback = cores
                                .last_working_id
                                .as_ref()
                                .is_some_and(|id| cores.active_id.as_ref() != Some(id));
                            settings_section(
                                "Engine",
                                "Global aria2 binary registry (not a connection profile). Local profiles with an empty Executable field use the Active core here. Import copies a binary into app data; Link registers an external path. Activate/Rollback update the registry immediately; restart AriaDeck so local profiles pick up the new binary.",
                                colors,
                            )
                            .child(
                                div()
                                    .mt_3()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(colors.text_muted)
                                            .child(if cores.installations.is_empty() {
                                                "No managed cores yet. Import or link an aria2c executable below."
                                                    .to_owned()
                                            } else {
                                                format!(
                                                    "{} installed · active {}",
                                                    cores.installations.len(),
                                                    cores
                                                        .active()
                                                        .map(|core| core.version.as_str())
                                                        .unwrap_or("none")
                                                )
                                            }),
                                    )
                                    .children(cores.installations.into_iter().map(|core| {
                                        let core_id = core.id.clone();
                                        let activate_id = core_id.clone();
                                        let verify_id = core_id.clone();
                                        let remove_id = core_id.clone();
                                        let is_active = core.is_active;
                                        div()
                                            .id(SharedString::from(format!("core-row-{}", core.id)))
                                            .flex()
                                            .flex_col()
                                            .gap_2()
                                            .px_3()
                                            .py_2()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(if is_active {
                                                colors.accent
                                            } else {
                                                colors.border
                                            })
                                            .bg(if is_active {
                                                with_alpha(colors.accent, 0.08)
                                            } else {
                                                colors.surface
                                            })
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .flex()
                                                            .flex_col()
                                                            .gap_0p5()
                                                            .child(
                                                                div()
                                                                    .text_sm()
                                                                    .font_weight(FontWeight::MEDIUM)
                                                                    .text_color(colors.text_primary)
                                                                    .child(format!(
                                                                        "aria2 {}",
                                                                        core.version
                                                                    )),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(colors.text_muted)
                                                                    .child(format!(
                                                                        "{} · {} · {}{}",
                                                                        core.source.label(),
                                                                        core.target,
                                                                        core.status.label(),
                                                                        if core.is_last_working {
                                                                            " · last working"
                                                                        } else {
                                                                            ""
                                                                        }
                                                                    )),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(colors.text_muted)
                                                                    .child(if core.executable.is_empty() {
                                                                        "executable missing".into()
                                                                    } else {
                                                                        core.executable.clone()
                                                                    }),
                                                            ),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_wrap()
                                                    .gap_2()
                                                    .child(
                                                        Button::new(
                                                            SharedString::from(format!(
                                                                "activate-core-{}",
                                                                activate_id
                                                            )),
                                                            if is_active { "Active" } else { "Activate" },
                                                        )
                                                        .aria_label(if is_active {
                                                            format!(
                                                                "aria2 {} is active",
                                                                core.version
                                                            )
                                                        } else {
                                                            format!("Activate aria2 {}", core.version)
                                                        })
                                                        .style(if is_active {
                                                            ButtonStyle::Secondary
                                                        } else {
                                                            ButtonStyle::Primary
                                                        })
                                                        .disabled(is_active)
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            this.request_core_command(
                                                                CoreCommandView::Activate {
                                                                    core_id: activate_id.clone(),
                                                                },
                                                                cx,
                                                            );
                                                        }))
                                                        .render(colors),
                                                    )
                                                    .child(
                                                        Button::new(
                                                            SharedString::from(format!(
                                                                "verify-core-{}",
                                                                verify_id
                                                            )),
                                                            "Verify",
                                                        )
                                                        .aria_label(format!(
                                                            "Verify aria2 {}",
                                                            core.version
                                                        ))
                                                        .style(ButtonStyle::Secondary)
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            this.request_core_command(
                                                                CoreCommandView::Verify {
                                                                    core_id: verify_id.clone(),
                                                                },
                                                                cx,
                                                            );
                                                        }))
                                                        .render(colors),
                                                    )
                                                    .child(
                                                        Button::new(
                                                            SharedString::from(format!(
                                                                "remove-core-{}",
                                                                remove_id
                                                            )),
                                                            "Remove",
                                                        )
                                                        .aria_label(format!(
                                                            "Remove aria2 {}",
                                                            core.version
                                                        ))
                                                        .style(ButtonStyle::Secondary)
                                                        .disabled(is_active)
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            this.request_core_command(
                                                                CoreCommandView::Remove {
                                                                    core_id: remove_id.clone(),
                                                                },
                                                                cx,
                                                            );
                                                        }))
                                                        .render(colors),
                                                    )
                                            )
                                    })),
                            )
                            .child(
                                div()
                                    .mt_3()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(colors.text_primary)
                                            .child("Import or link local aria2c"),
                                    )
                                    .child(self.settings_core_path_input.clone())
                                    .child(
                                        div()
                                            .flex()
                                            .flex_wrap()
                                            .gap_2()
                                            .child(
                                                Button::new("import-core", "Import copy")
                                                    .aria_label(
                                                        "Import a copy of the aria2c path into managed cores",
                                                    )
                                                    .style(ButtonStyle::Primary)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.request_import_core_from_input(cx);
                                                    }))
                                                    .render(colors),
                                            )
                                            .child(
                                                Button::new("link-core", "Link path")
                                                    .aria_label(
                                                        "Register the aria2c path without copying",
                                                    )
                                                    .style(ButtonStyle::Secondary)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.request_link_core_from_input(cx);
                                                    }))
                                                    .render(colors),
                                            )
                                            .child(
                                                Button::new("rollback-core", "Rollback")
                                                    .aria_label(
                                                        "Activate the last working managed aria2 core",
                                                    )
                                                    .style(ButtonStyle::Secondary)
                                                    .disabled(!can_rollback)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.request_core_command(
                                                            CoreCommandView::Rollback,
                                                            cx,
                                                        );
                                                    }))
                                                    .render(colors),
                                            ),
                                    ),
                            )
                        })
                        .child(
                            settings_section(
                                "Appearance",
                                "Choose the interface color scheme.",
                                colors,
                            )
                            .child(div().mt_3().flex().items_start().child(scheme_control)),
                        )
                        .child(
                            settings_section(
                                "Downloads",
                                "New tasks use this directory unless aria2 overrides it.",
                                colors,
                            )
                            .child(
                                div()
                                    .mt_3()
                                    .max_w(px(620.0))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .child(self.settings_directory_input.clone()),
                                    )
                                    .child(
                                        Button::new(
                                            "save-settings",
                                            if directory_saving {
                                                "Saving..."
                                            } else {
                                                "Save"
                                            },
                                        )
                                        .aria_label(if directory_saving {
                                            "Saving download directory"
                                        } else {
                                            "Save download directory"
                                        })
                                        .style(ButtonStyle::Primary)
                                        .disabled(pending || !directory_dirty)
                                        .loading(directory_saving)
                                        .track_focus(self.settings_save_focus.clone())
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.submit_settings(cx)),
                                        )
                                        .render(colors),
                                    ),
                            ),
                        )
                        .child(
                            settings_section(
                                "Network proxy",
                                "Configure the proxy used by aria2 download traffic.",
                                colors,
                            )
                            .child(div().mt_3().flex().items_start().child(proxy_mode_control))
                            .when(manual_proxy, |section| {
                                section.child(
                                    div()
                                        .mt_4()
                                        .max_w(px(620.0))
                                        .flex()
                                        .flex_col()
                                        .gap_3()
                                        .child(settings_labeled_input(
                                            "All protocols",
                                            self.settings_all_proxy_input.clone(),
                                            colors,
                                        ))
                                        .child(
                                            div()
                                                .flex()
                                                .gap_3()
                                                .child(
                                                    settings_labeled_input(
                                                        "HTTP",
                                                        self.settings_http_proxy_input.clone(),
                                                        colors,
                                                    )
                                                    .flex_1()
                                                    .min_w_0(),
                                                )
                                                .child(
                                                    settings_labeled_input(
                                                        "HTTPS",
                                                        self.settings_https_proxy_input.clone(),
                                                        colors,
                                                    )
                                                    .flex_1()
                                                    .min_w_0(),
                                                ),
                                        )
                                        .child(settings_labeled_input(
                                            "FTP",
                                            self.settings_ftp_proxy_input.clone(),
                                            colors,
                                        ))
                                        .child(settings_labeled_input(
                                            "Bypass hosts",
                                            self.settings_no_proxy_input.clone(),
                                            colors,
                                        ))
                                        .child(
                                            div()
                                                .flex()
                                                .gap_3()
                                                .items_end()
                                                .child(
                                                    settings_labeled_input(
                                                        "Username",
                                                        self.settings_proxy_username_input.clone(),
                                                        colors,
                                                    )
                                                    .flex_1()
                                                    .min_w_0(),
                                                )
                                                .child(
                                                    settings_labeled_input(
                                                        "Password",
                                                        self.settings_proxy_password_input.clone(),
                                                        colors,
                                                    )
                                                    .flex_1()
                                                    .min_w_0(),
                                                )
                                                .when(
                                                    self.settings.download_proxy.has_password,
                                                    |row| {
                                                        row.child(
                                                            IconButton::new(
                                                                "clear-proxy-password",
                                                                password_button_icon,
                                                            )
                                                            .aria_label(password_button_label)
                                                            .disabled(pending)
                                                            .tooltip(Tooltip::new(
                                                                password_button_label,
                                                            ))
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    this.clear_saved_proxy_password(
                                                                        cx,
                                                                    );
                                                                },
                                                            ))
                                                            .render(colors),
                                                        )
                                                    },
                                                ),
                                        )
                                        .when(proxy_has_password, |form| {
                                            form.child(
                                                div()
                                                    .text_xs()
                                                    .text_color(colors.text_muted)
                                                    .child("A proxy password is saved in the system credential store."),
                                            )
                                        }),
                                )
                            })
                            .when(
                                !manual_proxy && self.settings.download_proxy.has_password,
                                |section| {
                                    section.child(
                                        div()
                                            .mt_4()
                                            .max_w(px(620.0))
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .text_xs()
                                            .text_color(colors.text_muted)
                                            .child(if password_cleared {
                                                "The saved proxy password will be removed."
                                            } else {
                                                "A proxy password is saved in the system credential store."
                                            })
                                            .child(
                                                IconButton::new(
                                                    "clear-disabled-proxy-password",
                                                    password_button_icon,
                                                )
                                                .aria_label(password_button_label)
                                                .disabled(pending)
                                                .tooltip(Tooltip::new(password_button_label))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.clear_saved_proxy_password(cx);
                                                }))
                                                .render(colors),
                                            ),
                                    )
                                },
                            )
                            .child(
                                div()
                                    .mt_4()
                                    .flex()
                                    .items_center()
                                    .child(
                                        Button::new(
                                            "save-proxy-settings",
                                            if proxy_saving { "Saving..." } else { "Save proxy" },
                                        )
                                        .aria_label(if proxy_saving {
                                            "Saving download proxy settings"
                                        } else {
                                            "Save download proxy settings"
                                        })
                                        .style(ButtonStyle::Primary)
                                        .disabled(pending || !proxy_dirty)
                                        .loading(proxy_saving)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.submit_proxy_settings(cx);
                                        }))
                                        .render(colors),
                                    ),
                            ),
                        )
                        .child(
                            settings_section(
                                "Speed limits",
                                "Throttle aria2's total transfer rate. These limits affect all current and future downloads on this engine. Leave a field blank for no limit; values accept a K/M/G suffix (for example 2M).",
                                colors,
                            )
                            .child(
                                div()
                                    .mt_4()
                                    .max_w(px(620.0))
                                    .flex()
                                    .gap_3()
                                    .child(
                                        settings_labeled_input(
                                            "Download limit",
                                            self.settings_download_limit_input.clone(),
                                            colors,
                                        )
                                        .flex_1()
                                        .min_w_0(),
                                    )
                                    .child(
                                        settings_labeled_input(
                                            "Upload limit",
                                            self.settings_upload_limit_input.clone(),
                                            colors,
                                        )
                                        .flex_1()
                                        .min_w_0(),
                                    ),
                            )
                            .child(
                                div()
                                    .mt_4()
                                    .flex()
                                    .items_center()
                                    .child(
                                        Button::new(
                                            "save-speed-limits",
                                            if speed_limit_saving {
                                                "Saving..."
                                            } else {
                                                "Save limits"
                                            },
                                        )
                                        .aria_label(if speed_limit_saving {
                                            "Saving speed limits"
                                        } else {
                                            "Save speed limits"
                                        })
                                        .style(ButtonStyle::Primary)
                                        .disabled(
                                            pending || !speed_limit_dirty || !speed_limit_valid,
                                        )
                                        .loading(speed_limit_saving)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.submit_speed_limits(cx);
                                        }))
                                        .render(colors),
                                    ),
                            ),
                        )
                        .child({
                            let allocation_shell = cx.entity().downgrade();
                            let allocation_control = SegmentedControl::new(
                                "settings-file-allocation",
                                FileAllocationView::all().map(|method| Segment::new(method.label())),
                                allocation_selected,
                                self.theme,
                            )
                            .disabled(pending)
                            .on_select(move |index, _window, cx| {
                                let method = FileAllocationView::all()
                                    .get(index)
                                    .copied()
                                    .unwrap_or_default();
                                allocation_shell
                                    .update(cx, |shell, cx| {
                                        shell.select_file_allocation(method, cx)
                                    })
                                    .ok();
                            });
                            settings_section(
                                "Transfer policy",
                                "Connection and allocation defaults for aria2. Maximum concurrent downloads affects the live queue immediately; connections, split, allocation, and integrity checks apply as defaults for new downloads.",
                                colors,
                            )
                            .child(
                                div()
                                    .mt_4()
                                    .max_w(px(620.0))
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .child(
                                        div()
                                            .flex()
                                            .gap_3()
                                            .child(
                                                settings_labeled_input(
                                                    "Max concurrent downloads",
                                                    self.settings_max_concurrent_input.clone(),
                                                    colors,
                                                )
                                                .flex_1()
                                                .min_w_0(),
                                            )
                                            .child(
                                                settings_labeled_input(
                                                    "Connections per server",
                                                    self.settings_max_connection_input.clone(),
                                                    colors,
                                                )
                                                .flex_1()
                                                .min_w_0(),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .gap_3()
                                            .child(
                                                settings_labeled_input(
                                                    "Split",
                                                    self.settings_split_input.clone(),
                                                    colors,
                                                )
                                                .flex_1()
                                                .min_w_0(),
                                            )
                                            .child(
                                                settings_labeled_input(
                                                    "Min split size",
                                                    self.settings_min_split_size_input.clone(),
                                                    colors,
                                                )
                                                .flex_1()
                                                .min_w_0(),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(colors.text_muted)
                                                    .child("File allocation"),
                                            )
                                            .child(allocation_control),
                                    )
                                    .child(
                                        Button::new(
                                            "toggle-check-integrity",
                                            if self.settings_page.draft_check_integrity {
                                                "Integrity check: On"
                                            } else {
                                                "Integrity check: Off"
                                            },
                                        )
                                        .aria_label(if self.settings_page.draft_check_integrity {
                                            "Disable integrity check for new downloads"
                                        } else {
                                            "Enable integrity check for new downloads"
                                        })
                                        .style(ButtonStyle::Secondary)
                                        .disabled(pending)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.toggle_check_integrity(cx);
                                        }))
                                        .render(colors),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(colors.text_muted)
                                            .child(
                                                "Concurrent downloads: all current and future downloads. Connections/split/allocation/integrity: new downloads by default.",
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .mt_4()
                                    .flex()
                                    .items_center()
                                    .child(
                                        Button::new(
                                            "save-transfer-policy",
                                            if transfer_policy_saving {
                                                "Saving..."
                                            } else {
                                                "Save transfer policy"
                                            },
                                        )
                                        .aria_label(if transfer_policy_saving {
                                            "Saving transfer policy"
                                        } else {
                                            "Save transfer policy"
                                        })
                                        .style(ButtonStyle::Primary)
                                        .disabled(
                                            pending
                                                || !transfer_policy_dirty
                                                || !transfer_policy_valid,
                                        )
                                        .loading(transfer_policy_saving)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.submit_transfer_policy(cx);
                                        }))
                                        .render(colors),
                                    ),
                            )
                        })
                        .child({
                            let volume_shell = cx.entity().downgrade();
                            let volume_control = SegmentedControl::new(
                                "settings-notification-volume",
                                NotificationVolumeView::all()
                                    .map(|volume| Segment::new(volume.label())),
                                volume_selected,
                                self.theme,
                            )
                            .disabled(pending)
                            .on_select(move |index, _window, cx| {
                                let volume = NotificationVolumeView::all()
                                    .get(index)
                                    .copied()
                                    .unwrap_or_default();
                                volume_shell
                                    .update(cx, |shell, cx| {
                                        shell.select_notification_volume(volume, cx)
                                    })
                                    .ok();
                            });
                            settings_section(
                                "Notifications",
                                "Grouped completion and error surfaces stay in-app. Quiet keeps command feedback but hides automatic completion/error toasts; Silent suppresses all toasts while activity history continues.",
                                colors,
                            )
                            .child(
                                div()
                                    .mt_4()
                                    .max_w(px(620.0))
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(colors.text_muted)
                                                    .child("Volume"),
                                            )
                                            .child(volume_control),
                                    )
                                    .child(
                                        Button::new(
                                            "toggle-notify-completion",
                                            if self.settings_page.draft_notify_on_completion {
                                                "Download completed: On"
                                            } else {
                                                "Download completed: Off"
                                            },
                                        )
                                        .aria_label(
                                            if self.settings_page.draft_notify_on_completion {
                                                "Disable completion notices"
                                            } else {
                                                "Enable completion notices"
                                            },
                                        )
                                        .style(ButtonStyle::Secondary)
                                        .disabled(pending)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.toggle_notify_on_completion(cx);
                                        }))
                                        .render(colors),
                                    )
                                    .child(
                                        Button::new(
                                            "toggle-notify-error",
                                            if self.settings_page.draft_notify_on_error {
                                                "Download failed: On"
                                            } else {
                                                "Download failed: Off"
                                            },
                                        )
                                        .aria_label(if self.settings_page.draft_notify_on_error {
                                            "Disable error notices"
                                        } else {
                                            "Enable error notices"
                                        })
                                        .style(ButtonStyle::Secondary)
                                        .disabled(pending)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.toggle_notify_on_error(cx);
                                        }))
                                        .render(colors),
                                    )
                                    .child(
                                        Button::new(
                                            "toggle-notify-engine",
                                            if self.settings_page.draft_notify_on_engine_events {
                                                "Engine events: On"
                                            } else {
                                                "Engine events: Off"
                                            },
                                        )
                                        .aria_label(
                                            if self.settings_page.draft_notify_on_engine_events {
                                                "Disable engine event notices"
                                            } else {
                                                "Enable engine event notices"
                                            },
                                        )
                                        .style(ButtonStyle::Secondary)
                                        .disabled(pending)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.toggle_notify_on_engine_events(cx);
                                        }))
                                        .render(colors),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(colors.text_muted)
                                            .child(
                                                "Batch completions and failures collapse into one notice per snapshot. Open Activity from the status bar for the recent history.",
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .mt_4()
                                    .flex()
                                    .items_center()
                                    .child(
                                        Button::new(
                                            "save-notifications",
                                            if notifications_saving {
                                                "Saving..."
                                            } else {
                                                "Save notification preferences"
                                            },
                                        )
                                        .aria_label(if notifications_saving {
                                            "Saving notification preferences"
                                        } else {
                                            "Save notification preferences"
                                        })
                                        .style(ButtonStyle::Primary)
                                        .disabled(pending || !notifications_dirty)
                                        .loading(notifications_saving)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.submit_notifications(cx);
                                        }))
                                        .render(colors),
                                    ),
                            )
                        })
                        .when_some(error, |element, error| {
                            element.child(
                                div()
                                    .id("settings-error")
                                    .role(Role::Alert)
                                    .aria_label(error.summary.clone())
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .text_xs()
                                    .text_color(colors.danger)
                                    .child(
                                        Icon::new(IconName::CircleAlert)
                                            .size(IconSize::XSmall)
                                            .color(colors.danger),
                                    )
                                    .child(error.summary),
                            )
                        }),
                            ),
                    )
                    .child(render_vertical_scrollbar(
                        &self.settings_scroll,
                        colors,
                    )),
            )
    }

    fn render_remove_confirmation(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let (display_name, has_live_tasks, has_terminal_tasks, delete_files) = self
            .remove_confirmation
            .as_ref()
            .map(|confirmation| {
                (
                    confirmation.display_name.clone(),
                    confirmation.has_live_tasks,
                    confirmation.has_terminal_tasks,
                    confirmation.delete_files,
                )
            })
            .unwrap_or_default();
        let local_files_available = !matches!(self.engine_health, EngineHealthView::External);
        let removal_description = match (has_live_tasks, has_terminal_tasks) {
            (true, true) => format!(
                "{display_name}: live tasks will be stopped and terminal records will be removed from aria2."
            ),
            (true, false) => {
                format!("{display_name} will be stopped and retained as a removed aria2 result.")
            }
            (false, true) => {
                format!("{display_name} will be removed from aria2's stopped results.")
            }
            (false, false) => format!("{display_name} will be removed from aria2."),
        };
        let file_choice = if local_files_available {
            div()
                .id("remove-task-files")
                .role(Role::CheckBox)
                .aria_label("Move exact task files to the Recycle Bin")
                .aria_toggled(if delete_files {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .flex()
                .items_start()
                .gap_2()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_remove_files(cx);
                }))
                .child(
                    Icon::new(if delete_files {
                        IconName::SquareCheckBig
                    } else {
                        IconName::Square
                    })
                    .size(IconSize::Small)
                    .color(if delete_files {
                        colors.danger
                    } else {
                        colors.text_muted
                    }),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .text_sm()
                        .text_color(colors.text_primary)
                        .child("Move exact task files to the Recycle Bin")
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.text_muted)
                                .child("Incomplete-task .aria2 control files are included; unrelated files are kept."),
                        ),
                )
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(colors.text_secondary)
                .child(Icon::new(IconName::Info).size(IconSize::Small))
                .child("This is an external engine; files on the engine host will be kept.")
                .into_any_element()
        };
        Dialog::new("remove-task-dialog", "Remove task?", self.theme)
            .description(removal_description)
            .key_context("RemoveTaskDialog")
            .track_focus(self.remove_dialog_focus.clone())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(colors.text_secondary)
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .size(IconSize::Small)
                            .color(colors.danger),
                    )
                    .child(if delete_files {
                        "Selected task files will be moved to the Recycle Bin."
                    } else {
                        "Downloaded files will be kept."
                    }),
            )
            .child(file_choice)
            .action(
                Button::new("cancel-remove-task", "Cancel")
                    .aria_label("Cancel task removal")
                    .style(ButtonStyle::Secondary)
                    .track_focus(self.remove_cancel_focus.clone())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_remove_confirmation(window, cx);
                    }))
                    .render(colors),
            )
            .action(
                Button::new(
                    "confirm-remove-task",
                    if delete_files {
                        "Remove and move files"
                    } else {
                        "Remove"
                    },
                )
                .aria_label(if delete_files {
                    "Remove task and move exact local files to the Recycle Bin"
                } else {
                    "Remove task from aria2 and keep files"
                })
                .style(ButtonStyle::Danger)
                .track_focus(self.remove_submit_focus.clone())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.submit_remove_confirmation(cx);
                }))
                .render(colors),
            )
            .into_any_element()
    }

    fn render_batch_failure_details(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(details) = self.batch_failure_details.as_ref() else {
            return div().into_any_element();
        };
        let command = details.command.label();
        let failures = details
            .failures
            .iter()
            .enumerate()
            .map(|(index, failure)| {
                let task_name = failure.identity.as_ref().map_or_else(
                    || "Batch request".to_owned(),
                    |identity| {
                        self.snapshot
                            .tasks
                            .iter()
                            .find(|task| task.identity == *identity)
                            .map(task_display_name)
                            .unwrap_or_else(|| format!("Task {}", identity.gid))
                    },
                );
                div()
                    .id(SharedString::from(format!("batch-failure-{index}")))
                    .role(Role::ListItem)
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(
                        Icon::new(IconName::CircleAlert)
                            .size(IconSize::Small)
                            .color(colors.danger),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(task_name),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.text_muted)
                                    .child(failure.error.summary.clone()),
                            ),
                    )
            })
            .collect::<Vec<_>>();
        Dialog::new("batch-failure-dialog", "Batch action details", self.theme)
            .description(format!(
                "{} task{} failed. Failed tasks remain selected for follow-up.",
                details.failures.len(),
                if details.failures.len() == 1 { "" } else { "s" }
            ))
            .key_context("BatchFailureDialog")
            .track_focus(self.batch_failure_dialog_focus.clone())
            .width(560.0)
            .child(
                div()
                    .id("batch-failure-list")
                    .role(Role::List)
                    .aria_label(format!("Failed {command} tasks"))
                    .max_h(px(360.0))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .children(failures),
            )
            .action(
                Button::new("close-batch-failures", "Close")
                    .aria_label("Close batch action details")
                    .style(ButtonStyle::Secondary)
                    .track_focus(self.batch_failure_close_focus.clone())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_batch_failure_details(window, cx);
                    }))
                    .render(colors),
            )
            .into_any_element()
    }

    fn render_task_output_name_dialog(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(dialog) = self.output_name_dialog.as_ref() else {
            return div().into_any_element();
        };
        let identity = dialog.identity.clone();
        let display_name = dialog.display_name.clone();
        let active = dialog.active;
        let error = dialog.error.clone();
        let pending = self.pending_task_command.as_ref().is_some_and(|pending| {
            pending.identity == identity
                && matches!(&pending.command, TaskCommandView::SetOutputName { .. })
        });
        let content = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(colors.text_secondary)
                    .child("Filename"),
            )
            .child(self.output_name_input.clone())
            .when(active, |element| {
                element.child(
                    div()
                        .id("active-output-name-warning")
                        .role(Role::Status)
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .text_color(colors.warning)
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .size(IconSize::Small)
                                .color(colors.warning),
                        )
                        .child("Changing an active task's output name may restart its transfer."),
                )
            })
            .when_some(error, |element, error| {
                element.child(
                    div()
                        .id("task-output-name-error")
                        .role(Role::Alert)
                        .aria_label(error.summary.clone())
                        .text_xs()
                        .text_color(colors.danger)
                        .child(error.summary),
                )
            });

        Dialog::new("task-output-name-dialog", "Change output name", self.theme)
            .description(format!(
                "Set the filename used by aria2 for {display_name}."
            ))
            .key_context("TaskOutputNameDialog")
            .track_focus(self.output_name_dialog_focus.clone())
            .width(520.0)
            .child(content)
            .action(
                Button::new("cancel-task-output-name", "Cancel")
                    .aria_label("Cancel output name change")
                    .style(ButtonStyle::Secondary)
                    .disabled(pending)
                    .track_focus(self.output_name_cancel_focus.clone())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_task_output_name(window, cx);
                    }))
                    .render(colors),
            )
            .action(
                Button::new(
                    "submit-task-output-name",
                    if pending { "Saving..." } else { "Save" },
                )
                .aria_label(if pending {
                    "Saving task output name"
                } else {
                    "Save task output name"
                })
                .style(ButtonStyle::Primary)
                .loading(pending)
                .track_focus(self.output_name_submit_focus.clone())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.submit_task_output_name(cx);
                }))
                .render(colors),
            )
            .into_any_element()
    }

    fn render_task_speed_limit_dialog(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(dialog) = self.task_speed_limit_dialog.as_ref() else {
            return div().into_any_element();
        };
        let identity = dialog.identity.clone();
        let display_name = dialog.display_name.clone();
        let error = dialog.error.clone();
        let pending = self.pending_task_command.as_ref().is_some_and(|pending| {
            pending.identity == identity
                && matches!(&pending.command, TaskCommandView::SetSpeedLimit { .. })
        });
        let content = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        settings_labeled_input(
                            "Download limit",
                            self.task_download_limit_input.clone(),
                            colors,
                        )
                        .flex_1()
                        .min_w_0(),
                    )
                    .child(
                        settings_labeled_input(
                            "Upload limit",
                            self.task_upload_limit_input.clone(),
                            colors,
                        )
                        .flex_1()
                        .min_w_0(),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(
                        "Applies to this download only. Leave a field blank for no limit; values accept a K/M/G suffix (for example 2M).",
                    ),
            )
            .when_some(error, |element, error| {
                element.child(
                    div()
                        .id("task-speed-limit-error")
                        .role(Role::Alert)
                        .aria_label(error.summary.clone())
                        .text_xs()
                        .text_color(colors.danger)
                        .child(error.summary),
                )
            });

        Dialog::new("task-speed-limit-dialog", "Set speed limits", self.theme)
            .description(format!(
                "Throttle aria2's transfer rate for {display_name}."
            ))
            .key_context("TaskSpeedLimitDialog")
            .track_focus(self.task_speed_limit_dialog_focus.clone())
            .width(520.0)
            .child(content)
            .action(
                Button::new("cancel-task-speed-limit", "Cancel")
                    .aria_label("Cancel speed limit change")
                    .style(ButtonStyle::Secondary)
                    .disabled(pending)
                    .track_focus(self.task_speed_limit_cancel_focus.clone())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_task_speed_limit(window, cx);
                    }))
                    .render(colors),
            )
            .action(
                Button::new(
                    "submit-task-speed-limit",
                    if pending { "Saving..." } else { "Save" },
                )
                .aria_label(if pending {
                    "Saving task speed limits"
                } else {
                    "Save task speed limits"
                })
                .style(ButtonStyle::Primary)
                .loading(pending)
                .track_focus(self.task_speed_limit_submit_focus.clone())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.submit_task_speed_limit(cx);
                }))
                .render(colors),
            )
            .into_any_element()
    }

    fn render_task_options_dialog(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let Some(dialog) = self.task_options_dialog.as_ref() else {
            return div().into_any_element();
        };
        let identity = dialog.identity.clone();
        let display_name = dialog.display_name.clone();
        let supports_seed_rules = dialog.supports_seed_rules;
        let error = dialog.error.clone();
        let pending = self.pending_task_command.as_ref().is_some_and(|pending| {
            pending.identity == identity
                && matches!(&pending.command, TaskCommandView::SetOptions { .. })
        });
        let content = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(if supports_seed_rules {
                        "Stops seeding when the first of seed-ratio or seed-time is reached. Use 0 for seed-ratio to disable the ratio condition."
                    } else {
                        "Seed-ratio and seed-time apply only to BitTorrent tasks."
                    }),
            )
            .when(supports_seed_rules, |element| {
                element.child(
                    div()
                        .flex()
                        .gap_3()
                        .child(
                            settings_labeled_input(
                                "Seed ratio",
                                self.task_seed_ratio_input.clone(),
                                colors,
                            )
                            .flex_1()
                            .min_w_0(),
                        )
                        .child(
                            settings_labeled_input(
                                "Seed time (minutes)",
                                self.task_seed_time_input.clone(),
                                colors,
                            )
                            .flex_1()
                            .min_w_0(),
                        ),
                )
            })
            .when_some(error, |element, error| {
                element.child(
                    div()
                        .id("task-options-error")
                        .role(Role::Alert)
                        .aria_label(error.summary.clone())
                        .text_xs()
                        .text_color(colors.danger)
                        .child(error.summary),
                )
            });

        Dialog::new("task-options-dialog", "Edit task options", self.theme)
            .description(format!("Change typed aria2 options for {display_name}."))
            .key_context("TaskOptionsDialog")
            .track_focus(self.task_options_dialog_focus.clone())
            .width(520.0)
            .child(content)
            .action(
                Button::new("cancel-task-options", "Cancel")
                    .aria_label("Cancel task option change")
                    .style(ButtonStyle::Secondary)
                    .disabled(pending)
                    .track_focus(self.task_options_cancel_focus.clone())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_task_options(window, cx);
                    }))
                    .render(colors),
            )
            .action(
                Button::new(
                    "submit-task-options",
                    if pending { "Saving..." } else { "Save" },
                )
                .aria_label(if pending {
                    "Saving task options"
                } else {
                    "Save task options"
                })
                .style(ButtonStyle::Primary)
                .loading(pending)
                .disabled(!supports_seed_rules)
                .track_focus(self.task_options_submit_focus.clone())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.submit_task_options(cx);
                }))
                .render(colors),
            )
            .into_any_element()
    }

    fn render_activity_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let entries = self.activity_log.clone();
        div()
            .id("activity-panel-layer")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.close_activity_panel(window, cx);
                }),
            )
            .child(
                div()
                    .id("activity-panel")
                    .absolute()
                    .right(px(8.0))
                    .bottom(px(36.0))
                    .w(px(ACTIVITY_PANEL_WIDTH))
                    .max_h(px(420.0))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.elevated_surface)
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        Icon::new(IconName::Activity)
                                            .size(IconSize::Small)
                                            .color(colors.information),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(colors.text_primary)
                                            .child("Activity"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        Button::new("clear-activity-log", "Clear")
                                            .aria_label("Clear activity history")
                                            .style(ButtonStyle::Secondary)
                                            .disabled(entries.is_empty())
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.clear_activity_log(cx);
                                            }))
                                            .render(colors),
                                    )
                                    .child(
                                        IconButton::new("close-activity-panel", IconName::X)
                                            .aria_label("Close activity history")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.close_activity_panel(window, cx);
                                            }))
                                            .render(colors),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child(
                                "Recent completion, error, and engine events for this session. Grouped when many finish together.",
                            ),
                    )
                    .child(if entries.is_empty() {
                        div()
                            .flex_1()
                            .py_6()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .text_color(colors.text_muted)
                            .child("No activity yet.")
                            .into_any_element()
                    } else {
                        div()
                            .id("activity-panel-scroll")
                            .flex_1()
                            .min_h_0()
                            .max_h(px(320.0))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .children(entries.into_iter().map(|entry| {
                                let kind_color = match entry.kind {
                                    ActivityKindView::Completion => colors.success,
                                    ActivityKindView::Error => colors.danger,
                                    ActivityKindView::Engine => colors.warning,
                                    ActivityKindView::Command => colors.information,
                                    ActivityKindView::Info => colors.text_muted,
                                };
                                div()
                                    .id(SharedString::from(format!(
                                        "activity-entry-{}",
                                        entry.id
                                    )))
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .p_2()
                                    .rounded_md()
                                    .bg(colors.surface)
                                    .border_1()
                                    .border_color(colors.border)
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(StatusIndicator::new(kind_color))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(colors.text_muted)
                                                    .child(entry.kind.label()),
                                            )
                                            .when(entry.count > 1, |element| {
                                                element.child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(colors.text_muted)
                                                        .child(format!("x{}", entry.count)),
                                                )
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(colors.text_primary)
                                            .child(entry.summary.clone()),
                                    )
                                    .when_some(entry.detail.clone(), |element, detail| {
                                        element.child(
                                            div()
                                                .text_xs()
                                                .text_color(colors.text_muted)
                                                .child(detail),
                                        )
                                    })
                            }))
                            .into_any_element()
                    }),
            )
            .into_any_element()
    }

    fn render_toast(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(notice) = self.status_notice.as_ref() else {
            return div().into_any_element();
        };
        let kind = if notice.is_error {
            ToastKind::Error
        } else {
            ToastKind::Success
        };
        div()
            .absolute()
            .right(px(16.0))
            .bottom(px(44.0))
            .child(
                Toast::new("operation-toast", notice.message.clone(), kind, self.theme)
                    .on_close(cx.listener(|this, _, _, cx| this.dismiss_notice(cx))),
            )
            .into_any_element()
    }

    fn render_empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = self.theme.colors;
        let (icon, title, show_clear) = match &self.snapshot.connection {
            ConnectionView::Connecting
            | ConnectionView::Authenticating
            | ConnectionView::Synchronizing
            | ConnectionView::Reconnecting { .. }
                if self.snapshot.tasks.is_empty() =>
            {
                (
                    IconName::LoaderCircle,
                    "Connecting to aria2".to_owned(),
                    false,
                )
            }
            ConnectionView::Failed { .. } => {
                (IconName::CloudOff, "Connection failed".to_owned(), false)
            }
            ConnectionView::Disconnected if self.snapshot.tasks.is_empty() => {
                (IconName::CloudOff, "aria2 is unavailable".to_owned(), false)
            }
            _ if !self.query.search.trim().is_empty() => {
                (IconName::SearchX, "No matching downloads".to_owned(), true)
            }
            _ if self.query.filter != WorkspaceFilter::All => (
                IconName::Inbox,
                format!(
                    "No {} tasks",
                    self.query.filter.short_label().to_lowercase()
                ),
                true,
            ),
            _ => (IconName::Inbox, "No downloads".to_owned(), false),
        };
        let show_add = self.query.filter == WorkspaceFilter::All
            && self.query.search.trim().is_empty()
            && self.snapshot.commands_available()
            && !self.add_dialog.open;

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .max_w(px(420.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .text_center()
                    .child(
                        div()
                            .size(px(48.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .border_1()
                            .border_color(colors.border)
                            .bg(colors.elevated_surface)
                            .child(
                                Icon::new(icon)
                                    .size(IconSize::Large)
                                    .color(colors.text_muted),
                            ),
                    )
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .when(show_clear, |element| {
                        element.child(
                            Button::new("clear-empty-filter", "Clear filter")
                                .aria_label("Clear search and task filter")
                                .style(ButtonStyle::Secondary)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.query.filter = WorkspaceFilter::All;
                                    this.search_input
                                        .update(cx, |input, cx| input.set_text("", cx));
                                    window.focus(&this.focus_handle, cx);
                                    this.emit_query(cx);
                                }))
                                .render(colors),
                        )
                    })
                    .when(show_add, |element| {
                        element.child(
                            Button::new("add-download-empty-state", "Add download")
                                .icon(IconName::Plus)
                                .aria_label("Add a URL or magnet download")
                                .style(ButtonStyle::Primary)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_add_download(&OpenAddDownload, window, cx);
                                }))
                                .render(colors),
                        )
                    })
                    .when(self.snapshot.connection.can_retry(), |element| {
                        element.child(
                            Button::new("retry-connection", "Retry")
                                .aria_label("Retry aria2 connection now")
                                .style(ButtonStyle::Primary)
                                .on_click(cx.listener(|this, _, _, cx| this.request_retry(cx)))
                                .render(colors),
                        )
                    }),
            )
            .into_any_element()
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
        let task_layout = task_layout_mode(
            f32::from(window.viewport_size().width),
            self.details_drawer.is_some(),
        );
        div()
            .id("download-workspace")
            .key_context("DownloadWorkspace")
            .role(Role::Application)
            .aria_label("AriaDeck download workspace")
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
                this.add_metadata_paths(paths.paths().to_vec(), window, cx);
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
                    .child(self.render_sidebar(cx))
                    .child(match self.page {
                        AppPage::Downloads => self.render_main(task_layout, cx).into_any_element(),
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

fn titlebar_drag_region() -> Div {
    let region = div().flex_1().min_w_0().h_full();
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        region.window_control_area(WindowControlArea::Drag)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        region
    }
}

fn theme_for_scheme(scheme: ColorSchemeView) -> Theme {
    match scheme {
        ColorSchemeView::Light => Theme::light(),
        ColorSchemeView::Dark => Theme::dark(),
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowControlKind {
    Minimize,
    Maximize,
    Close,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WindowControlConfig {
    id: &'static str,
    icon: IconName,
    label: &'static str,
    area: WindowControlArea,
    danger: bool,
}

#[cfg(target_os = "windows")]
fn window_control_config(kind: WindowControlKind, maximized: bool) -> WindowControlConfig {
    match kind {
        WindowControlKind::Minimize => WindowControlConfig {
            id: "window-minimize",
            icon: IconName::WindowMinimize,
            label: "Minimize window",
            area: WindowControlArea::Min,
            danger: false,
        },
        WindowControlKind::Maximize => WindowControlConfig {
            id: "window-maximize",
            icon: if maximized {
                IconName::WindowRestore
            } else {
                IconName::WindowMaximize
            },
            label: if maximized {
                "Restore window"
            } else {
                "Maximize window"
            },
            area: WindowControlArea::Max,
            danger: false,
        },
        WindowControlKind::Close => WindowControlConfig {
            id: "window-close",
            icon: IconName::WindowClose,
            label: "Close window",
            area: WindowControlArea::Close,
            danger: true,
        },
    }
}

/// Windows 11 caption button width (WinUI AppWindow title bar convention).
#[cfg(target_os = "windows")]
const WINDOW_CONTROL_WIDTH: f32 = 46.0;

#[cfg(target_os = "windows")]
fn window_control_button(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    area: WindowControlArea,
    colors: crate::ThemeColors,
    danger: bool,
) -> Stateful<Div> {
    // Caption glyphs must set Icon.color explicitly: embedded SVGs use a fixed
    // stroke and do not inherit the parent's text_color, so omitting color made
    // them render black on the dark titlebar (invisible).
    let idle = colors.text_primary;
    let hover_bg = if danger {
        colors.danger
    } else {
        colors.surface_hover
    };
    let active_bg = if danger {
        colors.danger
    } else {
        colors.surface_active
    };
    // Fluent Close: red fill + light glyph. Other captions keep the idle glyph
    // color while the fill changes (GPUI recolors SVG strokes via text_color).
    let hover_fg = if danger {
        Theme::light().colors.text_inverse
    } else {
        idle
    };
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .window_control_area(area)
        .focusable()
        .tab_stop(true)
        .h(px(TITLEBAR_HEIGHT))
        .w(px(WINDOW_CONTROL_WIDTH))
        .min_w(px(WINDOW_CONTROL_WIDTH))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_none()
        .cursor_pointer()
        .text_color(idle)
        .hover(move |style| style.bg(hover_bg).text_color(hover_fg))
        .active(move |style| style.bg(active_bg).text_color(hover_fg))
        .tooltip(Tooltip::text(label, None, colors))
        // Match toolbar IconButton optical size (Medium / 16px) so caption
        // glyphs and chrome actions feel consistent.
        .child(Icon::new(icon).size(IconSize::Medium).color(idle))
}

fn speed_chart_column(download_height: f32, upload_height: f32, colors: crate::ThemeColors) -> Div {
    div()
        .h_full()
        .flex_1()
        .min_w(px(1.0))
        .flex()
        .items_end()
        .child(
            div()
                .w(relative(0.5))
                .h(relative(download_height.clamp(0.0, 1.0)))
                .bg(colors.progress_download),
        )
        .child(
            div()
                .w(relative(0.5))
                .h(relative(upload_height.clamp(0.0, 1.0)))
                .bg(colors.progress_upload),
        )
}

fn speed_chart_window(history: &[SpeedSampleView]) -> &[SpeedSampleView] {
    if history.len() > SPEED_CHART_SAMPLES {
        &history[history.len() - SPEED_CHART_SAMPLES..]
    } else {
        history
    }
}

fn speed_chart_legend(label: &'static str, color: Hsla, colors: crate::ThemeColors) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(div().size(px(6.0)).rounded_sm().bg(color))
        .child(div().text_color(colors.text_muted).child(label))
}

fn toolbar_icon_button(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    state: ToolbarButtonState,
    danger: bool,
    shortcut: Option<&'static str>,
    colors: crate::ThemeColors,
) -> Stateful<Div> {
    let enabled = state == ToolbarButtonState::Enabled;
    let loading = state == ToolbarButtonState::Loading;
    let tooltip = shortcut.map_or_else(
        || Tooltip::new(label),
        |shortcut| Tooltip::new(label).meta(shortcut),
    );
    IconButton::new(id, icon)
        .aria_label(if enabled || loading {
            label.to_owned()
        } else {
            format!("{label} unavailable")
        })
        .style(if danger {
            ButtonStyle::Danger
        } else {
            ButtonStyle::Ghost
        })
        .disabled(!enabled && !loading)
        .loading(loading)
        .tooltip(tooltip)
        .render(colors)
}

#[allow(clippy::too_many_arguments)]
fn queue_move_button(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    command: TaskCommandView,
    enabled: bool,
    pending_command: Option<&TaskCommandView>,
    shortcut: Option<&'static str>,
    colors: crate::ThemeColors,
    cx: &mut Context<AppShell>,
) -> Stateful<Div> {
    let loading = pending_command == Some(&command);
    toolbar_icon_button(
        id,
        icon,
        label,
        ToolbarButtonState::from_flags(enabled, loading),
        false,
        shortcut,
        colors,
    )
    .when(enabled, move |button| {
        button.on_click(cx.listener(move |this, _, _window, cx| {
            this.begin_task_command(command.clone(), cx);
        }))
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ContextMenuAction {
    Details,
    OpenDownload,
    OpenFolder,
    CopySource,
    CopyGid,
    Pause,
    ForcePause,
    Resume,
    Retry,
    MoveTop,
    MoveUp,
    MoveDown,
    MoveBottom,
    OutputName,
    SpeedLimit,
    TaskOptions,
    Remove,
}

fn context_menu_item(
    action: ContextMenuAction,
    label: &'static str,
    shortcut: Option<&'static str>,
    enabled: bool,
    destructive: bool,
    colors: crate::ThemeColors,
    cx: &mut Context<AppShell>,
) -> AnyElement {
    div()
        .id(SharedString::from(format!("ctx-menu-{label}")))
        .role(Role::MenuItem)
        .aria_label(label)
        .w_full()
        .h(px(32.0))
        .px_3()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .rounded_sm()
        .cursor_pointer()
        .text_sm()
        .text_color(if !enabled {
            colors.text_muted
        } else if destructive {
            colors.danger
        } else {
            colors.text_primary
        })
        .when(enabled, |element| {
            element
                .hover(|style| style.bg(colors.surface_hover))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.activate_context_menu_action(action, window, cx);
                }))
        })
        .child(label)
        .when_some(shortcut, |element, shortcut| {
            element.child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(shortcut),
            )
        })
        .into_any_element()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToolbarButtonState {
    Enabled,
    Disabled,
    Loading,
}

impl ToolbarButtonState {
    fn from_flags(enabled: bool, loading: bool) -> Self {
        if loading {
            Self::Loading
        } else if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}

fn render_vertical_scrollbar(scroll: &ScrollHandle, colors: crate::ThemeColors) -> AnyElement {
    let bounds = scroll.bounds();
    let max_offset = scroll.max_offset();
    let offset = scroll.offset();
    let viewport = bounds.size.height;
    let content = viewport + max_offset.y;
    if max_offset.y <= px(0.0) || viewport <= px(0.0) || content <= px(0.0) {
        return div().w(px(10.0)).flex_none().into_any_element();
    }
    let thumb_ratio = (viewport / content).clamp(0.12, 1.0);
    let thumb_height = (viewport * thumb_ratio).max(px(28.0));
    let travel = (viewport - thumb_height).max(px(0.0));
    let progress = if max_offset.y.as_f32().abs() < f32::EPSILON {
        0.0
    } else {
        ((-offset.y) / max_offset.y).clamp(0.0, 1.0)
    };
    let thumb_top = travel * progress;
    let handle = scroll.clone();
    let max_y = max_offset.y;
    div()
        .id("vertical-scrollbar-track")
        .w(px(10.0))
        .flex_none()
        .h_full()
        .py_1()
        .pr_1()
        .child(
            div()
                .id("vertical-scrollbar-rail")
                .relative()
                .w(px(6.0))
                .h_full()
                .rounded_full()
                .bg(with_alpha(colors.border, 0.35))
                .child(
                    div()
                        .id("vertical-scrollbar-thumb")
                        .absolute()
                        .top(thumb_top)
                        .left_0()
                        .right_0()
                        .h(thumb_height)
                        .rounded_full()
                        .bg(with_alpha(colors.text_muted, 0.55))
                        .hover(|style| style.bg(with_alpha(colors.text_muted, 0.8)))
                        .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let track_y = event.position.y - bounds.origin.y;
                            let ratio = (track_y / viewport).clamp(0.0, 1.0);
                            let y = -(max_y * ratio);
                            handle.set_offset(point(px(0.0), y));
                        }),
                ),
        )
        .into_any_element()
}

fn settings_section(title: &'static str, detail: &'static str, colors: crate::ThemeColors) -> Div {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div()
                .mt_1()
                .text_xs()
                .text_color(colors.text_muted)
                .child(detail),
        )
}

fn settings_input_config(
    element_id: &'static str,
    accessibility_label: &'static str,
    placeholder: &'static str,
    leading_icon: Option<IconName>,
    secure: bool,
) -> TextFieldConfig {
    TextFieldConfig {
        element_id: element_id.into(),
        key_context: TEXT_FIELD_KEY_CONTEXT.into(),
        role: Role::TextInput,
        accessibility_label: accessibility_label.into(),
        placeholder: placeholder.into(),
        leading_icon,
        clearable: !secure,
        allow_newlines: false,
        secure,
    }
}

fn settings_labeled_input(
    label: &'static str,
    input: Entity<TextField>,
    colors: crate::ThemeColors,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_xs().text_color(colors.text_muted).child(label))
        .child(input)
}

fn filter_icon(filter: WorkspaceFilter) -> IconName {
    match filter {
        WorkspaceFilter::All => IconName::List,
        WorkspaceFilter::Active => IconName::Activity,
        WorkspaceFilter::Waiting => IconName::Clock3,
        WorkspaceFilter::Paused => IconName::Pause,
        WorkspaceFilter::Completed => IconName::CircleCheck,
        WorkspaceFilter::Failed => IconName::CircleAlert,
    }
}

fn task_status_icon(status: TaskStatusView) -> IconName {
    match status {
        TaskStatusView::Active => IconName::Activity,
        TaskStatusView::Seeding => IconName::ArrowUp,
        TaskStatusView::Waiting => IconName::Clock3,
        TaskStatusView::Paused => IconName::Pause,
        TaskStatusView::Complete => IconName::CircleCheck,
        TaskStatusView::Failed => IconName::CircleAlert,
        TaskStatusView::Verifying => IconName::ScanSearch,
        TaskStatusView::Removed => IconName::Trash2,
        TaskStatusView::Unknown => IconName::CircleHelp,
    }
}

fn task_display_name(task: &DownloadRowView) -> String {
    if task.name_state.is_resolving() {
        "Resolving filename...".into()
    } else {
        task.display_name.clone()
    }
}

fn parse_add_download_sources(input: &str) -> Vec<AddDownloadSourceView> {
    input
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let uri = line.trim();
            (!uri.is_empty()).then(|| AddDownloadSourceView::Uri {
                line: index + 1,
                uri: uri.to_owned(),
            })
        })
        .collect()
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

fn can_accept_metadata_drop(enabled: bool, paths: &[PathBuf]) -> bool {
    enabled
        && paths
            .iter()
            .any(|path| metadata_kind_from_path(path).is_some())
}

fn metadata_path_key(path: &Path) -> String {
    let key = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        key.to_ascii_lowercase()
    } else {
        key
    }
}

fn metadata_selection_summary(preview: &AddDownloadMetadataPreviewView) -> String {
    let mut known_bytes = 0_u64;
    let mut unknown_sizes = 0_usize;
    for file in &preview.files {
        if preview
            .selected_file_indices
            .binary_search(&file.index)
            .is_ok()
        {
            if let Some(length) = file.length {
                known_bytes = known_bytes.saturating_add(length);
            } else {
                unknown_sizes = unknown_sizes.saturating_add(1);
            }
        }
    }
    let count = preview.selected_file_indices.len();
    let total = preview.files.len();
    if unknown_sizes == 0 {
        format!(
            "{count} of {total} selected · {}",
            format_bytes(known_bytes)
        )
    } else {
        format!(
            "{count} of {total} selected · {} + {unknown_sizes} unknown",
            format_bytes(known_bytes)
        )
    }
}

fn selected_metadata_known_bytes(previews: &[AddDownloadMetadataPreviewView]) -> Option<u64> {
    previews.iter().try_fold(0_u64, |total, preview| {
        preview.files.iter().try_fold(total, |total, file| {
            if preview
                .selected_file_indices
                .binary_search(&file.index)
                .is_ok()
            {
                file.length
                    .map_or(Some(total), |length| total.checked_add(length))
            } else {
                Some(total)
            }
        })
    })
}

fn successor_task(
    previous: &WorkspaceSnapshot,
    next: &WorkspaceSnapshot,
    selected: &TaskIdentity,
) -> Option<DownloadRowView> {
    if selected.profile_id != next.profile_id {
        return None;
    }

    let previous_task = previous
        .tasks
        .iter()
        .find(|task| task.identity == *selected);
    if let Some(previous_task) = previous_task
        && let Some(successor) = previous_task
            .followed_by
            .iter()
            .find_map(|gid| next.tasks.iter().find(|task| task.identity.gid == *gid))
    {
        return Some(successor.clone());
    }

    next.tasks
        .iter()
        .find(|task| task.belongs_to.as_deref() == Some(selected.gid.as_str()))
        .cloned()
}

fn task_overview_summary(task: &DownloadRowView, colors: crate::ThemeColors) -> Div {
    let basis_points = task.progress_basis_points();
    let progress = f32::from(basis_points.unwrap_or(0)) / 10_000.0;
    let seeding = task.status == TaskStatusView::Seeding;
    let size_label = if task.total_bytes == 0 {
        format_bytes(task.completed_bytes)
    } else {
        format!(
            "{} / {}",
            format_bytes(task.completed_bytes),
            format_bytes(task.total_bytes)
        )
    };
    let eta_label = if seeding {
        format!(
            "{} observed this session",
            format_eta(task.observed_seeding_seconds)
        )
    } else {
        task.eta_seconds.filter(|seconds| *seconds > 0).map_or_else(
            || task.status.label().to_owned(),
            |seconds| format!("{} remaining", format_eta(Some(seconds))),
        )
    };
    let error_label = task.error.as_ref().map(|error| {
        error.code.map_or_else(
            || error.summary.clone(),
            |code| format!("Error {code}: {}", error.summary),
        )
    });
    let error_id = SharedString::from(format!("task-error-{}", task.identity.gid));

    div()
        .flex_none()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .border_b_1()
        .border_color(colors.border)
        .bg(colors.elevated_surface)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.text_secondary)
                        .child(if seeding { "Share ratio" } else { "Progress" }),
                )
                .child(task_status_badge(task.status, colors)),
        )
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .font_features(tabular_numbers())
                .child(
                    div()
                        .text_size(px(24.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(if seeding {
                            format_share_ratio(task.share_ratio_milli())
                        } else {
                            format_percent(basis_points)
                        }),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.text_muted)
                        .child(eta_label),
                ),
        )
        .child(task_progress_bar(progress, task.status, colors).h(px(5.0)))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .font_features(tabular_numbers())
                .text_xs()
                .text_color(colors.text_muted)
                .child(if seeding {
                    format!("Uploaded {}", format_bytes(task.uploaded_bytes))
                } else {
                    size_label
                })
                .child(if seeding {
                    format!("Up {}", format_rate(task.upload_rate))
                } else {
                    format_rate(task.download_rate)
                }),
        )
        .when_some(error_label, |element, error| {
            element.child(
                div()
                    .id(error_id)
                    .role(Role::Alert)
                    .text_xs()
                    .text_color(colors.danger)
                    .child(error),
            )
        })
}

fn drawer_message(
    title: &'static str,
    detail: &'static str,
    colors: crate::ThemeColors,
) -> AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_2()
        .px_5()
        .text_center()
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(div().text_xs().text_color(colors.text_muted).child(detail))
        .into_any_element()
}

fn detail_line(
    label: &'static str,
    value: impl Into<SharedString>,
    colors: crate::ThemeColors,
) -> Div {
    div()
        .flex()
        .items_start()
        .gap_3()
        .child(
            div()
                .w(px(76.0))
                .flex_none()
                .text_xs()
                .text_color(colors.text_muted)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(colors.text_secondary)
                .child(value.into()),
        )
}

fn detail_line_with_action(
    label: &'static str,
    value: impl Into<SharedString>,
    action: impl IntoElement,
    colors: crate::ThemeColors,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(
            div()
                .w(px(76.0))
                .flex_none()
                .text_xs()
                .text_color(colors.text_muted)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family("monospace")
                .text_xs()
                .text_color(colors.text_secondary)
                .child(value.into()),
        )
        .child(action)
}

fn detail_collection_section(
    title: &'static str,
    empty_message: &'static str,
    rows: Vec<AnyElement>,
    colors: crate::ThemeColors,
) -> Div {
    let count = rows.len();
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(colors.text_secondary)
                        .child(title),
                )
                .child(
                    div()
                        .font_features(tabular_numbers())
                        .text_xs()
                        .text_color(colors.text_muted)
                        .child(count.to_string()),
                ),
        )
        .when(count == 0, |element| {
            element.child(
                div()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.elevated_surface)
                    .p_3()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(empty_message),
            )
        })
        .when(count != 0, |element| {
            element.child(div().flex().flex_col().gap_2().children(rows))
        })
}

fn detail_collection_row(
    primary: impl Into<SharedString>,
    secondary: impl Into<SharedString>,
    badge: Option<&'static str>,
    colors: crate::ThemeColors,
) -> AnyElement {
    div()
        .rounded_md()
        .border_1()
        .border_color(colors.border)
        .bg(colors.elevated_surface)
        .p_3()
        .flex()
        .items_start()
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.text_secondary)
                        .font_family("monospace")
                        .child(primary.into()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.text_muted)
                        .child(secondary.into()),
                ),
        )
        .when_some(badge, |element, badge| {
            element.child(
                div()
                    .flex_none()
                    .rounded_full()
                    .bg(colors.surface_active)
                    .px_2()
                    .py_0p5()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(badge),
            )
        })
        .into_any_element()
}

fn render_task_uri(source: TaskUriView, colors: crate::ThemeColors) -> AnyElement {
    detail_collection_row(source.uri, source.status.label(), None, colors)
}

fn render_task_tracker(tracker: TaskTrackerView, colors: crate::ThemeColors) -> AnyElement {
    detail_collection_row(
        tracker.uri,
        format!("Announce tier {}", tracker.tier),
        None,
        colors,
    )
}

fn render_task_server(server: TaskServerView, colors: crate::ThemeColors) -> AnyElement {
    let current_uri = if server.current_uri.is_empty() {
        server.uri.clone()
    } else {
        server.current_uri.clone()
    };
    let secondary = if server.uri.is_empty() || server.uri == current_uri {
        format!(
            "File {} · Download {}",
            server.file_index,
            format_rate(server.download_rate)
        )
    } else {
        format!(
            "From {} · File {} · Download {}",
            server.uri,
            server.file_index,
            format_rate(server.download_rate)
        )
    };
    detail_collection_row(current_uri, secondary, None, colors)
}

fn render_task_peer(peer: TaskPeerView, colors: crate::ThemeColors) -> AnyElement {
    let address = if peer.address.contains(':') {
        format!("[{}]:{}", peer.address, peer.port)
    } else {
        format!("{}:{}", peer.address, peer.port)
    };
    detail_collection_row(
        address,
        format!(
            "Down {} · Up {}",
            format_rate(peer.download_rate),
            format_rate(peer.upload_rate)
        ),
        peer.seeder.then_some("Seed"),
        colors,
    )
}

fn render_task_option(option: TaskOptionView, colors: crate::ThemeColors) -> AnyElement {
    let value = if option.redacted {
        "Hidden".to_owned()
    } else if option.value.is_empty() {
        "Empty".to_owned()
    } else {
        option.value
    };
    detail_collection_row(
        option.key,
        value,
        option.redacted.then_some("Sensitive"),
        colors,
    )
}

fn format_seed_stop_rules(options: &[TaskOptionView]) -> String {
    let value = |key: &str| {
        options
            .iter()
            .find(|option| option.key.eq_ignore_ascii_case(key))
            .map(|option| option.value.as_str())
            .unwrap_or("not reported")
    };
    let ratio = value("seed-ratio");
    let ratio = if ratio.parse::<f64>().is_ok_and(|value| value == 0.0) {
        "ratio disabled (0.0)".to_owned()
    } else {
        format!("ratio {ratio}")
    };
    format!(
        "Stops at the first reached limit: {ratio} · time {} min",
        value("seed-time")
    )
}

fn render_file_row(
    gid: &str,
    index: usize,
    file: TaskFileView,
    file_count: usize,
    colors: crate::ThemeColors,
) -> Stateful<Div> {
    let basis_points = if file.length == 0 {
        None
    } else {
        let completed = u128::from(file.completed_length.min(file.length));
        Some(((completed * 10_000) / u128::from(file.length)) as u16)
    };
    let stable_id = SharedString::from(format!("task-file:{gid}:{}", file.index));
    div()
        .id(stable_id)
        .role(Role::ListItem)
        .aria_position_in_set(index + 1)
        .aria_size_of_set(file_count)
        .aria_label(format!(
            "{}, {}, {}, {}",
            file.path,
            if file.selected { "enabled" } else { "skipped" },
            format_bytes(file.length),
            format_percent(basis_points)
        ))
        .h(px(52.0))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .gap_3()
        .px_4()
        .child(
            div()
                .w(px(18.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    Icon::new(if file.selected {
                        IconName::CircleCheck
                    } else {
                        IconName::CircleX
                    })
                    .size(IconSize::Small)
                    .color(if file.selected {
                        colors.success
                    } else {
                        colors.text_muted
                    }),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(colors.text_secondary)
                .child(file.path),
        )
        .child(
            div()
                .w(px(78.0))
                .flex_none()
                .text_right()
                .font_features(tabular_numbers())
                .text_xs()
                .text_color(colors.text_muted)
                .child(format_percent(basis_points)),
        )
}

fn task_command_label(command: &TaskCommandView) -> &'static str {
    match command {
        TaskCommandView::Pause => "Pause",
        TaskCommandView::ForcePause => "Force pause",
        TaskCommandView::Resume => "Resume",
        TaskCommandView::MoveToQueueTop => "Move to top",
        TaskCommandView::MoveUpInQueue => "Move up",
        TaskCommandView::MoveDownInQueue => "Move down",
        TaskCommandView::MoveToQueueBottom => "Move to bottom",
        TaskCommandView::Retry => "Retry",
        TaskCommandView::SetOutputName { .. } => "Change output name",
        TaskCommandView::SetSpeedLimit { .. } => "Set speed limits",
        TaskCommandView::SetConnectionPolicy { .. } => "Set connection policy",
        TaskCommandView::SetOptions { .. } => "Edit task options",
        TaskCommandView::RemoveTask | TaskCommandView::RemoveTaskAndFiles => "Remove",
        TaskCommandView::ForceRemoveTask => "Force remove",
    }
}

fn output_name_validation_error(output_name: &str) -> Option<&'static str> {
    if output_name.is_empty() {
        Some("Enter a filename.")
    } else if output_name == "." || output_name == ".." {
        Some("A filename cannot be '.' or '..'.")
    } else if output_name.contains(['/', '\\', '\0']) {
        Some("Use a filename without path separators.")
    } else {
        None
    }
}

fn stale_session_error() -> OperationErrorView {
    OperationErrorView {
        code: "command.stale_session".into(),
        summary: "The aria2 session changed. Review current state before submitting again.".into(),
        retryable: false,
    }
}

fn tabular_numbers() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}

fn connection_color(connection: &ConnectionView, colors: crate::ThemeColors) -> Hsla {
    match connection {
        ConnectionView::Connected => colors.success,
        ConnectionView::Failed { .. } => colors.danger,
        ConnectionView::Disconnected => colors.text_muted,
        ConnectionView::Connecting
        | ConnectionView::Authenticating
        | ConnectionView::Synchronizing
        | ConnectionView::Reconnecting { .. } => colors.information,
    }
}

fn engine_health_color(health: &EngineHealthView, colors: crate::ThemeColors) -> Hsla {
    match health {
        EngineHealthView::External => colors.information,
        EngineHealthView::Running { restarts: 0 } => colors.success,
        EngineHealthView::Running { .. } | EngineHealthView::Restarting { .. } => colors.warning,
        EngineHealthView::Failed { .. } => colors.danger,
    }
}

fn task_status_color(status: TaskStatusView, colors: crate::ThemeColors) -> Hsla {
    match status {
        TaskStatusView::Active => colors.accent,
        TaskStatusView::Seeding => colors.progress_upload,
        TaskStatusView::Waiting | TaskStatusView::Paused => colors.warning,
        TaskStatusView::Complete => colors.success,
        TaskStatusView::Failed | TaskStatusView::Removed => colors.danger,
        TaskStatusView::Verifying => colors.information,
        TaskStatusView::Unknown => colors.text_muted,
    }
}

fn task_progress_bar(progress: f32, status: TaskStatusView, colors: crate::ThemeColors) -> Div {
    let fill = match status {
        TaskStatusView::Failed | TaskStatusView::Removed => colors.danger,
        TaskStatusView::Complete => colors.success,
        TaskStatusView::Seeding => colors.progress_upload,
        _ => colors.progress_download,
    };
    div()
        .h(px(4.0))
        .w_full()
        .rounded_full()
        .overflow_hidden()
        .bg(colors.progress_track)
        .child(
            div()
                .h_full()
                .w(relative(progress.clamp(0.0, 1.0)))
                .rounded_full()
                .bg(fill),
        )
}

fn task_table_value(width: f32, value: String, colors: crate::ThemeColors) -> Div {
    div()
        .w(px(width))
        .flex_none()
        .truncate()
        .font_features(tabular_numbers())
        .text_xs()
        .text_color(colors.text_secondary)
        .child(value)
}

fn task_status_badge(status: TaskStatusView, colors: crate::ThemeColors) -> Div {
    let color = task_status_color(status, colors);
    div()
        .h(px(22.0))
        .max_w_full()
        .px_2()
        .flex()
        .items_center()
        .justify_center()
        .rounded_sm()
        .border_1()
        .border_color(with_alpha(color, 0.28))
        .bg(with_alpha(color, 0.1))
        .text_size(px(11.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(color)
        .child(status.label())
}

fn with_alpha(mut color: Hsla, alpha: f32) -> Hsla {
    color.a = alpha;
    color
}

#[cfg(test)]
mod tests {
    use gpui::{TestAppContext, point, px};

    use super::*;
    use crate::{
        AddDownloadMetadataFileView, AddDownloadMetadataPreviewItemView, CoreInstallStatusView,
        CoreInstallationView, CoreSourceView, SpeedLimitSettingsView, TaskCountsView,
        TaskNameStateView, TaskSourceKindView, TaskStatusView,
    };

    fn task(index: usize) -> DownloadRowView {
        DownloadRowView {
            identity: TaskIdentity {
                profile_id: "profile".into(),
                gid: format!("{index:016x}"),
            },
            display_name: format!("archive-{index:05}.bin"),
            name_state: TaskNameStateView::Resolved,
            source_kind: TaskSourceKindView::DirectUri,
            primary_source: Some("https://example.test/file.bin".into()),
            directory: Some("C:/downloads".into()),
            followed_by: Vec::new(),
            belongs_to: None,
            status: TaskStatusView::Complete,
            error: None,
            total_bytes: 1_048_576,
            completed_bytes: 1_048_576,
            uploaded_bytes: 0,
            download_rate: 0,
            upload_rate: 0,
            eta_seconds: Some(0),
            observed_seeding_seconds: None,
            revision: 1,
        }
    }

    fn snapshot(count: usize) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            profile_id: "profile".into(),
            session_id: "session".into(),
            generation: 1,
            source_revision: 1,
            connection: ConnectionView::Connected,
            stale: false,
            local_path_actions_available: true,
            download_rate: 0,
            upload_rate: 0,
            speed_history: Vec::new(),
            counts: TaskCountsView {
                all: count,
                completed: count,
                ..TaskCountsView::default()
            },
            stopped_history: crate::StoppedHistoryView {
                loaded: count,
                total: Some(count),
                can_load_more: false,
            },
            tasks: (0..count).map(task).collect(),
            capabilities: crate::EngineCapabilitiesView::unknown(),
        }
    }

    fn details(file_count: usize) -> TaskDetailsView {
        TaskDetailsView {
            directory: Some("C:/downloads".into()),
            primary_source: Some("https://example.test/file.bin".into()),
            output_path: Some("C:/downloads".into()),
            path_validation: TaskPathValidationView::Valid {
                existing_files: file_count,
                missing_paths: 0,
            },
            info_hash: Some("0123456789abcdef".into()),
            piece_length: Some(1_048_576),
            piece_count: Some(file_count as u32),
            trackers: vec![TaskTrackerView {
                tier: 1,
                uri: "https://tracker.example/announce".into(),
            }],
            uris: vec![TaskUriView {
                uri: "https://example.test/file.bin".into(),
                status: crate::TaskUriStatusView::Used,
            }],
            servers: Vec::new(),
            peers: Vec::new(),
            options: vec![TaskOptionView {
                key: "max-download-limit".into(),
                value: "0".into(),
                redacted: false,
            }],
            files: (0..file_count)
                .map(|index| TaskFileView {
                    index: index as u32 + 1,
                    path: format!("C:/downloads/file-{index:05}.bin"),
                    length: 1_048_576,
                    completed_length: 524_288,
                    selected: true,
                })
                .collect(),
        }
    }

    fn metadata_preview(
        path: &str,
        kind: AddDownloadMetadataKindView,
        file_count: u32,
    ) -> AddDownloadMetadataPreviewView {
        AddDownloadMetadataPreviewView {
            path: PathBuf::from(path),
            kind,
            content_sha256: "digest".into(),
            info_hash: (kind == AddDownloadMetadataKindView::Torrent)
                .then(|| "0123456789abcdef0123456789abcdef01234567".into()),
            files: (1..=file_count)
                .map(|index| AddDownloadMetadataFileView {
                    index,
                    path: format!("file-{index}.bin"),
                    length: Some(u64::from(index) * 100),
                })
                .collect(),
            selected_file_indices: (1..=file_count).collect(),
        }
    }

    #[test]
    fn task_layout_uses_the_remaining_main_pane_width() {
        assert_eq!(task_layout_mode(1_180.0, false), TaskLayoutMode::Wide);
        assert_eq!(task_layout_mode(1_180.0, true), TaskLayoutMode::Compact);
        assert_eq!(task_layout_mode(960.0, false), TaskLayoutMode::Compact);
        assert_eq!(task_layout_mode(1_400.0, true), TaskLayoutMode::Wide);
    }

    #[test]
    fn search_bounds_are_centered_and_ignore_workspace_drawers() {
        for viewport_width in [960.0, 1_180.0, 1_600.0] {
            let (left, right) = centered_search_bounds(viewport_width);
            assert!(((left + right) / 2.0 - viewport_width / 2.0).abs() < f32::EPSILON);
            assert!(right - left <= SEARCH_WIDTH);
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn window_controls_map_to_native_areas_and_accessible_labels() {
        let minimize = window_control_config(WindowControlKind::Minimize, false);
        assert_eq!(minimize.area, WindowControlArea::Min);
        assert_eq!(minimize.icon, IconName::WindowMinimize);
        assert_eq!(minimize.label, "Minimize window");
        assert!(!minimize.danger);

        let maximize = window_control_config(WindowControlKind::Maximize, false);
        assert_eq!(maximize.area, WindowControlArea::Max);
        assert_eq!(maximize.icon, IconName::WindowMaximize);
        assert_eq!(maximize.label, "Maximize window");

        let restore = window_control_config(WindowControlKind::Maximize, true);
        assert_eq!(restore.icon, IconName::WindowRestore);
        assert_eq!(restore.label, "Restore window");

        let close = window_control_config(WindowControlKind::Close, false);
        assert_eq!(close.area, WindowControlArea::Close);
        assert_eq!(close.icon, IconName::WindowClose);
        assert_eq!(close.label, "Close window");
        assert!(close.danger);
    }

    #[test]
    fn speed_chart_uses_only_the_latest_bounded_window() {
        let history = (0..=SPEED_CHART_SAMPLES)
            .map(|index| SpeedSampleView {
                download_rate: index as u64,
                upload_rate: 0,
            })
            .collect::<Vec<_>>();

        let visible = speed_chart_window(&history);
        assert_eq!(visible.len(), SPEED_CHART_SAMPLES);
        assert_eq!(visible.first().map(|sample| sample.download_rate), Some(1));
        assert_eq!(
            visible.last().map(|sample| sample.download_rate),
            Some(SPEED_CHART_SAMPLES as u64)
        );
    }

    #[gpui::test]
    fn local_engine_health_surfaces_recovery_and_terminal_failure(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

        view.update(cx, |shell, cx| {
            shell.set_engine_health(EngineHealthView::Running { restarts: 0 }, cx);
            shell.set_engine_health(EngineHealthView::Restarting { attempt: 1 }, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.engine_health.label(), "Local engine restarting");
            assert!(
                shell.status_notice.is_none(),
                "persistent restart state belongs in the status bar"
            );
        });

        view.update(cx, |shell, cx| {
            shell.set_engine_health(EngineHealthView::Running { restarts: 1 }, cx);
        });
        view.read_with(cx, |shell, _| {
            let notice = shell.status_notice.as_ref().expect("recovery notice");
            assert!(!notice.is_error);
            assert_eq!(
                notice.message,
                "Local aria2 recovered after 1 restart attempt."
            );
        });

        view.update(cx, |shell, cx| {
            shell.set_engine_health(
                EngineHealthView::Failed {
                    summary: "restart budget exhausted".into(),
                },
                cx,
            );
        });
        view.read_with(cx, |shell, _| {
            let notice = shell.status_notice.as_ref().expect("failure notice");
            assert!(notice.is_error);
            assert_eq!(shell.engine_health.label(), "Local engine stopped");
        });
    }

    #[gpui::test]
    fn ten_thousand_tasks_render_only_a_viewport_window(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(10_000);
            shell
        });

        view.read_with(cx, |shell, _| {
            let rendered = shell.rendered_range();
            assert!(!rendered.is_empty());
            assert!(rendered.len() < 64, "rendered {} rows", rendered.len());
            assert_eq!(shell.snapshot.tasks.len(), 10_000);
        });
    }

    #[gpui::test]
    fn selection_survives_filtered_snapshots_for_the_same_profile(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(3);
            shell.selected = Some(shell.snapshot.tasks[1].identity.clone());
            shell
        });
        let selected = view.read_with(cx, |shell, _| shell.selected.clone());

        view.update(cx, |shell, cx| {
            let mut filtered = snapshot(1);
            filtered.tasks[0] = task(2);
            shell.set_snapshot(filtered, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.selected, selected);
        });
    }

    #[gpui::test]
    fn task_selection_supports_toggle_range_and_visible_select_all(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(5);
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.select_at_with_modifiers(1, false, false, window, cx);
            shell.select_at_with_modifiers(3, true, false, window, cx);
        });
        view.read_with(cx, |shell, _| {
            let selected = [1, 2, 3]
                .into_iter()
                .map(|index| task(index).identity)
                .collect::<HashSet<_>>();
            assert_eq!(shell.selected_tasks, selected);
            assert_eq!(shell.range_anchor, Some(task(1).identity));
            assert_eq!(shell.selected, Some(task(3).identity));
        });

        view.update_in(cx, |shell, window, cx| {
            shell.select_at_with_modifiers(2, false, true, window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(!shell.selected_tasks.contains(&task(2).identity));
            assert_eq!(shell.selected_tasks.len(), 2);
        });

        view.update_in(cx, |shell, window, cx| {
            shell.toggle_select_all(window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.selected_tasks.len(), 5);
        });
        view.update_in(cx, |shell, window, cx| {
            shell.toggle_select_all(window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(shell.selected_tasks.is_empty());
            assert!(shell.range_anchor.is_none());
        });
    }

    #[gpui::test]
    fn select_all_shortcut_selects_the_current_loaded_query(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(4);
            window.focus(&shell.focus_handle, cx);
            shell
        });

        cx.simulate_keystrokes("secondary-a");
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.visible_selected_task_count(), 4);
            assert_eq!(shell.selected_task_count(), 4);
            assert_eq!(shell.selected, Some(task(0).identity));
        });
    }

    #[gpui::test]
    fn context_menu_opens_from_right_click_and_preserves_multi_selection(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(3);
            shell.selected = Some(task(0).identity);
            shell.selected_tasks = HashSet::from([task(0).identity, task(1).identity]);
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.open_task_context_menu(
                1,
                Point {
                    x: px(120.0),
                    y: px(80.0),
                },
                window,
                cx,
            );
            assert!(shell.context_menu.is_some());
            assert_eq!(shell.selected.as_ref(), Some(&task(1).identity));
            assert_eq!(shell.selected_tasks.len(), 2);
            assert!(shell.selected_tasks.contains(&task(0).identity));
            assert!(shell.selected_tasks.contains(&task(1).identity));

            // Multi-select still targets the right-clicked row for copy.
            let target = shell.context_menu_task_view().expect("menu target");
            shell.copy_task_gid(&target, cx);
            assert_eq!(
                cx.read_from_clipboard().and_then(|item| item.text()),
                Some(task(1).identity.gid.clone()),
            );

            shell.close_task_context_menu(cx);
            assert!(shell.context_menu.is_none());
            // Closing the menu does not drop the multi-selection.
            assert_eq!(shell.selected_tasks.len(), 2);
        });
    }

    #[gpui::test]
    fn queue_priority_keyboard_actions_are_wired_on_the_workspace(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(3);
            // Active tasks so queue moves are status-eligible; query stays the
            // authoritative ascending queue (All, no search, Queue/asc).
            for task in &mut shell.snapshot.tasks {
                task.status = TaskStatusView::Active;
            }
            shell.selected = Some(task(1).identity);
            shell.selected_tasks = HashSet::from([task(1).identity]);
            window.focus(&shell.focus_handle, cx);
            shell
        });

        cx.simulate_keystrokes("cmd-shift-up");

        view.read_with(cx, |shell, _| {
            assert!(
                shell.pending_task_command.as_ref().is_some_and(|pending| {
                    matches!(pending.command, TaskCommandView::MoveUpInQueue)
                        && pending.identity == task(1).identity
                }),
                "Cmd+Shift+Up should submit MoveUpInQueue for the focused task"
            );
        });
    }

    #[gpui::test]
    fn visible_selection_counts_and_header_toggle_exclude_hidden_tasks(cx: &mut TestAppContext) {
        let hidden = task(99).identity;
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(2);
            shell.selected = Some(task(0).identity);
            shell.selected_tasks = HashSet::from([task(0).identity, hidden.clone()]);
            shell
        });

        view.read_with(cx, |shell, _| {
            assert_eq!(shell.selected_task_count(), 2);
            assert_eq!(shell.visible_selected_task_count(), 1);
        });
        view.update_in(cx, |shell, window, cx| {
            shell.toggle_select_all(window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.visible_selected_task_count(), 2);
            assert_eq!(shell.selected_task_count(), 3);
        });
        view.update_in(cx, |shell, window, cx| {
            shell.toggle_select_all(window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.visible_selected_task_count(), 0);
            assert_eq!(shell.selected_tasks, HashSet::from([hidden]));
        });
    }

    #[gpui::test]
    fn query_change_clears_the_query_scoped_task_selection(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(3);
            shell.select_at_with_modifiers(0, false, false, window, cx);
            shell.select_at_with_modifiers(1, false, true, window, cx);
            shell
        });
        view.update_in(cx, |shell, window, cx| {
            shell.set_filter(WorkspaceFilter::Completed, window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(shell.selected_tasks.is_empty());
            assert!(shell.range_anchor.is_none());
        });
    }

    #[gpui::test]
    fn batch_partial_result_retains_only_failed_source_tasks(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(3);
            for task in &mut shell.snapshot.tasks {
                task.status = TaskStatusView::Active;
            }
            shell.select_at_with_modifiers(0, false, false, window, cx);
            shell.select_at_with_modifiers(1, false, true, window, cx);
            shell.begin_batch_task_command(BatchTaskCommandView::Pause, cx);
            shell
        });
        let result = view.read_with(cx, |shell, _| {
            let pending = shell
                .pending_batch_command
                .as_ref()
                .expect("batch command pending");
            assert_eq!(pending.identities, vec![task(0).identity, task(1).identity]);
            BatchTaskCommandResultView {
                request_id: pending.request_id,
                session: pending.session.clone(),
                identities: pending.identities.clone(),
                command: pending.command,
                outcome: BatchCommandOutcomeView::PartialSuccess {
                    succeeded: vec![task(0).identity],
                    failed: vec![BatchTaskFailureView {
                        identity: Some(task(1).identity),
                        error: OperationErrorView {
                            code: "rpc.command_rejected".into(),
                            summary: "aria2 rejected pause".into(),
                            retryable: false,
                        },
                    }],
                },
            }
        });
        view.update_in(cx, |shell, window, cx| {
            shell.set_batch_task_command_result(result, window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(shell.pending_batch_command.is_none());
            assert_eq!(shell.selected_tasks, HashSet::from([task(1).identity]));
            assert_eq!(
                shell
                    .batch_failure_details
                    .as_ref()
                    .map(|details| details.failures.len()),
                Some(1)
            );
            assert!(
                shell
                    .status_notice
                    .as_ref()
                    .is_some_and(|notice| notice.is_error)
            );
        });
    }

    #[gpui::test]
    fn hidden_selection_arrows_start_at_the_visible_edges(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(3);
            shell.selected = Some(task(99).identity);
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.select_next(&SelectNextTask, window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.selected, Some(task(0).identity));
        });

        view.update(cx, |shell, _| {
            shell.selected = Some(task(99).identity);
        });
        view.update_in(cx, |shell, window, cx| {
            shell.select_previous(&SelectPreviousTask, window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.selected, Some(task(2).identity));
        });
    }

    #[gpui::test]
    fn magnet_successor_relationship_preserves_selected_task(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            let mut previous = snapshot(1);
            previous.tasks[0].followed_by = vec![format!("{:016x}", 1)];
            shell.snapshot = previous;
            shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
            shell
        });

        view.update_in(cx, |shell, _window, cx| {
            let mut next = snapshot(1);
            next.tasks[0] = task(1);
            shell.set_snapshot(next, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(
                shell.selected.as_ref().map(|task| task.gid.as_str()),
                Some("0000000000000001")
            );
        });
    }

    #[gpui::test]
    fn magnet_successor_migrates_nonfocused_selection_anchor_and_details(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            let mut previous = snapshot(3);
            previous.tasks[0].followed_by = vec![format!("{:016x}", 3)];
            let parent = previous.tasks[0].clone();
            let focused = previous.tasks[1].identity.clone();
            shell.snapshot = previous;
            shell.selected = Some(focused.clone());
            shell.selected_tasks = HashSet::from([parent.identity.clone(), focused]);
            shell.range_anchor = Some(parent.identity.clone());
            shell.open_details_for(parent, cx);
            shell
        });

        view.update_in(cx, |shell, _window, cx| {
            let mut next = snapshot(3);
            next.tasks[0] = task(3);
            next.tasks[0].belongs_to = Some(format!("{:016x}", 0));
            shell.set_snapshot(next, cx);
        });
        view.read_with(cx, |shell, _| {
            let parent = task(0).identity;
            let successor = task(3).identity;
            assert_eq!(shell.selected, Some(task(1).identity));
            assert_eq!(shell.selected_tasks.len(), 2);
            assert!(!shell.selected_tasks.contains(&parent));
            assert!(shell.selected_tasks.contains(&successor));
            assert_eq!(shell.range_anchor, Some(successor.clone()));
            assert_eq!(
                shell
                    .details_drawer
                    .as_ref()
                    .map(|drawer| drawer.identity.clone()),
                Some(successor)
            );
        });
    }

    #[gpui::test]
    fn add_download_submission_is_single_flight(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.add_dialog.open = true;
            shell
        });

        view.update(cx, |shell, cx| {
            shell.add_input.update(cx, |input, cx| {
                input.set_text("https://example.com/archive.bin", cx);
            });
            shell.submit_add_download(cx);
            let first = shell
                .add_dialog
                .pending
                .as_ref()
                .expect("first submit must become pending")
                .request_id;
            shell.submit_add_download(cx);
            assert_eq!(
                shell
                    .add_dialog
                    .pending
                    .as_ref()
                    .expect("second submit must retain pending request")
                    .request_id,
                first
            );
            assert_eq!(shell.next_request_id, first.get() + 1);
        });
    }

    #[gpui::test]
    fn task_command_submission_is_single_flight(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.snapshot.tasks[0].status = TaskStatusView::Active;
            shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
            shell
        });

        view.update(cx, |shell, cx| {
            shell.begin_task_command(TaskCommandView::Pause, cx);
            let first = shell
                .pending_task_command
                .as_ref()
                .expect("first command must become pending")
                .request_id;
            shell.begin_task_command(TaskCommandView::Pause, cx);
            assert_eq!(
                shell
                    .pending_task_command
                    .as_ref()
                    .expect("duplicate command must retain the first request")
                    .request_id,
                first
            );
            assert_eq!(shell.next_request_id, first.get() + 1);
        });
    }

    #[gpui::test]
    fn queue_reordering_is_authoritative_only_for_the_unfiltered_ascending_queue(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));

        view.read_with(cx, |shell, _| {
            assert!(
                shell.queue_reordering_available(),
                "default query is All / no search / Queue / Ascending"
            );
        });

        view.update(cx, |shell, cx| {
            shell.set_sort_key(WorkspaceSortKey::Progress, cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(
                !shell.queue_reordering_available(),
                "a value sort is not an authoritative queue position"
            );
        });

        view.update(cx, |shell, cx| {
            shell.set_sort_key(WorkspaceSortKey::Queue, cx);
            shell.set_sort_direction(WorkspaceSortDirection::Descending, cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(
                !shell.queue_reordering_available(),
                "a reversed queue is not an authoritative position"
            );
        });

        view.update_in(cx, |shell, window, cx| {
            shell.set_sort_direction(WorkspaceSortDirection::Ascending, cx);
            shell.set_filter(WorkspaceFilter::Active, window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(
                !shell.queue_reordering_available(),
                "a filtered projection hides the global queue position"
            );
        });

        view.update_in(cx, |shell, window, cx| {
            shell.set_filter(WorkspaceFilter::All, window, cx);
            shell.search_input.update(cx, |input, cx| {
                input.set_text("archive", cx);
            });
        });
        view.read_with(cx, |shell, _| {
            assert!(
                !shell.queue_reordering_available(),
                "a searched projection hides the global queue position"
            );
        });
    }

    #[gpui::test]
    fn load_more_stopped_history_is_single_flight_and_gated(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(2);
            shell.snapshot.stopped_history = crate::StoppedHistoryView {
                loaded: 2,
                total: Some(5),
                can_load_more: true,
            };
            shell
        });
        let events = Arc::new(std::sync::Mutex::new(0usize));
        let sink = events.clone();
        let _subscription = view.update(cx, |_, cx| {
            cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| {
                if matches!(event, AppShellEvent::LoadMoreStoppedRequested) {
                    *sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) += 1;
                }
            })
        });

        // Stale or disconnected snapshots must not request another page.
        view.update(cx, |shell, cx| {
            shell.snapshot.stale = true;
            shell.request_load_more_stopped(cx);
            assert!(!shell.pending_load_more_stopped);
            shell.snapshot.stale = false;
            shell.snapshot.connection = ConnectionView::Disconnected;
            shell.request_load_more_stopped(cx);
            assert!(!shell.pending_load_more_stopped);
            shell.snapshot.connection = ConnectionView::Connected;
        });

        view.update(cx, |shell, cx| {
            shell.request_load_more_stopped(cx);
            assert!(shell.pending_load_more_stopped);
            // Single-flight: a second click while pending must not re-emit.
            shell.request_load_more_stopped(cx);
        });
        assert_eq!(
            *events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            1,
            "only one LoadMoreStoppedRequested event while pending"
        );

        view.update(cx, |shell, cx| {
            shell.set_load_more_stopped_result(
                true,
                Some("Loaded more history (4 of 5).".into()),
                cx,
            );
            assert!(!shell.pending_load_more_stopped);
            assert_eq!(
                shell
                    .status_notice
                    .as_ref()
                    .map(|notice| notice.message.as_str()),
                Some("Loaded more history (4 of 5).")
            );
        });
    }

    #[gpui::test]
    fn changing_the_sort_preserves_selection_and_emits_the_query(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(3);
            shell
        });
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = events.clone();
        let _subscription = view.update(cx, |_, cx| {
            cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| {
                if let AppShellEvent::QueryChanged(query) = event {
                    sink.lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(query.clone());
                }
            })
        });

        let selected = view.update(cx, |shell, _| {
            let identity = shell.snapshot.tasks[1].identity.clone();
            shell.selected = Some(identity.clone());
            shell.selected_tasks.insert(identity.clone());
            identity
        });

        view.update(cx, |shell, cx| {
            shell.set_sort_key(WorkspaceSortKey::Size, cx);
        });

        view.read_with(cx, |shell, _| {
            assert_eq!(shell.query.sort_key, WorkspaceSortKey::Size);
            assert!(
                shell.selected_tasks.contains(&selected),
                "sort changes must preserve identity-based selection (D-014)"
            );
            assert_eq!(shell.selected.as_ref(), Some(&selected));
        });
        let captured = events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(
            captured
                .iter()
                .any(|query| query.sort_key == WorkspaceSortKey::Size),
            "changing the sort key must emit a QueryChanged event"
        );
    }

    #[gpui::test]
    fn queue_priority_command_is_blocked_outside_the_authoritative_queue(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(2);
            shell.snapshot.tasks[0].status = TaskStatusView::Waiting;
            shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
            shell
        });

        // Reversed queue: priority movement is not authoritative and is rejected.
        view.update(cx, |shell, cx| {
            shell.set_sort_direction(WorkspaceSortDirection::Descending, cx);
            shell.begin_task_command(TaskCommandView::MoveUpInQueue, cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(
                shell.pending_task_command.is_none(),
                "queue movement must not start while the query is reversed"
            );
            assert!(
                shell
                    .status_notice
                    .as_ref()
                    .is_some_and(|notice| notice.is_error)
            );
        });

        // Restore the authoritative queue: the command now becomes pending.
        view.update(cx, |shell, cx| {
            shell.set_sort_direction(WorkspaceSortDirection::Ascending, cx);
            shell.begin_task_command(TaskCommandView::MoveToQueueTop, cx);
        });
        view.read_with(cx, |shell, _| {
            let pending = shell
                .pending_task_command
                .as_ref()
                .expect("queue movement must be pending in the authoritative queue");
            assert_eq!(pending.command, TaskCommandView::MoveToQueueTop);
        });
    }

    #[gpui::test]
    fn global_pause_all_becomes_pending_and_emits_the_engine_wide_command(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(2);
            shell
        });
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = events.clone();
        let _subscription = view.update(cx, |_, cx| {
            cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| {
                if let AppShellEvent::GlobalTaskCommandRequested(request) = event {
                    sink.lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(request.command);
                }
            })
        });

        view.update(cx, |shell, cx| {
            shell.begin_global_task_command(GlobalTaskCommandView::PauseAll, cx);
        });

        view.read_with(cx, |shell, _| {
            let pending = shell
                .pending_global_task_command
                .as_ref()
                .expect("pause-all must become pending");
            assert_eq!(pending.command, GlobalTaskCommandView::PauseAll);
        });
        let captured = events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(captured.as_slice(), &[GlobalTaskCommandView::PauseAll]);
    }

    #[test]
    fn add_download_input_parses_trimmed_non_empty_lines_with_source_positions() {
        let sources = parse_add_download_sources(
            "  https://example.test/one  \r\n\r\nmagnet:?xt=urn:btih:abc\n",
        );

        assert_eq!(
            sources,
            vec![
                AddDownloadSourceView::Uri {
                    line: 1,
                    uri: "https://example.test/one".into(),
                },
                AddDownloadSourceView::Uri {
                    line: 3,
                    uri: "magnet:?xt=urn:btih:abc".into(),
                },
            ]
        );
    }

    #[gpui::test]
    fn metadata_paths_are_classified_deduplicated_switchable_and_removable(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.add_metadata_paths(
                vec![
                    PathBuf::from("sample.TORRENT"),
                    PathBuf::from("sample.TORRENT"),
                    PathBuf::from("bundle.meta4"),
                    PathBuf::from("notes.txt"),
                ],
                window,
                cx,
            );
            assert!(shell.add_dialog.open);
            assert_eq!(
                shell.add_dialog.input_mode,
                AddDownloadInputModeView::MetadataFiles
            );
            assert_eq!(shell.add_dialog.mode, AddDownloadModeView::SeparateTasks);
            assert_eq!(
                shell.add_dialog.file_conflict,
                FileConflictPolicyView::Reject
            );
            assert!(shell.add_dialog.metadata_files.is_empty());
            let pending = shell
                .add_dialog
                .preview_pending
                .as_ref()
                .expect("metadata preview must be pending");
            assert_eq!(
                pending.paths,
                vec![
                    PathBuf::from("sample.TORRENT"),
                    PathBuf::from("bundle.meta4")
                ]
            );
            let request_id = pending.request_id;
            assert!(shell.add_dialog.error.is_some());

            shell.set_add_download_metadata_preview_result(
                AddDownloadMetadataPreviewResultView {
                    request_id,
                    items: vec![
                        AddDownloadMetadataPreviewItemView {
                            path: PathBuf::from("sample.TORRENT"),
                            outcome: AddDownloadMetadataPreviewOutcomeView::Ready(
                                metadata_preview(
                                    "sample.TORRENT",
                                    AddDownloadMetadataKindView::Torrent,
                                    2,
                                ),
                            ),
                        },
                        AddDownloadMetadataPreviewItemView {
                            path: PathBuf::from("bundle.meta4"),
                            outcome: AddDownloadMetadataPreviewOutcomeView::Ready(
                                metadata_preview(
                                    "bundle.meta4",
                                    AddDownloadMetadataKindView::Metalink,
                                    1,
                                ),
                            ),
                        },
                    ],
                },
                cx,
            );
            assert_eq!(shell.add_dialog.metadata_files.len(), 2);
            assert_eq!(
                shell.add_dialog.metadata_files[0].selected_file_indices,
                vec![1, 2]
            );
            shell.toggle_metadata_file_entry(0, 2, cx);
            assert_eq!(
                shell.add_dialog.metadata_files[0].selected_file_indices,
                vec![1]
            );
            shell.toggle_all_metadata_file_entries(0, cx);
            assert_eq!(
                shell.add_dialog.metadata_files[0].selected_file_indices,
                vec![1, 2]
            );

            shell.set_add_input_mode(AddDownloadInputModeView::Links, cx);
            assert_eq!(shell.add_dialog.input_mode, AddDownloadInputModeView::Links);
            shell.remove_metadata_file(0, cx);
            assert_eq!(shell.add_dialog.metadata_files.len(), 1);
        });
    }

    #[gpui::test]
    fn metadata_preview_keeps_successes_reports_failures_and_ignores_stale_results(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell
        });
        let request_id = view.update_in(cx, |shell, window, cx| {
            shell.add_metadata_paths(
                vec![PathBuf::from("one.torrent"), PathBuf::from("two.meta4")],
                window,
                cx,
            );
            shell
                .add_dialog
                .preview_pending
                .as_ref()
                .expect("metadata preview must be pending")
                .request_id
        });

        view.update(cx, |shell, cx| {
            shell.set_add_download_metadata_preview_result(
                AddDownloadMetadataPreviewResultView {
                    request_id: RequestId::from_u64(request_id.get() + 1),
                    items: vec![
                        AddDownloadMetadataPreviewItemView {
                            path: PathBuf::from("one.torrent"),
                            outcome: AddDownloadMetadataPreviewOutcomeView::Ready(
                                metadata_preview(
                                    "one.torrent",
                                    AddDownloadMetadataKindView::Torrent,
                                    2,
                                ),
                            ),
                        },
                        AddDownloadMetadataPreviewItemView {
                            path: PathBuf::from("two.meta4"),
                            outcome: AddDownloadMetadataPreviewOutcomeView::Failed(
                                OperationErrorView {
                                    code: "validation.invalid_request".into(),
                                    summary: "bad metadata".into(),
                                    retryable: false,
                                },
                            ),
                        },
                    ],
                },
                cx,
            );
        });
        view.read_with(cx, |shell, _| {
            assert!(shell.add_dialog.metadata_files.is_empty());
            assert_eq!(
                shell
                    .add_dialog
                    .preview_pending
                    .as_ref()
                    .map(|pending| pending.request_id),
                Some(request_id)
            );
        });

        view.update(cx, |shell, cx| {
            shell.set_add_download_metadata_preview_result(
                AddDownloadMetadataPreviewResultView {
                    request_id,
                    items: vec![
                        AddDownloadMetadataPreviewItemView {
                            path: PathBuf::from("one.torrent"),
                            outcome: AddDownloadMetadataPreviewOutcomeView::Ready(
                                metadata_preview(
                                    "one.torrent",
                                    AddDownloadMetadataKindView::Torrent,
                                    2,
                                ),
                            ),
                        },
                        AddDownloadMetadataPreviewItemView {
                            path: PathBuf::from("two.meta4"),
                            outcome: AddDownloadMetadataPreviewOutcomeView::Failed(
                                OperationErrorView {
                                    code: "validation.invalid_request".into(),
                                    summary: "bad metadata".into(),
                                    retryable: false,
                                },
                            ),
                        },
                    ],
                },
                cx,
            );
        });
        view.read_with(cx, |shell, _| {
            assert!(shell.add_dialog.preview_pending.is_none());
            assert_eq!(shell.add_dialog.metadata_files.len(), 1);
            assert_eq!(
                shell.add_dialog.metadata_files[0].selected_file_indices,
                vec![1, 2]
            );
            assert!(
                shell
                    .add_dialog
                    .error
                    .as_ref()
                    .is_some_and(|error| error.summary.contains("bad metadata"))
            );
        });
    }

    #[gpui::test]
    fn metadata_submit_rejects_zero_selection_and_sums_selected_known_sizes(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.add_dialog.open = true;
            shell.add_dialog.input_mode = AddDownloadInputModeView::MetadataFiles;
            shell.add_dialog.metadata_files = vec![metadata_preview(
                "one.torrent",
                AddDownloadMetadataKindView::Torrent,
                3,
            )];
            shell.add_dialog.metadata_files[0].files[2].length = None;
            shell
        });

        view.update(cx, |shell, cx| {
            assert_eq!(
                selected_metadata_known_bytes(&shell.add_dialog.metadata_files),
                Some(300)
            );
            shell.toggle_all_metadata_file_entries(0, cx);
            shell.submit_add_download(cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(shell.add_dialog.pending.is_none());
            assert!(
                shell
                    .add_dialog
                    .error
                    .as_ref()
                    .is_some_and(|error| error.summary.contains("Select at least one file"))
            );
        });
    }

    #[test]
    fn metadata_drop_is_disabled_while_an_add_request_is_pending() {
        let paths = [PathBuf::from("sample.torrent")];

        assert!(can_accept_metadata_drop(true, &paths));
        assert!(!can_accept_metadata_drop(false, &paths));
    }

    #[gpui::test]
    fn add_download_advanced_options_toggle_and_collect_secrets(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.open_add_download(&OpenAddDownload, window, cx);
            assert!(!shell.add_dialog.advanced_open);
            shell.toggle_add_advanced(cx);
            assert!(shell.add_dialog.advanced_open);
            shell.add_referer_input.update(cx, |input, cx| {
                input.set_text("https://example.test/ref", cx);
            });
            shell.add_user_agent_input.update(cx, |input, cx| {
                input.set_text("AriaDeck-Test/1.0", cx);
            });
            shell.add_headers_input.update(cx, |input, cx| {
                input.set_text("X-Token: one\nAccept: */*", cx);
            });
            shell.add_cookie_input.update(cx, |input, cx| {
                input.set_text("session=secret-cookie", cx);
            });
            shell.add_http_user_input.update(cx, |input, cx| {
                input.set_text("alice", cx);
            });
            shell.add_http_passwd_input.update(cx, |input, cx| {
                input.set_text("s3cret", cx);
            });
            shell.add_checksum_input.update(cx, |input, cx| {
                input.set_text(format!("sha-256={}", "ab".repeat(32)), cx);
            });
        });

        view.read_with(cx, |shell, cx| {
            let advanced = shell.collect_add_advanced_options(cx);
            assert_eq!(advanced.referer, "https://example.test/ref");
            assert_eq!(advanced.user_agent, "AriaDeck-Test/1.0");
            assert!(advanced.headers.contains("X-Token: one"));
            assert_eq!(
                advanced
                    .cookie
                    .as_ref()
                    .map(|value| value.clone().into_inner()),
                Some("session=secret-cookie".into())
            );
            assert_eq!(advanced.http_user, "alice");
            assert_eq!(
                advanced
                    .http_passwd
                    .as_ref()
                    .map(|value| value.clone().into_inner()),
                Some("s3cret".into())
            );
            assert!(advanced.checksum.starts_with("sha-256="));
            let debug = format!("{advanced:?}");
            assert!(!debug.contains("s3cret"));
            assert!(!debug.contains("secret-cookie"));
            assert!(shell.add_cookie_input.read(cx).is_secure());
            assert!(shell.add_http_passwd_input.read(cx).is_secure());
        });
    }

    #[gpui::test]
    fn add_download_dialog_accepts_keyboard_input(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.open_add_download(&OpenAddDownload, window, cx);
        });
        cx.simulate_input("https://example.com/file");

        view.read_with(cx, |shell, cx| {
            assert_eq!(shell.add_input.read(cx).text(), "https://example.com/file");
        });
    }

    #[gpui::test]
    fn add_download_dialog_input_can_be_clicked_before_typing(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.open_add_download(&OpenAddDownload, window, cx);
        });
        let input_bounds = view.read_with(cx, |shell, cx| {
            shell
                .add_input
                .read(cx)
                .text_bounds()
                .expect("add-download input must be painted")
        });
        view.update_in(cx, |shell, window, cx| {
            window.focus(&shell.search_input.focus_handle(cx), cx);
        });
        cx.simulate_click(
            point(input_bounds.left() - px(16.0), input_bounds.center().y),
            Default::default(),
        );
        cx.simulate_input("https://example.com/file");

        view.read_with(cx, |shell, cx| {
            assert_eq!(shell.add_input.read(cx).text(), "https://example.com/file");
        });
    }

    #[gpui::test]
    fn add_download_dialog_supports_standard_clipboard_shortcuts(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.open_add_download(&OpenAddDownload, window, cx);
            shell.add_input.update(cx, |input, cx| {
                input.set_text("https://example.com/file", cx);
            });
        });
        cx.simulate_keystrokes("secondary-a secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("https://example.com/file".to_owned())
        );

        cx.write_to_clipboard(ClipboardItem::new_string(
            "magnet:?xt=urn:btih:test".to_owned(),
        ));
        cx.simulate_keystrokes("secondary-v");
        view.read_with(cx, |shell, cx| {
            assert_eq!(shell.add_input.read(cx).text(), "magnet:?xt=urn:btih:test");
        });

        cx.simulate_keystrokes("secondary-a secondary-x");
        view.read_with(cx, |shell, cx| {
            assert!(shell.add_input.read(cx).text().is_empty());
        });
        assert_eq!(
            cx.read_from_clipboard().and_then(|item| item.text()),
            Some("magnet:?xt=urn:btih:test".to_owned())
        );
    }

    #[gpui::test]
    fn add_download_input_preserves_pasted_lines_and_shift_enter(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.open_add_download(&OpenAddDownload, window, cx);
            shell
        });

        cx.write_to_clipboard(ClipboardItem::new_string(
            "https://example.test/one\r\nhttps://example.test/two".into(),
        ));
        cx.simulate_keystrokes("secondary-v shift-enter");
        cx.simulate_input("magnet:?xt=urn:btih:abc");

        view.read_with(cx, |shell, cx| {
            assert_eq!(
                shell.add_input.read(cx).text(),
                "https://example.test/one\nhttps://example.test/two\nmagnet:?xt=urn:btih:abc"
            );
            assert_eq!(
                parse_add_download_sources(shell.add_input.read(cx).text()).len(),
                3
            );
        });
    }

    #[gpui::test]
    fn partial_add_result_keeps_only_sources_that_are_safe_to_retry(cx: &mut TestAppContext) {
        let accepted = task(10).identity;
        let accepted_second = task(11).identity;
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.add_dialog.open = true;
            shell.add_input.update(cx, |input, cx| {
                input.set_text(
                    "https://example.test/accepted\nhttps://example.test/retry\nhttps://example.test/unknown",
                    cx,
                );
            });
            shell.submit_add_download(cx);
            shell
        });
        let (request_id, session) = view.read_with(cx, |shell, _| {
            let pending = shell.add_dialog.pending.as_ref().expect("add pending");
            (pending.request_id, pending.session.clone())
        });

        view.update_in(cx, |shell, window, cx| {
            shell.set_add_download_result(
                AddDownloadResultView {
                    request_id,
                    session,
                    items: vec![
                        AddDownloadItemResultView {
                            sources: vec![AddDownloadSourceView::Uri {
                                line: 1,
                                uri: "https://example.test/accepted".into(),
                            }],
                            existing_task: None,
                            outcome: CommandOutcomeView::Success {
                                tasks: vec![accepted.clone(), accepted_second.clone()],
                            },
                        },
                        AddDownloadItemResultView {
                            sources: vec![AddDownloadSourceView::Uri {
                                line: 2,
                                uri: "https://example.test/retry".into(),
                            }],
                            existing_task: None,
                            outcome: CommandOutcomeView::Failure(OperationErrorView {
                                code: "rpc.add_not_observed".into(),
                                summary: "Safe to retry".into(),
                                retryable: true,
                            }),
                        },
                        AddDownloadItemResultView {
                            sources: vec![AddDownloadSourceView::Uri {
                                line: 3,
                                uri: "https://example.test/unknown".into(),
                            }],
                            existing_task: None,
                            outcome: CommandOutcomeView::Failure(OperationErrorView {
                                code: "rpc.command_outcome_unknown".into(),
                                summary: "Still unknown".into(),
                                retryable: false,
                            }),
                        },
                    ],
                },
                window,
                cx,
            );
        });
        view.read_with(cx, |shell, cx| {
            assert!(shell.add_dialog.open);
            assert_eq!(shell.add_dialog.results.len(), 3);
            assert_eq!(
                shell.add_input.read(cx).text(),
                "https://example.test/retry"
            );
            assert_eq!(
                shell.selected_tasks,
                HashSet::from([accepted.clone(), accepted_second])
            );
            assert_eq!(shell.selected.as_ref(), Some(&accepted));
        });
    }

    #[gpui::test]
    fn duplicate_add_result_focuses_the_existing_task_and_closes_the_dialog(
        cx: &mut TestAppContext,
    ) {
        let existing = task(0).identity;
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.add_dialog.open = true;
            shell.add_input.update(cx, |input, cx| {
                input.set_text("https://example.test/existing", cx);
            });
            shell.submit_add_download(cx);
            shell
        });
        let (request_id, session) = view.read_with(cx, |shell, _| {
            let pending = shell.add_dialog.pending.as_ref().expect("add pending");
            (pending.request_id, pending.session.clone())
        });

        view.update_in(cx, |shell, window, cx| {
            shell.set_add_download_result(
                AddDownloadResultView {
                    request_id,
                    session,
                    items: vec![AddDownloadItemResultView {
                        sources: vec![AddDownloadSourceView::Uri {
                            line: 1,
                            uri: "https://example.test/existing".into(),
                        }],
                        existing_task: Some(existing.clone()),
                        outcome: CommandOutcomeView::Failure(OperationErrorView {
                            code: "validation.duplicate_task".into(),
                            summary: "Already present".into(),
                            retryable: false,
                        }),
                    }],
                },
                window,
                cx,
            );
        });
        view.read_with(cx, |shell, _| {
            assert!(!shell.add_dialog.open);
            assert_eq!(shell.selected.as_ref(), Some(&existing));
            assert_eq!(shell.selected_tasks, HashSet::from([existing.clone()]));
        });
    }

    #[gpui::test]
    fn successful_retry_selects_the_new_task_identity(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.snapshot.tasks[0].status = TaskStatusView::Failed;
            shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
            shell
        });
        let (request_id, session, old_identity) = view.update(cx, |shell, cx| {
            shell.begin_task_command(TaskCommandView::Retry, cx);
            let pending = shell
                .pending_task_command
                .as_ref()
                .expect("retry must become pending");
            (
                pending.request_id,
                pending.session.clone(),
                pending.identity.clone(),
            )
        });
        let new_identity = TaskIdentity {
            profile_id: old_identity.profile_id.clone(),
            gid: "0000000000000063".into(),
        };

        view.update_in(cx, |shell, window, cx| {
            shell.set_task_command_result(
                TaskCommandResultView {
                    request_id,
                    session,
                    identity: old_identity,
                    command: TaskCommandView::Retry,
                    outcome: CommandOutcomeView::Success {
                        tasks: vec![new_identity.clone()],
                    },
                },
                window,
                cx,
            );
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.selected.as_ref(), Some(&new_identity));
            assert!(shell.pending_task_command.is_none());
            assert!(shell.details_drawer.is_none());
        });
    }

    #[gpui::test]
    fn output_name_dialog_accepts_only_non_terminal_direct_uri_tasks(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.open_task_output_name(window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(shell.output_name_dialog.is_none());
        });

        view.update_in(cx, |shell, window, cx| {
            shell.snapshot.tasks[0].status = TaskStatusView::Waiting;
            shell.snapshot.tasks[0].source_kind = TaskSourceKindView::Magnet;
            shell.open_task_output_name(window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(shell.output_name_dialog.is_none());
        });

        view.update_in(cx, |shell, window, cx| {
            shell.snapshot.tasks[0].source_kind = TaskSourceKindView::DirectUri;
            shell.open_task_output_name(window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(shell.output_name_dialog.is_some());
        });
    }

    #[gpui::test]
    fn output_name_dialog_validates_and_submits_the_exact_filename(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.snapshot.tasks[0].status = TaskStatusView::Waiting;
            shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
            shell.open_task_output_name(window, cx);
            shell
        });

        view.update(cx, |shell, cx| {
            shell.output_name_input.update(cx, |input, cx| {
                input.set_text("folder/archive.iso", cx);
            });
        });
        view.update(cx, |shell, cx| {
            shell.submit_task_output_name(cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(shell.pending_task_command.is_none());
            assert!(
                shell
                    .output_name_dialog
                    .as_ref()
                    .and_then(|dialog| dialog.error.as_ref())
                    .is_some()
            );
        });

        view.update(cx, |shell, cx| {
            shell.output_name_input.update(cx, |input, cx| {
                input.set_text("  archive-renamed.iso  ", cx);
            });
        });
        view.update(cx, |shell, cx| {
            shell.submit_task_output_name(cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(matches!(
                shell
                    .pending_task_command
                    .as_ref()
                    .map(|pending| &pending.command),
                Some(TaskCommandView::SetOutputName { output_name })
                    if output_name == "archive-renamed.iso"
            ));
        });
    }

    #[gpui::test]
    fn output_name_result_closes_on_success_and_stays_open_on_failure(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.snapshot.tasks[0].status = TaskStatusView::Waiting;
            shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
            shell.open_task_output_name(window, cx);
            shell
        });

        view.update(cx, |shell, cx| {
            shell.output_name_input.update(cx, |input, cx| {
                input.set_text("first.iso", cx);
            });
        });
        let first = view.update(cx, |shell, cx| {
            shell.submit_task_output_name(cx);
            let pending = shell
                .pending_task_command
                .as_ref()
                .expect("pending command");
            TaskCommandResultView {
                request_id: pending.request_id,
                session: pending.session.clone(),
                identity: pending.identity.clone(),
                command: pending.command.clone(),
                outcome: CommandOutcomeView::Failure(OperationErrorView {
                    code: "rpc.command_rejected".into(),
                    summary: "aria2 rejected the output name".into(),
                    retryable: false,
                }),
            }
        });
        view.update_in(cx, |shell, window, cx| {
            shell.set_task_command_result(first, window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(shell.pending_task_command.is_none());
            assert!(
                shell
                    .output_name_dialog
                    .as_ref()
                    .and_then(|dialog| dialog.error.as_ref())
                    .is_some()
            );
        });

        view.update(cx, |shell, cx| {
            shell.output_name_input.update(cx, |input, cx| {
                input.set_text("second.iso", cx);
            });
        });
        let second = view.update(cx, |shell, cx| {
            shell.submit_task_output_name(cx);
            let pending = shell
                .pending_task_command
                .as_ref()
                .expect("pending command");
            TaskCommandResultView {
                request_id: pending.request_id,
                session: pending.session.clone(),
                identity: pending.identity.clone(),
                command: pending.command.clone(),
                outcome: CommandOutcomeView::Success { tasks: Vec::new() },
            }
        });
        view.update_in(cx, |shell, window, cx| {
            shell.set_task_command_result(second, window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(shell.pending_task_command.is_none());
            assert!(shell.output_name_dialog.is_none());
        });
    }

    #[gpui::test]
    fn theme_applies_only_after_the_matching_save_succeeds(cx: &mut TestAppContext) {
        let initial = SettingsView {
            color_scheme: ColorSchemeView::Dark,
            download_directory: "C:/Downloads".into(),
            ..SettingsView::default()
        };
        let expected_initial = initial.clone();
        let (view, cx) =
            cx.add_window_view(move |window, cx| AppShell::new_with_settings(initial, window, cx));
        let (request_id, requested) = view.update(cx, |shell, cx| {
            shell.page = AppPage::Settings;
            shell.select_color_scheme(ColorSchemeView::Light, cx);
            let pending = shell
                .pending_settings_save
                .as_ref()
                .expect("settings save must become pending");
            (pending.request_id, pending.settings.clone())
        });

        view.update_in(cx, |shell, window, cx| {
            shell.set_settings_save_result(
                SettingsSaveResultView {
                    request_id: RequestId::from_u64(request_id.get() + 1),
                    settings: requested.clone(),
                    outcome: SettingsSaveOutcomeView::Success,
                },
                window,
                cx,
            );
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.settings, expected_initial);
            assert!(shell.pending_settings_save.is_some());
        });

        view.update_in(cx, |shell, window, cx| {
            shell.set_settings_save_result(
                SettingsSaveResultView {
                    request_id,
                    settings: requested.clone(),
                    outcome: SettingsSaveOutcomeView::Success,
                },
                window,
                cx,
            );
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.settings, requested);
            assert_eq!(shell.theme.mode, ThemeMode::Light);
            assert!(shell.pending_settings_save.is_none());
            assert_eq!(shell.page, AppPage::Settings);
        });
    }

    #[gpui::test]
    fn proxy_settings_build_a_manual_draft_with_a_masked_password(cx: &mut TestAppContext) {
        let initial = SettingsView {
            color_scheme: ColorSchemeView::Dark,
            download_directory: "C:/Downloads".into(),
            download_proxy: DownloadProxySettingsView {
                mode: ProxyModeView::Disabled,
                ..DownloadProxySettingsView::default()
            },
            speed_limits: SpeedLimitSettingsView::default(),
            transfer_policy: TransferPolicySettingsView::default(),
            notifications: NotificationSettingsView::default(),
        };
        let (view, cx) =
            cx.add_window_view(move |window, cx| AppShell::new_with_settings(initial, window, cx));

        view.update_in(cx, |shell, window, cx| {
            shell.open_settings(&OpenSettings, window, cx);
            shell.select_proxy_mode(ProxyModeView::Manual, cx);
            shell.settings_all_proxy_input.update(cx, |input, cx| {
                input.set_text("proxy.example:8080", cx);
            });
            shell
                .settings_proxy_username_input
                .update(cx, |input, cx| input.set_text("proxy-user", cx));
            shell
                .settings_proxy_password_input
                .update(cx, |input, cx| input.set_text("never-render-this", cx));
            shell.submit_proxy_settings(cx);
        });

        view.read_with(cx, |shell, cx| {
            assert!(shell.settings_proxy_password_input.read(cx).is_secure());
            let pending = shell
                .pending_settings_save
                .as_ref()
                .expect("proxy settings save must become pending");
            assert_eq!(pending.source, SettingsSaveSource::Proxy);
            assert_eq!(pending.settings.download_proxy.mode, ProxyModeView::Manual);
            assert_eq!(
                pending.settings.download_proxy.all_proxy,
                "proxy.example:8080"
            );
            assert_eq!(pending.settings.download_proxy.username, "proxy-user");
            assert!(pending.settings.download_proxy.has_password);
            assert_eq!(pending.settings.download_directory, "C:/Downloads");
        });
    }

    #[gpui::test]
    fn stale_details_result_cannot_replace_the_active_request(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.selected = Some(task(0).identity);
            shell.open_details_for(task(0), cx);
            shell
        });
        let (request_id, session, identity) = view.read_with(cx, |shell, _| {
            let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
            assert!(matches!(drawer.state, TaskDetailsLoadState::Loading));
            (
                drawer
                    .pending
                    .as_ref()
                    .expect("details request must be pending")
                    .request_id,
                drawer.session.clone(),
                drawer.identity.clone(),
            )
        });

        view.update(cx, |shell, cx| {
            shell.set_task_details_result(
                TaskDetailsResultView {
                    request_id: RequestId::from_u64(request_id.get() + 1),
                    session: session.clone(),
                    identity: identity.clone(),
                    outcome: TaskDetailsOutcomeView::Ready(Box::new(details(1))),
                },
                cx,
            );
        });
        view.read_with(cx, |shell, _| {
            let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
            assert!(matches!(drawer.state, TaskDetailsLoadState::Loading));
            assert_eq!(
                drawer.pending.as_ref().map(|pending| pending.request_id),
                Some(request_id)
            );
        });

        view.update(cx, |shell, cx| {
            shell.set_task_details_result(
                TaskDetailsResultView {
                    request_id,
                    session,
                    identity,
                    outcome: TaskDetailsOutcomeView::Ready(Box::new(details(1))),
                },
                cx,
            );
        });
        view.read_with(cx, |shell, _| {
            let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
            assert!(matches!(drawer.state, TaskDetailsLoadState::Ready { .. }));
            assert!(drawer.pending.is_none());
        });
    }

    #[gpui::test]
    fn task_revision_refreshes_visible_file_details_without_loading_flicker(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.open_details_for(task(0), cx);
            shell
        });
        let (initial_request, session, identity) = view.read_with(cx, |shell, _| {
            let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
            (
                drawer.pending.as_ref().expect("initial request").request_id,
                drawer.session.clone(),
                drawer.identity.clone(),
            )
        });
        let mut first_details = details(1);
        first_details.files[0].completed_length = 100;
        view.update(cx, |shell, cx| {
            shell.set_task_details_result(
                TaskDetailsResultView {
                    request_id: initial_request,
                    session: session.clone(),
                    identity: identity.clone(),
                    outcome: TaskDetailsOutcomeView::Ready(Box::new(first_details)),
                },
                cx,
            );
        });

        view.update(cx, |shell, cx| {
            let mut revision_two = snapshot(1);
            revision_two.tasks[0].revision = 2;
            shell.set_snapshot(revision_two, cx);
        });
        let refresh_request = view.read_with(cx, |shell, _| {
            let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
            let TaskDetailsLoadState::Ready { details } = &drawer.state else {
                panic!("existing details must remain visible while refreshing")
            };
            assert_eq!(details.files[0].completed_length, 100);
            let pending = drawer.pending.as_ref().expect("refresh request");
            assert_eq!(pending.source_revision, 2);
            pending.request_id
        });

        view.update(cx, |shell, cx| {
            let mut revision_three = snapshot(1);
            revision_three.tasks[0].revision = 3;
            shell.set_snapshot(revision_three, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(
                shell
                    .details_drawer
                    .as_ref()
                    .and_then(|drawer| drawer.pending.as_ref())
                    .map(|pending| pending.request_id),
                Some(refresh_request),
                "a second refresh must not be started while one is pending"
            );
        });

        let mut second_details = details(1);
        second_details.files[0].completed_length = 200;
        view.update(cx, |shell, cx| {
            shell.set_task_details_result(
                TaskDetailsResultView {
                    request_id: refresh_request,
                    session: session.clone(),
                    identity: identity.clone(),
                    outcome: TaskDetailsOutcomeView::Ready(Box::new(second_details)),
                },
                cx,
            );
        });
        let catch_up_request = view.read_with(cx, |shell, _| {
            let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
            let TaskDetailsLoadState::Ready { details } = &drawer.state else {
                panic!("refreshed details must stay visible")
            };
            assert_eq!(details.files[0].completed_length, 200);
            let pending = drawer.pending.as_ref().expect("catch-up request");
            assert_eq!(pending.source_revision, 3);
            assert_ne!(pending.request_id, refresh_request);
            pending.request_id
        });

        let mut stale_details = details(1);
        stale_details.files[0].completed_length = 50;
        view.update(cx, |shell, cx| {
            shell.set_task_details_result(
                TaskDetailsResultView {
                    request_id: refresh_request,
                    session: session.clone(),
                    identity: identity.clone(),
                    outcome: TaskDetailsOutcomeView::Ready(Box::new(stale_details)),
                },
                cx,
            );
        });
        view.read_with(cx, |shell, _| {
            let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
            let TaskDetailsLoadState::Ready { details } = &drawer.state else {
                panic!("details must remain ready")
            };
            assert_eq!(details.files[0].completed_length, 200);
            assert_eq!(
                drawer.pending.as_ref().map(|pending| pending.request_id),
                Some(catch_up_request)
            );
        });
    }

    #[gpui::test]
    fn detail_requests_are_task_scoped_and_clear_active_only_network_data(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            let mut initial = snapshot(1);
            initial.tasks[0].status = TaskStatusView::Seeding;
            initial.tasks[0].source_kind = TaskSourceKindView::Unknown;
            shell.snapshot = initial;
            shell
        });
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = events.clone();
        let _subscription = view.update(cx, |_, cx| {
            cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| {
                if let AppShellEvent::TaskDetailsRequested(request) = event {
                    sink.lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(request.clone());
                }
            })
        });

        view.update(cx, |shell, cx| {
            shell.open_details_for(shell.snapshot.tasks[0].clone(), cx);
        });
        let first = events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())[0]
            .clone();
        assert!(first.active);
        assert!(first.is_bittorrent);

        let mut loaded = details(1);
        loaded.servers.push(TaskServerView {
            file_index: 1,
            uri: "https://origin.example/file".into(),
            current_uri: "https://cdn.example/file".into(),
            download_rate: 1_024,
        });
        loaded.peers.push(TaskPeerView {
            address: "192.0.2.1".into(),
            port: 6_881,
            download_rate: 2_048,
            upload_rate: 512,
            seeder: true,
        });
        view.update(cx, |shell, cx| {
            shell.set_task_details_result(
                TaskDetailsResultView {
                    request_id: first.request_id,
                    session: first.session.clone(),
                    identity: first.identity.clone(),
                    outcome: TaskDetailsOutcomeView::Ready(Box::new(loaded)),
                },
                cx,
            );
        });

        view.update(cx, |shell, cx| {
            let mut completed = snapshot(1);
            completed.tasks[0].status = TaskStatusView::Complete;
            completed.tasks[0].source_kind = TaskSourceKindView::BitTorrent;
            completed.tasks[0].revision = 2;
            shell.set_snapshot(completed, cx);
        });
        view.read_with(cx, |shell, _| {
            let drawer = shell.details_drawer.as_ref().expect("drawer remains open");
            let TaskDetailsLoadState::Ready { details } = &drawer.state else {
                panic!("background refresh must keep details visible")
            };
            assert!(details.peers.is_empty());
            assert!(details.servers.is_empty());
        });
        let requests = events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(requests.len(), 2);
        assert!(!requests[1].active);
        assert!(requests[1].is_bittorrent);
    }

    #[gpui::test]
    fn details_drawer_survives_filtering_that_hides_its_task(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(3);
            shell.selected = Some(task(1).identity);
            shell.open_details_for(task(1), cx);
            shell
        });
        let selected = view.read_with(cx, |shell, _| shell.selected.clone());

        view.update(cx, |shell, cx| {
            let mut filtered = snapshot(1);
            filtered.tasks[0] = task(2);
            shell.set_snapshot(filtered, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.selected, selected);
            assert_eq!(
                shell.details_drawer.as_ref().map(|drawer| &drawer.identity),
                selected.as_ref()
            );
        });
    }

    #[gpui::test]
    fn ten_thousand_detail_files_render_only_a_viewport_window(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            let overview = task(0);
            shell.selected = Some(overview.identity.clone());
            shell.details_drawer = Some(TaskDetailsDrawer {
                identity: overview.identity.clone(),
                overview,
                session: shell.snapshot.engine_session().expect("test session"),
                state: TaskDetailsLoadState::Ready {
                    details: Box::new(details(10_000)),
                },
                pending: None,
                open_pending: None,
                tab: TaskDetailsTab::Files,
                file_scroll: UniformListScrollHandle::new(),
                rendered_file_range: 0..0,
            });
            shell
        });

        view.read_with(cx, |shell, _| {
            let drawer = shell.details_drawer.as_ref().expect("drawer must exist");
            assert!(!drawer.rendered_file_range.is_empty());
            assert!(
                drawer.rendered_file_range.len() < 64,
                "rendered {} files",
                drawer.rendered_file_range.len()
            );
            let TaskDetailsLoadState::Ready { details } = &drawer.state else {
                panic!("drawer must be ready")
            };
            assert_eq!(details.files.len(), 10_000);
        });
    }

    #[gpui::test]
    fn task_removal_requires_the_matching_internal_confirmation(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
            shell.confirm_remove_selected(window, cx);
            shell
        });
        view.read_with(cx, |shell, _| {
            assert!(shell.remove_confirmation.is_some());
            assert!(
                !shell
                    .remove_confirmation
                    .as_ref()
                    .is_some_and(|value| value.delete_files)
            );
            assert!(shell.pending_task_command.is_none());
        });
        view.update(cx, |shell, cx| shell.submit_remove_confirmation(cx));
        view.read_with(cx, |shell, _| {
            assert!(shell.remove_confirmation.is_none());
            assert!(matches!(
                shell
                    .pending_task_command
                    .as_ref()
                    .map(|pending| pending.command.clone()),
                Some(TaskCommandView::RemoveTask)
            ));
        });
    }

    #[gpui::test]
    fn local_removal_can_explicitly_request_recycle_bin_files(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.engine_health = EngineHealthView::Running { restarts: 0 };
            shell.snapshot = snapshot(1);
            shell.selected = Some(shell.snapshot.tasks[0].identity.clone());
            shell.confirm_remove_selected(window, cx);
            shell
        });
        view.update(cx, |shell, cx| shell.toggle_remove_files(cx));
        view.read_with(cx, |shell, _| {
            assert!(
                shell
                    .remove_confirmation
                    .as_ref()
                    .is_some_and(|value| value.delete_files)
            );
        });
        view.update(cx, |shell, cx| shell.submit_remove_confirmation(cx));
        view.read_with(cx, |shell, _| {
            assert!(matches!(
                shell
                    .pending_task_command
                    .as_ref()
                    .map(|pending| pending.command.clone()),
                Some(TaskCommandView::RemoveTaskAndFiles)
            ));
        });
    }

    #[gpui::test]
    fn navigation_shortcuts_return_to_downloads_and_preserve_selection(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(2);
            shell.selected = Some(shell.snapshot.tasks[1].identity.clone());
            shell.page = AppPage::Settings;
            shell
        });
        let selected = view.read_with(cx, |shell, _| shell.selected.clone());

        view.update_in(cx, |shell, window, cx| {
            shell.focus_search(&FocusSearch, window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.page, AppPage::Downloads);
            assert_eq!(shell.selected, selected);
        });

        view.update_in(cx, |shell, window, cx| {
            shell.page = AppPage::Settings;
            shell.open_add_download(&OpenAddDownload, window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert_eq!(shell.page, AppPage::Downloads);
            assert!(shell.add_dialog.open);
            assert_eq!(shell.selected, selected);
        });
    }

    #[gpui::test]
    fn escape_priority_closes_popover_then_settings_then_search(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.page = AppPage::Settings;
            shell.speed_popover_open = true;
            shell.search_input.update(cx, |input, cx| {
                input.set_text("archive", cx);
            });
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.clear_search(&ClearSearch, window, cx);
        });
        view.read_with(cx, |shell, cx| {
            assert!(!shell.speed_popover_open);
            assert_eq!(shell.page, AppPage::Settings);
            assert_eq!(shell.search_input.read(cx).text(), "archive");
        });

        view.update_in(cx, |shell, window, cx| {
            shell.clear_search(&ClearSearch, window, cx);
        });
        view.read_with(cx, |shell, cx| {
            assert_eq!(shell.page, AppPage::Downloads);
            assert_eq!(shell.search_input.read(cx).text(), "archive");
        });

        view.update_in(cx, |shell, window, cx| {
            shell.clear_search(&ClearSearch, window, cx);
        });
        view.read_with(cx, |shell, cx| {
            assert!(shell.search_input.read(cx).text().is_empty());
        });
    }

    #[gpui::test]
    fn failed_directory_save_keeps_the_draft(cx: &mut TestAppContext) {
        let initial = SettingsView {
            color_scheme: ColorSchemeView::Dark,
            download_directory: "C:/Downloads".into(),
            ..SettingsView::default()
        };
        let (view, cx) =
            cx.add_window_view(move |window, cx| AppShell::new_with_settings(initial, window, cx));
        let (request_id, requested) = view.update(cx, |shell, cx| {
            shell.page = AppPage::Settings;
            shell.settings_directory_input.update(cx, |input, cx| {
                input.set_text("D:/Transfers", cx);
            });
            shell.submit_settings(cx);
            let pending = shell
                .pending_settings_save
                .as_ref()
                .expect("settings save must become pending");
            (pending.request_id, pending.settings.clone())
        });

        view.update_in(cx, |shell, window, cx| {
            shell.set_settings_save_result(
                SettingsSaveResultView {
                    request_id,
                    settings: requested,
                    outcome: SettingsSaveOutcomeView::Failure(OperationErrorView {
                        code: "settings.write_failed".into(),
                        summary: "Could not write settings.".into(),
                        retryable: true,
                    }),
                },
                window,
                cx,
            );
        });
        view.read_with(cx, |shell, cx| {
            assert_eq!(shell.settings.download_directory, "C:/Downloads");
            assert_eq!(
                shell.settings_directory_input.read(cx).text(),
                "D:/Transfers"
            );
            assert_eq!(shell.page, AppPage::Settings);
            assert!(shell.settings_page.error.is_some());
        });
    }

    #[gpui::test]
    fn global_speed_limit_save_emits_parsed_request_and_normalizes_on_success(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = events.clone();
        let _subscription = view.update(cx, |_, cx| {
            cx.subscribe(&view, move |_, _, event: &AppShellEvent, _| {
                if let AppShellEvent::SettingsSaveRequested(request) = event {
                    sink.lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(request.clone());
                }
            })
        });

        view.update(cx, |shell, cx| {
            shell.page = AppPage::Settings;
            // "2M" and blank (unlimited) both go through the K/M parser.
            shell.settings_download_limit_input.update(cx, |input, cx| {
                input.set_text("2M", cx);
            });
        });
        let request_id = view.update(cx, |shell, cx| {
            shell.submit_speed_limits(cx);
            shell
                .pending_settings_save
                .as_ref()
                .expect("speed-limit save must become pending")
                .request_id
        });

        let request = events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .last()
            .cloned()
            .expect("a settings-save event should have been emitted");
        // The view carries the raw editable text; byte parsing happens in the
        // desktop mapping layer, not here.
        assert_eq!(request.settings.speed_limits.download_limit, "2M");
        assert!(request.settings.speed_limits.upload_limit.is_empty());

        // The desktop persists normalized bytes and echoes back the compact form.
        let mut normalized = request.settings.clone();
        normalized.speed_limits.download_limit = crate::format_speed_limit_field(2 * 1024 * 1024);
        normalized.speed_limits.upload_limit = crate::format_speed_limit_field(0);
        view.update_in(cx, |shell, window, cx| {
            shell.set_settings_save_result(
                SettingsSaveResultView {
                    request_id,
                    settings: normalized,
                    outcome: SettingsSaveOutcomeView::Success,
                },
                window,
                cx,
            );
        });
        view.read_with(cx, |shell, cx| {
            assert!(shell.pending_settings_save.is_none());
            assert_eq!(shell.settings.speed_limits.download_limit, "2M");
            assert_eq!(shell.settings_download_limit_input.read(cx).text(), "2M");
            assert!(shell.settings_upload_limit_input.read(cx).text().is_empty());
        });
    }

    #[gpui::test]
    fn invalid_global_speed_limit_is_rejected_before_a_save_request(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
        // Set the text in its own cycle so the field's change event (which
        // dismisses stale errors) is flushed before the submit runs, matching
        // the real "type, then click Save" order.
        view.update(cx, |shell, cx| {
            shell.page = AppPage::Settings;
            shell.settings_download_limit_input.update(cx, |input, cx| {
                input.set_text("5MB", cx);
            });
        });
        view.update(cx, |shell, cx| {
            shell.submit_speed_limits(cx);
        });
        view.read_with(cx, |shell, _cx| {
            assert!(shell.pending_settings_save.is_none());
            assert!(shell.settings_page.error.is_some());
        });
    }

    #[gpui::test]
    fn speed_popover_toggles_and_restores_previous_focus(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let shell = AppShell::new(Theme::dark(), window, cx);
            window.focus(&shell.focus_handle, cx);
            shell
        });

        view.update_in(cx, |shell, window, cx| {
            shell.toggle_speed_popover(window, cx);
            assert!(shell.speed_popover_open);
            shell.close_speed_popover(window, cx);
        });
        view.read_with(cx, |shell, _| {
            assert!(!shell.speed_popover_open);
            assert!(shell.speed_popover_previous_focus.is_none());
        });
    }

    #[gpui::test]
    #[gpui::test]
    fn task_status_transitions_group_completions_into_one_notice(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            let mut initial = snapshot(3);
            for task in &mut initial.tasks {
                task.status = TaskStatusView::Active;
            }
            // Seed known statuses without treating the first snapshot as transitions.
            shell.set_snapshot(initial, cx);
            shell
        });

        view.update(cx, |shell, cx| {
            let mut next = shell.snapshot.clone();
            next.tasks[0].status = TaskStatusView::Complete;
            next.tasks[1].status = TaskStatusView::Complete;
            next.tasks[2].status = TaskStatusView::Failed;
            next.tasks[2].error = Some(crate::TaskErrorView {
                code: Some(1),
                summary: "Network failed".into(),
                details: None,
            });
            shell.set_snapshot(next, cx);
            assert_eq!(
                shell.activity_log.len(),
                2,
                "one completion group + one failure"
            );
            assert_eq!(shell.activity_log[0].kind, ActivityKindView::Error);
            assert_eq!(shell.activity_log[0].count, 1);
            assert_eq!(shell.activity_log[1].kind, ActivityKindView::Completion);
            assert_eq!(shell.activity_log[1].count, 2);
            assert!(
                shell
                    .status_notice
                    .as_ref()
                    .is_some_and(|notice| notice.is_error && notice.message.contains("failed")),
                "latest automatic notice should be the failure group"
            );
        });
    }

    #[gpui::test]
    fn quiet_volume_suppresses_automatic_toasts_but_keeps_history(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.settings.notifications.volume = NotificationVolumeView::Quiet;
            let mut initial = snapshot(1);
            initial.tasks[0].status = TaskStatusView::Active;
            shell.set_snapshot(initial, cx);
            shell
        });

        view.update(cx, |shell, cx| {
            let mut next = shell.snapshot.clone();
            next.tasks[0].status = TaskStatusView::Complete;
            shell.set_snapshot(next, cx);
            assert_eq!(shell.activity_log.len(), 1);
            assert!(
                shell.status_notice.is_none(),
                "Quiet must hide automatic completion toasts"
            );
            // Command feedback still surfaces in Quiet.
            shell.show_notice("Copied.", false, cx);
            assert!(shell.status_notice.is_some());
        });
    }

    #[gpui::test]
    fn silent_volume_suppresses_all_toasts(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.settings.notifications.volume = NotificationVolumeView::Silent;
            shell
        });

        view.update(cx, |shell, cx| {
            shell.show_notice("Command feedback.", false, cx);
            assert!(shell.status_notice.is_none());
            shell.record_activity(ActivityKindView::Info, "Still recorded", None, None, 1, cx);
            assert_eq!(shell.activity_log.len(), 1);
        });
    }

    #[gpui::test]
    fn notification_preferences_save_emits_settings_request(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
        view.update_in(cx, |shell, window, cx| {
            shell.open_settings(&OpenSettings, window, cx);
            shell.select_notification_volume(NotificationVolumeView::Quiet, cx);
            shell.toggle_notify_on_completion(cx);
            shell.submit_notifications(cx);
            let pending = shell
                .pending_settings_save
                .as_ref()
                .expect("notification save pending");
            assert_eq!(pending.source, SettingsSaveSource::Notifications);
            assert_eq!(
                pending.settings.notifications.volume,
                NotificationVolumeView::Quiet
            );
            assert!(!pending.settings.notifications.notify_on_completion);
        });
    }

    #[gpui::test]
    fn activity_panel_toggles_and_clears(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| AppShell::new(Theme::dark(), window, cx));
        view.update_in(cx, |shell, window, cx| {
            shell.record_activity(
                ActivityKindView::Completion,
                "One finished.",
                None,
                None,
                1,
                cx,
            );
            shell.toggle_activity_panel(window, cx);
            assert!(shell.activity_panel_open);
            shell.clear_activity_log(cx);
            assert!(shell.activity_log.is_empty());
            shell.close_activity_panel(window, cx);
            assert!(!shell.activity_panel_open);
        });
    }

    #[gpui::test]
    fn force_pause_is_blocked_when_capabilities_omit_the_method(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.snapshot = snapshot(1);
            shell.snapshot.tasks[0].status = TaskStatusView::Active;
            shell.selected = Some(task(0).identity);
            shell.selected_tasks = std::collections::HashSet::from([task(0).identity]);
            // Probed methods without forcePause: UI must explain and not submit.
            shell.snapshot.capabilities = crate::EngineCapabilitiesView {
                version: "1.37.0".into(),
                methods_probed: true,
                force_pause: false,
                force_pause_all: false,
                force_remove: false,
                queue_positioning: true,
                change_option: true,
                change_global_option: true,
                get_peers: true,
                get_servers: true,
                multicall: true,
            };
            shell
        });

        view.update(cx, |shell, cx| {
            shell.begin_task_command(TaskCommandView::ForcePause, cx);
            assert!(shell.pending_task_command.is_none());
            let notice = shell.status_notice.as_ref().expect("capability notice");
            assert!(notice.is_error);
            assert!(
                notice.message.contains("force-pause") || notice.message.contains("forcePause"),
                "{}",
                notice.message
            );
        });
    }

    #[gpui::test]
    fn profile_catalog_can_switch_and_add_drafts(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.set_profiles(
                ProfileCatalogView {
                    active_profile_id: "p1".into(),
                    profiles: vec![
                        ProfileEntryView {
                            profile_id: "p1".into(),
                            name: "Local".into(),
                            kind: ProfileKindView::LocalManaged,
                            executable: "aria2c".into(),
                            download_dir: "D:/Downloads".into(),
                            endpoint: String::new(),
                            has_secret: false,
                        },
                        ProfileEntryView {
                            profile_id: "p2".into(),
                            name: "NAS".into(),
                            kind: ProfileKindView::RemoteRpc,
                            executable: String::new(),
                            download_dir: "D:/Downloads".into(),
                            endpoint: "wss://nas.example/jsonrpc".into(),
                            has_secret: false,
                        },
                    ],
                },
                cx,
            );
            shell
        });

        view.update(cx, |shell, cx| {
            shell.request_switch_profile("p1".into(), cx);
            assert!(
                shell
                    .status_notice
                    .as_ref()
                    .is_some_and(|notice| notice.message.contains("already active"))
            );
            shell.add_draft_local_profile(cx);
            assert_eq!(shell.profiles.profiles.len(), 3);
            let draft = shell
                .profiles
                .profiles
                .iter()
                .find(|profile| profile.profile_id.starts_with("draft-local-"))
                .expect("local draft");
            assert!(draft.executable.is_empty(), "local draft uses managed core");
            assert_eq!(
                shell.settings_page.editing_profile_id.as_deref(),
                Some(draft.profile_id.as_str())
            );
            shell.settings_profile_name_input.update(cx, |input, cx| {
                input.set_text("Home NAS-ready", cx);
            });
            shell.apply_profile_editor(cx);
            assert!(shell.settings_page.editing_profile_id.is_none());
            assert!(
                shell
                    .profiles
                    .profiles
                    .iter()
                    .any(|profile| profile.name == "Home NAS-ready")
            );

            shell.add_draft_remote_profile(cx);
            assert_eq!(shell.profiles.profiles.len(), 4);
            assert!(
                shell
                    .profiles
                    .profiles
                    .iter()
                    .any(|profile| profile.kind == ProfileKindView::RemoteRpc
                        && profile.profile_id.starts_with("draft-remote-"))
            );

            // Delete requires confirmation, then persists via save request.
            let remove_id = shell.profiles.profiles[3].profile_id.clone();
            shell.request_remove_profile(remove_id.clone(), cx);
            assert!(shell.settings_page.pending_profile_delete.is_some());
            shell.cancel_remove_profile(cx);
            assert!(shell.settings_page.pending_profile_delete.is_none());
            assert_eq!(shell.profiles.profiles.len(), 4);
            shell.request_remove_profile(remove_id, cx);
            shell.confirm_remove_profile(cx);
            assert_eq!(shell.profiles.profiles.len(), 3);

            // Remote secret set is staged until Save profiles.
            shell.add_draft_remote_profile(cx);
            let remote_id = shell
                .profiles
                .profiles
                .iter()
                .find(|profile| profile.profile_id.starts_with("draft-remote-"))
                .map(|profile| profile.profile_id.clone())
                .expect("remote draft");
            shell.open_profile_editor(remote_id.clone(), cx);
            shell.settings_profile_secret_input.update(cx, |input, cx| {
                input.set_text("s3cret", cx);
            });
            shell.apply_profile_editor(cx);
            assert!(
                shell
                    .settings_page
                    .profile_secret_updates
                    .get(&remote_id)
                    .is_some_and(|update| matches!(
                        update,
                        crate::ProfileRpcSecretUpdateView::Set(_)
                    ))
            );
            assert!(
                shell
                    .profiles
                    .profiles
                    .iter()
                    .find(|profile| profile.profile_id == remote_id)
                    .is_some_and(|profile| profile.has_secret)
            );

            shell.set_switch_profile_result(
                SwitchProfileResultView {
                    request_id: RequestId::from_u64(9),
                    profile_id: "p2".into(),
                    catalog: ProfileCatalogView {
                        active_profile_id: "p2".into(),
                        profiles: shell.profiles.profiles.clone(),
                    },
                    outcome: SwitchProfileOutcomeView::Success,
                },
                cx,
            );
            assert_eq!(shell.profiles.active_profile_id, "p2");
            assert_eq!(
                shell.profiles.active().map(|profile| profile.name.as_str()),
                Some("NAS")
            );
        });
    }

    #[gpui::test]
    fn core_registry_commands_emit_and_apply_results(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(|window, cx| {
            let mut shell = AppShell::new(Theme::dark(), window, cx);
            shell.set_cores(
                CoreRegistryView {
                    active_id: Some("c1".into()),
                    last_working_id: Some("c1".into()),
                    installations: vec![CoreInstallationView {
                        id: "c1".into(),
                        version: "1.36.0".into(),
                        target: "windows-x86_64".into(),
                        source: CoreSourceView::Imported,
                        executable: "D:/cores/aria2c.exe".into(),
                        features: vec!["BitTorrent".into()],
                        is_active: true,
                        is_last_working: true,
                        validated_version: Some("1.36.0".into()),
                        status: CoreInstallStatusView::Ready,
                    }],
                },
                cx,
            );
            shell
        });

        view.update(cx, |shell, cx| {
            shell.request_core_command(
                CoreCommandView::Verify {
                    core_id: "c1".into(),
                },
                cx,
            );
            shell.set_core_command_result(
                CoreCommandResultView {
                    request_id: RequestId::from_u64(3),
                    command: CoreCommandView::Verify {
                        core_id: "c1".into(),
                    },
                    registry: CoreRegistryView {
                        active_id: Some("c1".into()),
                        last_working_id: Some("c1".into()),
                        installations: vec![CoreInstallationView {
                            id: "c1".into(),
                            version: "1.36.0".into(),
                            target: "windows-x86_64".into(),
                            source: CoreSourceView::Imported,
                            executable: "D:/cores/aria2c.exe".into(),
                            features: vec!["BitTorrent".into(), "HTTPS".into()],
                            is_active: true,
                            is_last_working: true,
                            validated_version: Some("1.36.0".into()),
                            status: CoreInstallStatusView::Ready,
                        }],
                    },
                    outcome: CoreCommandOutcomeView::Success,
                },
                cx,
            );
            assert_eq!(shell.cores.installations[0].features.len(), 2);
            assert!(
                shell
                    .status_notice
                    .as_ref()
                    .is_some_and(|notice| notice.message.contains("verified"))
            );
        });
    }
}
