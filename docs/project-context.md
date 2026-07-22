# AriaDeck — Project Design & Context

**Status:** Product-ready core; ACCESS-001, en/zh-CN i18n foundation, and SEC-001 privacy redaction landed; remaining work is performance hardening, release packaging, and broader string migration  
**Last updated:** 2026-07-22  
**Primary stack:** Rust 1.96 · GPUI (Zed `v1.11.3` pin) · aria2 JSON-RPC over WebSocket · Tokio

This document is the single source of truth for product intent, architecture, accepted decisions, current capability, and remaining work. Prefer the workspace code over this file when they diverge; update this file when scope or architecture changes.

---

## 1. What AriaDeck Is

AriaDeck is a **native Rust desktop client for aria2**. It does not embed aria2 as a library. It manages or connects to an independent `aria2c` process through authenticated JSON-RPC (WebSocket only today).

Users can:

- Run a **managed local** engine (AriaDeck owns process lifecycle, session, and optional core registry).
- Point at an **external** local executable (user-owned binary; AriaDeck still may spawn it).
- Connect to a **remote** RPC endpoint (`ws`/`wss` …`/jsonrpc`).
- Keep multiple **profiles** (local/remote catalog); active profile switch is restart-bound.
- Install/import multiple **managed aria2 cores** side by side (import/link/verify/activate/rollback; no network update channel yet).

Product feel: modern desktop download manager (keyboard-first, high-density lists, batch actions), not a browser admin panel for aria2.

### Non-goals (still)

- Reimplementing the download engine
- Web UI / mobile
- AriaDeck cloud sync
- Arbitrary in-process plugins
- Treating remote engine paths as local filesystem paths
- Silent HTTP fallback when WebSocket/TLS/auth fails

---

## 2. Architecture

```text
GPUI
  → ariadeck-ui (tokens, components, shell)
  → ariadeck-desktop (composition root, windows, tray, bridges)
  → ariadeck-application (store, sync, commands, ports, views)
  → ariadeck-domain (IDs, task/engine/transfer types)
  → ariadeck-rpc | ariadeck-engine | ariadeck-settings
       ↓ WebSocket JSON-RPC
  managed / external / remote aria2
```

### Crates (actual workspace)

| Crate | Responsibility |
| --- | --- |
| `ariadeck-domain` | Strong IDs, task/engine/transfer value types, no I/O |
| `ariadeck-application` | Incremental download store, sync coordinator, commands, ports, derived views |
| `ariadeck-rpc` | WebSocket transport, auth token injection, typed aria2 adapter, notifications as refresh hints |
| `ariadeck-engine` | Local process lifecycle, profile ownership lock, core registry, crash restart |
| `ariadeck-settings` | Versioned typed settings, atomic save, migration, corruption recovery |
| `ariadeck-ui` | Design tokens, themes, GPUI components; pages must not depend on third-party widgets directly |
| `ariadeck-i18n` | Fluent catalogs (en, zh-CN), locale resolution, Translator |
| `ariadeck-telemetry` | Structured tracing setup |
| `ariadeck-desktop` | Bootstrap, composition, workspace model, platform (tray/notifications), dialogs |

**Not yet separate crates** (design-time names only): `ariadeck-core-manager` (lives in `ariadeck-engine::cores`), `ariadeck-storage` (SQLite still deferred), `ariadeck-platform` (partially in desktop).

### Dependency rules

1. Business logic must not depend on GPUI types.
2. Application pages depend only on `ariadeck-ui` + application/domain ports.
3. aria2 wire models stay in `ariadeck-rpc`; domain types are clean.
4. Secrets never appear in logs, settings JSON, or UI debug dumps.
5. Background work (RPC, FS, hashing, process wait) never runs on the GPUI render path; desktop owns a Tokio runtime for blocking work launched from GPUI tasks.

---

## 3. Core Design Principles

1. **GUI ⊥ engine** — only RPC; no fixed path/version assumption.
2. **Capabilities over versions** — `system.listMethods` → `EngineCapabilities`; empty probe is open-handed, non-empty probe is fail-closed for advanced writes.
3. **Incremental state** — tasks keyed by session-scoped GID; patches by field; no full-list replace every poll.
4. **Session generations** — stale-generation responses discarded; task identity is profile + session aware (Magnet `followedBy`/`belongsTo` migrate selection/details).
5. **Destructive ops are explicit** — remove task ≠ delete files; local Trash only for managed local filesystem capability.
6. **Unknown mutations: refresh once, never auto-replay** — timeouts/disconnects after a write are outcome-unknown; one authoritative reconcile; user may retry safely when engine state confirms absence.
7. **Local vs remote paths** — external/remote profiles never get “open folder” / Trash as if paths were local.
8. **Virtualization first** — off-screen task rows do not create GPUI elements.

