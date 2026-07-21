# AriaDeck Post-MVP Progress

**Status:** active

**Last updated:** 2026-07-21

This document is the persistent task state for the post-MVP usability and
download-management work. It records decisions, dependencies, implementation
status, and verification evidence. A task is not complete until its behavior is
implemented and the relevant tests or runtime checks are recorded here.

## Working Rules

- Use the current workspace as the implementation source of truth.
- Research a comparable product or the aria2 contract before making a product
  choice that changes user-visible behavior.
- Prefer provider-neutral application contracts; keep aria2-specific options in
  the RPC adapter boundary.
- Keep local and remote engine paths distinct. The UI must not assume that a
  remote path is accessible from the desktop machine.
- Every completed item must include a verification command or runtime check.

## Decision Log

### D-001 - Filename resolution is engine-owned

**Decision:** Add the task first, display a resolving state, then update the
display name from aria2 task metadata. A URL-derived name may be shown only as
an estimate. Do not issue a second HEAD/GET probe from AriaDeck.

**Override rule:** Support a custom output name for direct URI tasks through the
aria2 `out` option. Do not present that field as a universal rename operation:
aria2 documents `out` as unsupported for BitTorrent and Metalink downloads.
Torrent/Metalink tasks use directory and per-file selection/output rules.

**Identity rule:** Track `followedBy`/`belongsTo` relationships for Magnet and
metadata-following tasks so selection, details, retry, and notices survive a
GID transition.

**Evidence:**

- aria2 manual: https://aria2.github.io/manual/en/html/aria2c.html
- Motrix feature set: https://github.com/agalwood/Motrix/blob/master/README.md
- Current adapter derives a name from BT metadata or the first file path:
  `crates/ariadeck-rpc/src/models.rs`.

### D-002 - Multi-line URL input means multiple tasks

**Decision:** One non-empty line creates one task. A separate advanced control
is required to treat multiple URIs as mirrors for one task. This matches aria2's
contract, where multiple URIs in one `addUri` item are alternate sources for the
same entity.

**Input rule:** Trim every line and ignore blank separator lines. An all-blank
submission is invalid. In separate-task mode, the first normalized occurrence
is submitted and later duplicates are reported as skipped item results. Mirror
mode is explicit and submits the unique source group as one aria2 task.

**Unknown-outcome rule:** Capture the authoritative task identities before
submission. If `addUri` has an unknown outcome, force a full refresh and resolve
only a matching new GID by normalized URI or Magnet info hash. A pre-existing
matching task cannot resolve the submission. If a connected, non-stale refresh
observes no new match, the source is safe to retry; if the refresh is not
authoritative, keep the outcome unknown and do not replay automatically.

**Evidence:**

- aria2 RPC manual: https://aria2.github.io/manual/en/html/aria2c.html -
  `aria2.addUri` URI arrays must point to the same resource; mixing different
  resources may fail or corrupt the download.
- Motrix README: https://github.com/agalwood/Motrix/blob/master/README.md -
  multi-connection behavior is exposed per task.
- Free Download Manager features: https://www.freedownloadmanager.org/features.htm -
  simultaneous mirror downloads are exposed as one download.

### D-003 - Selection is query-scoped and identity-based

**Decision:** Keep focused row, range anchor, and selected identity set as
separate state. Select-all applies to the current loaded query result, not
unloaded historical pages. Refresh and sorting preserve selection; profile or
query changes clear it. Batch actions report eligible, skipped, succeeded, and
failed items separately.

### D-004 - Proxy is initially a profile download setting

**Decision:** First implement aria2 download proxy configuration, separate from
the AriaDeck RPC connection proxy and future update/core-download proxy. Support
explicit disabled/manual modes, protocol-specific values where aria2 supports
them, no-proxy domains, and credential references. Proxy passwords must not be
written to JSON settings or logs.

**Runtime rule:** Apply download proxy changes through the session-bound
`aria2.changeGlobalOption` path. Clear every endpoint, bypass, username, and
password option explicitly when Disabled is selected. A timeout or disconnect
is an unknown mutation outcome and is not replayed in the same engine session;
a newly connected session receives the latest persisted configuration once.

**Credential rule:** Persist only a generated credential reference in settings
schema v2. Store the password in the operating-system credential manager,
require an explicit clear action, mask the password field and debug values, and
roll the credential back if RPC application or JSON persistence fails. A missing
credential-manager entry is surfaced instead of silently applying an
unauthenticated proxy.

**Evidence:**

- aria2 1.37.0 local help confirms `all/http/https/ftp-proxy`, `no-proxy`,
  `*-proxy-user`, and `*-proxy-passwd` option names.
- The real aria2 flow recorded below handles a Basic `407` challenge through
  the configured credentials, routes one request through the proxy, bypasses
  loopback through `no-proxy`, and returns to direct traffic after Disabled.

### D-005 - Removal and file deletion are separate commands

**Decision:** Removing an aria2 task/result never implies deleting files. Local
file deletion requires an explicit confirmation and filesystem capability. A
remote engine can only remove its task through the current RPC boundary until a
separate remote file-management capability exists.

### D-006 - Retry is an option-preserving replacement task

**Decision:** Retry creates a new aria2 task and GID; it never resumes or
mutates the failed result, which remains visible until explicitly removed. The
RPC adapter reads the failed task's `getOption` and `getUris` projections,
replays the returned task options and mirrors through `addUri`, and strips the
old `gid` and `pause` values so the replacement is new and starts immediately.
If a failed direct task reports aria2's explicit "No URI data is available"
error, the adapter falls back to the source already held in the task snapshot.

**Option rule:** Preserve aria2 task options including `dir`, `out`, HTTP
headers (including `Cookie:`), HTTP credentials, proxy values, task limits and
connection settings, checksum, and file selection. `load-cookies` is not
returned as task state by aria2; it belongs to the engine cookie configuration
and continues to apply within the same engine. Credentials and option values
stay inside the RPC adapter and are not projected into UI results or logs.

**Unknown-outcome rule:** Capture the pre-command GID set. If the replacement
`addUri` result is unknown, perform one authoritative refresh and resolve only
a matching new GID by normalized URI or Magnet info hash. Existing tasks and
new GIDs already returned by successful batch items are excluded. If no match
is observed, expose a safe manual retry; never replay automatically.

**Evidence:**

- aria2 RPC manual: https://aria2.github.io/manual/en/html/aria2c.html -
  `getOption` returns per-download options, `getUris` returns sources, and
  `addUri` accepts task options including multi-value `header`/`index-out`.
- Real aria2 1.37.0 verification recorded below confirms option preservation,
  distinct GIDs, and retention of the old failed result.

### D-007 - Task removal keeps files unless local deletion is explicit

**Decision:** Removing a live aria2 task or a stopped result keeps downloaded
files by default. A confirmation may explicitly request local file deletion,
but AriaDeck moves content to the operating-system Recycle Bin/Trash instead of
permanently deleting it. Live tasks are stopped before any file operation;
stopped records remain a distinct removal operation.

**Capability rule:** File operations are available only for the locally managed
aria2 profile. An external RPC profile is treated as remote even when its paths
look local; AriaDeck does not assume that the desktop and engine share a
filesystem. Remote confirmations state that engine-host files will be kept.

**Path rule:** Build deletion candidates from the exact `tellStatus.files`
projection, not a display name or URL. Every existing candidate must be a
regular file beneath both the configured local download root and the task
directory after canonicalization. Reject parent traversal, symlinks/reparse
points, the download root itself, directories, and paths outside either root.
For incomplete tasks, include existing aria2 control-file candidates derived
from the exact file paths and their common top-level task path. Missing files
are harmless and unrelated directory contents are never swept recursively.

**Failure rule:** Preflight before mutating aria2. Revalidate immediately before
moving each path to Trash. If a live-task removal has an unknown result, refresh
once and move files only after the task is authoritatively observed as removed
or absent. Batch operations retain per-item failures and never replay an
unknown mutation automatically.

**Evidence:**

- aria2 RPC manual: `aria2.remove` stops a live download and marks its result
  removed; `aria2.removeDownloadResult` only removes terminal state from memory.
- Motrix deletion dialog defaults to task removal and exposes a separate
  "Delete with Files" checkbox; its local adapter also guards against deleting
  the task directory for unresolved Magnets and handles `.aria2` files.
- qBittorrent's confirmation separates "Remove torrent" from the explicit
  "Also remove the content files" option.

### D-008 - Output conflicts are explicit and local preflight is capability-gated

**Decision:** New tasks default to "Keep both": force `allow-overwrite=false`
and `auto-file-renaming=true`. Users may instead choose "Reject"
(`false`/`false`) or the explicitly destructive "Overwrite" (`true`/`false`).
The typed policy overrides conflicting values in the generic option bag.

**Protocol rule:** aria2 auto-renaming applies only to HTTP(S)/FTP. Magnet,
Torrent, and Metalink tasks still keep forced overwrite disabled under the
default policy; their per-file conflict and containment flow must be resolved
after metadata is available.

