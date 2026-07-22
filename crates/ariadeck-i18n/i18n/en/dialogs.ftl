## Dialogs (English)

dialog-add-download = Add download
dialog-add-input-links = Links
dialog-add-input-metadata = Torrent / Metalink
dialog-add-kind-torrent = Torrent
dialog-add-kind-metalink = Metalink
dialog-add-mode-separate = Separate tasks
dialog-add-mode-mirrors = Mirrors (one task)
dialog-add-conflict-keep-both = Keep both
dialog-add-conflict-reject = Reject
dialog-add-conflict-overwrite = Overwrite
dialog-add-input-type-aria = Download source type
dialog-add-mode-aria = Multiple link handling
dialog-add-file-conflict-aria = Existing file handling
dialog-add-cancel-aria = Cancel adding a download
dialog-add-url-or-magnet = URL or magnet link
dialog-add-no-sources = No sources detected
dialog-add-sources-detected = { $n ->
    [one] 1 source detected
   *[other] { $n } sources detected
}
dialog-add-if-file-exists = If file exists
dialog-add-choose-metadata = Choose Torrent or Metalink files
dialog-add-drop-target-aria = Torrent and Metalink file drop target
dialog-add-choose-files-aria = Choose Torrent or Metalink files
dialog-add-selected-files-aria = Selected Torrent and Metalink files
dialog-add-results-aria = Add download results
dialog-add-remove-file = Remove file
dialog-add-submit = Add download
dialog-add-submitting = Adding download…
dialog-add-hide-advanced-aria = Hide advanced download options
dialog-add-show-advanced-aria = Show advanced download options
dialog-add-hide-advanced = Hide
dialog-add-show-advanced = Show
dialog-add-advanced-hint = Applies only to direct URL downloads. Cookies and HTTP passwords stay out of task rows and logs.
dialog-add-referer = Referer
dialog-add-user-agent = User-Agent
dialog-add-custom-headers = Custom headers
dialog-add-cookie = Cookie
dialog-add-http-username = HTTP username
dialog-add-http-password = HTTP password
dialog-add-checksum = Checksum
dialog-add-metadata-row-aria = { $kind } { $name }, { $selected } of { $total } files selected
dialog-add-metadata-row-summary = { $kind } · { $selected }/{ $total } files · { $path }
dialog-add-remove-kind-aria = Remove { $kind } file
dialog-add-size-unknown = Unknown size
dialog-add-file-row-aria = File { $index }, { $path }, { $size }
dialog-add-reading-metadata = Reading metadata…
dialog-add-sources-ready = { $n ->
    [one] 1 source ready
   *[other] { $n } sources ready
}
dialog-add-clear-file-selection = Clear file selection
dialog-add-selection-summary = { $selected } of { $total } selected · { $size }
dialog-add-selection-summary-with-unknown = { $selected } of { $total } selected · { $size } · { $unknown } unknown-size files
dialog-add-source-uri = Line { $line } · { $source }
dialog-add-source-metadata = { $kind } · { $name }
dialog-add-result-accepted = Accepted
dialog-add-result-accepted-gid = Accepted · GID { $gid }
dialog-add-result-accepted-tasks = { $n ->
    [one] Accepted · 1 task
   *[other] Accepted · { $n } tasks
}

dialog-remove-title = Remove task
dialog-remove-cancel-aria = Cancel task removal
dialog-remove-confirm = Remove
dialog-remove-and-files = Remove and move files
dialog-remove-files-checkbox = Move exact task files to the Recycle Bin
dialog-remove-files-aria = Move exact task files to the Recycle Bin
dialog-remove-submit-aria = Remove task and move exact local files to the Recycle Bin
dialog-remove-description-mixed = { $name }: live tasks will be stopped and terminal records will be removed from aria2.
dialog-remove-description-live = { $name } will be stopped and retained as a removed aria2 result.
dialog-remove-description-terminal = { $name } will be removed from aria2's stopped results.
dialog-remove-description-generic = { $name } will be removed from aria2.
dialog-remove-files-warning = Selected task files will be moved to the Recycle Bin.
dialog-remove-files-kept = Downloaded files will be kept.
dialog-selected-task-count = { $n ->
    [one] 1 selected task
   *[other] { $n } selected tasks
}

dialog-batch-title = Batch action details
dialog-batch-close-aria = Close batch action details
dialog-batch-request = Batch request
dialog-batch-task-name = Task { $gid }
dialog-batch-failure-summary = { $n ->
    [one] 1 task failed. The failed task remains selected for follow-up.
   *[other] { $n } tasks failed. Failed tasks remain selected for follow-up.
}
dialog-batch-failure-list-aria = Failed { $action } tasks

