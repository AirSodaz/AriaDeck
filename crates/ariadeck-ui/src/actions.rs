use gpui::{App, KeyBinding, actions};

actions!(
    ariadeck,
    [
        Backspace,
        Delete,
        MoveLeft,
        MoveRight,
        SelectLeft,
        SelectRight,
        SelectAll,
        MoveHome,
        MoveEnd,
        Paste,
        Cut,
        Copy,
        FocusSearch,
        ClearSearch,
        SelectNextTask,
        SelectPreviousTask,
        ToggleTheme,
        FocusNext,
        FocusPrevious,
    ]
);

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("SearchInput")),
        KeyBinding::new("delete", Delete, Some("SearchInput")),
        KeyBinding::new("left", MoveLeft, Some("SearchInput")),
        KeyBinding::new("right", MoveRight, Some("SearchInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("SearchInput")),
        KeyBinding::new("shift-right", SelectRight, Some("SearchInput")),
        KeyBinding::new("cmd-a", SelectAll, Some("SearchInput")),
        KeyBinding::new("home", MoveHome, Some("SearchInput")),
        KeyBinding::new("end", MoveEnd, Some("SearchInput")),
        KeyBinding::new("cmd-v", Paste, Some("SearchInput")),
        KeyBinding::new("cmd-x", Cut, Some("SearchInput")),
        KeyBinding::new("cmd-c", Copy, Some("SearchInput")),
        KeyBinding::new("cmd-f", FocusSearch, None),
        KeyBinding::new("escape", ClearSearch, Some("SearchInput")),
        KeyBinding::new("escape", ClearSearch, Some("DownloadWorkspace")),
        KeyBinding::new("down", SelectNextTask, Some("DownloadWorkspace")),
        KeyBinding::new("up", SelectPreviousTask, Some("DownloadWorkspace")),
        KeyBinding::new("cmd-shift-t", ToggleTheme, Some("DownloadWorkspace")),
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrevious, None),
    ]);
}