**Filesystem rule:** Only the managed local engine receives a destination
gateway. It requires an absolute directory, rejects traversal and symlinked
destinations, performs and removes a real write probe, reads available space,
and rejects a known required size that exceeds it. External RPC paths are
persisted and sent to the engine without creating or probing a desktop-local
directory. URL and Magnet sizes are commonly unknown before submission, so
exact size enforcement must run again after metadata becomes authoritative.

**Evidence:**

- aria2 manual: `allow-overwrite` defaults to false; `auto-file-renaming`
  defaults to true and adds `.1` through `.9999` for HTTP(S)/FTP conflicts.
- Motrix keeps the same `allow-overwrite=false` and
  `auto-file-renaming=true` defaults.
- Real aria2 1.37.0 verification below confirms a second same-name HTTP
  download is written with the `.1` suffix.

### D-009 - Exact space checks happen before allocation when size is known

**Decision:** A managed-local add request with an authoritative required size
is rejected before engine mutation when the destination has insufficient free
space. URL and Magnet submissions normally have no authoritative size at that
point and continue after directory/space availability preflight.

**Runtime rule:** Do not automatically pause or remove a task by comparing its
later `totalLength` with current free space. aria2 may already have allocated
the output, so that comparison can count reserved bytes twice. aria2 owns
runtime allocation and aborts immediately with error code 9 on actual disk
exhaustion; AriaDeck preserves that error and presents a specific
"Not enough disk space" reason in the task row and details drawer.

**Root rule:** Every local destination that passes preflight is appended to a
shared authorized-root registry. Changing settings keeps earlier roots valid
for old-task file removal, while directories that were never configured remain
outside deletion capability.

**Evidence:**

- aria2's `AbstractDiskWriter` maps allocation/write `ENOSPC` and Windows disk
  full errors to `NOT_ENOUGH_DISK_SPACE` (error code 9) and aborts the download.
- qBittorrent reports selected size and free space in its add-torrent dialog
  and continuously publishes free space, without a separate client-side
  auto-pause policy in its checker.

### D-010 - Mutation outcomes are reconciled, never guessed or replayed

**Decision:** Keep task mutation submission single-flight in the UI. A success
or outcome-unknown result schedules an authoritative refresh, while a known
rejection leaves the existing snapshot unchanged. Never replay an unknown
mutation automatically: the engine may already have accepted it. Output-name
changes use a targeted task refresh; other task commands use a full refresh.

**Session rule:** Command responses are accepted only for the exact request and
engine session that produced them. A stale response cannot mutate the current
selection, notice, details drawer, or task store.

**Identity rule:** When aria2 replaces a Magnet metadata parent with a child
GID, migrate every selected identity, the focused row, range anchor, and open
details drawer together. Remove the obsolete parent identity rather than
leaving a hidden duplicate selection.

**Evidence:**

- Motrix task store at commit
  `7012040fec926e16fe8f6c403cf038527f5c18b9` refreshes task state in the
  `finally` path of pause, resume, and remove mutations:
  https://github.com/agalwood/Motrix/blob/7012040fec926e16fe8f6c403cf038527f5c18b9/src/renderer/store/modules/task.js
- qBittorrent WebUI at commit
  `bc42af9fd8fb9f39df04ed6747e82f912aff4cc0` serializes `sync/maindata`,
  schedules the next poll after errors, and restores selected rows after a
  full update:
  https://github.com/qbittorrent/qBittorrent/blob/bc42af9fd8fb9f39df04ed6747e82f912aff4cc0/src/webui/www/private/scripts/client.js

### D-011 - Remote RPC is explicit WebSocket with fail-closed TLS

**Decision:** AriaDeck supports `ws://` and `wss://` aria2 JSON-RPC at the exact
`/jsonrpc` path. Plain WebSocket is allowed for loopback only. A remote
plaintext endpoint requires the explicit
`ARIADECK_RPC_ALLOW_INSECURE_REMOTE=true` startup override; remote connections
otherwise require WSS.

**Fallback rule:** Do not automatically fall back to HTTP/HTTPS. aria2 HTTP
JSON-RPC does not carry server notifications, and a silent fallback could hide
a TLS or authentication failure. HTTP transport remains a future explicit
profile capability rather than an error-recovery behavior.

**Trust and credential rule:** Validate WSS with operating-system trust roots
and expose certificate failures as terminal `sync.tls` errors. Do not add a
certificate-validation bypass. Reject URL user information, query strings, and
fragments; accept the aria2 method token only from `ARIADECK_RPC_SECRET`.
WebSocket handshake failures retain the HTTP status but discard response
headers, so proxy or authentication headers cannot enter diagnostics.

**Control rule:** External connections default to 10-second connect and
15-second request timeouts; managed local connections keep 750 ms and 5-second
defaults. Startup environment settings can bound connection/request timeouts,
reconnect base/max delay, stable-connection reset time, and total attempts.
Invalid values fail before starting a connector and are reported without
echoing their contents.

**Evidence:**

- aria2 manual documents `/jsonrpc`, `ws`/`wss`, method-level `token:`
  authorization, TLS certificates, and the absence of notifications over HTTP:
  https://aria2.github.io/manual/en/html/aria2c.html
- AriaNg at commit `d6a765377e1eecfbcc387dcb824124df114decfb`
  explicitly chooses HTTP or WebSocket from the configured scheme; its HTTP
  service has no reconnect implementation rather than silently changing
  transport:
  https://github.com/mayswind/AriaNg/blob/d6a765377e1eecfbcc387dcb824124df114decfb/src/scripts/services/ariaNgSettingService.js
  https://github.com/mayswind/AriaNg/blob/d6a765377e1eecfbcc387dcb824124df114decfb/src/scripts/services/aria2HttpRpcService.js

### D-012 - Metadata files are client uploads, not engine-side paths

**Decision:** Keep link entry and Torrent/Metalink file import as separate add
modes. The native picker and window drop target accept multiple `.torrent`,
`.metalink`, and `.meta4` files; each metadata file is submitted independently
so one invalid or rejected file does not hide the other results. Torrent and
Metalink imports initially select every contained file. The preview and
`select-file` workflow belongs to `FILE-001` rather than being implied by the
basic import path.

**Local/remote rule:** AriaDeck reads a selected file on the desktop, validates
its type and a 16 MiB raw-content limit, and uploads only its Base64-encoded
contents through
`aria2.addTorrent` or `aria2.addMetalink`. Never pass a desktop path to aria2.
This gives managed and remote engines the same behavior and avoids assuming
that a remote daemon can see the desktop filesystem. Preserve every GID from an
`addMetalink` result because one Metalink document can register multiple
downloads.

**Persistence rule:** Start the managed aria2 process with uploaded-metadata
persistence enabled and a request-size limit large enough for AriaDeck's
metadata-file limit. External profiles remain subject to their daemon's
`rpc-max-request-size` and `rpc-save-upload-metadata` settings; a connection
failure after submission remains outcome-unknown and is never replayed
automatically.

**Evidence:**

- The aria2 manual defines `aria2.addTorrent` and `aria2.addMetalink` as Base64
  uploads, documents the multi-GID Metalink result, and notes that uploaded
  metadata must be saved for `--save-session` persistence. Its default
  `rpc-max-request-size` is 2 MiB:
  https://aria2.github.io/manual/en/html/aria2c.html
- AriaNg at commit `d6a765377e1eecfbcc387dcb824124df114decfb`
  reads the selected file with `FileReader`, removes the data-URL prefix, and
  sends the Base64 content through the matching RPC method:
  https://github.com/mayswind/AriaNg/blob/d6a765377e1eecfbcc387dcb824124df114decfb/src/scripts/services/ariaNgFileService.js
  https://github.com/mayswind/AriaNg/blob/d6a765377e1eecfbcc387dcb824124df114decfb/src/scripts/services/aria2RpcService.js
- Motrix at commit `7012040fec926e16fe8f6c403cf038527f5c18b9`
  uses a dedicated drag/select surface for Torrent files, parses the metadata,
  selects every file initially, and then emits Base64 plus selected indexes:
  https://github.com/agalwood/Motrix/blob/7012040fec926e16fe8f6c403cf038527f5c18b9/src/renderer/components/Task/SelectTorrent.vue
- qBittorrent at commit `bc42af9fd8fb9f39df04ed6747e82f912aff4cc0`
  separates its add-torrent dialog and exposes explicit all/none and per-file
  priority controls before acceptance:
  https://github.com/qbittorrent/qBittorrent/blob/bc42af9fd8fb9f39df04ed6747e82f912aff4cc0/src/gui/addnewtorrentdialog.cpp

### D-013 - File selection is preview-bound and add-time first

**Decision:** Parse Torrent and Metalink metadata on the desktop before
submission and show one flat, indexed file list for the active metadata source.
Every file starts selected. Provide tri-state select-all and per-file toggles,
show selected count and size, and reject submission when a source has no files
selected. For a partial selection, send a canonical 1-based `select-file`
range; omit the option when every file is selected so aria2 keeps its native
default.

