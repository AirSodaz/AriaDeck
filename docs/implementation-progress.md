# AriaDeck Implementation Progress

**Status:** MVP complete; post-MVP work remains

**Current stage:** Post-MVP download-management usability

**Last updated:** 2026-07-20

This document is the persistent source of truth for implementation state. It is
updated whenever scope, architecture, verification results, or commit boundaries
change.

## Delivery Plan

- [x] Stage 1 - Bootstrap workspace, pin GPUI, open a native window, enable tracing.
- [x] Stage 2 - Add domain and application state core with incremental patches.
- [x] Stage 3 - Implement typed aria2 WebSocket RPC transport and client.
- [x] Stage 4 - Coordinate polling, notifications, generations, and reconnection.
- [x] Stage 5 - Build the live, virtualized download workspace.
- [x] Stage 6 - Add interactive download commands and details.
- [x] Stage 7 - Manage a local external aria2 process and persistent profile.
- [x] Stage 8 - Complete and harden the MVP.
- [ ] Post-MVP - Managed aria2 core installation, platform integration, and release work.

Post-MVP usability and download-management work is tracked in
[`docs/post-mvp-progress.md`](post-mvp-progress.md).

## Current Stage

### Stage 1 - Bootstrap and GPUI risk probe

- [x] Confirm Rust toolchain availability (`rustc 1.96.0`).
- [x] Select and pin the GPUI revision from Zed stable release `v1.11.3`.
- [x] Define the initial workspace and dependency policy.
- [x] Add semantic light and dark theme tokens behind `ariadeck-ui`.
- [x] Add a minimal native desktop shell behind the AriaDeck UI boundary.
- [x] Add structured console tracing initialization.
- [x] Format the workspace.
- [x] Compile and test the workspace on Windows.
- [x] Launch the native window and verify the process remains healthy.
- [x] Commit the verified bootstrap milestone.

### Stage 2 - Domain and application state core

- [x] Add strong profile, session, task, GID, byte, and rate types.
- [x] Add task status, metadata, errors, progress, and ETA behavior.
- [x] Add a session-scoped incremental download store.
- [x] Reconcile full active/waiting snapshots separately from stopped pages.
- [x] Add stable derived GID views with filtering and sorting.
- [x] Add fixed-capacity speed history.
- [x] Define application ports, typed commands, and structured outcomes.
- [x] Verify semantic no-op patches do not increase revisions.

### Stage 3 - Typed aria2 WebSocket RPC

- [x] Define JSON-RPC request, response, error, and notification envelopes.
- [x] Centralize secret token injection without exposing secrets to logs.
- [x] Add a concurrent WebSocket transport with unique request IDs and timeouts.
- [x] Match out-of-order responses to pending requests.
- [x] Add typed `getVersion`, `getGlobalStat`, `tellActive`, `tellWaiting`, and
  `tellStopped` methods.
- [x] Convert aria2 decimal strings and optional fields into domain types safely.
- [x] Treat notifications as targeted refresh hints rather than complete state.
- [x] Add contract tests for malformed data, RPC errors, authentication, timeout,
  notification, and out-of-order responses.
- [x] Run a real `aria2c` WebSocket smoke test using the local Scoop installation.

### Stage 4 - Synchronization and reconnection coordinator

- [x] Add capability verification and an initial batched state snapshot.
- [x] Apply live and stopped responses only to the matching session generation.
- [x] Coordinate foreground/background polling intervals without overlapping cycles.
- [x] Convert WebSocket notifications into deduplicated targeted refresh requests.
- [x] Preserve the last-known store and mark it stale while disconnected.
- [x] Add exponential reconnect backoff with jitter and a maximum delay.
- [x] Discard late responses from superseded connection attempts.
- [x] Test reconnect, cancellation, notification storms, and stale-generation races.

### Stage 5 - Live virtualized download workspace

