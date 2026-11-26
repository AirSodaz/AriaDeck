use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use ariadeck_i18n::{FluentArgs, FluentValue};
use gpui::{
    Anchor, AnyElement, AnyWindowHandle, App, Axis, ClickEvent, ClipboardItem, Context, Div,
    DragMoveEvent, ElementId, Entity, ExternalPaths, FocusHandle, Focusable, FontFeatures,
    FontWeight, Hsla, IntoElement, MouseButton, MouseDownEvent, PathPromptOptions, Pixels, Render,
    Role, ScrollHandle, ScrollStrategy, SharedString, Stateful, Subscription, Toggled,
    UniformListScrollHandle, WeakFocusHandle, Window, WindowControlArea, div, img, prelude::*, px,
    uniform_list,
};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, InteractiveElementExt as _, Sizable as _, WindowExt as _,
    alert::Alert,
    button::{Button as KitButton, ButtonVariants as _},
    chart::AreaChart,
    clipboard::Clipboard,
    collapsible::Collapsible,
    empty::{
        Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant,
        EmptyTitle,
    },
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem},
    notification::Notification,
    popover::Popover,
    progress::Progress as KitProgress,
    resizable::{HandleEdge, resize_handle},
    scroll::ScrollableElement as _,
    separator::Separator,
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarGroup, SidebarItem as _, SidebarMenu, SidebarMenuItem,
    },
    status_bar::StatusBar,
};

use crate::{
    ActivityEntryView, ActivityKindView, AddDownloadAdvancedOptionsView, AddDownloadInputModeView,
    AddDownloadItemResultView, AddDownloadMetadataKindView, AddDownloadMetadataPreviewOutcomeView,
    AddDownloadMetadataPreviewRequestView, AddDownloadMetadataPreviewResultView,
    AddDownloadMetadataPreviewView, AddDownloadModeView, AddDownloadRequestView,
    AddDownloadResultView, AddDownloadSourceView, BatchCommandOutcomeView,
    BatchTaskCommandRequestView, BatchTaskCommandResultView, BatchTaskCommandView,
    BatchTaskFailureView, BridgeDownloadView, BrowserBridgeSettingsView, Checkbox, ClearSearch,
    CloseAddDownload, CloseBatchFailures, CloseBehaviorView, CloseCoreSetup, CloseSettings,
    CloseTaskOptions, CloseTaskOutputName, CloseTaskSpeedLimit, ColorSchemeView,
    CommandOutcomeView, CompletionPowerActionView, ConnectionView, CoreCommandOutcomeView,
    CoreCommandRequestView, CoreCommandResultView, CoreCommandView, CoreDiscoveryResultView,
    CoreDownloadOfferView, CoreDownloadResultView, CoreRegistryView, DiagnosticExportOutcomeView,
    DiagnosticExportRequestView, DiagnosticExportResultView, DiscoveredCoreView,
    DownloadProxySettingsView, DownloadRowView, EngineHealthView, EngineSessionView,
    ExtendSelectNextTask, ExtendSelectPreviousTask, FileAllocationView, FileConflictPolicyView,
    FocusSearch, GlobalTaskCommandRequestView, GlobalTaskCommandResultView, GlobalTaskCommandView,
    Icon, IconName, IconSize, LanguagePreferenceView, Link, LocaleId, MoveTaskDownInQueue,
    MoveTaskToQueueBottom, MoveTaskToQueueTop, MoveTaskUpInQueue, NotificationSettingsView,
    NotificationVolumeView, OpenAddDownload, OpenSettings, OpenTaskDetails, OpenTaskOutputName,
    OpenTaskSpeedLimit, OperationErrorView, PauseSelectedTask, PlatformCapabilitiesView,
    PlatformSettingsView, ProfileCatalogView, ProfileEntryView, ProfileKindView,
    ProfileRpcSecretUpdateView, ProxyModeView, ProxyPasswordUpdateView, RemoveSelectedTask,
    RequestId, ResumeSelectedTask, RetrySelectedTask, SaveProfileCatalogOutcomeView,
    SaveProfileCatalogRequestView, SaveProfileCatalogResultView, SaveSettings, SecretStringView,
    Segment, SegmentedControl, SelectAllTasks, SelectFirstTask, SelectLastTask, SelectNextTask,
    SelectPageDownTask, SelectPageUpTask, SelectPreviousTask, SettingsExportOutcomeView,
    SettingsExportRequestView, SettingsExportResultView, SettingsImportOutcomeView,
    SettingsImportRequestView, SettingsImportResultView, SettingsSaveOutcomeView,
    SettingsSaveRequestView, SettingsSaveResultView, SettingsView, SpeedLimitSettingsView,
    SpeedSampleView, StatusIndicator, SubmitAddDownload, SubmitTaskOptions, SubmitTaskOutputName,
    SubmitTaskSpeedLimit, Switch, SwitchProfileOutcomeView, SwitchProfileRequestView,
    SwitchProfileResultView, TaskCommandRequestView, TaskCommandResultView, TaskCommandView,
    TaskDetailsOutcomeView, TaskDetailsRequestView, TaskDetailsResultView, TaskDetailsView,
    TaskFileView, TaskIdentity, TaskOpenOutcomeView, TaskOpenRequestView, TaskOpenResultView,
    TaskOpenTargetView, TaskOptionView, TaskPathValidationView, TaskSourceKindExt as _,
    TaskStatusView, Theme, ThemeMode, TrackerListSettingsView, TrackerListSourceView,
    TransferPolicySettingsView, Translator, WorkspaceFilter, WorkspaceFilterExt as _,
    WorkspaceQuery, WorkspaceSnapshot, WorkspaceSortDirection, WorkspaceSortKey, format_bytes,
    format_eta, format_percent, format_rate, format_share_ratio,
};
use crate::{
    CloseEditTask, DomainHeaderPresetView, FileChecksumProgressView, FileChecksumRequestView,
    FileChecksumResultView, MirrorRewriteSettingsView, OpenEditTask, PrepareEditTaskRequestView,
    PrepareEditTaskResultView, ProbeDownloadUrlRequestView, ProbeDownloadUrlResultView,
    ProbedDownloadMetadataView, RelinkEditOutcomeView, RelinkEditTaskRequestView,
    RelinkEditTaskResultView, SubmitEditTask, TaskEditableStateView,
};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};

