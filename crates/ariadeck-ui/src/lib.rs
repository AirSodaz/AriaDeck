//! AriaDeck-owned GPUI design system and application components.

mod actions;
mod model;
mod search_input;
mod shell;
mod theme;

pub use actions::init;
pub use actions::{
    Backspace, ClearSearch, Copy, Cut, Delete, FocusNext, FocusPrevious, FocusSearch, MoveEnd,
    MoveHome, MoveLeft, MoveRight, Paste, SelectAll, SelectLeft, SelectNextTask,
    SelectPreviousTask, SelectRight, ToggleTheme,
};
pub use model::{
    ConnectionView, DownloadRowView, TaskCountsView, TaskIdentity, TaskStatusView, WorkspaceFilter,
    WorkspaceQuery, WorkspaceSnapshot, format_bytes, format_eta, format_percent, format_rate,
};
pub use search_input::{SearchInput, SearchInputEvent};
pub use shell::{AppShell, AppShellEvent};
pub use theme::{Theme, ThemeColors, ThemeMode};
