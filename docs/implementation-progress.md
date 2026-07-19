# AriaDeck Implementation Progress

**Status:** In progress

**Current stage:** 4 - Synchronization and reconnection coordinator

**Last updated:** 2026-07-19

This document is the persistent source of truth for implementation state. It is
updated whenever scope, architecture, verification results, or commit boundaries
change.

## Delivery Plan

- [x] Stage 1 - Bootstrap workspace, pin GPUI, open a native window, enable tracing.
- [x] Stage 2 - Add domain and application state core with incremental patches.
- [x] Stage 3 - Implement typed aria2 WebSocket RPC transport and client.
- [ ] Stage 4 - Coordinate polling, notifications, generations, and reconnection.
- [ ] Stage 5 - Build the live, virtualized download workspace.
- [ ] Stage 6 - Add interactive download commands and details.
- [ ] Stage 7 - Manage a local external aria2 process and persistent profile.
- [ ] Stage 8 - Complete and harden the MVP.
- [ ] Post-MVP - Managed aria2 core installation, platform integration, and release work.

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

- [ ] Add capability verification and an initial batched state snapshot.
- [ ] Apply live and stopped responses only to the matching session generation.
- [ ] Coordinate foreground/background polling intervals without overlapping cycles.
- [ ] Convert WebSocket notifications into deduplicated targeted refresh requests.
- [ ] Preserve the last-known store and mark it stale while disconnected.
- [ ] Add exponential reconnect backoff with jitter and a maximum delay.
- [ ] Discard late responses from superseded connection attempts.
- [ ] Test reconnect, cancellation, notification storms, and stale-generation races.

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

## Known Gaps

- The bootstrap shell is static until the application and RPC layers exist.
- The optional Windows DXGI debug layer is absent; GPUI logs a development-only
  warning and continues with DirectX debugging disabled.
- The repository license has not been selected; release metadata remains provisional.
- HTTP JSON-RPC fallback is intentionally after the WebSocket MVP path.
- Managed aria2 download, verification, extraction, activation, and rollback are post-MVP.

## Commit Log

- `chore: bootstrap AriaDeck workspace` - Stage 1 foundation.
- `feat: add domain and application state core` - Stage 2 state and command foundation.
- `feat: implement typed aria2 websocket RPC` - Stage 3 transport and adapter.
