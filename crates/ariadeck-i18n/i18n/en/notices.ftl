
notice-select-task-first = Select a visible task first.
notice-select-task-details = Select a visible task to open its details.
notice-no-copyable-source = This task has no copyable source.
notice-source-copied = Source copied to the clipboard.
notice-gid-copied = Task GID copied to the clipboard.
notice-gid-copied-short = GID copied.
notice-opening-path = Opening local path…
notice-open-path-unavailable = Opening task paths is available only for the managed local engine.
notice-open-path-managed-local-only = Opening task paths is available only for the managed local engine.
notice-open-path-failed = Could not open the task path: { $summary }
notice-engine-not-ready = The engine is not ready for commands.
notice-output-name-unavailable = Output names can be changed only for non-terminal direct URL tasks.
notice-speed-limit-unavailable = Speed limits can be set only while a task is downloading.
notice-task-options-unavailable = Task options can be changed only while a download is live.
notice-command-status-unavailable = { $action } is not available while the task is { $status }.
notice-batch-select-two = Select at least two visible tasks for a batch action.
notice-hidden-selection = Selected tasks are outside the current result. Clear the hidden selection or change the query.
notice-remove-unavailable = The selected task cannot be removed in the current engine state.
notice-removal-selection-changed = The task selection changed. Review it before removing tasks.
notice-force-pause-unsupported = This aria2 build does not support force pause.
notice-queue-positioning-unsupported = This aria2 build does not support queue positioning.
notice-change-option-unsupported = This aria2 build does not support changing task options.
notice-force-remove-unsupported = This aria2 build does not support force remove.
notice-batch-command-failed = The batch command failed for { $count } tasks. { $detail }
notice-settings-appearance = Appearance updated.
notice-settings-proxy = Download proxy settings saved.
notice-settings-speed = Speed limits saved.
notice-settings-transfer-policy = Transfer policy saved.
notice-settings-transfers = Transfer settings saved.
notice-settings-tracker-refreshed = Tracker list refreshed ({ $count } entries).
notice-settings-notifications = Notification preferences saved.
notice-settings-platform = Window and tray preferences saved.
notice-settings-exported = Settings exported without credentials to { $path }.
notice-settings-imported = Settings imported. Passwords stored on this device were unchanged.
notice-diagnostics-exported = Redacted diagnostics exported to { $path }.

# Tray
tray-show = Show AriaDeck
tray-pause-all = Pause all
tray-resume-all = Resume all
tray-quit = Quit AriaDeck
tray-tooltip-header-active = AriaDeck · { $status } · { $count } active
tray-tooltip-header-idle = AriaDeck · { $status } · Idle
tray-tooltip-more = +{ $count } more

# Profiles
notice-profile-select = Select a profile to activate.
notice-profile-already-active = That profile is already active.
notice-profile-switching = Switching profile…
notice-profile-at-least-one = At least one profile is required.
notice-profile-active-must-exist = The active profile must exist in the catalog.
notice-profile-saving = Saving profiles…
notice-profile-saved = Profiles saved.
notice-profile-gone = That profile is no longer in the catalog.
notice-profile-none-editing = No profile is open for editing.
notice-profile-name-empty = Profile name cannot be empty.
notice-profile-remote-endpoint = Remote profiles need a ws/wss endpoint.
notice-profile-at-least-remain = At least one profile must remain.
notice-profile-enter-aria2c = Enter a path to an aria2c executable first.
notice-profile-activated = Active profile set to { $name }. Categories, proxy, speed limits, transfer policy, and tracker list now match this profile. Restart AriaDeck to reconnect the engine.
notice-connect-before-add = Connect and finish synchronization before adding a download.
notice-enter-url = Enter at least one URL or magnet link.
notice-choose-metadata = Choose at least one Torrent or Metalink file.

notice-filename-empty = Enter a filename.
notice-filename-dot = A filename cannot be "." or "..".
notice-filename-separators = Use a filename without path separators.
notice-refresh-details = Could not refresh task details: { $summary }

# Command progress / success
cmd-pause-all-progress = Pausing all tasks…
cmd-force-pause-all-progress = Force-pausing all tasks…
cmd-resume-all-progress = Resuming all tasks…
cmd-pause-all-success = All eligible tasks paused.
cmd-force-pause-all-success = All eligible tasks force-paused.
cmd-resume-all-success = All paused tasks resumed.
cmd-batch-pause-progress = Pausing selected tasks…
cmd-batch-force-pause-progress = Force-pausing selected tasks…
cmd-batch-resume-progress = Resuming selected tasks…
cmd-batch-retry-progress = Creating new tasks from selected failed tasks…
cmd-batch-remove-progress = Removing selected tasks…
cmd-batch-force-remove-progress = Force-removing selected tasks…
cmd-batch-remove-files-progress = Removing selected tasks and moving local files to the Recycle Bin…
cmd-pause-progress = Pausing task…
cmd-force-pause-progress = Force-pausing task…
cmd-resume-progress = Resuming task…
cmd-queue-top-progress = Moving task to the top of the queue…
cmd-queue-up-progress = Moving task up in the queue…
cmd-queue-down-progress = Moving task down in the queue…
cmd-queue-bottom-progress = Moving task to the bottom of the queue…
cmd-retry-progress = Creating a new task from the failed source…
cmd-output-name-progress = Updating output name…
cmd-speed-limit-progress = Updating speed limits…
cmd-connection-policy-progress = Updating connection policy…
cmd-options-progress = Updating task options…
cmd-remove-progress = Removing task…
cmd-force-remove-progress = Force-removing task…
cmd-remove-files-progress = Removing task and moving local files to the Recycle Bin…
cmd-pause-success = Task paused.
cmd-force-pause-success = Task force-paused.
cmd-resume-success = Task resumed.
cmd-queue-top-success = Task moved to the top of the queue.
cmd-queue-up-success = Task moved up in the queue.
cmd-queue-down-success = Task moved down in the queue.
cmd-queue-bottom-success = Task moved to the bottom of the queue.
cmd-retry-success = New retry task created; the failed result was kept.
cmd-output-name-success = Output name updated.
cmd-speed-limit-success = Speed limits updated for this task.
cmd-connection-policy-success = Connection policy updated for this task.
cmd-options-success = Task options updated.
cmd-remove-success = Task removed from aria2; downloaded files were kept.
cmd-force-remove-success = Task force-removed.
cmd-remove-files-success = Task removed and local files moved to the Recycle Bin.