#[cfg(test)]
use crate::{TaskPeerView, TaskServerView, TaskTrackerView, TaskUriView};
#[cfg(test)]
use gpui::Point;

mod activity;
mod chrome;
mod commands;
mod core_setup;
mod details;
mod dialogs_add;
mod helpers;
mod modals;
mod power;
mod profiles;
mod settings;
mod task_list;
use core_setup::{CoreSetupPanelView, CoreSetupState};
use details::TaskDetailsPanelView;
use dialogs_add::AddDownloadDialogView;
#[allow(unused_imports)]
use helpers::*;
use modals::{ActiveModal, ModalKind};
pub use power::PowerCountdownState;

use crate::theme::TITLEBAR_HEIGHT;

const SPEED_CHART_SAMPLES: usize = 120;
const TITLEBAR_SIDE_WIDTH: f32 = 240.0;
const TITLEBAR_HORIZONTAL_PADDING: f32 = 12.0;
const SEARCH_MIN_WIDTH: f32 = 300.0;
const SEARCH_WIDTH: f32 = 460.0;
const SIDEBAR_WIDTH: f32 = 208.0;
const COMPACT_SIDEBAR_WIDTH: f32 = 184.0;
const DETAILS_DRAWER_WIDTH: f32 = 380.0;
const DETAILS_DRAWER_MIN_WIDTH: f32 = 320.0;
const DETAILS_DRAWER_MAX_WIDTH: f32 = 560.0;
const WIDE_SHELL_MIN_WIDTH: f32 = 1280.0;
const TASK_ROW_HEIGHT: f32 = 64.0;
const ACTIVITY_HISTORY_LIMIT: usize = 100;
const ACTIVITY_PANEL_WIDTH: f32 = 360.0;
/// Minimum gap between revision-driven details re-fetches while the drawer is open (PERF-001).
const DETAILS_REFRESH_MIN_INTERVAL: Duration = Duration::from_millis(500);

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
    ProbeDownloadUrlRequested(ProbeDownloadUrlRequestView),
    PrepareEditTaskRequested(PrepareEditTaskRequestView),
    RelinkEditTaskRequested(RelinkEditTaskRequestView),
    FileChecksumRequested(FileChecksumRequestView),
    FileChecksumCancelled(RequestId),
    TaskCommandRequested(TaskCommandRequestView),
    GlobalTaskCommandRequested(GlobalTaskCommandRequestView),
    BatchTaskCommandRequested(BatchTaskCommandRequestView),
    TaskDetailsRequested(TaskDetailsRequestView),
    TaskOpenRequested(TaskOpenRequestView),
    SettingsSaveRequested(SettingsSaveRequestView),
    SettingsExportRequested(SettingsExportRequestView),
    SettingsImportRequested(SettingsImportRequestView),
    /// Fetch the configured tracker list URL and apply it (D1 / D-041).
    TrackerListRefreshRequested {
        request_id: RequestId,
    },
    DiagnosticExportRequested(DiagnosticExportRequestView),
    SwitchProfileRequested(SwitchProfileRequestView),
    SaveProfileCatalogRequested(SaveProfileCatalogRequestView),
    CoreCommandRequested(CoreCommandRequestView),
    /// Scan this machine for existing aria2 binaries (B4). Local filesystem only.
    CoreDiscoveryRequested {
        request_id: RequestId,
    },
    /// Fetch the pinned, checksum-verified aria2 build (B4). Only ever emitted
    /// from an explicit button press.
    CoreDownloadRequested {
        request_id: RequestId,
    },
    CheckForUpdatesRequested {
        request_id: RequestId,
    },
    DownloadUpdateRequested {
        request_id: RequestId,
    },
    InstallUpdateRequested,
    /// The first-run core-setup guide was closed; persist that it was shown so it
    /// does not reopen on its own (B4).
    CoreSetupOnboardingDismissed,
    /// Hide the main window while keeping the process and managed engine alive.
    HideToTrayRequested,
    /// Bring the main window back from tray/minimized state.
    ShowFromTrayRequested,
    /// Fully quit AriaDeck (stops a managed engine on drop).
    QuitRequested,
    /// Execute power action (sleep, hibernate, shutdown) after completion countdown (D5).
    CompletionPowerActionRequested(CompletionPowerActionView),
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
    Ready { details: Arc<TaskDetailsView> },
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
    Ready(Arc<TaskDetailsView>),
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
    Verify,
}

pub(crate) const DEFAULT_LINKS_INPUT_HEIGHT: f32 = 120.0;
pub(crate) const MIN_LINKS_INPUT_HEIGHT: f32 = 80.0;
pub(crate) const MAX_LINKS_INPUT_HEIGHT: f32 = 360.0;

#[derive(Clone)]
pub(crate) struct ResizeLinkInput;

