//! AriaDeck-owned GPUI design system and application components.

mod actions;
mod assets;
mod components;
mod model;
mod search_input;
mod shell;
mod theme;

pub use actions::init;
pub use actions::{
    Backspace, ClearSearch, CloseAddDownload, CloseBatchFailures, CloseSettings,
    CloseTaskOutputName, CloseTaskSpeedLimit, Copy, Cut, Delete, FocusNext, FocusPrevious,
    FocusSearch, InsertNewline, MoveEnd, MoveHome, MoveLeft, MoveRight, MoveTaskDownInQueue,
    MoveTaskToQueueBottom, MoveTaskToQueueTop, MoveTaskUpInQueue, OpenAddDownload, OpenSettings,
    OpenTaskDetails, OpenTaskOutputName, OpenTaskSpeedLimit, Paste, PauseSelectedTask,
    RemoveSelectedTask, ResumeSelectedTask, RetrySelectedTask, SaveSettings, SelectAll,
    SelectAllTasks, SelectLeft, SelectNextTask, SelectPreviousTask, SelectRight, SubmitAddDownload,
    SubmitTaskOutputName, SubmitTaskSpeedLimit,
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
    DownloadProxySettingsView, DownloadRowView, EngineCapabilitiesView, EngineHealthView,
    EngineSessionView, FileAllocationView, FileConflictPolicyView, GlobalTaskCommandRequestView,
    GlobalTaskCommandResultView, GlobalTaskCommandView, NotificationSettingsView,
    NotificationVolumeView, OperationErrorView, PlatformSettingsView, ProfileCatalogView,
    ProfileEntryView, ProfileKindView, ProfileRpcSecretUpdateView, ProxyModeView,
    ProxyPasswordUpdateView, RequestId, SaveProfileCatalogOutcomeView,
    SaveProfileCatalogRequestView, SaveProfileCatalogResultView, SecretStringView,
    SettingsSaveOutcomeView, SettingsSaveRequestView, SettingsSaveResultView, SettingsView,
    SpeedLimitSettingsView, SpeedSampleView, StoppedHistoryView, SwitchProfileOutcomeView,
    SwitchProfileRequestView, SwitchProfileResultView, TaskCommandRequestView,
    TaskCommandResultView, TaskCommandView, TaskCountsView, TaskDetailsOutcomeView,
    TaskDetailsRequestView, TaskDetailsResultView, TaskDetailsView, TaskErrorView, TaskFileView,
    TaskIdentity, TaskNameStateView, TaskOpenOutcomeView, TaskOpenRequestView, TaskOpenResultView,
    TaskOpenTargetView, TaskOptionView, TaskPathValidationView, TaskPeerView, TaskServerView,
    TaskSourceKindView, TaskStatusView, TaskTrackerView, TaskUriStatusView, TaskUriView,
    TransferPolicySettingsView, WorkspaceFilter, WorkspaceQuery, WorkspaceSnapshot,
    WorkspaceSortDirection, WorkspaceSortKey, format_bytes, format_eta, format_percent,
    format_rate, format_share_ratio, format_speed_limit_field, parse_speed_limit_field,
};
pub use search_input::{SearchInput, SearchInputEvent, TextField, TextFieldConfig, TextFieldEvent};
pub use shell::{AppShell, AppShellEvent};
pub use theme::{Theme, ThemeColors, ThemeMode};
