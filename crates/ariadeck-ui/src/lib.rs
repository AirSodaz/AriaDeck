//! AriaDeck-owned GPUI design system and application components.

mod actions;
mod model;
mod search_input;
mod shell;
mod theme;

pub use actions::init;
pub use actions::{
    Backspace, ClearSearch, CloseAddDownload, CloseSettings, Copy, Cut, Delete, FocusNext,
    FocusPrevious, FocusSearch, MoveEnd, MoveHome, MoveLeft, MoveRight, OpenAddDownload,
    OpenSettings, OpenTaskDetails, Paste, PauseSelectedTask, RemoveSelectedTask,
    ResumeSelectedTask, RetrySelectedTask, SaveSettings, SelectAll, SelectLeft, SelectNextTask,
    SelectPreviousTask, SelectRight, SubmitAddDownload, ToggleTheme,
};
pub use model::{
    AddDownloadRequestView, AddDownloadResultView, ColorSchemeView, CommandOutcomeView,
    ConnectionView, DownloadRowView, EngineSessionView, OperationErrorView, RequestId,
    SettingsSaveOutcomeView, SettingsSaveRequestView, SettingsSaveResultView, SettingsView,
    SpeedSampleView, TaskCommandRequestView, TaskCommandResultView, TaskCommandView,
    TaskCountsView, TaskDetailsOutcomeView, TaskDetailsRequestView, TaskDetailsResultView,
    TaskDetailsView, TaskFileView, TaskIdentity, TaskStatusView, WorkspaceFilter, WorkspaceQuery,
    WorkspaceSnapshot, format_bytes, format_eta, format_percent, format_rate,
};
pub use search_input::{SearchInput, SearchInputEvent, TextField, TextFieldConfig, TextFieldEvent};
pub use shell::{AppShell, AppShellEvent};
pub use theme::{Theme, ThemeColors, ThemeMode};