- [x] Compose the RPC connector and sync coordinator in the desktop application.
- [x] Bridge coordinator events and snapshots into a GPUI-owned workspace model.
- [x] Build the sidebar, header, search, filters, status summary, and task rows.
- [x] Virtualize task rendering so off-screen rows do not create GPUI elements.
- [x] Preserve selection by stable task identity across filtering and updates.
- [x] Represent connecting, stale, disconnected, empty, and error states explicitly.
- [x] Verify light/dark themes, keyboard focus, and accessible control names.
- [x] Exercise a 10,000-task fixture and confirm only visible rows are rendered.
- [x] Launch the desktop application against the local aria2 process as a smoke test.

### Stage 6 - Interactive download commands and task details

The add-dialog, safe lifecycle commands, details drawer, and session-bound
desktop composition are implemented. Retry creates a new task only when the
failed task exposes a replayable URI or info hash; the failed result remains
visible until the user explicitly removes it.

- [x] Bind command and details requests to the exact engine session and reject stale requests.
- [x] Reject commands immediately while connecting, synchronizing, stale, or disconnected.
- [x] Model remote engine paths without treating them as local filesystem paths.
- [x] Add an on-demand typed task-details projection with file metadata.
- [x] Keep high-frequency live refreshes on a lightweight projection and cache static metadata.
- [x] Distinguish live-task removal from stopped download-result removal.
- [x] Report mutating RPC timeouts and disconnects as unknown outcomes without auto-retry.
- [x] Extend the typed aria2 adapter for add, pause, resume, and remove commands.
- [x] Apply profile download proxy settings through session-bound
  `changeGlobalOption`, with explicit clearing, unknown-outcome handling, and
  once-per-new-session reapplication.
- [x] Extend the typed adapter and application contract for explicit failed-task retry.
- [x] Preserve replayable per-task options and mirrors when retry creates its replacement GID.
- [x] Reconcile unknown single/batch retry outcomes through one authoritative refresh without replay.
- [x] Expose typed keep-both/reject/overwrite policy for new tasks and map it
  authoritatively to aria2 conflict options.
- [x] Preflight managed-local download directories with a real write probe and
  available-space query while treating external RPC paths as remote-only.
- [x] Accumulate only successfully preflighted local download roots so changing
  settings preserves safe file removal for old and new tasks.
- [x] Reject known-size local submissions before engine mutation when free
  space is insufficient, and expose aria2 disk-full error code 9 in task UI.
- [x] Keep downloaded files by default when removing tasks or stopped results.
- [x] Gate explicit file deletion to the managed local engine, move exact task
  files and incomplete control files to Trash, and reject unsafe paths.
- [x] Preserve per-item removal outcomes and reconcile unknown mutations once
  without automatic replay.
- [x] Execute commands through the application ports with structured outcomes.
- [x] Add a focused add-download flow for URLs and magnet links.
- [x] Add row actions and keyboard commands for the safe task lifecycle operations.
- [x] Add a right-side details drawer that preserves list context and selection.
- [x] Load task overview and file details without blocking the GPUI render thread.
- [x] Require explicit confirmation before destructive removal or file deletion.
- [x] Verify command success, RPC failure, stale-generation, and reconnect behavior.
- [x] Exercise the complete command flow against the local aria2 process.

### Stage 7 - Local external aria2 process and persistent profile

- [x] Resolve an explicit executable path, PATH entry, or the local Scoop aria2 installation.
- [x] Validate the executable with `--version` before spawning it.
- [x] Create a profile-scoped data directory with separate config, session, log,
  and download paths.
- [x] Select an available loopback port and generate an ephemeral RPC secret.
- [x] Start aria2 with isolated RPC, session, and peer-discovery arguments.
- [x] Keep the process handle and secret out of debug output and profile metadata.
- [x] Request `aria2.shutdown` during desktop teardown and retain a kill/wait fallback.
- [x] Persist non-secret profile metadata through an atomic JSON replacement.
- [x] Verify local process startup and desktop composition without a pre-existing RPC URL.
- [x] Add supervised crash restart and repeated-crash recovery policy.

### Stage 8 - MVP completion and hardening

