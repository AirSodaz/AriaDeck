//! AriaDeck-owned GPUI design system and application components.

pub extern crate gpui_kit as gpui;

pub use gpui_kit::component;
pub use gpui_kit::*;

mod accessibility;
mod actions;
mod app_assets;
mod components;
mod model;
mod shell;
mod theme;

pub use accessibility::{prefers_reduced_motion, set_prefers_reduced_motion_override};
pub use ariadeck_i18n::{LanguagePreference, LocaleId, Translator};

pub use actions::init;
pub use actions::{
    Backspace, ClearSearch, CloseAddDownload, CloseBatchFailures, CloseCoreSetup, CloseSettings,
    CloseTaskOptions, CloseTaskOutputName, CloseTaskSpeedLimit, Copy, Cut, Delete, FocusNext,
    FocusPrevious, FocusSearch, InsertNewline, MoveEnd, MoveHome, MoveLeft, MoveRight,
    MoveTaskDownInQueue, MoveTaskToQueueBottom, MoveTaskToQueueTop, MoveTaskUpInQueue,
    OpenAddDownload, OpenSettings, OpenTaskDetails, OpenTaskOutputName, OpenTaskSpeedLimit, Paste,
    PauseSelectedTask, RemoveSelectedTask, ResumeSelectedTask, RetrySelectedTask, SaveSettings,
    SelectAll, SelectAllTasks, SelectLeft, SelectNextTask, SelectPreviousTask, SelectRight,
    SubmitAddDownload, SubmitTaskOptions, SubmitTaskOutputName, SubmitTaskSpeedLimit,
};
pub use app_assets::Assets;
pub use components::{
    Badge, Button, ButtonStyle, ButtonVariant, Checkbox, Dialog, Icon, IconButton, IconName,
    IconSize, Kbd, LoadingIndicator, Notification, Progress, ProgressCircle, Segment,
    SegmentedControl, Spinner, StatusIndicator, Switch, Tab, TabBar, TabVariant, Tag, TagVariant,
    Toast, ToastKind, Toggle, Tooltip,
};
pub use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
pub use model::{
    ActivityEntryView, ActivityKindView, AddDownloadAdvancedOptionsView, AddDownloadInputModeView,
    AddDownloadItemResultView, AddDownloadMetadataFileView, AddDownloadMetadataKindView,
    AddDownloadMetadataPreviewItemView, AddDownloadMetadataPreviewOutcomeView,
    AddDownloadMetadataPreviewRequestView, AddDownloadMetadataPreviewResultView,
    AddDownloadMetadataPreviewView, AddDownloadModeView, AddDownloadRequestView,
    AddDownloadResultView, AddDownloadSourceView, BatchCommandOutcomeView,
    BatchTaskCommandRequestView, BatchTaskCommandResultView, BatchTaskCommandView,
    BatchTaskFailureView, BridgeDownloadView, BrowserBridgeSettingsView, CloseBehaviorView,
    ColorSchemeView, CommandOutcomeView, CompletionPowerActionView, ConnectionView,
    CoreCommandOutcomeView, CoreCommandRequestView, CoreCommandResultView, CoreCommandView,
    CoreDiscoveryOriginView, CoreDiscoveryResultView, CoreDownloadOfferView,
    CoreDownloadResultView, CoreInstallStatusView, CoreInstallationView, CoreRegistryView,
    CoreSourceView, DiagnosticExportOutcomeView, DiagnosticExportRequestView,
    DiagnosticExportResultView, DiscoveredCoreView, DownloadCategoryView,
    DownloadProxySettingsView, DownloadRowView, EngineCapabilitiesView, EngineHealthView,
    EngineSessionView, FileAllocationView, FileConflictPolicyView, FormatOptions,
    GlobalTaskCommandRequestView, GlobalTaskCommandResultView, GlobalTaskCommandView,
    LanguagePreferenceView, NotificationSettingsView, NotificationVolumeView, OperationErrorView,
    PlatformSettingsView, ProfileCatalogView, ProfileEntryView, ProfileKindView,
    ProfileRpcSecretUpdateView, ProxyModeView, ProxyPasswordUpdateView, RequestId,
    SaveProfileCatalogOutcomeView, SaveProfileCatalogRequestView, SaveProfileCatalogResultView,
    SecretStringView, SettingsExportOutcomeView, SettingsExportRequestView,
    SettingsExportResultView, SettingsImportOutcomeView, SettingsImportRequestView,
    SettingsImportResultView, SettingsSaveOutcomeView, SettingsSaveRequestView,
    SettingsSaveResultView, SettingsView, SpeedLimitSettingsView, SpeedSampleView,
    StoppedHistoryView, SwitchProfileOutcomeView, SwitchProfileRequestView,
    SwitchProfileResultView, TaskCommandRequestView, TaskCommandResultView, TaskCommandView,
    TaskCountsView, TaskDetailsOutcomeView, TaskDetailsRequestView, TaskDetailsResultView,
    TaskDetailsView, TaskErrorView, TaskFileView, TaskIdentity, TaskNameStateView,
    TaskOpenOutcomeView, TaskOpenRequestView, TaskOpenResultView, TaskOpenTargetView,
    TaskOptionView, TaskPathValidationView, TaskPeerView, TaskServerView, TaskSourceKindView,
    TaskStatusView, TaskTrackerView, TaskUriStatusView, TaskUriView, TrackerListSettingsView,
    TrackerListSourceView, TransferPolicySettingsView, WorkspaceFilter, WorkspaceQuery,
    WorkspaceSnapshot, WorkspaceSortDirection, WorkspaceSortKey, active_format_options,
    format_bytes, format_bytes_with, format_eta, format_eta_with, format_percent,
    format_percent_with, format_rate, format_rate_with, format_relative_time,
    format_relative_time_with, format_share_ratio, format_share_ratio_with,
    format_speed_limit_field, parse_speed_limit_field, set_active_format_options,
};
pub use shell::{AppShell, AppShellEvent};
pub use theme::{Theme, ThemeColors, ThemeMode};
