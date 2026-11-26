use gpui::{App, KeyBinding, OperatingSystem, actions};

actions!(
    ariadeck,
    [
        SelectAllTasks,
        FocusSearch,
        ClearSearch,
        SelectNextTask,
        SelectPreviousTask,
        SelectFirstTask,
        SelectLastTask,
        SelectPageUpTask,
        SelectPageDownTask,
        ExtendSelectNextTask,
        ExtendSelectPreviousTask,
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
        OpenEditTask,
        CloseEditTask,
        SubmitEditTask,
        CloseTaskOptions,
        SubmitTaskOptions,
        CloseBatchFailures,
        CloseCoreSetup,
        RemoveSelectedTask,
    ]
);

pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
    if let Some(reduced) = crate::accessibility::explicit_reduced_motion_preference() {
        cx.set_reduce_motion(reduced);
    }
    let is_mac = cx.platform_info().os == OperatingSystem::Mac;
    let pick = |mac: &'static str, other: &'static str| if is_mac { mac } else { other };
    let bindings = [
        KeyBinding::new(pick("cmd-f", "ctrl-f"), FocusSearch, None),
        KeyBinding::new(
            pick("cmd-a", "ctrl-a"),
            SelectAllTasks,
            Some("DownloadWorkspace"),
        ),
        KeyBinding::new(
            pick("cmd-n", "ctrl-n"),
            OpenAddDownload,
            Some("DownloadWorkspace"),
        ),
        KeyBinding::new("escape", CloseAddDownload, Some("AddDownloadDialog")),
        KeyBinding::new(
            pick("cmd-enter", "ctrl-enter"),
            SubmitAddDownload,
            Some("AddDownloadDialog"),
        ),
        KeyBinding::new("escape", ClearSearch, Some("SearchInput")),
        KeyBinding::new("escape", ClearSearch, Some("DownloadWorkspace")),
        KeyBinding::new("down", SelectNextTask, Some("DownloadWorkspace")),
        KeyBinding::new("up", SelectPreviousTask, Some("DownloadWorkspace")),
        KeyBinding::new("home", SelectFirstTask, Some("DownloadWorkspace")),
        KeyBinding::new("end", SelectLastTask, Some("DownloadWorkspace")),
        KeyBinding::new("pageup", SelectPageUpTask, Some("DownloadWorkspace")),
        KeyBinding::new("pagedown", SelectPageDownTask, Some("DownloadWorkspace")),
        KeyBinding::new(
            "shift-down",
            ExtendSelectNextTask,
            Some("DownloadWorkspace"),
        ),
        KeyBinding::new(
            "shift-up",
            ExtendSelectPreviousTask,
            Some("DownloadWorkspace"),
        ),
        KeyBinding::new("enter", OpenTaskDetails, Some("DownloadWorkspace")),
        KeyBinding::new(
            pick("cmd-shift-p", "ctrl-shift-p"),
            PauseSelectedTask,
            Some("DownloadWorkspace"),
        ),
        KeyBinding::new(
            pick("cmd-shift-r", "ctrl-shift-r"),
            ResumeSelectedTask,
            Some("DownloadWorkspace"),
        ),
        KeyBinding::new(
            pick("cmd-alt-r", "ctrl-alt-r"),
            RetrySelectedTask,
            Some("DownloadWorkspace"),
        ),
        KeyBinding::new(
            pick("cmd-shift-home", "ctrl-shift-home"),
            MoveTaskToQueueTop,
            Some("DownloadWorkspace"),
        ),
        KeyBinding::new(
            pick("cmd-shift-up", "ctrl-shift-up"),
            MoveTaskUpInQueue,
            Some("DownloadWorkspace"),
        ),
        KeyBinding::new(
            pick("cmd-shift-down", "ctrl-shift-down"),
            MoveTaskDownInQueue,
            Some("DownloadWorkspace"),
        ),
        KeyBinding::new(
            pick("cmd-shift-end", "ctrl-shift-end"),
            MoveTaskToQueueBottom,
            Some("DownloadWorkspace"),
        ),
        KeyBinding::new("f2", OpenTaskOutputName, Some("DownloadWorkspace")),
        KeyBinding::new("delete", RemoveSelectedTask, Some("DownloadWorkspace")),
        KeyBinding::new("escape", CloseTaskOutputName, Some("TaskOutputNameDialog")),
        KeyBinding::new("enter", SubmitTaskOutputName, Some("TaskOutputNameDialog")),
        KeyBinding::new(
            pick("cmd-enter", "ctrl-enter"),
            SubmitTaskOutputName,
            Some("TaskOutputNameDialog"),
        ),
        KeyBinding::new("escape", CloseTaskSpeedLimit, Some("TaskSpeedLimitDialog")),
        KeyBinding::new("enter", SubmitTaskSpeedLimit, Some("TaskSpeedLimitDialog")),
        KeyBinding::new(
            pick("cmd-enter", "ctrl-enter"),
            SubmitTaskSpeedLimit,
            Some("TaskSpeedLimitDialog"),
        ),
        KeyBinding::new("escape", CloseTaskOptions, Some("TaskOptionsDialog")),
        KeyBinding::new("enter", SubmitTaskOptions, Some("TaskOptionsDialog")),
        KeyBinding::new(
            pick("cmd-enter", "ctrl-enter"),
            SubmitTaskOptions,
            Some("TaskOptionsDialog"),
        ),
        KeyBinding::new("escape", CloseEditTask, Some("EditTaskDialog")),
        KeyBinding::new(
            pick("cmd-enter", "ctrl-enter"),
            SubmitEditTask,
            Some("EditTaskDialog"),
        ),
        KeyBinding::new("escape", CloseBatchFailures, Some("BatchFailureDialog")),
        // Escape dismisses the first-run guide, which counts as "you were asked".
        KeyBinding::new("escape", CloseCoreSetup, Some("CoreSetupDialog")),
        KeyBinding::new(
            pick("cmd-,", "ctrl-,"),
            OpenSettings,
            Some("DownloadWorkspace"),
        ),
        KeyBinding::new("escape", CloseSettings, Some("SettingsPage")),
        KeyBinding::new(
            pick("cmd-enter", "ctrl-enter"),
            SaveSettings,
            Some("SettingsPage"),
        ),
    ];
    cx.bind_keys(bindings);
}
