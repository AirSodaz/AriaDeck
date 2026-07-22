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
        InsertNewline,
        SelectAll,
        SelectAllTasks,
        MoveHome,
        MoveEnd,
        Paste,
        Cut,
        Copy,
        FocusSearch,
        ClearSearch,
        SelectNextTask,
        SelectPreviousTask,
        OpenAddDownload,
        CloseAddDownload,
        SubmitAddDownload,
        OpenTaskDetails,
        OpenSettings,
        CloseSettings,
        SaveSettings,
        PauseSelectedTask,
        ResumeSelectedTask,
        RetrySelectedTask,
        MoveTaskToQueueTop,
        MoveTaskUpInQueue,
        MoveTaskDownInQueue,
        MoveTaskToQueueBottom,
        OpenTaskOutputName,
        CloseTaskOutputName,
        SubmitTaskOutputName,
        OpenTaskSpeedLimit,
        CloseTaskSpeedLimit,
        SubmitTaskSpeedLimit,
        CloseTaskOptions,
        SubmitTaskOptions,
        CloseBatchFailures,
        RemoveSelectedTask,
        FocusNext,
        FocusPrevious,
    ]
);

/// Shared key context for every `TextField` so paste/copy/select-all work everywhere.
pub const TEXT_FIELD_KEY_CONTEXT: &str = "TextField";

fn text_field_bindings() -> Vec<KeyBinding> {
    let context = Some(TEXT_FIELD_KEY_CONTEXT);
    vec![
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("left", MoveLeft, context),
        KeyBinding::new("right", MoveRight, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("secondary-a", SelectAll, context),
        KeyBinding::new("home", MoveHome, context),
        KeyBinding::new("end", MoveEnd, context),
        KeyBinding::new("secondary-v", Paste, context),
        KeyBinding::new("secondary-x", Cut, context),
        KeyBinding::new("secondary-c", Copy, context),
        // Common Windows/Linux aliases for clipboard when secondary-* is unavailable.
        KeyBinding::new("ctrl-v", Paste, context),
        KeyBinding::new("ctrl-c", Copy, context),
        KeyBinding::new("ctrl-x", Cut, context),
        KeyBinding::new("ctrl-a", SelectAll, context),
        KeyBinding::new("shift-enter", InsertNewline, context),
    ]
}

pub fn init(cx: &mut App) {
    let mut bindings = text_field_bindings();
    bindings.extend([
        KeyBinding::new("cmd-f", FocusSearch, None),
        KeyBinding::new("secondary-a", SelectAllTasks, Some("DownloadWorkspace")),
        KeyBinding::new("cmd-n", OpenAddDownload, Some("DownloadWorkspace")),
        KeyBinding::new("escape", CloseAddDownload, Some("AddDownloadDialog")),
        KeyBinding::new("enter", SubmitAddDownload, Some("AddDownloadDialog")),
        KeyBinding::new("cmd-enter", SubmitAddDownload, Some("AddDownloadDialog")),
        KeyBinding::new("escape", ClearSearch, Some("SearchInput")),
        KeyBinding::new("escape", ClearSearch, Some("DownloadWorkspace")),
        KeyBinding::new("down", SelectNextTask, Some("DownloadWorkspace")),
        KeyBinding::new("up", SelectPreviousTask, Some("DownloadWorkspace")),
        KeyBinding::new("enter", OpenTaskDetails, Some("DownloadWorkspace")),
        KeyBinding::new("cmd-shift-p", PauseSelectedTask, Some("DownloadWorkspace")),
        KeyBinding::new("cmd-shift-r", ResumeSelectedTask, Some("DownloadWorkspace")),
        KeyBinding::new("cmd-alt-r", RetrySelectedTask, Some("DownloadWorkspace")),
        KeyBinding::new(
            "cmd-shift-home",
            MoveTaskToQueueTop,
            Some("DownloadWorkspace"),
        ),
        KeyBinding::new("cmd-shift-up", MoveTaskUpInQueue, Some("DownloadWorkspace")),
        KeyBinding::new(
            "cmd-shift-down",
            MoveTaskDownInQueue,
            Some("DownloadWorkspace"),
        ),
        KeyBinding::new(
            "cmd-shift-end",
            MoveTaskToQueueBottom,
            Some("DownloadWorkspace"),
        ),
        KeyBinding::new("f2", OpenTaskOutputName, Some("DownloadWorkspace")),
        KeyBinding::new("delete", RemoveSelectedTask, Some("DownloadWorkspace")),
        KeyBinding::new("escape", CloseTaskOutputName, Some("TaskOutputNameDialog")),
        KeyBinding::new("enter", SubmitTaskOutputName, Some("TaskOutputNameDialog")),
        KeyBinding::new(
            "cmd-enter",
            SubmitTaskOutputName,
            Some("TaskOutputNameDialog"),
        ),
        KeyBinding::new("escape", CloseTaskSpeedLimit, Some("TaskSpeedLimitDialog")),
        KeyBinding::new("enter", SubmitTaskSpeedLimit, Some("TaskSpeedLimitDialog")),
        KeyBinding::new(
            "cmd-enter",
            SubmitTaskSpeedLimit,
            Some("TaskSpeedLimitDialog"),
        ),
        KeyBinding::new("escape", CloseTaskOptions, Some("TaskOptionsDialog")),
        KeyBinding::new("enter", SubmitTaskOptions, Some("TaskOptionsDialog")),
        KeyBinding::new("cmd-enter", SubmitTaskOptions, Some("TaskOptionsDialog")),
        KeyBinding::new("escape", CloseBatchFailures, Some("BatchFailureDialog")),
        KeyBinding::new("cmd-,", OpenSettings, Some("DownloadWorkspace")),
        KeyBinding::new("escape", CloseSettings, Some("SettingsPage")),
        KeyBinding::new("cmd-enter", SaveSettings, Some("SettingsPage")),
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrevious, None),
    ]);
    cx.bind_keys(bindings);
}