# B4 — aria2 core download
notice-core-downloading = Downloading and verifying aria2…
notice-core-download-installed = aria2 { $version } downloaded and verified. Activate it in Settings > Engine, then restart AriaDeck.
notice-core-download-unavailable = No verified aria2 download is published for this platform.

# D5 — completion power actions
notice-power-action-cancelled = Scheduled power action cancelled.
notice-power-action-armed = Will { $action } when all downloads complete.

# Session, engine, task & batch notices
notice-session-changed-command = The engine session changed before the command completed. Its outcome was not replayed.
notice-session-changed-batch = The engine session changed before the batch command completed. Its outcome was not replayed.
notice-engine-recovered = Local aria2 recovered after { $n } { $n ->
    [1] restart attempt
   *[other] restart attempts
}.
notice-engine-restart-failed = Local aria2 could not be restarted: { $summary }
notice-add-accepted = { $n } { $n ->
    [1] download
   *[other] downloads
} accepted by aria2.
notice-add-duplicates = { $n } { $n ->
    [1] download is
   *[other] downloads are
} already in the transfer list.
notice-add-partial = { $accepted } accepted, { $failed } need attention.
notice-batch-retry-succeeded = Created { $n } new retry { $n ->
    [1] task
   *[other] tasks
}; failed results were kept.
notice-batch-succeeded = { $action } completed for { $count } { $count ->
    [1] task
   *[other] tasks
}.
notice-batch-retry-partial = Retry created { $succeeded } new { $succeeded ->
    [1] task
   *[other] tasks
}; { $failed } failed. Original failed results were kept and unresolved items remain selected.
notice-batch-partial = { $action }: { $succeeded } succeeded, { $failed } failed. Failed tasks remain selected.
notice-opened-download = Opened the downloaded item.
notice-opened-folder = Opened the download folder.
notice-low-disk-space = Low disk space: { $free } free (threshold { $threshold }). New downloads may fail.
notice-task-completed-single = { $name } finished downloading.
notice-task-completed-multiple = { $n } downloads finished.
notice-task-failed-single = { $name } failed.
notice-task-failed-multiple = { $n } downloads failed.
notice-profile-missing-catalog = The selected profile is no longer in the catalog.
notice-profile-draft-local-added = Local profile draft added (uses managed core). Edit fields, Apply, then Save profiles.
notice-profile-draft-remote-added = Remote profile draft added. Set the endpoint, Apply, then Save profiles.
notice-profile-draft-updated = Profile updated in the draft catalog. Click Save profiles to persist.
notice-profile-removed-saving = Removed “{ $name }”. Saving catalog…
notice-core-importing = Importing aria2 core…
notice-core-linking = Linking aria2 core…
notice-core-verifying = Verifying aria2 core…
notice-core-activating = Activating aria2 core…
notice-core-rolling-back = Rolling back to last working core…
notice-core-removing = Removing aria2 core…
notice-core-imported = aria2 core imported. Activate it, then restart AriaDeck to use it.
notice-core-linked = aria2 core linked. Activate it, then restart AriaDeck to use it.
notice-core-verified = aria2 core verified.
notice-core-activated = Active aria2 core updated. Restart AriaDeck to start that version.
notice-core-rolled-back = Rolled back to the last working core. Restart AriaDeck to apply.
notice-core-removed = aria2 core removed.
notice-capability-force-pause-all-unsupported = The connected engine does not support force pause all.
notice-power-action-failed = Could not execute power action: { $summary }

# Startup recovery notices
notice-startup-settings-reset = Invalid settings were reset; the original was preserved at { $path }.
notice-startup-settings-load-failed = Settings could not be loaded and saving is disabled: { $summary }
notice-startup-profiles-reset = Invalid profile catalog was reset; the original was preserved at { $path }.
notice-startup-profiles-recovered = Profile catalog was recovered from a backup.
notice-startup-profiles-load-failed = Profile catalog could not be loaded: { $summary }
notice-startup-profile-env-failed = Profile environment could not be loaded ({ $summary }); using shared settings until fixed.
notice-startup-session-recovered = Corrupt aria2 session data was reset so downloads could start; the original was preserved at { $path }.
notice-startup-session-recovered-no-backup = Corrupt aria2 session data was reset so downloads could start.
notice-connect-before-command = Connect to the engine before running task commands.
notice-edit-task-unavailable = Editing or relinking is not available for the selected task.
notice-task-relinked = Task updated.
notice-checksum-copied = Checksum copied to clipboard.
notice-settings-network-rules = Network presets and mirror rules saved.
notice-settings-network = Network settings saved.
