# AriaDeck Implementation Progress

**Status:** In progress

**Current stage:** 2 - Domain and application state core

**Last updated:** 2026-07-19

This document is the persistent source of truth for implementation state. It is
updated whenever scope, architecture, verification results, or commit boundaries
change.

## Delivery Plan

- [x] Stage 1 - Bootstrap workspace, pin GPUI, open a native window, enable tracing.
- [ ] Stage 2 - Add domain and application state core with incremental patches.
- [ ] Stage 3 - Implement typed aria2 WebSocket RPC and synchronization.
- [ ] Stage 4 - Build the live, virtualized download workspace.
- [ ] Stage 5 - Add download commands and structured partial outcomes.
- [ ] Stage 6 - Manage a local external aria2 process and persistent profile.
- [ ] Stage 7 - Complete and harden the MVP.
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

- [ ] Add strong profile, session, task, GID, byte, and rate types.
- [ ] Add task status, metadata, errors, progress, and ETA behavior.
- [ ] Add a session-scoped incremental download store.
- [ ] Reconcile full active/waiting snapshots separately from stopped pages.
- [ ] Add stable derived GID views with filtering and sorting.
- [ ] Add fixed-capacity speed history.
- [ ] Define application ports, typed commands, and structured outcomes.
- [ ] Verify semantic no-op patches do not increase revisions.

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

## Known Gaps

- The bootstrap shell is static until the application and RPC layers exist.
- The optional Windows DXGI debug layer is absent; GPUI logs a development-only
  warning and continues with DirectX debugging disabled.
- The repository license has not been selected; release metadata remains provisional.
- HTTP JSON-RPC fallback is intentionally after the WebSocket MVP path.
- Managed aria2 download, verification, extraction, activation, and rollback are post-MVP.

## Commit Log

- `chore: bootstrap AriaDeck workspace` - Stage 1 foundation.