**Safety rule:** Bind every preview to the SHA-256 digest of the raw metadata.
Submission rereads the bounded desktop file and rejects it if the digest has
changed, so previously selected indexes can never be applied to replacement
content. Parser output must contain at least one uniquely indexed regular file;
paths are display data until the engine reports its own destination paths.

**Refresh rule:** The task details drawer keeps aria2's `files[].selected`,
`length`, and `completedLength` as the authority and refreshes those details
when the visible task revision advances. The existing rows therefore become
live per-file progress rather than a one-time snapshot. Changing selection on
an already-added task is deferred to the task-option editor in `RPC-001`, where
`aria2.changeOption` race and refresh semantics can be handled consistently.

**Evidence:**

- aria2 documents `--select-file` for both BitTorrent and Metalink, with
  1-based comma/range syntax; omitting it selects every file. The RPC details
  projection exposes each file's `selected`, `length`, and `completedLength`,
  and `select-file` is also one of the dynamically changeable options:
  https://aria2.github.io/manual/en/html/aria2c.html#cmdoption-select-file
  https://aria2.github.io/manual/en/html/aria2c.html#aria2.changeOption
- Motrix at commit `7012040fec926e16fe8f6c403cf038527f5c18b9`
  parses Torrent metadata before submission, initially selects every file,
  exposes per-row/all selection, summarizes selected count and size, and shows
  per-file percentage/completed bytes in task details:
  https://github.com/agalwood/Motrix/blob/7012040fec926e16fe8f6c403cf038527f5c18b9/src/renderer/components/Task/SelectTorrent.vue
  https://github.com/agalwood/Motrix/blob/7012040fec926e16fe8f6c403cf038527f5c18b9/src/renderer/components/TaskDetail/TaskFiles.vue
- AriaNg at commit `d6a765377e1eecfbcc387dcb824124df114decfb`
  derives file/directory selection state and per-file progress from aria2's
  task projection, then applies later edits through `select-file`:
  https://github.com/mayswind/AriaNg/blob/d6a765377e1eecfbcc387dcb824124df114decfb/src/scripts/services/aria2TaskService.js

### D-014 - Sorting is local; priority changes the authoritative queue

**Decision:** Expose all existing list sort keys through one compact sort menu.
Changing the sort key or direction only changes the current AriaDeck query and
preserves identity-based selection; it never writes a new engine priority.
Queue priority is a separate task command with four direct actions: move to
top, move up, move down, and move to bottom.

**Scope rule:** Enable priority actions only for a live task while the visible
query is All tasks, has no search text, and uses ascending queue order. aria2's
queue is global across active, waiting, and paused tasks, so offering relative
movement inside a filtered, searched, reversed, or value-sorted projection
would imply a position that is not authoritative. A successful or
unknown-outcome mutation forces an authoritative refresh.

**Global command rule:** Pause all and resume all are engine-wide commands,
not batch operations over the currently loaded query. Use `aria2.pauseAll` and
`aria2.unpauseAll`, surface one pending global command at a time, and do not
replay an unknown mutation outcome in the same session.

**Evidence:**

- aria2 documents queue positions as zero-based and supports `POS_SET`,
  `POS_CUR`, and `POS_END`; it also defines `pauseAll` and `unpauseAll` as
  engine-wide operations:
  https://aria2.github.io/manual/en/html/aria2c.html#aria2.changePosition
  https://aria2.github.io/manual/en/html/aria2c.html#aria2.pauseAll
  https://aria2.github.io/manual/en/html/aria2c.html#aria2.unpauseAll
- AriaNg uses `changePosition` with an explicit absolute queue position and
  keeps that operation separate from display filtering and sorting:
  https://github.com/mayswind/AriaNg/blob/d6a765377e1eecfbcc387dcb824124df114decfb/src/scripts/services/aria2TaskService.js
- qBittorrent exposes top, up, down, and bottom as four distinct queue actions
  in both its toolbar and task menu. Motrix exposes pause-all/resume-all in its
  task toolbar, while its move-up/move-down commands remain placeholders and
  are therefore not used as implementation evidence:
  https://github.com/qbittorrent/qBittorrent/blob/bc42af9fd8fb9f39df04ed6747e82f912aff4cc0/src/webui/www/private/index.html
  https://github.com/agalwood/Motrix/blob/7012040fec926e16fe8f6c403cf038527f5c18b9/src/renderer/components/Task/TaskActions.vue
  https://github.com/agalwood/Motrix/blob/7012040fec926e16fe8f6c403cf038527f5c18b9/src/renderer/pages/index/commands.js

### D-015 - RPC-001 is the shared adapter prerequisite for advanced controls

**Decision:** `QUEUE-001`, `DETAIL-001`, `RATE-001`, and `BT-001` all depend on
typed RPC surface that belongs to `RPC-001`: `changePosition`,
`getUris`/`getPeers`/`getServers`, `getOption`/`changeOption`,
`changeGlobalOption`, and force operations. Rather than block every P1 feature
on a single large `RPC-001` milestone, each adapter method is added to the
`DownloadEngineGateway` boundary at the point of first use, with an
`Unsupported` default so an engine that lacks the method returns a typed
capability error instead of a raw RPC failure. `RPC-001` remains the task that
consolidates the remaining projections (`getPeers`/`getServers`, force
operations, multicall) and adds cross-version capability tests.

**Dependency rule:** A P1 feature that consumes an RPC method already present
on the gateway is not blocked by `RPC-001`. A feature that needs a method not
yet on the gateway adds it behind the same `Unsupported`-defaulted trait method
and records the addition here. The Task Matrix marks `DETAIL-001`, `RATE-001`,
and `BT-001` as depending on `RPC-001` for their not-yet-added projections.

**Evidence:**

- aria2 RPC manual documents every method named above and its parameters:
  https://aria2.github.io/manual/en/html/aria2c.html
- The current gateway already defaults `pause_all`, `resume_all`,
  `move_in_queue`, and `apply_download_proxy` to `GatewayErrorKind::Unsupported`
  in `crates/ariadeck-application/src/ports.rs`, so capability degradation is an
  established pattern rather than a new one.

### D-016 - Transfer limits are typed, scope-labeled, and capability-aware

**Decision:** `RATE-001` exposes aria2 speed limits as typed controls, not a
free-form option bag. Global download/upload limits use
`aria2.changeGlobalOption` with `max-overall-download-limit` and
`max-overall-upload-limit`; per-task limits use `aria2.changeOption` with
`max-download-limit` and `max-upload-limit`. Each control states whether it
affects existing tasks, new tasks, or both, because aria2 applies per-task
option changes only to the targeted download.

**Value rule:** Accept aria2's documented `K`/`M` suffix syntax and `0`
(unlimited). Validate and normalize on the desktop before submission; reject
values aria2 would silently ignore. Per-task limit changes follow the same
outcome-unknown reconciliation as other `changeOption` mutations (D-010).

**Evidence:**

- aria2 manual documents `max-overall-download-limit`,
  `max-overall-upload-limit`, `max-download-limit`, and `max-upload-limit`,
  their `K`/`M` suffix syntax, and their presence in the dynamically
  changeable option set:
  https://aria2.github.io/manual/en/html/aria2c.html#cmdoption-max-download-limit
- Motrix exposes global and per-task speed limits as distinct settings, not one
  combined value:
  https://github.com/agalwood/Motrix/blob/7012040fec926e16fe8f6c403cf038527f5c18b9/src/renderer/components/Preference/Basic.vue

### D-017 - On-demand detail projections are request-scoped and refresh-bounded

**Decision:** `DETAIL-001` loads URI/mirror, peer, tracker, and server data
through `aria2.getUris`, `aria2.getPeers`, and `aria2.getServers` only when the
details drawer requests them, never on the list-refresh path. Peer and server
data exist only while a task is active, so those sections are shown only for
active downloads and are cleared when the task leaves the active state.

**Refresh rule:** Detail projections follow the same visible-revision refresh
model already used for `files[]` in `FILE-001`: they refresh when the task
revision advances while the drawer is open and are dropped when the drawer
closes or the session changes. Task-option projections (`getOption`) are
read-only display here; editing options is deferred to `RPC-001`'s task-option
editor.

**Presentation rule:** Follow the properties-panel pattern used by mature
download clients: the drawer exposes separate Info, Files, Network, and Options
tabs. Network groups source/mirror URIs, BitTorrent announce tiers, active
HTTP(S)/FTP servers, and active BitTorrent peers without placing any of those
queries on the list-refresh path. Leaving aria2's active state clears peer and
server rows immediately while the revision-bound background refresh catches up.

**Sensitive-option rule:** `getOption` can return HTTP/proxy credentials,
Cookie/header values, authentication material, certificate/private-key paths,
and private tracker values. These entries keep their option key but are replaced
with a redacted marker inside the RPC adapter; the original value never enters
application, desktop, or UI state. Other options remain read-only and sorted by
key.

**Evidence:**

- aria2 manual documents `getUris`, `getPeers` (BitTorrent-only, active tasks),
  and `getServers` (active HTTP(S)/FTP tasks):
  https://aria2.github.io/manual/en/html/aria2c.html#aria2.getPeers