impl Render for ResizeLinkInput {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[derive(Clone)]
pub(crate) struct ResizeDetailsDrawer;

impl Render for ResizeDetailsDrawer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[cfg_attr(not(test), allow(dead_code))]
struct StatusNotice {
    message: String,
    is_error: bool,
}

pub(crate) fn has_kit_root(window: &Window) -> bool {
    window
        .root::<gpui_kit::component::Root>()
        .flatten()
        .is_some()
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
    draft_auto_probe_on_add: bool,
    draft_header_presets: Vec<crate::DomainHeaderPresetView>,
    draft_header_preset_edit_index: Option<usize>,
    next_draft_header_preset_seq: u64,
    draft_mirror_rules: crate::MirrorRewriteSettingsView,
    draft_mirror_rule_edit_index: Option<usize>,
    next_draft_mirror_rule_seq: u64,
    draft_file_allocation: FileAllocationView,
    draft_check_integrity: bool,
    draft_notifications: NotificationSettingsView,
    draft_platform: PlatformSettingsView,
    /// Browser bridge drafts (D-045); saved with System.
    draft_browser_bridge: BrowserBridgeSettingsView,
    /// Working copy of download categories (C1 / D-042); saved with General.
    draft_categories: Vec<crate::DownloadCategoryView>,
    /// Which draft category row is bound to the shared name/extensions fields.
    draft_category_edit_index: Option<usize>,
    /// Monotonic counter so newly added draft categories always get unique IDs.
    next_draft_category_seq: u64,
    /// Tracker list drafts (D1); saved with Transfers.
    draft_tracker_enabled: bool,
    draft_tracker_source: crate::TrackerListSourceView,
    draft_tracker_auto_refresh: bool,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PathPickTarget {
    ProfileExecutable,
    /// Browse path for a draft category by its stable category ID.
    CategoryDirectory {
        category_id: String,
    },
}

/// Top-level navigation categories for the settings two-pane layout.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum SettingsCategory {
    /// Appearance (theme) + download categories.
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

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Profiles => "profiles",
            Self::Engine => "engine",
            Self::Network => "network",
            Self::Transfers => "transfers",
            Self::Notifications => "notifications",
            Self::System => "system",
            Self::About => "about",
        }
    }

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
            Self::Profiles => crate::IconName::Server,
            Self::Engine => crate::IconName::Cpu,
            Self::Network => crate::IconName::Wifi,
            Self::Transfers => crate::IconName::ArrowDown,
            Self::Notifications => crate::IconName::Bell,
            Self::System => crate::IconName::Settings,
            Self::About => crate::IconName::CircleHelp,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SettingsSaveSource {
    Theme,
    Language,
    Categories,
    Proxy,
    NetworkRules,
    Network,
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
}

struct TaskOutputNameDialog {
    identity: TaskIdentity,
    display_name: String,
    active: bool,
    error: Option<OperationErrorView>,
}

struct TaskSpeedLimitDialog {
    identity: TaskIdentity,
    display_name: String,
    error: Option<OperationErrorView>,
}

struct TaskOptionsDialog {
    identity: TaskIdentity,
    display_name: String,
    supports_seed_rules: bool,
    error: Option<OperationErrorView>,
}