- [x] Add versioned, validated typed settings with atomic persistence and
  corruption-preserving recovery.
- [x] Migrate settings schema v1 to v2 with validated download-proxy fields and
  non-secret credential references.
- [x] Load and save the light/dark theme and default download directory outside
  the render path.
- [x] Store proxy passwords in the operating-system credential manager, mask
  password UI/debug values, and roll back credential changes when apply or
  persistence fails.
- [x] Add an accessible settings flow and apply the configured destination to
  both local-engine startup and newly added tasks.
- [x] Feed the application-layer fixed-capacity speed history from live global
  statistics and render a bounded one-minute chart.
- [x] Surface local-engine running, restarting, and terminal failure states in
  the desktop workspace.
- [x] Complete workspace tests, Clippy, native build, real aria2 flows, and UI
  smoke verification on the merged MVP tree.
- [x] Record explicit MVP deferrals and mark the implementation complete.

### MVP Completion Audit

The initial MVP scope in `docs/design.md` is implemented and verified on the
current tree: one active external local aria2 profile, authenticated WebSocket
RPC, active/waiting/stopped/completed/failed views, URL/magnet add, pause,
resume, retry, remove, virtualized lists, search/filtering, task details,
global speed, bounded speed chart, configurable download directory, persisted
light/dark theme, session-safe state, and local engine health monitoring.

The explicit post-MVP deferrals are managed core installation and rollback,
multiple simultaneous engines, peer/proxy/queue advanced controls, remote path
mapping, auto-update and packaging, persistent historical analytics, custom
tags/automation, SQLite-backed multi-profile metadata, and diagnostic bundle
export. These remain outside the first usable MVP and are listed in Known Gaps.

## Architecture Decisions

### ADR-001 - Add an application crate

The design document names Application Services, the Download Store, and command
coordination but does not assign them a physical package. Stage 2 will add
`ariadeck-application`. It will own use cases, ports, session coordination, and
incremental store behavior without depending on GPUI, JSON-RPC wire models, or
SQLite.

### ADR-002 - Pin the GPUI revision shipped by a Zed stable release

The workspace pins `gpui` and `gpui_platform` to the full commit SHA for Zed
`v1.11.3` (`952d712dac48a4af2c54fb22c82d82a9d69b72d4`). This revision includes the
current Windows platform split and renderer hardening, while avoiding movement
on `main`. Application pages will consume only `ariadeck-ui` abstractions.

### ADR-003 - Deliver the external-engine path before managed core installation

The MVP will first manage a user-provided aria2 executable. This satisfies the
design's local-engine requirement while preserving managed core installation as
an independent, security-sensitive vertical slice. The local executable at
`~/scoop/apps/aria2/current` is available for integration tests.

### ADR-004 - Scope mutable state to an engine session

An aria2 GID is not globally unique. Each download store will belong to an
explicit profile and engine-session generation. Responses from stale generations
will be discarded, and cross-engine references will use a stable task identity.

### ADR-005 - Give one actor exclusive ownership of each WebSocket

The WebSocket transport owns its socket in one background actor. Callers send
commands through a bounded channel; request IDs route out-of-order responses to
oneshot waiters, and a broadcast channel exposes notifications only as refresh
hints. Authentication is a transport decorator, so typed methods never construct
or log token parameters.

### ADR-006 - Keep synchronization serialized and cancellation-aware

One application actor owns the mutable store and serializes polling, notification
refreshes, generation transitions, and reconnect attempts. Terminal notifications
refresh live and stopped collections together; waiting snapshots are fully paged
before authoritative reconciliation. A separate cancellation signal interrupts
in-flight RPC work, and `stop()` waits for both the coordinator and WebSocket actor
to finish. Retry budgets span connect, initial-sync, and short-lived session
failures, and reset only after a configurable stable connection interval.

### ADR-007 - Use a typed JSON settings document for the single-profile MVP