- AriaNg loads peer/server data only for the active-task detail view and
  refreshes it on its detail poll rather than the global list poll:
  https://github.com/mayswind/AriaNg/blob/d6a765377e1eecfbcc387dcb824124df114decfb/src/scripts/services/aria2TaskService.js
- AriaNg requests `getOption` from its task-detail settings tab instead of the
  global task list:
  https://github.com/mayswind/AriaNg/blob/d6a765377e1eecfbcc387dcb824124df114decfb/src/scripts/controllers/task-detail.js
- qBittorrent separates General, Trackers, Peers, HTTP Sources, and Files in its
  properties panel and loads peer data only when the Peers tab is selected:
  https://github.com/qbittorrent/qBittorrent/blob/e70f13d46a42b204559d0e7b16a19eeca522fe9e/src/gui/properties/proptabbar.h
  https://github.com/qbittorrent/qBittorrent/blob/e70f13d46a42b204559d0e7b16a19eeca522fe9e/src/gui/properties/propertieswidget.cpp

### D-018 - Seeding is a distinct state from completed download

**Decision:** `BT-001` represents a BitTorrent task that has finished
downloading but is still seeding as a separate presentation state from a
terminal `Complete` download. The authoritative signal is aria2's top-level
`seeder=true` on an `active` task. Upload speed and uploaded bytes are display
metrics only: a seeder with no connected leecher can legitimately report zero
upload speed, so neither value participates in state detection. Integrity
verification remains the more specific transient presentation when
`verifyIntegrityPending=true`.

**Runtime rule:** A seeding row remains in the Active filter/count and keeps
live-task controls, but presents upload speed, uploaded bytes, and share ratio
instead of download speed and ETA. Share ratio is calculated as
`uploadLength / totalLength` with fixed-point integer arithmetic. aria2 does
not expose authoritative elapsed seeding time, so AriaDeck displays an
explicitly session-bound observed duration that starts when the current engine
session first sees `Seeding` and resets on state exit, removal, or engine-
session change. qBittorrent can show lifetime seeding time because its native
engine exposes that value; AriaDeck must not imply the same authority over an
aria2 boundary.

**Stop-rule rule:** The details drawer reads the effective `seed-ratio` and
`seed-time` values from the existing on-demand `getOption` projection. When
both are set, aria2 stops at the first satisfied condition; `seed-ratio=0.0`
disables the ratio condition. The Options tab retains the raw values while the
Info tab explains the combined rule. Editing these values is deferred to the
typed advanced-control portion of `RPC-001` rather than exposing a free-form
option mutation.

**Evidence:**

- aria2 documents the top-level `seeder` tellStatus field, `uploadLength`, and
  the `seed-ratio`/`seed-time` first-satisfied stop behavior:
  https://aria2.github.io/manual/en/html/aria2c.html#aria2.tellStatus
  https://aria2.github.io/manual/en/html/aria2c.html#cmdoption-seed-ratio
  https://aria2.github.io/manual/en/html/aria2c.html#cmdoption-seed-time
- Motrix maps `status=active` plus `seeder=true` to its distinct `SEEDING`
  presentation state rather than inferring from transfer speed:
  https://github.com/agalwood/Motrix/blob/7012040fec926e16fe8f6c403cf038527f5c18b9/src/shared/utils/index.js
- qBittorrent presents seeding as a distinct state and can expose authoritative
  native-engine seeding time and share limits:
  https://github.com/qbittorrent/qBittorrent/blob/bc42af9fd8fb9f39df04ed6747e82f912aff4cc0/src/base/bittorrent/torrentimpl.cpp

### D-019 - Post-metadata output conflicts are surfaced, not silently resolved

**Decision:** D-008 defers the per-file conflict and containment flow for
Magnet/Torrent/Metalink tasks until metadata is available at runtime. That
runtime path is owned by `TASK-001`: once aria2 reports authoritative file
paths, the managed-local details request re-runs exact-file containment against
the authorized-root registry. The result is displayed as a local-path
validation status without blocking remote task details. AriaDeck presents
aria2's disk-full and output-conflict codes as specific task-row/detail reasons:
code 9 is insufficient space, while codes 11, 12, and 13 identify concurrent
same-output, concurrent same-Torrent, and existing-output conflicts. The raw
aria2 message remains available as diagnostic detail. AriaDeck does not force
`allow-overwrite=true` for these tasks and does not silently delete or rename
engine-side files.

**Evidence:**

- aria2 manual: `allow-overwrite` defaults to false and auto-renaming applies
  only to HTTP(S)/FTP; BitTorrent/Metalink conflicts surface as download errors
  rather than renamed outputs:
  https://aria2.github.io/manual/en/html/aria2c.html#cmdoption-allow-overwrite
- D-008 and D-009 in this document define the add-time policy and the runtime
  disk-full presentation that this decision extends to post-metadata file
  paths.
- AriaNg maps aria2 filesystem/output codes 9 through 18 separately rather than
  collapsing them into one generic failure:
  https://github.com/mayswind/AriaNg/blob/master/src/scripts/config/aria2Errors.js

### D-020 - Duplicate and local-path actions are identity- and capability-scoped

**Duplicate rule:** Compare new sources only against a connected, non-stale,
authoritative task snapshot. Direct URIs use normalized URL equality; Magnet
and uploaded Torrent sources use normalized BitTorrent info hashes. An exact
duplicate is not submitted. AriaDeck returns the existing task identity,
selects it when the request accepted no new task, and does not offer tracker
merging until a typed tracker-mutation contract exists. Stable discovery
metadata is retained across sparse list refreshes so directory, source, and
info-hash values do not disappear between polls.

**Source rule:** Task rows retain a sanitized primary source and engine
directory. The Info tab shows source type, sanitized source, directory, and
effective output path. User-info, passwords, URL query values, fragments, and
Magnet tracker/display-name parameters are excluded from the new source field.

**Open rule:** `Open download` and `Open folder` exist only for the managed
local engine. The desktop refetches exact-session task details immediately
before the action, validates the task directory against accumulated authorized
roots, and passes the path as a process argument without shell interpolation.
For a one-file task, `Open download` opens the file; for a multi-file task it
opens the task directory. External RPC profiles keep both actions visible but
unavailable because their engine paths are not assumed to exist locally.

**Evidence:**

- qBittorrent blocks an already-present Torrent and identifies the existing
  transfer; its optional tracker merge is intentionally deferred here:
  https://github.com/qbittorrent/qBittorrent/blob/master/src/gui/guiaddtorrentmanager.cpp
- qBittorrent exposes separate Open, Open containing folder, and Copy path
  actions:
  https://github.com/qbittorrent/qBittorrent/blob/master/src/gui/torrentcontentwidget.cpp
- Motrix resolves task files/directories through its native shell boundary,
  checks existence before showing a path, and reports missing-file failures:
  https://github.com/agalwood/Motrix/blob/7012040fec926e16fe8f6c403cf038527f5c18b9/src/renderer/utils/native.js


### D-021 - Stopped history pages engine memory; restart keeps only the session file

**Decision:** AriaDeck does not invent a second completed/failed history store
before SQLite exists. Stopped results are always the engine's in-memory
`tellStopped` queue, bounded by aria2's `--max-download-result`. The UI loads
the first page on connect, discloses `loaded/total` from
`numStoppedTotal`, and appends later pages only through an explicit Load more
action. Periodic and force refreshes re-fetch every already-loaded contiguous
page so a prior Load more is not discarded.

**Retention rule:** Managed local aria2 starts with
`--max-download-result=5000` so more terminal results stay addressable through
RPC. External profiles keep their daemon's own limit. When the FIFO is full,
the oldest completed/error/removed result disappears from aria2 and therefore
from AriaDeck. Application restart restores only what aria2 reloads from
`--save-session` / `--input-file` (error and unfinished downloads, plus any
uploaded metadata saved by the daemon). Completed-download rows are not
guaranteed to survive a restart until a client-side history database exists.

**Presentation rule:** Status bar shows `History loaded/total` while the
engine reports a non-zero total. Load more is single-flight, disabled while
disconnected or stale, and cleared on engine-session change without replaying
an in-flight page request.

**Evidence:**

- aria2 manual: `--max-download-result` is a FIFO of completed/error/removed
  results; `--save-session` persists error/unfinished downloads for restart:
  https://aria2.github.io/manual/en/html/aria2c.html
- design.md requires paginated stopped-task loading for large collections.
- Motrix/AriaNg treat aria2 stopped results as the history surface rather than
  maintaining a parallel completed-download database in the MVP path.


### D-022 - Advanced add controls are typed, URI-only, and secret-isolated

**Decision:** Extend the add-download flow with a collapsed Advanced section for
direct URL tasks only. Typed fields cover referer, user-agent, multi-line custom
headers, cookie, HTTP username/password, and aria2 `type=digest` checksum.
Magnet/Torrent/Metalink submissions reject these fields so users cannot believe
a Cookie or Referer rewrites tracker/peer authentication. Scheduling remains
out of scope for this slice.