struct EditTaskDialog {
    identity: TaskIdentity,
    display_name: String,
    can_relink_url: bool,
    is_error_resume: bool,
    can_edit_out: bool,
    original_primary_uri: Option<String>,
    expected_total_bytes: Option<u64>,
    expected_filename: Option<String>,
    initial_state: Option<TaskEditableStateView>,
    loading_request: Option<RequestId>,
    submit_request: Option<RequestId>,
    mismatch_warning: Option<(String, Option<String>, Option<u64>)>,
    error: Option<OperationErrorView>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StatusBarPopover {
    Speed,
    Power,
    Activity,
}

struct BatchFailureDetails {
    command: BatchTaskCommandView,
    failures: Vec<BatchTaskFailureView>,
}

/// Settings-page InputState entities, grouped for navigation and theme updates.
pub(crate) struct SettingsInputs {
    pub(crate) directory: Entity<InputState>,
    pub(crate) profile_name: Entity<InputState>,
    pub(crate) profile_executable: Entity<InputState>,
    pub(crate) profile_endpoint: Entity<InputState>,
    pub(crate) profile_secret: Entity<InputState>,
    pub(crate) all_proxy: Entity<InputState>,
    pub(crate) http_proxy: Entity<InputState>,
    pub(crate) https_proxy: Entity<InputState>,
    pub(crate) ftp_proxy: Entity<InputState>,
    pub(crate) no_proxy: Entity<InputState>,
    pub(crate) proxy_username: Entity<InputState>,
    pub(crate) proxy_password: Entity<InputState>,
    pub(crate) download_limit: Entity<InputState>,
    pub(crate) upload_limit: Entity<InputState>,
    pub(crate) max_concurrent: Entity<InputState>,
    pub(crate) max_connection: Entity<InputState>,
    pub(crate) split: Entity<InputState>,
    pub(crate) min_split_size: Entity<InputState>,
    pub(crate) tracker_custom_url: Entity<InputState>,
    pub(crate) tracker_list_text: Entity<InputState>,
    pub(crate) category_name: Entity<InputState>,
    pub(crate) category_extensions: Entity<InputState>,
    pub(crate) header_preset_name: Entity<InputState>,
    pub(crate) header_preset_domains: Entity<InputState>,
    pub(crate) header_preset_referer: Entity<InputState>,
    pub(crate) header_preset_user_agent: Entity<InputState>,
    pub(crate) header_preset_headers: Entity<TextareaState>,
    pub(crate) mirror_rule_name: Entity<InputState>,
    pub(crate) mirror_rule_pattern: Entity<InputState>,
    pub(crate) mirror_rule_replacement: Entity<InputState>,
}

/// Add-download advanced option InputState entities (lazily owned by `AddDownloadDialogView`).
pub(crate) struct AddInputs {
    pub(crate) referer: Entity<InputState>,
    pub(crate) user_agent: Entity<InputState>,
    pub(crate) headers: Entity<InputState>,
    pub(crate) cookie: Entity<InputState>,
    pub(crate) http_user: Entity<InputState>,
    pub(crate) http_passwd: Entity<InputState>,
    pub(crate) checksum: Entity<InputState>,
}

pub struct AppShell {
    window_handle: AnyWindowHandle,
    theme: Theme,
    /// Active Fluent translator for the resolved display language.
    translator: Translator,
    settings: SettingsView,
    platform_capabilities: PlatformCapabilitiesView,
    profiles: ProfileCatalogView,
    cores: CoreRegistryView,
    page: AppPage,
    engine_health: EngineHealthView,
    snapshot: WorkspaceSnapshot,
    query: WorkspaceQuery,
    selected: Option<TaskIdentity>,
    selected_tasks: HashSet<TaskIdentity>,
    range_anchor: Option<TaskIdentity>,
    search_input: Entity<InputState>,
    active_modal: Option<ActiveModal>,
    active_kit_modal: Option<ModalKind>,
    settings_panel: Option<(Entity<settings::SettingsPanelView>, Subscription)>,
    /// Discovery / verified-download state for the shared Get-aria2 panel (B4).
    core_setup: CoreSetupState,
    update_status: crate::UpdateStatusView,
    update_enabled: bool,
    update_portable: bool,
    pending_update_check: Option<RequestId>,
    pending_update_download: Option<RequestId>,
    /// Set once the user has confirmed a browser hand-off in this session. Only
    /// consulted when cookies *and* auto-submit are both on, which D-045 §6 makes
    /// the one configuration that never gets a fully silent path.
    bridge_auto_submit_unlocked: bool,
    pub(crate) titlebar_drag_pending: bool,
    pending_settings_save: Option<PendingSettingsSave>,
    pending_tracker_refresh: Option<RequestId>,
    pending_task_command: Option<PendingTaskCommand>,
    pending_global_task_command: Option<PendingGlobalTaskCommand>,
    pending_batch_command: Option<PendingBatchTaskCommand>,
    pending_load_more_stopped: bool,
    details_panel: Option<Entity<TaskDetailsPanelView>>,
    _details_panel_sub: Option<Subscription>,
    details_drawer_width: f32,
    details_sheet_open: bool,
    active_status_popover: Option<StatusBarPopover>,
    speed_popover_focus: FocusHandle,
    power_popover_focus: FocusHandle,
    activity_popover_focus: FocusHandle,
    speed_popover_previous_focus: Option<WeakFocusHandle>,
    power_popover_previous_focus: Option<WeakFocusHandle>,
    activity_popover_previous_focus: Option<WeakFocusHandle>,
    context_menu_target: Option<TaskIdentity>,
    power_had_active_tasks: bool,
    power_countdown_generation: u64,
    status_notice: Option<StatusNotice>,
    activity_log: Vec<ActivityEntryView>,
    next_activity_id: u64,
    activity_scroll: ScrollHandle,
    /// Suppresses repeated low-disk warnings until free space recovers.
    low_disk_active: bool,
    known_task_status: HashMap<TaskIdentity, TaskStatusView>,
    next_request_id: u64,
    links_input_height: f32,
    list_scroll: UniformListScrollHandle,
    focus_handle: FocusHandle,
    rendered_range: Range<usize>,
    _search_subscription: Subscription,
    _window_bounds_subscription: Subscription,
    _window_appearance_subscription: Subscription,
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
        if !cx.has_global::<gpui_kit::component::Theme>() {
            gpui_kit::init(cx);
        }
        theme.sync_to_kit(Some(window), cx);