---

## 4. Engine & RPC Model

### Engine sources

```text
Managed  → AriaDeck owns core install (optional) + process + session dir + lock
External → user executable path; still may be supervised as local process
Remote   → ARIADECK_RPC_URL or profile endpoint; connection-only (no managed spawn)
```

Local managed startup (summary): resolve executable (env → active core → profile pin → discovery) → exclusive profile lock → session recover/validate → loopback port + ephemeral secret → spawn with isolated RPC/session args → WebSocket connect → capability probe → snapshot sync → apply global options (proxy, speed limits, transfer policy) once per new session.

Shutdown: `aria2.shutdown` then kill/wait fallback. Tray **close** may hide window (engine keeps running); **Quit** drops `DesktopRoot` and stops the owned managed engine. Remote engines are never stopped by AriaDeck.

### RPC policy (hard)

- Only `ws` / `wss` with path `/jsonrpc`.
- No automatic HTTP fallback.
- Plain `ws` defaults to loopback; remote plaintext needs `ARIADECK_RPC_ALLOW_INSECURE_REMOTE=true`.
- WSS uses OS trust store; no cert bypass.
- Credentials/query/fragment rejected in URL; secret via `ARIADECK_RPC_SECRET` or credential ref.
- One actor owns the WebSocket; concurrent requests via IDs; notifications = refresh hints only.

Startup env knobs: `ARIADECK_RPC_URL`, `ARIADECK_RPC_SECRET`, connect/request/reconnect timing vars (see README).

---

## 5. Product Behavior (accepted decisions)

Compressed product contracts agents must not casually reverse:

| ID | Rule |
| --- | --- |
| D-001 | Filename is engine-owned after add; optional `out` for direct URI only (not BT/Metalink rename). |
| D-002 | Multi-line add = one task per non-empty line; mirrors require explicit mode. |
| D-003 | Selection is identity-based and query-scoped; select-all = current loaded query. |
| D-004 | Download proxy ≠ RPC proxy; passwords in OS keychain; session-bound `changeGlobalOption`. |
| D-005/007 | Remove keeps files; local delete → Trash; exact `tellStatus.files` paths + control files; containment checks. |
| D-006 | Retry = new GID with option/mirror replay; old failed result stays until removed. |
| D-008 | Output conflict: Keep both / Reject / Overwrite mapped to aria2 overwrite + auto-rename. |
| D-009 | Known-size free-space preflight before mutate; disk-full (code 9) surfaced. |
| D-010 | Mutations single-flight; unknown → one refresh; never auto-replay. |
| D-011 | Remote RPC WebSocket-only, fail-closed trust. |
| D-012 | Torrent/Metalink: client reads file, Base64 upload; no engine-side local path for remote. |
| D-013 | File selection preview-bound at add; live per-file progress later. |
| D-014 | Sort is local; queue priority changes authoritative waiting order (unfiltered ascending only). |
| D-015/027 | Advanced controls gated by `listMethods` / capabilities. |
| D-016/023 | Speed limits & transfer policy typed, scope-labeled, reapplied on new session. |
| D-017 | Detail projections on-demand, request-scoped, revision-bounded while drawer open. |
| D-018 | Seeding ≠ completed (`seeder=true`); stays in Active filter. |
| D-019 | Post-metadata output conflicts surfaced, not silent. |
| D-020 | Duplicate detection by normalized URI / info hash; open file/folder local-only. |
| D-021 | Stopped history is aria2 memory (`tellStopped`); managed `--max-download-result=5000`; paginated Load more. |
| D-022 | Advanced add (headers/auth/checksum) URI-only; secrets redacted. |
| D-024 | Context menu parity with toolbar; no second undo stack (Trash is recovery). |
| D-025 | Grouped completion/error toasts; Normal/Quiet/Silent + categories; session activity panel. |
| D-026 | Profile dir exclusive lock; corrupt session/profile recover with backup + notice. |
| D-028 | Multi-profile catalog schema 2; activate saves then **restart** to rebind engine. |
| D-029 | Core registry under `data/cores/aria2`; import/link; activate/rollback restart-bound. |
| D-030 | Tray + close-to-tray prefs; OS notifications; low-disk warnings. |
| D-031 | System/Light/Dark theme; debounced `window.json` geometry; last filter/sort only (not search text). |
| D-032 | **Privacy (SEC-001):** secrets and high-entropy tokens must never appear in UI projections, clipboard “copy source”, notices/activity, `Debug`/`Display` of config types, or diagnostic snapshots. Domain/engine may retain raw engine data for RPC and retry. Redaction lives in `ariadeck_domain::privacy`. |