**Secret rule:** Cookie and HTTP password use secure inputs and
`SecretString`/`SecretStringView`. They are flattened into aria2 options only at
the application→RPC boundary. Debug output redacts them; task rows, notices, and
exported diagnostics never receive the raw values. Free-form headers may not
carry `Cookie:` or `Authorization:`; those secrets must use the dedicated fields.

**Conflict rule:** The existing Keep both / Reject / Overwrite control remains
the authoritative add-time file-conflict policy (D-008). Advanced options never
override it. Multi-value `header` pairs are collapsed into a JSON string array
before `aria2.addUri`.

**Clipboard rule:** Standard paste into the URL field remains the handoff path.
A separate browser-extension protocol is deferred.

**Evidence:**

- aria2 RPC accepts per-download options including multi-value `header`,
  `referer`, `user-agent`, `http-user`/`http-passwd`, and `checksum`:
  https://aria2.github.io/manual/en/html/aria2c.html
- Motrix exposes advanced headers/referer/UA/cookie controls on its new-task
  form rather than a free-form option bag.
- AriaNg maps sensitive option keys separately and keeps them out of ordinary
  task presentation.

### D-023 - Transfer policy is typed, scope-labeled, and session-reapplied

**Decision:** `RATE-002` exposes aria2 transfer policy as typed controls beyond
speed limits. Global settings cover `max-concurrent-downloads`,
`max-connection-per-server`, `split`, `min-split-size`, `file-allocation`, and
`check-integrity`. Values are validated on the desktop against aria2's
documented ranges (connections 1–16; concurrent/split/min-split ≥ 1) before
`aria2.changeGlobalOption`. Defaults match aria2 1.37.0 (`-j 5`, `-x 1`,
`-s 5`, `-k 20M`, `prealloc`, integrity off). Settings persist in schema v4 and
reapply on each connected engine session with the same save/reconnect/rollback
pattern as speed limits.

**Scope rule:** Each control states its authority:
- `max-concurrent-downloads` affects the live engine queue immediately (all
  current and future downloads).
- connection count, split, min-split size, file allocation, and integrity check
  act as engine defaults for new downloads; existing tasks keep their own
  options until mutated.
- Per-task connection policy (`max-connection-per-server`/`split`/
  `min-split-size`) uses `aria2.changeOption` and affects only the targeted
  live download, with the same outcome-unknown reconciliation as other
  `changeOption` mutations (D-010).

**Out of scope:** Pause/resume scheduling remains deferred (no client-side
cron). Piece-length and stream-piece-selector stay at aria2 defaults for this
splice; free-form option bags are not exposed.

**Evidence:**

- aria2 manual: `max-concurrent-downloads` is immediately effective via
  `changeGlobalOption`; connection/split/file-allocation/check-integrity are
  both global defaults and per-download changeable options:
  https://aria2.github.io/manual/en/html/aria2c.html
- Motrix Basic preferences expose max concurrent downloads and max connections
  per server as distinct task-manage controls.
- Real aria2 1.37.0 verification accepts the RATE-002 global option set and
  echoes per-task connection/split values through `getOption`.

## Task Matrix

Legend: `[ ]` planned, `[-]` in progress, `[x]` implemented and verified.

### P0 - Correctness and Core Workflow

- [x] `FNM-001` Add resolving/final/custom filename state and preserve the
  final name in task metadata.
- [x] `FNM-002` Add direct-URI output-name override with validation and clear
  unsupported states for Torrent/Metalink. The task-level F2/edit-dialog path,
  application/RPC command, custom-name state preservation, and live aria2 flow
  are verified.
- [x] `FNM-003` Model Magnet metadata task relationships (`followedBy` and
  `belongsTo`) and migrate UI identity/selection safely.
- [x] `SEL-001` Add identity-based checkbox selection, tri-state header
  select-all, Ctrl/Cmd toggle, Shift range selection, Ctrl/Cmd+A, Escape clear,
  visible/hidden selection counts, and query-scoped clearing.
- [x] `SEL-002` Add batch pause/resume/retry/remove requests, mixed-state
  eligibility, partial-success handling, per-task failure details, and failed
  item selection retention for follow-up.
- [x] `ADD-001` Parse trimmed non-empty lines as independent tasks by default;
  expose explicit separate-task/mirror grouping and preserve source line
  positions in results.
- [x] `ADD-002` Resolve unknown add outcomes through an authoritative refresh,
  pre/post identity comparison, normalized URI or Magnet info-hash matching,
  and no automatic replay.
- [x] `RETRY-001` Preserve output, directory, headers/cookies, proxy, limits,
  checksum, and file-selection options when retrying.
- [x] `REMOVE-001` Split remove-task, remove-result, delete-local-files, and
  delete-incomplete-files flows with local/remote capability checks.
- [x] `NET-001` Add profile-scoped aria2 download proxy settings, no-proxy list,
  validation, and apply/reconnect semantics.
- [x] `NET-002` Add a credential-store boundary for proxy and RPC secrets. The
  proxy password uses the operating-system credential manager; RPC secrets
  remain ephemeral or environment-supplied and never enter the settings JSON.

### P1 - Expected Download-Manager Controls

- [x] `ADD-003` Add local/remote Torrent and Metalink files, drag/drop, and
  native multi-file picker flows. Desktop paths are read and bounded locally;
  only Base64 content reaches aria2, and every Metalink GID is preserved.
- [x] `FILE-001` Add Torrent/Metalink file selection and per-file progress.
- [x] `QUEUE-001` Add sorting controls, queue reordering, task priority, and
  pause-all/resume-all. Sort is local and selection-preserving; queue priority
  offers move-to-top/up/down/bottom, gated to the authoritative unfiltered,
  unsearched, ascending-queue projection (D-014); pause-all/resume-all are
  engine-wide commands with single-flight, no-replay reconciliation.
- [x] `RATE-001` Add global/per-task download and upload limits, with aria2
  capability-aware controls. Global limits apply through
  `aria2.changeGlobalOption` (`max-overall-download-limit`/
  `max-overall-upload-limit`) on save and on each new session; per-task limits
  use `aria2.changeOption` (`max-download-limit`/`max-upload-limit`) with the
  same outcome-unknown reconciliation as other `changeOption` mutations (D-010).
  Values accept aria2's `K`/`M`/`G` suffix syntax and `0`/blank (unlimited),
  normalized to bytes on the desktop; each control is scope-labeled
  ("all current and future downloads" globally, "this download only" per-task)
  per D-016. Settings persist through schema v3 and reapply on reconnect.
- [x] `DETAIL-001` Add URI/mirror, peer, tracker, server, and task-option
  projections on demand. Request-scoped and refresh-bounded per D-017; depends
  on `RPC-001` for the `getUris`/`getPeers`/`getServers`/`getOption` surface.
- [x] `BT-001` Represent seeding separately from completed download and expose
  share ratio, session-observed seeding time, upload activity, and effective
  stop rules. Distinct seeding state per D-018; depends on `RPC-001` for the
  top-level `seeder` and seed-option projections.
- [x] `TASK-001` Add duplicate detection, source/path display, open-file/open-
  folder actions, disk/permission/path-length error details, and the
  post-metadata output-conflict surfacing defined in D-019.
- [x] `RPC-001` Extend the typed RPC adapter for addTorrent/addMetalink,
  getUris/getPeers/getServers, get/changeOption, changePosition, global
  options, force operations, and multicall where needed. Force
  pause/remove/pause-all are gateway-backed with Unsupported defaults;
  connection-details projections use `system.multicall` with nested-only
  token injection; `system.listMethods` populates `EngineCapabilities.methods`;
  and a typed task-option editor mutates `seed-ratio`/`seed-time` (with
  `select-file` available on the same request contract).

### P2 - Platform and Long-Term Product Surface

- [ ] `PLAT-001` Add system tray, explicit close/leave-engine-running behavior,
  completion/error/low-disk notifications, and startup recovery UX.
- [ ] `PROFILE-001` Add multiple local/remote profiles and profile-scoped
  settings/history.
- [ ] `UI-001` Add system theme, window geometry, localization, saved filters,
  tags/categories, and browser/file associations.
- [ ] `CORE-001` Add managed aria2 installation, verification, switching,
  rollback, update channels, and packaging.

## Current Implementation Slice