        let initial_translator = translator_for_language(settings.language);
        let search_placeholder = initial_translator.t("search-placeholder");
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder(search_placeholder));
        let search_subscription = cx.subscribe(
            &search_input,
            |this: &mut Self, input, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let text = input.read(cx).value().to_string();
                    if this.query.search != text {
                        this.query.search = text;
                        this.clear_task_selection();
                        this.emit_query(cx);
                    }
                }
            },
        );
        let window_bounds_subscription = cx.observe_window_bounds(window, |this, window, cx| {
            this.emit_window_geometry(window, cx);
            this.sync_details_sheet(window, cx);
            cx.notify();
        });
        let window_appearance_subscription =
            cx.observe_window_appearance(window, |this, window, cx| {
                if this.settings.color_scheme == ColorSchemeView::System {
                    let theme = resolve_theme(ColorSchemeView::System, window);
                    if this.theme.mode != theme.mode || this.theme.colors != theme.colors {
                        this.theme = theme;
                        this.theme.sync_to_kit(Some(window), cx);
                        this.sync_active_modal_context(None, cx);
                        this.sync_settings_panel(None, cx);
                        this.sync_details_panel_context(cx);
                        this.sync_core_setup_panels(cx);
                        cx.notify();
                    }
                }
            });
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let window_handle = window.window_handle();
        Self {
            window_handle,
            theme,
            translator: translator_for_language(settings.language),
            settings,
            platform_capabilities: PlatformCapabilitiesView::default(),
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
            active_modal: None,
            active_kit_modal: None,
            settings_panel: None,
            core_setup: CoreSetupState::default(),
            update_status: crate::UpdateStatusView::Disabled,
            update_enabled: false,
            update_portable: false,
            pending_update_check: None,
            pending_update_download: None,
            bridge_auto_submit_unlocked: false,
            titlebar_drag_pending: false,
            pending_settings_save: None,
            pending_tracker_refresh: None,
            pending_task_command: None,
            pending_global_task_command: None,
            pending_batch_command: None,
            pending_load_more_stopped: false,
            details_panel: None,
            _details_panel_sub: None,
            details_drawer_width: DETAILS_DRAWER_WIDTH,
            details_sheet_open: false,
            active_status_popover: None,
            speed_popover_focus: cx.focus_handle(),
            power_popover_focus: cx.focus_handle(),
            activity_popover_focus: cx.focus_handle(),
            speed_popover_previous_focus: None,
            power_popover_previous_focus: None,
            activity_popover_previous_focus: None,
            context_menu_target: None,
            power_had_active_tasks: false,
            power_countdown_generation: 0,
            status_notice: None,
            activity_log: Vec::new(),
            next_activity_id: 1,
            activity_scroll: ScrollHandle::new(),
            low_disk_active: false,
            known_task_status: HashMap::new(),
            next_request_id: 1,
            links_input_height: DEFAULT_LINKS_INPUT_HEIGHT,
            list_scroll: UniformListScrollHandle::new(),
            focus_handle,
            rendered_range: 0..0,
            _search_subscription: search_subscription,
            _window_bounds_subscription: window_bounds_subscription,
            _window_appearance_subscription: window_appearance_subscription,
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
            let version_changed =
                self.snapshot.capabilities.version != snapshot.capabilities.version;
            self.snapshot.capabilities = snapshot.capabilities;
            self.snapshot.local_path_actions_available = snapshot.local_path_actions_available;
            if version_changed {
                self.sync_settings_panel(None, cx);
            }
            self.check_power_action_trigger(cx);
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
                self.details_panel.as_ref().and_then(|panel| {
                    let identity = panel.read(cx).identity.clone();
                    successor_task(&self.snapshot, &snapshot, &identity)
                })
            })
            .flatten();
        let details_revision_advanced = self.details_panel.as_ref().is_some_and(|panel| {
            let drawer = panel.read(cx);
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
            self.clear_details_panel();
            if matches!(self.active_modal, Some(ActiveModal::BatchFailureDetails(_))) {
                self.active_modal = None;
                self.sync_kit_modals_deferred(cx);
            }
        }

        if session_changed {
            if let Some(view) = self.add_dialog_view().cloned() {
                view.update(cx, |dialog, cx| {
                    if dialog.pending.take().is_some() {
                        dialog.error = Some(stale_session_error());
                        cx.notify();
                    }
                });
                self.set_add_dialog_busy(cx);
            }
            if let Some(pending) = self.pending_task_command.take() {
                if matches!(&pending.command, TaskCommandView::SetOutputName { .. }) {
                    if let Some(ActiveModal::OutputName { dialog, .. }) = &mut self.active_modal {
                        dialog.error = Some(stale_session_error());
                    }
                } else {
                    self.show_notice(self.t("notice-session-changed-command"), true, cx);
                }
            }
            if self.pending_batch_command.take().is_some() {
                self.show_notice(self.t("notice-session-changed-batch"), true, cx);
            }
            if self.pending_load_more_stopped {
                self.pending_load_more_stopped = false;
            }
            if let (Some(panel), Some(session)) = (self.details_panel.clone(), &next_session) {
                let session = session.clone();
                panel.update(cx, |drawer, cx| {
                    drawer.session = session;
                    drawer.state = TaskDetailsLoadState::Stale;
                    drawer.pending = None;
                    cx.notify();
                });
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
        if let (Some(panel), Some(successor)) = (self.details_panel.clone(), drawer_successor) {
            panel.update(cx, |drawer, cx| {
                drawer.identity = successor.identity.clone();
                drawer.overview = successor;
                drawer.state = TaskDetailsLoadState::Stale;
                drawer.pending = None;
                cx.notify();
            });
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

        if let Some(panel) = self.details_panel.clone() {
            let tasks = &self.snapshot.tasks;
            let commands_available = self.snapshot.commands_available();
            let local_path_actions_available = self.snapshot.local_path_actions_available;
            let theme = self.theme;
            let translator = self.translator.clone();
            let drawer_width = self.details_drawer_width;
            panel.update(cx, |drawer, cx| {
                if let Some(task) = tasks.iter().find(|task| task.identity == drawer.identity) {
                    let left_active_state = drawer.overview.status.uses_active_connections()
                        && !task.status.uses_active_connections();
                    drawer.overview = task.clone();
                    if left_active_state
                        && let TaskDetailsLoadState::Ready { details } = &mut drawer.state
                    {
                        let details = Arc::make_mut(details);
                        details.peers.clear();
                        details.servers.clear();
                    }
                }
                if !commands_available {
                    drawer.state = TaskDetailsLoadState::Stale;
                    drawer.pending = None;
                }
                drawer.sync_context(
                    details::TaskDetailsPanelContext {
                        theme,
                        translator,
                        commands_available,
                        change_option_available: self.snapshot.capabilities.change_option,
                        local_path_actions_available,
                        drawer_width,
                    },
                    cx,
                );
            });
        }

        let force_details_refresh =
            followed_task || session_changed || !previous_commands_available;
        let should_refresh_details = self.details_panel.is_some()
            && self.snapshot.commands_available()
            && (force_details_refresh || details_revision_advanced);
        if should_refresh_details {
            // Revision-only refreshes are rate-limited; session/follow always run.
            self.request_current_details(!force_details_refresh && details_revision_advanced, cx);
        }
        self.check_power_action_trigger(cx);
        cx.notify();
    }

    pub fn set_engine_health(&mut self, health: EngineHealthView, cx: &mut Context<Self>) {
        if self.engine_health == health {
            return;
        }
        self.engine_health = health;
        match &self.engine_health {
            EngineHealthView::Running { restarts } if *restarts > 0 => {
                let message = self.t_count("notice-engine-recovered", u64::from(*restarts));
                self.record_activity(ActivityKindView::Engine, message.clone(), None, None, 1, cx);
                self.show_automatic_notice(message, false, true, cx);
            }
            EngineHealthView::Failed { summary } => {
                let message = self.t_args(
                    "notice-engine-restart-failed",
                    &[("summary", FluentValue::from(summary.as_str()))],
                );
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
        let Some(view) = self.add_dialog_view().cloned() else {
            return;
        };
        let matches_pending = view.read(cx).pending.as_ref().is_some_and(|pending| {
            pending.request_id == result.request_id && pending.session == result.session
        });
        if !matches_pending {
            return;
        }

        view.update(cx, |dialog, cx| {
            dialog.pending = None;
            cx.notify();
        });
        self.set_add_dialog_busy(cx);
        if result.items.is_empty() {
            view.update(cx, |dialog, cx| {
                dialog.error = Some(OperationErrorView {
                    code: "application.internal".into(),
                    summary: "The add request returned no item results.".into(),
                    retryable: false,
                });
                cx.notify();
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
            self.show_notice(
                self.t_count(
                    "notice-add-accepted",
                    u64::try_from(accepted.len()).unwrap_or(u64::MAX),
                ),
                false,
                cx,
            );
            self.close_add_download(window, cx);
            return;
        }

        if accepted.is_empty() && existing_duplicates.len() == result.items.len() {
            self.show_notice(
                self.t_count(
                    "notice-add-duplicates",
                    u64::try_from(existing_duplicates.len()).unwrap_or(u64::MAX),
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
        view.update(cx, |dialog, cx| {
            dialog.metadata_files.retain(|preview| {
                retryable_metadata_paths.contains(&metadata_path_key(&preview.path))
            });
            dialog.active_metadata_file = (!dialog.metadata_files.is_empty()).then_some(0);
            dialog
                .add_input
                .update(cx, |input, cx| input.set_value(retryable_uris, window, cx));
            dialog.results = result.items;
            cx.notify();
        });
        self.show_notice(
            self.t_args(
                "notice-add-partial",
                &[
                    (
                        "accepted",
                        FluentValue::from(i64::try_from(accepted.len()).unwrap_or(i64::MAX)),
                    ),
                    (
                        "failed",
                        FluentValue::from(i64::try_from(failed_count).unwrap_or(i64::MAX)),
                    ),
                ],
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
        let Some(view) = self.add_dialog_view().cloned() else {
            return;
        };
        let matched = view.update(cx, |dialog, cx| {
            let Some(pending) = dialog.preview_pending.as_ref() else {
                return false;
            };
            if pending.request_id != result.request_id
                || pending.paths.len() != result.items.len()
                || !pending
                    .paths
                    .iter()
                    .zip(&result.items)
                    .all(|(path, item)| path == &item.path)
            {
                return false;
            }

            dialog.preview_pending = None;
            let previous_error = dialog.error.take();
            let mut failures = Vec::new();
            for item in result.items {
                match item.outcome {
                    AddDownloadMetadataPreviewOutcomeView::Ready(preview) => {
                        let key = metadata_path_key(&preview.path);
                        if dialog
                            .metadata_files
                            .iter()
                            .all(|known| metadata_path_key(&known.path) != key)
                        {
                            dialog.metadata_files.push(preview);
                        }
                    }
                    AddDownloadMetadataPreviewOutcomeView::Failed(error) => {
                        failures.push(format!("{}: {}", item.path.display(), error.summary));
                    }
                }
            }
            if dialog.active_metadata_file.is_none() && !dialog.metadata_files.is_empty() {
                dialog.active_metadata_file = Some(0);
            }
            dialog.error = match (previous_error, failures.is_empty()) {
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
            dialog.results.clear();
            cx.notify();
            true
        });
        if matched {
            self.set_add_dialog_busy(cx);
            cx.notify();
        }
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
                        self.clear_details_panel();
                        self.sync_details_sheet(window, cx);
                        self.context_menu_target = None;
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
                        self.clear_details_panel();
                        self.sync_details_sheet(window, cx);
                    }
                    TaskCommandView::SetOutputName { .. } => {
                        self.close_task_output_name(window, cx);
                    }
                    TaskCommandView::SetSpeedLimit { .. } => {
                        self.close_task_speed_limit(window, cx);
                    }
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
                    if let Some(ActiveModal::OutputName { dialog, .. }) = &mut self.active_modal {
                        dialog.error = Some(error);
                    } else {
                        self.show_notice(self.te(&error), true, cx);
                    }
                } else if matches!(result.command, TaskCommandView::SetSpeedLimit { .. }) {
                    if let Some(ActiveModal::TaskSpeedLimit { dialog, .. }) = &mut self.active_modal
                    {
                        dialog.error = Some(error);
                    } else {
                        self.show_notice(self.te(&error), true, cx);
                    }
                } else if matches!(result.command, TaskCommandView::SetOptions { .. }) {
                    if let Some(ActiveModal::TaskOptions { dialog, .. }) = &mut self.active_modal {
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
                    self.t_count(
                        "notice-batch-retry-succeeded",
                        u64::try_from(succeeded.len()).unwrap_or(u64::MAX),
                    )
                } else {
                    let action = self.t(commands::batch_task_command_label_key(result.command));
                    self.t_args(
                        "notice-batch-succeeded",
                        &[
                            ("action", FluentValue::from(action.as_str())),
                            (
                                "count",
                                FluentValue::from(
                                    i64::try_from(succeeded.len()).unwrap_or(i64::MAX),
                                ),
                            ),
                        ],
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
                    self.t_args(
                        "notice-batch-retry-partial",
                        &[
                            (
                                "succeeded",
                                FluentValue::from(
                                    i64::try_from(succeeded.len()).unwrap_or(i64::MAX),
                                ),
                            ),
                            (
                                "failed",
                                FluentValue::from(i64::try_from(failed.len()).unwrap_or(i64::MAX)),
                            ),
                        ],
                    )
                } else {
                    let action = self.t(commands::batch_task_command_label_key(result.command));
                    self.t_args(
                        "notice-batch-partial",
                        &[
                            ("action", FluentValue::from(action.as_str())),
                            (
                                "succeeded",
                                FluentValue::from(
                                    i64::try_from(succeeded.len()).unwrap_or(i64::MAX),
                                ),
                            ),
                            (
                                "failed",
                                FluentValue::from(i64::try_from(failed.len()).unwrap_or(i64::MAX)),
                            ),
                        ],
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
        self.active_modal = Some(ActiveModal::BatchFailureDetails(BatchFailureDetails {
            command,
            failures,
        }));
        self.sync_kit_modals(window, cx);
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
        if !matches!(self.active_modal, Some(ActiveModal::BatchFailureDetails(_))) {
            return;
        }
        self.active_modal = None;
        self.sync_kit_modals(window, cx);
        if !has_kit_root(window) {
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
            self.clear_details_panel();
            self.context_menu_target = None;
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
        let Some(panel) = self.details_panel.clone() else {
            return;
        };
        let mut refresh_failure = None;
        let outcome = panel.update(cx, |drawer, cx| {
            let pending = drawer.pending.as_ref()?;
            if pending.request_id != result.request_id
                || drawer.session != result.session
                || drawer.identity != result.identity
            {
                return None;
            }

            let pending = drawer.pending.take()?;
            let background_refresh = matches!(drawer.state, TaskDetailsLoadState::Ready { .. });
            match result.outcome {
                TaskDetailsOutcomeView::Ready(details) => {
                    drawer.prefill_checksum_from_options(&details.options, cx);
                    drawer.state = TaskDetailsLoadState::Ready {
                        details: Arc::from(details),
                    };
                }
                TaskDetailsOutcomeView::Failed(error) if background_refresh => {
                    refresh_failure = Some(error);
                }
                TaskDetailsOutcomeView::Failed(error) => {
                    drawer.state = TaskDetailsLoadState::Failed { error };
                }
            }
            cx.notify();
            Some((
                commands_available && drawer.overview.revision > pending.source_revision,
                drawer.refresh_coalesced,
            ))
        });

        let Some((request_again, refresh_coalesced)) = outcome else {
            return;
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
        } else if refresh_coalesced {
            self.request_current_details(true, cx);
            cx.notify();
        } else {
            cx.notify();
        }
    }

    pub fn set_task_open_result(&mut self, result: TaskOpenResultView, cx: &mut Context<Self>) {
        let Some(panel) = self.details_panel.clone() else {
            return;
        };
        let matched = panel.update(cx, |drawer, cx| {
            let Some(pending) = drawer.open_pending.as_ref() else {
                return false;
            };
            if pending.request_id != result.request_id
                || pending.target != result.target
                || drawer.session != result.session
                || drawer.identity != result.identity
            {
                return false;
            }
            drawer.open_pending = None;
            cx.notify();
            true
        });
        if !matched {
            return;
        }
        match result.outcome {
            TaskOpenOutcomeView::Success => self.show_notice(
                match result.target {
                    TaskOpenTargetView::Download => self.t("notice-opened-download"),
                    TaskOpenTargetView::Folder => self.t("notice-opened-folder"),
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

    #[cfg(test)]
    #[must_use]
    pub(crate) fn translator(&self) -> &Translator {
        &self.translator
    }

    pub(crate) fn set_language_runtime(&mut self, language: LanguagePreferenceView) {
        self.translator = translator_for_language(language);
    }

    pub(crate) fn refresh_input_placeholders(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let search_placeholder = self.t("search-placeholder");
        self.search_input.update(cx, |input, cx| {
            input.set_placeholder(search_placeholder, window, cx);
        });
    }

    pub fn settings(&self) -> &SettingsView {
        &self.settings
    }

    pub fn set_platform_capabilities(
        &mut self,
        capabilities: PlatformCapabilitiesView,
        cx: &mut Context<Self>,
    ) {
        if self.platform_capabilities == capabilities {
            return;
        }
        self.platform_capabilities = capabilities;
        self.sync_settings_panel(None, cx);
        cx.notify();
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
            app_version: crate::app_version(),
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
        self.leave_settings_page();
        self.active_status_popover = None;
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
        self.leave_settings_page();
        self.active_status_popover = None;
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
        self.leave_settings_page();
        self.active_status_popover = None;
        window.focus(&self.search_input.focus_handle(cx), cx);
        cx.notify();
    }

    pub fn set_search_query(
        &mut self,
        text: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = text.into();
        self.search_input.update(cx, |input, cx| {
            input.set_value(text.clone(), window, cx);
        });
        if self.query.search != text {
            self.query.search = text;
            self.clear_task_selection();
            self.emit_query(cx);
        }
    }

    fn clear_search(&mut self, _: &ClearSearch, window: &mut Window, cx: &mut Context<Self>) {
        if self.power_countdown().is_some() {
            self.cancel_power_countdown(cx);
        } else if self.context_menu_target.take().is_some() {
            cx.notify();
        } else if let Some(popover) = self.active_status_popover {
            match popover {
                StatusBarPopover::Activity => self.close_activity_panel(window, cx),
                StatusBarPopover::Speed => self.close_speed_popover(window, cx),
                StatusBarPopover::Power => self.close_power_popover_with_window(window, cx),
            }
        } else if matches!(self.active_modal, Some(ActiveModal::OutputName { .. })) {
            self.close_task_output_name(window, cx);
        } else if matches!(self.active_modal, Some(ActiveModal::TaskSpeedLimit { .. })) {
            self.close_task_speed_limit(window, cx);
        } else if matches!(self.active_modal, Some(ActiveModal::TaskOptions { .. })) {
            self.close_task_options(window, cx);
        } else if matches!(self.active_modal, Some(ActiveModal::RemoveConfirmation(_))) {
            self.close_remove_confirmation(window, cx);
        } else if self.page == AppPage::Settings {
            self.close_settings(window, cx);
        } else if self.details_panel.is_some() {
            // Overlay and inline drawers both dismiss before clearing search/selection.
            self.close_task_details(window, cx);
        } else if !self.search_input.read(cx).value().is_empty() {
            self.set_search_query("", window, cx);
        } else if !self.selected_tasks.is_empty() {
            self.clear_task_selection();
            cx.notify();
        } else {
            window.focus(&self.focus_handle, cx);
        }
    }

    fn task_page_step(&self) -> usize {
        self.rendered_range.len().saturating_sub(1).clamp(1, 50)
    }

    fn navigate_task_list(
        &mut self,
        extend_range: bool,
        target_for_current: impl FnOnce(Option<usize>, usize) -> usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let len = self.snapshot.tasks.len();
        if len == 0 {
            return;
        }
        let target = target_for_current(self.selected_index(), len).min(len - 1);
        self.select_at_with_modifiers(target, extend_range, false, window, cx);
    }

    fn select_next(&mut self, _: &SelectNextTask, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate_task_list(
            false,
            |current, len| current.map_or(0, |idx| (idx + 1).min(len - 1)),
            window,
            cx,
        );
    }

    fn select_previous(
        &mut self,
        _: &SelectPreviousTask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate_task_list(
            false,
            |current, len| current.map_or(len - 1, |idx| idx.saturating_sub(1)),
            window,
            cx,
        );
    }

    fn select_first(&mut self, _: &SelectFirstTask, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate_task_list(false, |_, _| 0, window, cx);
    }

    fn select_last(&mut self, _: &SelectLastTask, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate_task_list(false, |_, len| len - 1, window, cx);
    }

    fn select_page_up(
        &mut self,
        _: &SelectPageUpTask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let step = self.task_page_step();
        self.navigate_task_list(
            false,
            |current, len| current.map_or(len - 1, |idx| idx.saturating_sub(step)),
            window,
            cx,
        );
    }

    fn select_page_down(
        &mut self,
        _: &SelectPageDownTask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let step = self.task_page_step();
        self.navigate_task_list(
            false,
            |current, len| current.map_or(0, |idx| (idx + step).min(len - 1)),
            window,
            cx,
        );
    }

    fn extend_select_next(
        &mut self,
        _: &ExtendSelectNextTask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate_task_list(
            true,
            |current, len| current.map_or(0, |idx| (idx + 1).min(len - 1)),
            window,
            cx,
        );
    }

    fn extend_select_previous(
        &mut self,
        _: &ExtendSelectPreviousTask,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate_task_list(
            true,
            |current, len| current.map_or(len - 1, |idx| idx.saturating_sub(1)),
            window,
            cx,
        );
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
        if self.details_panel.is_some() {
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
            || self.active_modal.is_some()
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

    #[must_use]
    pub fn has_active_downloads(&self) -> bool {
        self.snapshot.has_active_downloads()
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
            let message = self.t_args(
                "notice-low-disk-space",
                &[
                    ("free", FluentValue::from(free.as_str())),
                    ("threshold", FluentValue::from(threshold.as_str())),
                ],
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
        if has_kit_root(window) {
            if self.desired_kit_modal() != self.active_kit_modal {
                self.sync_kit_modals_deferred(cx);
            }
            if self.desired_details_sheet_open(window) != self.details_sheet_open {
                self.sync_details_sheet_deferred(cx);
            }
        }
        let colors = self.theme.colors;
        let metadata_drop_enabled = self.can_open_metadata_paths();
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
            .on_action(cx.listener(Self::select_first))
            .on_action(cx.listener(Self::select_last))
            .on_action(cx.listener(Self::select_page_up))
            .on_action(cx.listener(Self::select_page_down))
            .on_action(cx.listener(Self::extend_select_next))
            .on_action(cx.listener(Self::extend_select_previous))
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
            .on_action(cx.listener(Self::open_edit_task_action))
            .on_action(cx.listener(Self::close_edit_task_action))
            .on_action(cx.listener(Self::submit_edit_task_action))
            .on_action(cx.listener(Self::close_batch_failure_details_action))
            .on_action(cx.listener(Self::close_core_setup_onboarding_action))
            .on_action(cx.listener(Self::remove_selected))
            .on_drag_move(cx.listener(
                |this, e: &DragMoveEvent<ResizeDetailsDrawer>, window, cx| {
                    let width = f32::from(window.viewport_size().width - e.event.position.x)
                        .clamp(DETAILS_DRAWER_MIN_WIDTH, DETAILS_DRAWER_MAX_WIDTH);
                    if (width - this.details_drawer_width).abs() > 0.5 {
                        this.details_drawer_width = width;
                        this.sync_details_panel_context(cx);
                        cx.notify();
                    }
                },
            ))
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
                    .min_w_0()
                    .flex()
                    .child(self.render_sidebar(shell_layout.sidebar_width, window, cx))
                    .child(match self.page {
                        AppPage::Downloads => self
                            .render_main(
                                shell_layout.task_layout,
                                shell_layout.details_presentation,
                                cx,
                            )
                            .into_any_element(),
                        AppPage::Settings => self
                            .settings_panel
                            .as_ref()
                            .map(|(panel, _)| panel.clone().into_any_element())
                            .unwrap_or_else(|| div().into_any_element()),
                    }),
            )
            .child(self.render_status_bar(cx))
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