#### SEC-001 sensitive-flow inventory (redaction boundary)

| Source | Stored raw? | UI / copy / log |
| --- | --- | --- |
| Download URI userinfo/query/fragment | Domain `primary_uri` (engine) | `redact_source_uri` in list/details/labels/clipboard |
| Magnet `tr` / `dn` / extra params | Domain until redact | Magnets collapse to `xt` info-hash only |
| BT announce trackers (path passkeys) | Details trackers | `redact_tracker_uri` (origin or safe `/announce`) |
| Active server / redirect URIs | Connection details | Sanitized like download sources |
| `getOption` secrets (passwd, cookie, header, proxy, tracker opts) | Cleared in RPC adapter (`redacted: true`) | Details show “Hidden” |
| Advanced add cookie/http-passwd | `SecretString` → aria2 only | Debug `[REDACTED]` |
| Download proxy password | OS keychain + `credential` UUID | Settings JSON has no password field |
| RPC secret | Env / keychain / ephemeral local | `RpcSecret` Debug redacted; URL creds rejected |
| Filenames (`out`) | Engine after add | Reject path separators / `.` / `..` |
| Local paths / Trash | Exact engine paths | Symlink components + containment rejected |
| Diagnostic export | N/A (no support-bundle UI) | `DiagnosticSnapshot` + redacted endpoint only |

### Settings schemas (current direction)

Settings are versioned JSON with migrations (`ariadeck-settings`). Notable fields: theme/scheme, download dir, download proxy (+ credential ref), speed limits, transfer policy, notification volume, platform/tray prefs, UI filter/sort. Separate `window.json` for geometry. Profile catalog is its own document (multi-profile schema 2). Core registry: `cores.json`.

---

## 6. Implementation Status

### MVP (Stages 1–8) — complete

Bootstrap, domain/application store, typed WS RPC, sync/reconnect, virtualized workspace, add/pause/resume/retry/remove, details drawer, local engine supervision, typed settings, speed chart, themes, health presentation.

### Post-MVP download manager — complete (P0/P1 + most P2 product surface)

| Area | Status |
| --- | --- |
| Filename / Magnet identity | Done |
| Multi-select + batch actions | Done |
| Multiline add / mirrors / unknown outcomes | Done |
| Retry option replay | Done |
| Safe remove + Trash | Done |
| Download proxy + keychain | Done |
| Torrent/Metalink add + file select | Done |
| Queue sort/move + pause-all | Done |
| Global/per-task rate limits + transfer policy | Done |
| Details: files/network/options | Done |
| Seeding state + seed options | Done |
| Duplicates, open path, path errors | Done |
| Stopped history pagination | Done |
| Advanced add controls | Done |
| Context menu / list ergonomics | Done |
| In-app + OS notifications, activity | Done |
| Profile ownership + multi-profile catalog | Done |
| Capability gating (`listMethods`) | Done |
| Managed core registry (local import) | Done |
| System tray / close-to-tray | Done |
| System theme + window geometry + list prefs | Done |

### Remaining (before multi-platform distribution)

| ID | Scope |
| --- | --- |
| `I18N-001` | **Done (en/zh-CN surface)** — Fluent catalogs (~360 keys), settings language (schema v8), hot-swap Translator; chrome/empty states/connection/engine, settings general/nav, dialogs, profiles/notices, task status badges, tray labels, error-code FTL mapping via `OperationErrorView::localized_summary`. Residual: niche advanced-dialog microcopy and some application-layer validation detail strings (stable error codes are localized). |
| `ACCESS-001` | **Done** — SR labels on settings/controls, status icon+text (not color-only), reduced-motion caret/loading, larger toggle/segment hit targets, locale-shaped size/rate formatters (`FormatOptions`), integrity check uses unified `Toggle`+`settings_row`. Manual residual: high-DPI visual check at 125%/150% on Windows. |
| `SEC-001` | **Done** — Shared `ariadeck-domain` privacy helpers (`redact_source_uri` / `redact_tracker_uri` / `task_option_key_is_sensitive` / `DiagnosticSnapshot`); list + details projections redact URI userinfo/query/fragment, magnet extras, tracker path tokens, and server URIs; option secrets cleared in RPC adapter; proxy/RPC secrets stay in keychain with Debug redaction; duplicate-add errors redact credentials; filename `out` rejects path separators; symlink components rejected on destination preflight (unix test). Residual: raw engine data retained in domain for retry; no user-facing support-bundle UI; manual Windows reparse-point check. |
| `PERF-001` | Stress: 10k stopped, rapid updates, details polling, reconnect storms, minimized mode, memory growth |
| `RELEASE-001` | Signing, installer/portable packaging, uninstall data retention, license notices, schema migration tests, app update/rollback |