The filename, selection, add-outcome, retry, removal, proxy, command-state, and
metadata-import/file-selection slices (`FNM-001`
through `FNM-003`, `SEL-001`, `SEL-002`, `ADD-001`, `ADD-002`, `ADD-003`,
`ADD-004`, `RETRY-001`, `RETRY-002`, `REMOVE-001`, `FILE-001`, `FILE-002`,
`NET-001`, `NET-002`, `NET-003`, and
`STATE-001`) are complete. `QUEUE-001` (sorting, queue reordering, task
priority, and pause-all/resume-all) is now complete across the domain, RPC,
application, desktop, and UI layers, with unit coverage and a live aria2
`changePosition`/`pauseAll`/`unpauseAll` flow. `RATE-001` (typed global and
per-task download/upload speed limits) is now complete across all layers: a
`SpeedLimitConfig` domain type and settings schema v3, a session-bound
`apply_speed_limit` gateway/sync path that applies on save and reapplies on
reconnect, a per-task `SetTaskSpeedLimit` command with outcome-unknown
reconciliation, K/M/G-suffix parsing/formatting, scope-labeled settings and
per-task dialog UI, and a live aria2 `changeGlobalOption`/`changeOption` flow.
`DETAIL-001` is now complete across the domain, RPC, application, desktop, and
UI layers. The drawer uses Info/Files/Network/Options tabs, requests URI,
announce-tier, active server/peer, and sorted read-only option projections only
while open, refreshes them by visible task revision, clears active-only data on
state exit, rejects stale session/request results, and redacts sensitive option
values inside the RPC adapter.
`BT-001` is now complete across the domain, RPC, application, desktop, and UI
layers. aria2's explicit top-level `seeder=true` maps an active task to the
distinct Seeding state even at zero upload speed; integrity verification keeps
priority when both flags are present. Seeding stays in the Active filter and
retains live-task controls while rows/details show fixed-point share ratio,
uploaded bytes, upload speed, and explicitly session-observed seeding time.
The Info tab explains the effective `seed-ratio`/`seed-time` first-satisfied
rule while Options preserves the raw values. `TASK-001` is now complete:
authoritative duplicate detection covers normalized URLs and Magnet/Torrent
info hashes; sparse list refreshes preserve stable discovery metadata; task
details show sanitized source/directory/output fields, exact local-path
validation, aria2 codes 9-18 with raw details, and managed-local Open download/
Open folder actions that refetch exact-session details before touching the
filesystem. External profiles expose the capability as unavailable rather than
assuming their paths are local. `RPC-001` is now complete: force pause/remove (and force-pause-all) sit on the shared gateway with capability-safe Unsupported defaults; `system.multicall` batches on-demand URI/option/peer/server projections with nested-only authentication; `system.listMethods` feeds `EngineCapabilities.methods`; and the typed task-option editor applies seed-ratio/seed-time through `changeOption` with the same outcome-unknown reconciliation as other mutations. `HISTORY-001` is now complete: stopped-history state exposes loaded/total/next-offset, SyncHandle.load_more_stopped appends pages without dropping earlier ones, the status bar shows History loaded/total with single-flight Load more, and managed aria2 keeps 5000 terminal results in memory. `ADD-005` is now complete: typed advanced add controls cover referer, user-agent, custom headers, cookie, HTTP auth, and checksum for direct URL tasks; secrets stay redacted; multi-value headers collapse at the RPC boundary. `RATE-002` is now complete: typed transfer policy covers max concurrent downloads, connections per server, split, min-split size, file allocation, and integrity check with schema v4 persistence, session-bound apply/reapply/rollback, scope-labeled settings UI, and per-task connection-policy command surface (D-023). Pause/resume scheduling remains deferred. Remaining P1 audit items and P2 platform work are next.
The proxy slice includes schema migration, validated endpoint/bypass fields,
masked password input, system credential storage, session-bound runtime apply,
new-session reapply, and explicit clearing. `FILE-002` now has
safe deletion, accumulated authorized roots, local writable-directory/space and
known-size preflight, remote path isolation, direct-task conflict policy, and
  specific runtime disk-full errors. Torrent/Metalink import now includes native
  multi-file selection, window drop, bounded desktop reads, digest-bound file
  previews, partial `select-file` submission, live per-file progress, local/remote
  Base64 uploads, per-source outcomes, selected-size preflight, selected-path
  containment/conflict checks, and complete Metalink GID handling. The P0
  filename, selection, task-state, network, add/retry/removal, and file-safety
  slices are complete.

## Audit Additions

The matrix above covers the visible feature areas. The following items were
added by auditing behavior at task, engine, filesystem, and release boundaries.
They are intentionally phrased as acceptance outcomes rather than UI controls;
several can be shared by more than one feature.

### P0 - Behavior That Must Be Closed Before Broad Use

- [x] `ADD-004` Define add-dialog submission semantics: trim lines, ignore blank
  separators, reject all-blank input, report duplicates as skipped item results,
  prevent duplicate submits while a request is in flight, and report accepted,
  rejected, and unknown outcomes per item. Only confirmed-not-observed failures
  remain available for safe retry.
- [x] `RETRY-002` Make retry semantics explicit in the UI: retry creates a new
  GID rather than resuming the failed task, the old result remains until removed,
  and an unknown add outcome must not create a second replacement task.
- [x] `FILE-002` Add path and file-safety checks before destructive or output
  operations. Exact local file/control-file deletion, canonical containment,
  Trash use, accumulated authorized roots, writable-directory, available-space
  and known-size preflight, direct-task overwrite/reject/auto-rename policy,
  disk-full error presentation, local-versus-remote capability handling, and
  Torrent/Metalink selected-file containment/conflicts are complete.
- [x] `STATE-001` Specify command race behavior: disable or coalesce duplicate
  actions, reject stale session responses, refresh after outcome-unknown
  mutations, and preserve details/selection when a Magnet parent is replaced by
  a metadata child or when a task changes GID.
- [x] `NET-003` Cover RPC connection security separately from download proxy
  settings: endpoint/scheme validation, `ws`/`wss` and HTTP fallback policy,
  TLS certificate errors, authentication testing, timeout/reconnect settings,
  and redaction of credentials embedded in URLs or headers.

### P1 - Expected Download-Manager Completeness

- [x] `HISTORY-001` Define stopped-result retention and history behavior when
  aria2's result limit or session file is reached. Provide lazy loading with a
  clear loaded/total state, and decide which metadata survives an application
  restart before SQLite history exists. Stopped results remain aria2-owned
  (`tellStopped` / `numStoppedTotal`); managed local engines raise
  `--max-download-result=5000`; the UI discloses loaded/total and single-flight
  Load more appends contiguous pages (D-021). Restart survival is limited to
  the aria2 session file until SQLite history exists.
- [x] `ADD-005` Add source and request controls beyond a URL field: clipboard or
  browser handoff, custom headers/cookies/auth, referer and user-agent, checksum
  verification, output conflict policy, and optional scheduling. Keep secrets
  out of task rows, logs, and exported diagnostics. Collapsed Advanced section
  for direct-URI tasks only (D-022): typed referer/UA/headers/cookie/HTTP
  auth/checksum; secure secret fields; existing Keep both/Reject/Overwrite
  policy retained; scheduling deferred; multi-value headers collapsed for
  `aria2.addUri`.
- [x] `RATE-002` Expose capability-aware transfer policy controls beyond a
  single speed limit: connection and split counts, file allocation and
  integrity-check policy. Typed global settings (schema v4) apply through
  `aria2.changeGlobalOption` on save and reconnect; per-task connection policy
  uses `aria2.changeOption` with outcome-unknown reconciliation (D-010/D-023).
  Each control is scope-labeled (live concurrent queue vs new-download
  defaults vs this download only). Pause/resume scheduling remains deferred.
- [ ] `UI-002` Close list ergonomics around the selection work: deterministic
  sorting, action availability for mixed states, hidden-selection disclosure
  after filtering, context-menu parity, keyboard shortcuts, focus restoration,
  and an undo or recovery path for accidental task removal where possible.
- [ ] `OBS-001` Add grouped completion/error notifications and a task activity
  or error history. Batch completions must not produce one notification per
  item, and notification volume/quiet behavior must be configurable.
- [ ] `PROFILE-002` Define profile switching and startup recovery behavior:
  profile identity verification, running-engine ownership on close, session
  corruption recovery, endpoint changes, and a safe handoff when another aria2
  instance already owns the configured port or session.
- [ ] `RPC-002` Gate advanced controls by detected aria2 capabilities and add
  compatibility tests across supported versions/builds. Unsupported methods
  must become disabled explanations rather than raw RPC failures; include real
  download-proxy and remote-engine smoke paths.

### P2 - Platform, Security, and Release Readiness

- [ ] `ACCESS-001` Verify screen-reader names, non-color status cues, reduced
  motion, high-DPI/window resizing, keyboard-only flows, and localization of
  pluralized sizes, rates, dates, and errors.
- [ ] `SEC-001` Add privacy review for URLs with credentials/private tracker
  tokens, proxy secrets, downloaded filenames, symlinks, and diagnostic export.
  Redaction should be tested rather than only documented.
- [ ] `PERF-001` Add long-running and stress checks for 10,000 stopped tasks,
  rapid active updates, details polling, reconnect storms, minimized operation,
  and memory growth.
- [ ] `RELEASE-001` Close application signing, installer/portable packaging,
  uninstall data-retention behavior, license notices, settings/database schema
  migration tests, and application update/rollback behavior.

The recommended order is all P0 items before broad daily use, P1 items for a
complete download-manager experience, and P2 items before multi-platform
distribution. Existing matrix items remain the source of truth where these
acceptance outcomes overlap.

## Verification Evidence