The MVP stores its small, user-edited settings document through a dedicated
`ariadeck-settings` boundary with a schema version, validation, atomic
replacement, and corruption-preserving recovery. This keeps settings logic out
of GPUI and makes a later SQLite adapter possible without changing UI contracts.
SQLite remains the target for multi-profile metadata, history, installation
records, and diagnostics; persistent speed analytics are explicitly post-MVP.

### ADR-008 - Separate download proxy, RPC transport, and credential storage

aria2 download traffic is configured through a typed application command and
the RPC adapter's `changeGlobalOption` implementation. It is independent from
the WebSocket connection endpoint and any future application-update proxy.
Settings JSON stores validated non-secret proxy fields and an opaque credential
reference; the operating-system credential manager owns the password. Mutations
are bound to the exact engine session and are not replayed after an unknown
outcome in that session, while a newly connected session receives the latest
persisted configuration once.

### ADR-009 - Reconcile uncertain mutations from authoritative engine state

Task mutations remain single-flight in the desktop. Successful and
outcome-unknown commands schedule an authoritative refresh, but uncertain
mutations are never replayed automatically. Exact request/session matching
rejects stale results, and Magnet parent-to-child transitions migrate focused
and non-focused selections, the range anchor, and the details drawer together.
Blocking filesystem, settings, and credential work launched from GPUI tasks is
always dispatched through the desktop-owned Tokio runtime rather than assuming
the UI executor has entered a Tokio reactor.

### ADR-010 - Keep remote RPC WebSocket-only and fail closed on trust errors

The current RPC adapter accepts only `ws`/`wss` at `/jsonrpc`; HTTP is not an
automatic fallback because it removes notification semantics and could mask a
TLS or authentication failure. Plain WebSocket defaults to loopback-only, while
an explicit startup override can permit it on a trusted network. WSS uses
operating-system trust roots with no validation bypass. URL credentials,
queries, and fragments are rejected, the method token remains separate, and
handshake diagnostics retain status codes without response headers. Connection
and reconnect timing is bounded through validated startup configuration.

## Verification Log

