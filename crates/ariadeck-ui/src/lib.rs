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
    CloseTaskOutputName, Copy, Cut, Delete, FocusNext, FocusPrevious, FocusSearch, InsertNewline,
    MoveEnd, MoveHome, MoveLeft, MoveRight, OpenAddDownload, OpenSettings, OpenTaskDetails,
    OpenTaskOutputName, Paste, PauseSelectedTask, RemoveSelectedTask, ResumeSelectedTask,
    RetrySelectedTask, SaveSettings, SelectAll, SelectAllTasks, SelectLeft, SelectNextTask,
    SelectPreviousTask, SelectRight, SubmitAddDownload, SubmitTaskOutputName,
};
pub use assets::Assets;
pub use components::{
    Button, ButtonStyle, ButtonVariant, Dialog, Icon, IconButton, IconName, IconSize,
    LoadingIndicator, Segment, SegmentedControl, StatusIndicator, Toast, ToastKind, Tooltip,
};
pub use model::{
    AddDownloadInputModeView, AddDownloadItemResultView, AddDownloadMetadataKindView,
    AddDownloadModeView, AddDownloadRequestView, AddDownloadResultView, AddDownloadSourceView,
    BatchCommandOutcomeView, BatchTaskCommandRequestView, BatchTaskCommandResultView,
    BatchTaskCommandView, BatchTaskFailureView, ColorSchemeView, CommandOutcomeView,
    ConnectionView, DownloadProxySettingsView, DownloadRowView, EngineHealthView,
    EngineSessionView, FileConflictPolicyView, OperationErrorView, ProxyModeView,
    ProxyPasswordUpdateView, RequestId, SecretStringView, SettingsSaveOutcomeView,
    SettingsSaveRequestView, SettingsSaveResultView, SettingsView, SpeedSampleView,
    TaskCommandRequestView, TaskCommandResultView, TaskCommandView, TaskCountsView,
    TaskDetailsOutcomeView, TaskDetailsRequestView, TaskDetailsResultView, TaskDetailsView,
    TaskErrorView, TaskFileView, TaskIdentity, TaskNameStateView, TaskSourceKindView,
    TaskStatusView, WorkspaceFilter, WorkspaceQuery, WorkspaceSnapshot, format_bytes, format_eta,
    format_percent, format_rate,
};
pub use search_input::{SearchInput, SearchInputEvent, TextField, TextFieldConfig, TextFieldEvent};
pub use shell::{AppShell, AppShellEvent};
pub use theme::{Theme, ThemeColors, ThemeMode};