| Date | Tasks | Command or check | Result |
| --- | --- | --- | --- |
| 2026-07-20 | `FNM-001`, `FNM-002`, `FNM-003` | `cargo test -p ariadeck-domain -p ariadeck-application -p ariadeck-rpc -p ariadeck-ui -p ariadeck-desktop` | Pass - 118 passed, 3 ignored |
| 2026-07-20 | `FNM-001`, `FNM-002`, `FNM-003` | `cargo clippy -p ariadeck-domain -p ariadeck-application -p ariadeck-rpc -p ariadeck-ui -p ariadeck-desktop --all-targets -- -D warnings` | Pass - no issues |
| 2026-07-20 | `FNM-002` | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 session_bound_command_flow_handles_both_removal_contracts -- --ignored --exact` | Pass - real aria2 accepted `changeOption(out)`, refresh preserved the custom name, and task cleanup succeeded |
| 2026-07-20 | `SEL-001`, `SEL-002` | `cargo test -p ariadeck-domain -p ariadeck-application -p ariadeck-rpc -p ariadeck-ui -p ariadeck-desktop` | Pass - 125 passed, 3 ignored; includes range/toggle/select-all, query clearing, hidden selection accounting, batch partial success, and failure retention |
| 2026-07-20 | `SEL-001`, `SEL-002` | `cargo clippy -p ariadeck-domain -p ariadeck-application -p ariadeck-rpc -p ariadeck-ui -p ariadeck-desktop --all-targets -- -D warnings` | Pass - no issues |
| 2026-07-20 | `ADD-001`, `ADD-002`, `ADD-004` | `cargo test -p ariadeck-domain -p ariadeck-application -p ariadeck-rpc -p ariadeck-ui -p ariadeck-desktop` | Pass - 135 passed, 3 ignored; includes multiline parsing, explicit mirrors, duplicate item results, safe-retry filtering, Base32 Magnet matching, and an accepted-but-unknown coordinator refresh without replay |
| 2026-07-20 | `ADD-001`, `ADD-002`, `ADD-004` | `cargo clippy -p ariadeck-domain -p ariadeck-application -p ariadeck-rpc -p ariadeck-ui -p ariadeck-desktop --all-targets -- -D warnings` | Pass - no issues |
| 2026-07-20 | `ADD-001`, stopped-result removal regression | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 session_bound_command_flow_handles_both_removal_contracts -- --ignored --exact --nocapture` | Pass - real aria2 accepted a two-source mirror task, projected it as paused, removed the live task and stopped result, and left no test task behind |
| 2026-07-20 | `RETRY-001`, `RETRY-002` | `cargo test -p ariadeck-domain -p ariadeck-application -p ariadeck-rpc -p ariadeck-ui -p ariadeck-desktop` | Pass - 140 passed, 3 ignored; covers option/mirror replay, query-versus-mutation failure classification, single and partial-batch unknown reconciliation, safe manual retry, one gateway call, new GID selection, and old-result retention |
| 2026-07-20 | `RETRY-001`, `RETRY-002` | `cargo clippy -p ariadeck-domain -p ariadeck-application -p ariadeck-rpc -p ariadeck-ui -p ariadeck-desktop --all-targets -- -D warnings` | Pass - no issues |
| 2026-07-20 | `RETRY-001`, `RETRY-002` | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 session_bound_command_flow_handles_both_removal_contracts -- --ignored --exact --nocapture` | Pass - real aria2 created a distinct retry GID, retained the failed result, handled its explicit no-URI-data response through the known-source fallback, and preserved output, directory, Cookie header, HTTP credentials, limits, connection count, checksum, and retry options |
| 2026-07-20 | `REMOVE-001`, deletion side of `FILE-002` | `cargo test --workspace` | Pass - 153 passed, 5 ignored; covers local/remote capability gating, keep-files defaults, active and terminal removal ordering, exact path containment, Trash failures, partial batches, and unknown-outcome reconciliation without replay |
| 2026-07-20 | `REMOVE-001`, deletion side of `FILE-002` | `cargo clippy --workspace --all-targets -- -D warnings` | Pass - no issues |
| 2026-07-20 | `REMOVE-001` | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 session_bound_command_flow_handles_both_removal_contracts -- --ignored --exact --nocapture` | Pass - real aria2 removed active and terminal records while the completed download file remained byte-for-byte unchanged |
| 2026-07-20 | Output side of `FILE-002` | `cargo test --workspace` | Pass - 160 passed, 5 ignored; covers typed conflict mappings, local directory write/space checks, Windows path normalization, unsafe local destinations, and external RPC settings that never touch the desktop filesystem |
| 2026-07-20 | Output side of `FILE-002` | `cargo clippy --workspace --all-targets -- -D warnings` | Pass - no issues |
| 2026-07-20 | Output side of `FILE-002` | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 session_bound_command_flow_handles_both_removal_contracts -- --ignored --exact --nocapture` | Pass - real aria2 created `kept-after-remove.1.bin` for the second same-name HTTP download and kept both files after result removal |
| 2026-07-20 | Authorized roots and disk-full handling in `FILE-002` | `cargo test --workspace` | Pass - 164 passed, 5 ignored; covers accumulated preflighted roots, rejection of unconfigured roots, known-size insufficient-space rejection, aria2 error-code preservation, and task-row/detail presentation |
| 2026-07-20 | Authorized roots and disk-full handling in `FILE-002` | `cargo clippy --workspace --all-targets -- -D warnings` | Pass - no issues |
| 2026-07-20 | `NET-001`, `NET-002` and the completed checkpoint | `cargo test --workspace` | Pass - 178 passed, 6 ignored; covers schema v1-to-v2 migration, proxy validation/recovery, redacted UI/application debug values, masked UTF-8 input, credential mutation rollback, exact-session application, global-option mappings, and settings UI drafts |
| 2026-07-20 | `NET-001`, `NET-002` and the completed checkpoint | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo build -p ariadeck-desktop`; `cargo fmt --all -- --check` | Pass - no warnings, native desktop build succeeds, formatting clean |
| 2026-07-20 | `NET-001`, `NET-002` | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 -- --ignored --nocapture` | Pass - all 4 real aria2 flows; the proxy flow answered a Basic 407 challenge from credential options, routed traffic through the proxy, bypassed loopback through `no-proxy`, cleared proxy state when Disabled, and left no process behind |
| 2026-07-20 | `STATE-001` | `cargo test -p ariadeck-application -p ariadeck-ui` | Pass - 94 tests cover unknown-outcome refresh without replay, single-flight task submission, and focused/non-focused Magnet identity migration |
| 2026-07-20 | Desktop Tokio runtime regression | `cargo test -p ariadeck-desktop`; isolated native startup with a real local aria2 | Pass - 30 tests; proxy settings loading is dispatched through the explicit Tokio runtime from a non-Tokio context, and the connected desktop remained healthy for the six-second observation with no reactor panic |
| 2026-07-20 | `STATE-001` and desktop runtime checkpoint | `cargo test --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `cargo build -p ariadeck-desktop` | Pass - 183 tests, 6 ignored; no warnings, formatting clean, native desktop build succeeds |
| 2026-07-20 | `STATE-001` live regression | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 -- --ignored --nocapture` | Pass - all 4 real aria2 flows; authenticated reads, restart/reconnect, command/removal behavior, proxy routing/bypass/disable, and cleanup remain intact |
| 2026-07-20 | `NET-003` | `cargo test --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `cargo build -p ariadeck-desktop` | Pass - 190 tests, 7 ignored; strict endpoint/HTTP policy, timeout/reconnect parsing, terminal error mapping, URL/header redaction, local untrusted-WSS rejection, formatting, lints, and desktop build all pass |
| 2026-07-20 | `NET-003` authentication and live regression | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 -- --ignored --nocapture` | Pass - 5 real aria2 flows; correct authentication succeeds, an incorrect secret is rejected without either credential appearing in errors, and restart, command/removal, proxy, and cleanup behavior remains intact |
| 2026-07-20 | `ADD-003` | `cargo test --workspace` | Pass - 201 passed, 8 ignored; covers metadata validation and explicit-runtime file reads, 16 MiB bounds, extension/dedup/mode/remove/pending-drop UI behavior, Base64 RPC parameters, multi-GID propagation and result selection, empty-GID gateway rejection, managed aria2 arguments, and unknown-outcome no-replay behavior |
| 2026-07-20 | `ADD-003` | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `cargo build -p ariadeck-desktop`; `git diff --check` | Pass - no warnings, formatting clean, native desktop build succeeds, and the patch has no whitespace errors |
| 2026-07-20 | `ADD-003` live upload and regression | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 -- --ignored --nocapture` | Pass - all 6 real aria2 flows; uploaded Torrent metadata is registered as BitTorrent, every GID returned from uploaded Metalink metadata is observable, and authentication, restart, command/removal, proxy, and cleanup regressions remain green |
| 2026-07-21 | `FILE-001`, completed `FILE-002` | `cargo test --workspace --no-fail-fast` | Pass - 210 passed, 8 ignored; covers digest-bound Torrent/Metalink previews, zero/partial/all selection, stale indexes/results, live revision-driven file progress, selected-size preflight, safe relative paths, Reject conflicts, and remote filesystem isolation |
| 2026-07-21 | `FILE-001`, completed `FILE-002` | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo build -p ariadeck-desktop`; `cargo fmt --all -- --check`; `git diff --check` | Pass - no warnings, native desktop build succeeds, formatting clean, and the patch has no whitespace errors |
| 2026-07-21 | `FILE-001` live selection and regression | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 -- --ignored --nocapture` | Pass - all 6 real aria2 flows; a two-file Torrent reports `files[].selected` as `[false, true]`, a two-file Metalink returns only the selected file GID, and authentication, restart, command/removal, proxy, and cleanup regressions remain green |
| 2026-07-21 | Desktop Tokio runtime regression after metadata parsing | isolated `target/debug/ariadeck-desktop.exe` startup with `RUST_BACKTRACE=1` and a real local aria2 | Pass - the desktop remained alive for the six-second observation; logs contained no panic and no `there is no reactor running` failure |
| 2026-07-21 | `QUEUE-001` | `cargo test --workspace --no-fail-fast` | Pass - 218 passed, 9 ignored; adds queue-reordering scope gating (All/no-search/Queue/ascending), selection-preserving sort with query emission, blocked movement outside the authoritative queue, global pause-all pending/emit, application-layer queue-move dispatch and terminal rejection, UI→domain sort mapping, and `changePosition` argument/negative-position mapping |
| 2026-07-21 | `QUEUE-001` | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `cargo build -p ariadeck-desktop`; `git diff --check` | Pass - no warnings, formatting clean, native desktop build succeeds, and the patch has no whitespace errors |
| 2026-07-21 | `QUEUE-001` live queue and regression | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 -- --ignored` | Pass - all 7 real aria2 flows; a three-task paused queue reorders correctly under `changePosition` move-to-top and move-to-bottom, `unpauseAll`/`pauseAll` apply without error, and authentication, restart, command/removal, proxy, metadata-upload, and cleanup regressions remain green |
| 2026-07-21 | `RATE-001` | `cargo test --workspace --no-fail-fast` | Pass - 227 passed, 9 ignored; adds K/M/G-suffix parse/format round-trips and malformed/overflow rejection, per-task `changeOption` speed-limit forwarding and terminal-task rejection, global `changeGlobalOption` `max-overall-*` forwarding, and settings-UI parsed-request emission with compact-form normalization and invalid-input rejection |
| 2026-07-21 | `RATE-001` | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `cargo build -p ariadeck-desktop`; `git diff --check` | Pass - no warnings, formatting clean, native desktop build succeeds, and the patch has no whitespace errors |
| 2026-07-21 | `RATE-001` live limits and regression | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 -- --ignored` | Pass - all 8 real aria2 flows; real aria2 accepted global `max-overall-*` limits and their `0` clear, accepted per-task `max-*-limit` and reported them back through `getOption`, and authentication, restart, queue, command/removal, proxy, metadata-upload, and cleanup regressions remain green |
| 2026-07-21 | `DETAIL-001` | `cargo test --workspace --no-fail-fast` | Pass - 233 passed, 11 ignored; covers request/session scoping, revision catch-up, immediate active-only peer/server clearing, URI status, announce tiers, server/peer decoding, active-state races, sorted option projection, and adapter-level sensitive-value redaction |
| 2026-07-21 | `DETAIL-001` | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `cargo build -p ariadeck-desktop`; `git diff --check` | Pass - no warnings, formatting clean, native desktop build succeeds, and the patch has no whitespace errors |
| 2026-07-21 | `DETAIL-001` live projections and regression | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 -- --ignored --nocapture` | Pass - all 9 real aria2 flows; a paused two-mirror task exposed both URIs while HTTP credentials and Cookie/header values were redacted inside the adapter, an active slow HTTP transfer exposed its server projection, and all prior authentication, restart, queue, speed-limit, command/removal, proxy, metadata-upload, and cleanup regressions remain green |
| 2026-07-21 | `BT-001` | `cargo test --workspace --no-fail-fast` | Pass - 239 passed, 12 ignored; covers explicit seeder mapping at zero upload speed, verification priority, Active filtering/counting and controls, session-bound timer reset, fixed-point share ratio, desktop projection, and Seeding-aware detail requests/network cleanup |
| 2026-07-21 | `BT-001` | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `cargo build -p ariadeck-desktop`; `git diff --check` | Pass - no warnings, formatting clean, native desktop build succeeds, and the patch has no whitespace errors |
| 2026-07-21 | `BT-001` live seeding and regression | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 -- --ignored --nocapture` | Pass - all 10 real aria2 flows; a complete local Torrent entered Seeding from top-level `seeder=true` with no leecher and zero upload speed, retained configured `seed-ratio`/`seed-time`, and all prior authentication, restart, queue, limits, details, command/removal, proxy, metadata-upload, and cleanup regressions remained green |
| 2026-07-21 | `TASK-001`, final `BT-001` checkpoint | `cargo test --workspace --no-fail-fast` | Pass - 250 passed, 12 ignored; adds stable sparse-refresh metadata, normalized URL/Magnet/Torrent duplicate matching, existing-task focus, sanitized source fields, aria2 code 9-18 classification with raw details, managed-local post-details path validation, safe process-argument path opening, and external-profile capability gating |
| 2026-07-21 | `TASK-001`, final `BT-001` checkpoint | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `cargo build -p ariadeck-desktop`; `git diff --check` | Pass - no warnings, formatting clean, native desktop build succeeds, and the patch has no whitespace errors |
| 2026-07-21 | `TASK-001` live regression | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 -- --ignored --nocapture` | Pass - all 10 real aria2 flows; authenticated state, restart recovery, command/removal, proxy, queue, limits, metadata upload, task details, explicit seeding, and cleanup remain green after the task/source/path changes |

| 2026-07-21 | `RPC-001` | `cargo test --workspace --no-fail-fast` | Pass - 257 passed, 13 ignored; adds force pause/remove/pause-all gateway forwarding, typed seed-option changeOption mapping and validation, multicall decode/redaction, nested-only multicall authentication, and listMethods capability probe |
| 2026-07-21 | `RPC-001` | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `cargo build -p ariadeck-desktop`; `git diff --check` | Pass - no warnings, formatting clean, native desktop build succeeds, and the patch has no whitespace errors |
| 2026-07-21 | `RPC-001` live force/multicall/options and regression | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 -- --ignored --nocapture` | Pass - all 11 real aria2 flows; listMethods published force/multicall methods, multicall returned independent authenticated projections, forcePause/forceRemove operated on a live task, changeOption seed-ratio/seed-time echoed through getOption, and all prior authentication, restart, queue, limits, details, seeding, command/removal, proxy, metadata-upload, and cleanup regressions remained green |