| Date | Command or check | Result |
| --- | --- | --- |
| 2026-07-19 | `rustc --version --verbose` | Pass - Rust 1.96.0 MSVC host |
| 2026-07-19 | `cargo info gpui` | Pass - crates.io release 0.2.2 available |
| 2026-07-19 | GPUI release/API review | Selected Zed v1.11.3 immutable revision |
| 2026-07-19 | `cargo test --workspace` | Pass - 5 suites, 1 test |
| 2026-07-19 | `cargo clippy --workspace --all-targets -- -D warnings` | Pass - no issues |
| 2026-07-19 | `cargo build -p ariadeck-desktop` | Pass |
| 2026-07-19 | Desktop launch smoke (5 seconds) | Pass - process remained healthy and was cleaned up |
| 2026-07-19 | `cargo test -p ariadeck-domain -p ariadeck-application` | Pass - 15 tests |
| 2026-07-19 | `cargo test --workspace` | Pass - 16 tests across 9 suites |
| 2026-07-19 | `cargo clippy --workspace --all-targets -- -D warnings` | Pass - no issues after Stage 2 |
| 2026-07-19 | `cargo test -p ariadeck-rpc` | Pass - 14 tests, 1 live test ignored by default |
| 2026-07-19 | Live `aria2c 1.37.0` RPC test | Pass - authenticated version/stat/list/shutdown in 4.41 seconds |
| 2026-07-19 | `cargo test --workspace` | Pass - 30 tests, 1 ignored across 12 suites |
| 2026-07-19 | `cargo clippy --workspace --all-targets -- -D warnings` | Pass - no issues after Stage 3 |
| 2026-07-19 | `cargo test -p ariadeck-application -p ariadeck-rpc` | Pass - 38 tests, 2 live tests ignored by default |
| 2026-07-19 | Live coordinator and RPC tests against `aria2c 1.37.0` | Pass - initial sync, forced exit, stale state, higher-generation reconnect, and shutdown in 4.42 seconds |
| 2026-07-19 | Post-test `aria2c` process check | Pass - zero residual processes |
| 2026-07-19 | `cargo test --workspace` | Pass - 45 tests, 2 ignored across 12 suites |
| 2026-07-19 | `cargo clippy --workspace --all-targets -- -D warnings` | Pass - no issues after Stage 4 |
| 2026-07-19 | `cargo test -p ariadeck-desktop -p ariadeck-ui` | Pass - 7 focused presentation and composition tests |
| 2026-07-19 | `cargo clippy -p ariadeck-desktop -p ariadeck-ui --all-targets -- -D warnings` | Pass - no issues in the Stage 5 crates |
| 2026-07-19 | `cargo build -p ariadeck-desktop` | Pass - native desktop executable built successfully |
| 2026-07-19 | 10,000-task GPUI fixture | Pass - fewer than 64 task rows rendered for one viewport |
| 2026-07-19 | Native offline UI and accessibility smoke | Pass - 1180 x 760 content layout, search focus/IME path, filters, and both themes verified |
| 2026-07-19 | Authenticated live aria2 desktop smoke | Pass - connected state, paused task projection, selection, filtering, stale data, and higher-generation reconnect verified |
| 2026-07-19 | Post-Stage 5 smoke process check | Pass - zero residual `aria2c` or `ariadeck-desktop` processes |
| 2026-07-19 | `cargo test --workspace` | Pass - 51 tests, 2 ignored across 12 suites after Stage 5 |
| 2026-07-19 | `cargo clippy --workspace --all-targets -- -D warnings` | Pass - no issues after Stage 5 |
| 2026-07-19 | `cargo test -p ariadeck-domain -p ariadeck-application -p ariadeck-rpc` | Pass - 53 tests, 2 ignored for the reviewed Stage 6 session-bound command/details foundation |
| 2026-07-19 | `cargo clippy -p ariadeck-domain -p ariadeck-application -p ariadeck-rpc --all-targets -- -D warnings` | Pass - no issues in the Stage 6 backend slice |
| 2026-07-19 | `cargo test -p ariadeck-ui -p ariadeck-desktop` | Pass - 21 presentation/composition tests, including AccessKit text-field behavior |
| 2026-07-19 | `cargo test -p ariadeck-rpc -p ariadeck-application` | Pass - 48 tests, 3 ignored |
| 2026-07-19 | `cargo clippy -p ariadeck-ui -p ariadeck-desktop -p ariadeck-rpc -p ariadeck-application --all-targets -- -D warnings` | Pass - no issues |
| 2026-07-19 | `cargo build -p ariadeck-desktop` | Pass - native desktop executable built successfully |
| 2026-07-19 | Ignored live command flow with local `aria2c 1.37.0` | Pass - add, pause, resume, live removal, and stopped-result removal |
| 2026-07-19 | Post-command process check | Pass - zero residual `aria2c` or `ariadeck-desktop` processes |
| 2026-07-19 | `cargo test -p ariadeck-engine -p ariadeck-desktop` | Pass - 7 tests, 1 ignored; profile, process paths, and desktop composition compile |
| 2026-07-19 | Ignored `ariadeck-engine` process test with Scoop `aria2c 1.37.0` | Pass - dynamic endpoint, profile files, secret redaction, and cleanup |
| 2026-07-19 | Local desktop startup smoke without `ARIADECK_RPC_URL` | Pass - desktop stayed alive, aria2 child and profile metadata were created, and child was cleaned up |
| 2026-07-19 | aria2 argument compatibility smoke | Pass - aria2 1.37.0 stayed alive with loopback RPC and DHT/LPD disabled |
| 2026-07-19 | `cargo clippy -p ariadeck-engine -p ariadeck-desktop --all-targets -- -D warnings` | Pass - no issues |
| 2026-07-19 | `cargo test -p ariadeck-application -p ariadeck-rpc -p ariadeck-ui -p ariadeck-desktop` | Pass - 73 tests, 3 ignored after failed-task retry |
| 2026-07-19 | Failed-task live aria2 retry flow | Pass - terminal Error metadata produced a new task with a distinct GID |
| 2026-07-19 | `cargo clippy -p ariadeck-application -p ariadeck-rpc -p ariadeck-ui -p ariadeck-desktop --all-targets -- -D warnings` | Pass - no issues |
| 2026-07-19 | `cargo test -p ariadeck-engine -p ariadeck-desktop` | Pass - 7 tests, 2 real-process tests ignored by default |
| 2026-07-19 | `cargo clippy -p ariadeck-engine -p ariadeck-desktop --all-targets -- -D warnings` | Pass - no issues after local-engine supervision |
| 2026-07-19 | Ignored supervised-crash test with Scoop `aria2c 1.37.0` | Pass - first exit restarted with a new PID on the same endpoint and secret; the next in-window exit reached the crash budget and entered `Failed` |
| 2026-07-19 | `cargo test -p ariadeck-settings` | Pass - 4 tests covering initialization, round-trip, corrupt recovery, and future-version rejection |
| 2026-07-19 | `cargo clippy -p ariadeck-settings --all-targets -- -D warnings` | Pass - no issues in the typed settings boundary |
| 2026-07-19 | `cargo test -p ariadeck-settings -p ariadeck-ui -p ariadeck-desktop` | Pass - 31 tests covering ordered background persistence, stale-result rejection, theme application, and destination mapping |
| 2026-07-19 | `cargo clippy -p ariadeck-settings -p ariadeck-ui -p ariadeck-desktop --all-targets -- -D warnings` | Pass - no issues in settings integration |
| 2026-07-19 | `cargo build -p ariadeck-desktop` | Pass - native desktop executable built with persisted settings integration |
| 2026-07-19 | `cargo test -p ariadeck-application -p ariadeck-ui -p ariadeck-desktop` | Pass - 56 tests including bounded 500 ms sampling, unchanged-stat publication, latest-window clamping, and chart composition |
| 2026-07-19 | `cargo clippy -p ariadeck-application -p ariadeck-ui -p ariadeck-desktop --all-targets -- -D warnings` | Pass - no issues after speed-history integration |
| 2026-07-19 | `cargo test -p ariadeck-engine -p ariadeck-ui -p ariadeck-desktop` | Pass - 32 tests, 2 real-process tests ignored by default; includes health presentation and mapping |
| 2026-07-19 | `cargo clippy -p ariadeck-engine -p ariadeck-ui -p ariadeck-desktop --all-targets -- -D warnings` | Pass - no issues after local-engine health integration |
| 2026-07-19 | Ignored supervised double-crash test with Scoop `aria2c 1.37.0` | Pass - recovery count remained observable, terminal failure honored the budget, weak handle expired, and no process remained |
| 2026-07-19 | `cargo test --workspace` | Pass - 95 tests, 5 real-process tests ignored by default across 16 suites |
| 2026-07-19 | `cargo clippy --workspace --all-targets -- -D warnings` | Pass - no workspace issues |
| 2026-07-19 | `cargo build -p ariadeck-desktop` | Pass - native desktop executable built successfully |
| 2026-07-19 | All ignored live aria2 flows with Scoop `aria2c 1.37.0` | Pass - engine lifecycle and 3 RPC/coordinator/command flows; no residual current-tree desktop or aria2 process |
| 2026-07-19 | Native GPUI MVP smoke | Pass - connected local engine, speed chart accessibility group, health status, settings dialog focus trap, light-theme save, status notice, and clean close verified at 1180 x 760 |
| 2026-07-20 | Post-MVP filename and multi-selection tests across domain/application/RPC/UI/desktop | Pass - 125 passed, 3 ignored; includes custom output names, Magnet identity migration, query-scoped selection, batch partial results, and per-task failure details |
| 2026-07-20 | Post-MVP filename and multi-selection Clippy gate across domain/application/RPC/UI/desktop | Pass - no issues with `-D warnings` |
| 2026-07-20 | Post-MVP multiline add, mirror grouping, and unknown-outcome reconciliation tests | Pass - 135 passed, 3 ignored; coordinator test proves accepted-but-unknown add resolves from a new GID after refresh with one gateway call |
| 2026-07-20 | Real aria2 add/removal flow after multiline work | Pass - two-source mirror add, paused projection, live removal, stopped-result removal, and cleanup succeeded |
| 2026-07-20 | Post-MVP retry correctness tests across domain/application/RPC/UI/desktop | Pass - 140 passed, 3 ignored; option replay, new-GID semantics, old-result retention, and single/partial-batch unknown reconciliation are covered |
| 2026-07-20 | Post-MVP retry Clippy gate across domain/application/RPC/UI/desktop | Pass - no issues with `-D warnings` |
| 2026-07-20 | Real aria2 option-preserving retry flow | Pass - distinct GID, old failed result retained, no-URI-data source fallback, and output/directory/Cookie header/HTTP credential/limit/connection/checksum preservation verified |
| 2026-07-20 | `cargo test --workspace` after safe local file removal | Pass - 153 tests, 5 ignored; includes keep-files defaults, local/remote capability gating, exact path/control-file containment, Trash errors, partial batches, and unknown-outcome reconciliation |
| 2026-07-20 | `cargo clippy --workspace --all-targets -- -D warnings` after safe local file removal | Pass - no issues |
| 2026-07-20 | Real aria2 removal flow with a completed HTTP download | Pass - active and terminal records were removed while the downloaded file remained byte-for-byte unchanged |
| 2026-07-20 | `cargo test --workspace` after output conflict and destination preflight | Pass - 160 tests, 5 ignored; includes all three conflict mappings, write-probe cleanup, free-space and required-size checks, Windows path normalization, unsafe local-path rejection, and external RPC path isolation |
| 2026-07-20 | `cargo clippy --workspace --all-targets -- -D warnings` after output preflight | Pass - no issues |
| 2026-07-20 | Real aria2 same-name output flow | Pass - default keep-both created the `.1` file and result removal preserved both completed files |
| 2026-07-20 | `cargo test --workspace` after authorized-root and disk-full handling | Pass - 164 tests, 5 ignored; includes settings-root accumulation, unconfigured-root rejection, known-size preflight, RPC error-code preservation, and task error presentation |
| 2026-07-20 | `cargo clippy --workspace --all-targets -- -D warnings` after disk-full handling | Pass - no issues |
| 2026-07-20 | `cargo test --workspace` after download-proxy and credential-store integration | Pass - 178 tests, 6 ignored; includes settings migration/recovery, proxy validation, secret redaction, credential rollback, exact-session apply, aria2 option mapping, and GPUI proxy drafts |
| 2026-07-20 | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `cargo build -p ariadeck-desktop` | Pass - no warnings, formatting clean, native desktop executable built |
| 2026-07-20 | All ignored live RPC tests with Scoop `aria2c 1.37.0` | Pass - 4 flows; authenticated proxy 407 retry, routed request, `no-proxy` bypass, Disabled direct traffic, connection restart, command/removal contracts, and clean shutdown |
| 2026-07-20 | `cargo test -p ariadeck-application -p ariadeck-ui` after command-state reconciliation | Pass - 94 tests; unknown outcomes refresh once without replay, duplicate submissions remain single-flight, and Magnet successor state migrates completely |
| 2026-07-20 | `cargo test -p ariadeck-desktop`; isolated connected desktop startup | Pass - 30 tests plus six-second native observation; explicit runtime dispatch prevents the GPUI-thread `no reactor running` panic |
| 2026-07-20 | `cargo test --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `cargo build -p ariadeck-desktop` | Pass - 183 tests, 6 ignored; no warnings, formatting clean, native desktop executable built |
| 2026-07-20 | All ignored live RPC tests with real `aria2c 1.37.0` after command-state reconciliation | Pass - 4 flows; authenticated reads, restart/reconnect, command/removal contracts, proxy routing/bypass/disable, and clean shutdown |
| 2026-07-20 | `cargo test --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `cargo build -p ariadeck-desktop` after RPC connection hardening | Pass - 190 tests, 7 ignored; strict endpoint policy, startup timing controls, TLS/authentication classification, redaction, formatting, lints, and desktop build all pass |
| 2026-07-20 | All ignored live RPC tests with real `aria2c 1.37.0` after RPC connection hardening | Pass - 5 flows; valid authentication, invalid-secret rejection without leakage, restart/reconnect, command/removal contracts, proxy behavior, and clean shutdown |

