//! AriaDeck-owned GPUI design system and application components.

mod accessibility;
mod actions;
mod assets;
mod components;
mod model;
mod search_input;
mod shell;
mod theme;

pub use accessibility::{prefers_reduced_motion, set_prefers_reduced_motion_override};
pub use ariadeck_i18n::{LanguagePreference, LocaleId, Translator};

pub use actions::init;
pub use actions::{
    Backspace, ClearSearch, CloseAddDownload, CloseBatchFailures, CloseSettings, CloseTaskOptions,
    CloseTaskOutputName, CloseTaskSpeedLimit, Copy, Cut, Delete, FocusNext, FocusPrevious,
    FocusSearch, InsertNewline, MoveEnd, MoveHome, MoveLeft, MoveRight, MoveTaskDownInQueue,
    MoveTaskToQueueBottom, MoveTaskToQueueTop, MoveTaskUpInQueue, OpenAddDownload, OpenSettings,
    OpenTaskDetails, OpenTaskOutputName, OpenTaskSpeedLimit, Paste, PauseSelectedTask,
    RemoveSelectedTask, ResumeSelectedTask, RetrySelectedTask, SaveSettings, SelectAll,
    SelectAllTasks, SelectLeft, SelectNextTask, SelectPreviousTask, SelectRight, SubmitAddDownload,
    SubmitTaskOptions, SubmitTaskOutputName, SubmitTaskSpeedLimit,
};
pub use assets::Assets;
pub use components::{
    Button, ButtonStyle, ButtonVariant, Dialog, Icon, IconButton, IconName, IconSize,
    LoadingIndicator, Segment, SegmentedControl, StatusIndicator, Toast, ToastKind, Toggle,
    Tooltip,
};
pub use model::{
    ActivityEntryView, ActivityKindView, AddDownloadAdvancedOptionsView, AddDownloadInputModeView,
    AddDownloadItemResultView, AddDownloadMetadataFileView, AddDownloadMetadataKindView,
    AddDownloadMetadataPreviewItemView, AddDownloadMetadataPreviewOutcomeView,
    AddDownloadMetadataPreviewRequestView, AddDownloadMetadataPreviewResultView,
    AddDownloadMetadataPreviewView, AddDownloadModeView, AddDownloadRequestView,
    AddDownloadResultView, AddDownloadSourceView, BatchCommandOutcomeView,
    BatchTaskCommandRequestView, BatchTaskCommandResultView, BatchTaskCommandView,
    BatchTaskFailureView, CloseBehaviorView, ColorSchemeView, CommandOutcomeView, ConnectionView,
    CoreCommandOutcomeView, CoreCommandRequestView, CoreCommandResultView, CoreCommandView,
    CoreInstallStatusView, CoreInstallationView, CoreRegistryView, CoreSourceView,
    DiagnosticExportOutcomeView, DiagnosticExportRequestView, DiagnosticExportResultView,
    DownloadCategoryView, DownloadProxySettingsView, DownloadRowView, EngineCapabilitiesView,
    EngineHealthView, EngineSessionView, FileAllocationView, FileConflictPolicyView, FormatOptions,
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
    WorkspaceSnapshot, WorkspaceSortDirection, WorkspaceSortKey,
    active_format_options, format_bytes, format_bytes_with, format_eta, format_eta_with,
    format_percent, format_percent_with, format_rate, format_rate_with, format_relative_time,
    format_relative_time_with, format_share_ratio, format_share_ratio_with,
    format_speed_limit_field, parse_speed_limit_field, set_active_format_options,
};
pub use search_input::{SearchInput, SearchInputEvent, TextField, TextFieldConfig, TextFieldEvent};
pub use shell::{AppShell, AppShellEvent};
pub use theme::{Theme, ThemeColors, ThemeMode};