dialog-output-name-title = Change output name
dialog-output-name-filename = Filename
dialog-output-name-cancel-aria = Cancel output name change
dialog-output-name-saving = Saving task output name
dialog-output-name-save = Save task output name
dialog-output-name-description = Set the filename used by aria2 for { $name }.

dialog-speed-limit-title = Set speed limits
dialog-speed-limit-download = Download limit
dialog-speed-limit-upload = Upload limit
dialog-speed-limit-cancel-aria = Cancel speed limit change
dialog-speed-limit-saving = Saving task speed limits
dialog-speed-limit-save = Save task speed limits
dialog-speed-limit-help = Applies to this download only. Leave a field blank for no limit; values accept a K/M/G suffix (for example 2M).
dialog-speed-limit-description = Throttle aria2's transfer rate for { $name }.

dialog-task-options-title = Edit task options
dialog-task-options-seed-ratio = Seed ratio
dialog-task-options-cancel-aria = Cancel task option change
dialog-task-options-saving = Saving task options
dialog-task-options-save = Save task options
dialog-task-options-seed-help = Stops seeding when the first of seed-ratio or seed-time is reached. Use 0 for seed-ratio to disable the ratio condition.
dialog-task-options-seed-unavailable = Seed-ratio and seed-time apply only to BitTorrent tasks.
dialog-task-options-seed-time = Seed time (minutes)
dialog-task-options-description = Change typed aria2 options for { $name }.

dialog-details-loading = Loading task details
dialog-details-load-failed = Could not load task details
dialog-details-stale = Details are stale
dialog-details-refresh = Refresh
dialog-details-network = Network
dialog-details-options = Options
dialog-details-tabs-aria = Task details sections
dialog-details-copy-gid-aria = Copy task GID
dialog-details-copy-gid-tooltip = Copy GID
dialog-details-source-type = Source type
dialog-details-directory = Directory
dialog-details-not-reported = Not reported
dialog-details-path-check = Local path check
dialog-details-failure = Failure
dialog-details-info-hash = Info hash
dialog-details-seed-limits = Effective seed limits
dialog-details-no-files = No files reported
dialog-details-sources = Sources and mirrors
dialog-details-trackers = Trackers
dialog-details-servers = Servers
dialog-details-close = Close details
dialog-details-share-ratio = Share ratio
dialog-details-progress = Progress
dialog-details-info = Info
dialog-details-source = Source
dialog-details-output = Output
dialog-details-aria2-details = aria2 details
dialog-details-pieces = Pieces
dialog-details-path-unavailable = Unavailable for an external or remote engine profile.
dialog-details-path-valid = Validated locally: { $existing } existing, { $missing } missing.
dialog-details-no-files-detail = aria2 did not return any file entries for this task.
dialog-details-files-aria = { $n ->
    [one] Task files, 1 item
   *[other] Task files, { $n } items
}
dialog-details-file-enabled = Enabled
dialog-details-file-skipped = Skipped
dialog-details-file-aria = { $path }, { $state }, { $size }, { $progress }
dialog-details-no-sources = No source URIs reported.
dialog-details-no-trackers = No BitTorrent trackers reported.
dialog-details-no-servers = No active HTTP, HTTPS, or FTP servers.
dialog-details-peers = Peers
dialog-details-no-peers = No active BitTorrent peers.
dialog-details-read-only-options = Read-only task options
dialog-details-no-options = No task-specific options reported.
dialog-details-drawer-aria = Task details for { $name }
dialog-details-uri-used = In use
dialog-details-uri-waiting = Mirror
dialog-details-uri-unknown = Unknown
dialog-details-tracker-tier = Announce tier { $tier }
dialog-details-server-file-rate = File { $file } · Download { $rate }
dialog-details-server-source-file-rate = From { $source } · File { $file } · Download { $rate }
dialog-details-peer-rates = Down { $download } · Up { $upload }
dialog-details-peer-seed = Seed
dialog-details-option-hidden = Hidden
dialog-details-option-empty = Empty
dialog-details-option-sensitive = Sensitive
dialog-details-seed-ratio-disabled = ratio disabled (0.0)
dialog-details-seed-ratio-value = ratio { $ratio }
dialog-details-seed-stop-rules = Stops at the first reached limit: { $ratio } · time { $time } min
