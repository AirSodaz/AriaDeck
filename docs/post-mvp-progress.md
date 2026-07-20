# AriaDeck Post-MVP Progress

**Status:** active

**Last updated:** 2026-07-20

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
- [ ] `FILE-001` Add Torrent/Metalink file selection and per-file progress.
- [ ] `QUEUE-001` Add sorting controls, queue reordering, task priority, and
  pause-all/resume-all.
- [ ] `RATE-001` Add global/per-task download and upload limits, with aria2
  capability-aware controls.
- [ ] `DETAIL-001` Add URI/mirror, peer, tracker, server, and task-option
  projections on demand.
- [ ] `BT-001` Represent seeding separately from completed download and expose
  seed ratio/time/upload rules.
- [ ] `TASK-001` Add duplicate detection, source/path display, open-file/open-
  folder actions, and disk/permission/path-length error details.
- [ ] `RPC-001` Extend the typed RPC adapter for addTorrent/addMetalink,
  getUris/getPeers/getServers, get/changeOption, changePosition, global
  options, force operations, and multicall where needed.

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

The filename, selection, add-outcome, retry, removal, proxy, and command-state
slices (`FNM-001`
through `FNM-003`, `SEL-001`, `SEL-002`, `ADD-001`, `ADD-002`, `ADD-003`,
`ADD-004`, `RETRY-001`, `RETRY-002`, `REMOVE-001`, `NET-001`, `NET-002`, `NET-003`, and
`STATE-001`) are complete.
The proxy slice includes schema migration, validated endpoint/bypass fields,
masked password input, system credential storage, session-bound runtime apply,
new-session reapply, and explicit clearing. `FILE-002` now has
safe deletion, accumulated authorized roots, local writable-directory/space and
known-size preflight, remote path isolation, direct-task conflict policy, and
specific runtime disk-full errors. Torrent/Metalink import now includes native
multi-file selection, window drop, bounded desktop reads, local/remote Base64
uploads, per-source outcomes, and complete Metalink GID handling. The remaining
per-file containment/conflict slice depends on `FILE-001`. The independent
P0 filename, selection, task-state, network, add/retry/removal, and direct-file
safety slices are complete.

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
- [-] `FILE-002` Add path and file-safety checks before destructive or output
  operations. Exact local file/control-file deletion, canonical containment,
  Trash use, accumulated authorized roots, writable-directory, available-space
  and known-size preflight, direct-task overwrite/reject/auto-rename policy,
  disk-full error presentation, and local-versus-remote capability handling are
  complete. Torrent/Metalink per-file containment/conflicts remain and depend
  on their import and file-selection flows.
- [x] `STATE-001` Specify command race behavior: disable or coalesce duplicate
  actions, reject stale session responses, refresh after outcome-unknown
  mutations, and preserve details/selection when a Magnet parent is replaced by
  a metadata child or when a task changes GID.
- [x] `NET-003` Cover RPC connection security separately from download proxy
  settings: endpoint/scheme validation, `ws`/`wss` and HTTP fallback policy,
  TLS certificate errors, authentication testing, timeout/reconnect settings,
  and redaction of credentials embedded in URLs or headers.

### P1 - Expected Download-Manager Completeness

- [ ] `HISTORY-001` Define stopped-result retention and history behavior when
  aria2's result limit or session file is reached. Provide lazy loading with a
  clear loaded/total state, and decide which metadata survives an application
  restart before SQLite history exists.
- [ ] `ADD-005` Add source and request controls beyond a URL field: clipboard or
  browser handoff, custom headers/cookies/auth, referer and user-agent, checksum
  verification, output conflict policy, and optional scheduling. Keep secrets
  out of task rows, logs, and exported diagnostics.
- [ ] `RATE-002` Expose capability-aware transfer policy controls beyond a
  single speed limit: connection and split counts, piece/file allocation and
  integrity-check policy, and pause/resume scheduling. Each control must state
  whether it affects existing tasks, new tasks, or both.
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

Existing MVP evidence remains in `docs/implementation-progress.md`.