## Known Gaps

- Failed-task retry preserves aria2's replayable per-task options and mirror
  sources. Engine-scoped cookie-file loading is not exposed as per-task state;
  task-level Cookie headers are preserved.
- Explicit local deletion now moves exact engine-reported task files and
  incomplete control files to the operating-system Trash. Output-side safety
  now covers local directory writability/space checks, remote path isolation,
  accumulated authorized roots, known-size submission checks, direct-task
  conflict policy, and explicit runtime disk-full errors. Torrent/Metalink
  per-file containment/conflicts remain coupled to their future import and
  file-selection flows.
- Local process recovery and terminal failure are visible in the desktop;
  persistent engine-health history and exported diagnostic bundles are post-MVP.
- Profile and typed settings persistence use atomic JSON documents for the
  single-profile MVP; SQLite remains planned for multi-profile metadata,
  history, installation records, and diagnostics.
- Add-download, task lifecycle commands, and the details drawer are implemented
  for the external WebSocket engine path.
- Theme, download-directory, and aria2 download-proxy choices persist; proxy
  passwords remain outside JSON in the system credential manager. Window
  geometry remains session-only.
- WSS validation and remote RPC transport policy are complete. An application-
  side network proxy for reaching a remote RPC endpoint remains future work;
  the current proxy controls affect aria2 download traffic only.