| 2026-07-21 | `HISTORY-001` | `cargo test --workspace --no-fail-fast` | Pass - 259 passed, 13 ignored; adds StoppedHistoryState loaded/total/next-offset, contiguous Load more paging, periodic refresh of already-loaded pages, status-bar History loaded/total disclosure, single-flight Load more UI, managed `--max-download-result=5000`, and restart-retention decision D-021 |
| 2026-07-21 | `HISTORY-001` | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `cargo build -p ariadeck-desktop`; `git diff --check` | Pass - no warnings, formatting clean, native desktop build succeeds, and the patch has no whitespace errors |

| 2026-07-21 | `HISTORY-001` live regression | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 -- --ignored` | Pass - all 11 real aria2 flows remain green after stopped-history paging and managed max-download-result changes |

| 2026-07-21 | `ADD-005` | `cargo test --workspace --no-fail-fast` | Pass - 265 passed, 13 ignored; adds typed advanced add options (referer/UA/headers/cookie/HTTP auth/checksum), secret redaction, multi-value header array collapse, collapsed Advanced add-dialog section, and URI-only validation (D-022) |
| 2026-07-21 | `ADD-005` | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `cargo build -p ariadeck-desktop`; `git diff --check` | Pass - no warnings, formatting clean, native desktop build succeeds, and the patch has no whitespace errors |
| 2026-07-21 | `ADD-005` live regression | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 -- --ignored` | Pass - all 11 real aria2 flows remain green after advanced add-option mapping and multi-header collapse |

| 2026-07-21 | `RATE-002` | `cargo test --workspace --no-fail-fast` | Pass - 274 passed, 13 ignored; adds TransferPolicyConfig/TaskConnectionPolicy domain validation, settings schema v4 migration, global `changeGlobalOption` transfer-policy mapping, per-task connection-policy `changeOption` forwarding and terminal/out-of-range rejection, settings-UI draft parsing (counts + K/M/G min-split), and live-test coverage for concurrent/connection/split options |
| 2026-07-21 | `RATE-002` | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `cargo build -p ariadeck-desktop`; `git diff --check` | Pass - no warnings, formatting clean, native desktop build succeeds, and the patch has no whitespace errors |
| 2026-07-21 | `RATE-002` live transfer policy and regression | `env ARIA2C_PATH=... cargo test -p ariadeck-rpc --test live_aria2 -- --ignored` | Pass - all 11 real aria2 flows remain green; the speed-limit flow also accepts RATE-002 global concurrent/connection/split/allocation/integrity options and echoes per-task connection/split values through `getOption` |

Existing MVP evidence remains in `docs/implementation-progress.md`.