### Explicitly deferred

- Network channels for downloading official aria2 packages
- SQLite multi-profile history/analytics (stopped history still aria2-owned)
- Per-profile proxy/limit bags (global settings for now)
- In-process profile switch without restart
- HTTP JSON-RPC transport as first-class profile option
- Pause/resume scheduling
- Tags/categories, browser/file associations
- Additional UI locales beyond en/zh-CN; remaining hard-coded English strings in dialogs/notices/application errors
- Remote path mapping / remote file management
- Application auto-update productization

---

## 7. Key Architecture Decisions (ADRs)

| ADR | Decision |
| --- | --- |
| 001 | `ariadeck-application` owns use cases, ports, store (no GPUI/RPC/SQLite). |
| 002 | Pin GPUI to Zed `v1.11.3` SHA `952d712dac48a4af2c54fb22c82d82a9d69b72d4`. |
| 003 | External/managed process path before full networked core installer. |
| 004 | Mutable state scoped to engine session generation. |
| 005 | Single actor owns each WebSocket; auth is transport decorator. |
| 006 | Sync serialized & cancellation-aware; retry budget resets after stable interval. |
| 007 | Typed JSON settings for app prefs; SQLite later for multi-entity storage. |
| 008 | Download proxy ≠ RPC transport; credentials in OS keychain. |
| 009 | Uncertain mutations reconciled from engine; Tokio for blocking from GPUI. |
| 010 | Remote RPC WebSocket-only; fail closed on trust/auth errors. |
| 011 | Fluent FTL catalogs in `ariadeck-i18n`; settings `language` (system/en/zh_cn); UI resolves locale and hot-swaps Translator; application errors stay code-oriented (full keying later). |

---

## 8. Developer Context

### Run / verify

```sh
cargo run -p ariadeck-desktop

cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p ariadeck-desktop
```

Live aria2 tests (ignored by default) need a real `aria2c` (e.g. Scoop) and `ARIA2C_PATH` where applicable.

### Where to look

| Concern | Start here |
| --- | --- |
| Task model / status / seeding | `crates/ariadeck-domain/src/task.rs` |
| Store, filters, selection identity | `crates/ariadeck-application/src/store.rs`, `view.rs` |
| Polling, reconnect, generations | `crates/ariadeck-application/src/sync.rs` |
| Commands & outcomes | `crates/ariadeck-application/src/commands.rs`, `ports.rs` |
| Wire models / options / multicall | `crates/ariadeck-rpc/src/` |
| Process / lock / cores | `crates/ariadeck-engine/src/` |
| Settings migrations | `crates/ariadeck-settings/src/lib.rs` |
| Workspace UI / dialogs | `apps/ariadeck-desktop/src/workspace.rs` |
| Design tokens / components | `crates/ariadeck-ui/src/` |
| i18n catalogs / Translator | `crates/ariadeck-i18n/` (see `docs/i18n.md`) |

### Implementation invariants (do not break)

- Session-bind every mutating command; reject stale session/generation.
- Prefer authoritative engine refresh over optimistic multi-step writes.
- Keep secrets in adapter/credential store; redact in UI projections, notices, clipboard source copy, Debug, and diagnostic snapshots (`ariadeck_domain::privacy`).
- Gate filesystem actions on managed-local capability + path containment.
- Prefer capability preflight over raw “method missing” transport errors for advanced UI.
- Profile activate and core activate are **restart-bound** until bridges support hot rebind.

### Working rules for agents

1. Code is source of truth; update this doc when behavior or package boundaries change.
2. Research aria2 manual / comparable clients before changing user-visible contracts.
3. Keep provider-neutral contracts in application; aria2 option strings at RPC boundary.
4. A feature is not done without tests or a recorded live check for engine-touching paths.
5. Do not expand scope into RELEASE/network installers unless asked.

---

## 9. Risks

| Risk | Mitigation in tree |
| --- | --- |
| GPUI API churn | Pinned Zed revision; UI confined to `ariadeck-ui` + desktop |
| Large queues | Virtualization, incremental patches, paged stopped history |
| aria2 build variance | `listMethods` capabilities; open-handed empty probe |
| Process/session corruption | Atomic writes, session backup rename, ownership lock, restart recovery |
| Remote path confusion | Capability flags; no local open/Trash for remote profiles |

---

## 10. Document history

Previous long-form design (`design.md`), stage checklist (`implementation-progress.md`), and post-MVP task log (`post-mvp-progress.md`) were consolidated into this file on 2026-07-22. Historical verification tables and commit-by-commit stage notes were intentionally dropped; recover from git history if needed.