- The optional Windows DXGI debug layer is absent; GPUI logs a development-only
  warning and continues with DirectX debugging disabled.
- The repository license has not been selected; release metadata remains provisional.
- HTTP JSON-RPC remains a future explicit profile capability. AriaDeck does not
  automatically downgrade to it after WebSocket, TLS, or authentication errors.
- Managed aria2 download, verification, extraction, activation, and rollback are post-MVP.

## Commit Log

- `chore: bootstrap AriaDeck workspace` - Stage 1 foundation.
- `feat: add domain and application state core` - Stage 2 state and command foundation.
- `feat: implement typed aria2 websocket RPC` - Stage 3 transport and adapter.
- `feat: coordinate aria2 synchronization and reconnects` - Stage 4 live-state coordinator.
- `feat: build live virtualized download workspace` - Stage 5 native presentation and composition.
- `feat: add session-bound task command foundation` - Stage 6 command and details backend checkpoint.
- `feat: complete interactive command and details workspace` - Stage 6 UI, AccessKit input, command outcomes, and live aria2 verification (retry pending).
- `feat: manage local external aria2 profiles` - Stage 7 process lifecycle, isolated runtime files, and atomic profile metadata.
- `feat: retry failed downloads from known sources` - session-bound replay using discovery metadata and a new aria2 GID.
- `feat: supervise local aria2 crash recovery` - bounded same-endpoint restart and terminal health state.
- `feat: persist typed application settings` - versioned JSON settings with validation and corruption-preserving recovery.
- `feat: integrate persistent desktop preferences` - accessible settings UI, ordered background saves, and configured add-task destinations.
- `feat: chart bounded transfer speed history` - application-owned half-second samples and a stable one-minute download/upload chart.
- `feat: surface local engine recovery health` - weak supervision observer, retained restart counts, and persistent failure presentation.
- `feat: complete post-mvp download safety and proxy controls` - filename,
  multi-selection, add/retry/removal safety, output preflight, and profile
  download-proxy checkpoint.

## Final MVP Checkpoint

- `2026-07-19` - Stage 8 completion audit passed on the merged current tree.
- `2026-07-19` - Native smoke used the current `target/debug/ariadeck-desktop.exe`;
  the unrelated legacy `D:\projects\ariadeck` window was not used as evidence.
