# AriaDeck — Project Context

**Status:** Product-ready core (MVP + download-manager surface). Landed: ACCESS-001, I18N-001 (en/zh-CN), SEC-001, PERF-001, RELEASE-001 (Windows).  
**Last updated:** 2026-07-22  
**Stack:** Rust 1.96 · GPUI (Zed `v1.11.3`) · aria2 JSON-RPC (WebSocket) · Tokio  

Single source of truth for intent, architecture, contracts, and residual work. Prefer code when it diverges; update this file when scope or boundaries change.  
**Next work:** [`docs/roadmap.md`](roadmap.md) · **Release:** [`docs/release.md`](release.md) · **i18n:** [`docs/i18n.md`](i18n.md)

---

## 1. Product

Native Rust desktop client for **aria2**. Does not embed aria2 as a library. Manages or connects to an independent `aria2c` via authenticated JSON-RPC (WebSocket only).

| Mode | Behavior |
| --- | --- |
| Managed local | Owns process lifecycle, session, optional core registry |
| External local | User binary; may still supervise process |
| Remote | Profile / `ARIADECK_RPC_URL`; connection-only |

Multiple **profiles** (activate = restart-bound). Multiple **managed cores** (import/link/verify/activate/rollback; no network install channel).

**Feel:** keyboard-first download manager (dense lists, batch ops)—not a browser admin panel for aria2.

### Non-goals

Reimplement the engine · Web/mobile UI · Cloud sync · In-process plugins · Treat remote paths as local FS · Silent HTTP fallback when WS/TLS/auth fails

---

## 2. Architecture

```text
GPUI → ariadeck-ui → ariadeck-desktop (composition, tray, bridges)
                  → ariadeck-application (store, sync, commands, ports)
                  → ariadeck-domain
                  → ariadeck-rpc | ariadeck-engine | ariadeck-settings
                       ↓ WS JSON-RPC
                  managed / external / remote aria2
```

| Crate | Responsibility |
| --- | --- |
| `ariadeck-domain` | IDs, task/engine/transfer types; privacy redaction helpers |
| `ariadeck-application` | Store, sync, commands, ports, derived views |
| `ariadeck-rpc` | WS transport, auth, typed adapter; notifications = refresh hints |
| `ariadeck-engine` | Process lifecycle, profile lock, core registry |
| `ariadeck-settings` | Versioned JSON settings, migrate, atomic save |
| `ariadeck-ui` | Tokens, themes, GPUI components (pages use only this + app ports) |
| `ariadeck-i18n` | Fluent catalogs (en, zh-CN) |
| `ariadeck-telemetry` | Tracing setup |
| `ariadeck-desktop` | Bootstrap, composition root, platform |

**Not separate crates yet:** core-manager (in engine), storage/SQLite (deferred), platform (partially desktop).

### Dependency rules

1. Business logic must not depend on GPUI.
2. Pages depend on `ariadeck-ui` + application/domain only.
3. Wire models stay in `ariadeck-rpc`.
4. Secrets never in logs, settings JSON, or UI dumps.
5. RPC/FS/process work never on the GPUI render path (Tokio via desktop).

---

## 3. Design principles

1. **GUI ⊥ engine** — RPC only; no fixed path/version assumption.
2. **Capabilities over versions** — `listMethods` → gate advanced writes (empty probe open-handed; non-empty fail-closed).
3. **Incremental state** — GID-keyed patches; no full-list replace every poll.
4. **Session generations** — discard stale responses; Magnet identity migration for selection/details.
5. **Destructive ops explicit** — remove ≠ delete files; Trash only for managed-local paths.
6. **Unknown mutations** — one authoritative refresh; never auto-replay writes.
7. **Local vs remote paths** — no open-folder/Trash for remote.
8. **Virtualization first** — off-screen rows create no GPUI elements.

---

## 4. Engine & RPC

**Local managed startup (summary):** resolve exe → profile lock → session recover → loopback + ephemeral secret → spawn → WS connect → capability probe → snapshot → apply globals once per session.  
**Shutdown:** `aria2.shutdown` then kill/wait. Tray close may keep engine; Quit stops owned managed engine. Remote never stopped by AriaDeck.

**RPC hard rules:** only `ws`/`wss` path `/jsonrpc` · no HTTP auto-fallback · plain `ws` = loopback unless `ARIADECK_RPC_ALLOW_INSECURE_REMOTE` · WSS uses OS trust (no cert bypass) · credentials not in URL · one actor per socket.

Env knobs: see root `README.md` (`ARIADECK_RPC_*`).

---

## 5. Product contracts (do not reverse casually)

| ID | Rule |
| --- | --- |
| D-001 | Filename engine-owned after add; optional `out` for direct URI only |
| D-002 | Multi-line add = one task per line; mirrors need explicit mode |
| D-003 | Selection identity + query scoped; select-all = current loaded query |
| D-004 | Download proxy ≠ RPC proxy; passwords in OS keychain |
| D-005/007 | Remove keeps files; local delete → Trash; exact paths + containment |
| D-006 | Retry = new GID + option/mirror replay |
| D-008 | Output conflict: Keep both / Reject / Overwrite |
| D-009 | Known-size free-space preflight; disk-full surfaced |
| D-010 | Mutations single-flight; unknown → one refresh |
| D-011 | Remote RPC WebSocket-only, fail-closed trust |
| D-012 | Torrent/Metalink: client reads file, Base64 upload |
| D-013 | File selection preview-bound at add |
| D-014 | Sort local; queue priority = waiting order (unfiltered ascending) |
| D-015/027 | Advanced UI gated by capabilities |
| D-016/023 | Speed limits & transfer policy typed, reapplied on new session |
| D-017 | Detail projections on-demand, revision-bounded while open |
| D-018 | Seeding ≠ completed (`seeder=true`); stays in Active |
| D-019 | Post-metadata output conflicts surfaced |
| D-020 | Duplicates by URI/info-hash; open path local-only |
| D-021 | Stopped history = aria2 memory; paginated Load more |
| D-022 | Advanced add URI-only; secrets redacted |
| D-024 | Context menu = toolbar parity; no second undo stack |
| D-025 | Grouped toasts; Normal/Quiet/Silent; activity panel |
| D-026 | Profile exclusive lock; corrupt session → backup + notice |
| D-028 | Multi-profile catalog schema 2; activate → restart |
| D-029 | Core registry under `data/cores/aria2`; activate → restart |
| D-030 | Tray + close-to-tray; OS notifications; low-disk warnings |
| D-031 | Theme System/Light/Dark; debounced window geometry |
| D-032 | **SEC-001:** redact secrets in UI/clipboard/notices/Debug/diagnostics (`domain::privacy`) |
| D-033 | **PERF-001:** tray → Background poll; virtualize; details coalesce ≥500ms; 10k stress |
| D-034 | **RELEASE-001:** Windows portable (+ Inno); MIT + notices; no in-app auto-update |

**SEC inventory (boundary):** raw URIs/options may live in domain for RPC/retry; list/details/clipboard/tracker/server URIs and option secrets must be redacted or keychain-only.  
**PERF guards:** 10k stopped stress, light snapshot short-circuit, ActivityMode tray intervals, reconnect backoff.

Settings: versioned JSON (`ariadeck-settings`); separate `window.json`, `profiles.json`, `cores.json`.

---

## 6. Status

### Done

Bootstrap, domain store, typed WS RPC, sync/reconnect, virtualized workspace, add/pause/resume/retry/remove, details, local supervision, settings, themes, multi-select/batch, multiline/mirrors, Trash, proxy+keychain, torrent/metalink+file select, queue ops, rate limits, seeding, duplicates, stopped pagination, advanced add, context menu, notifications/activity, multi-profile, capabilities, core registry, tray, window prefs, i18n en/zh-CN, a11y baseline, privacy redaction, perf hardening, Windows portable/installer packaging, CI matrix (fmt/test/clippy/release-build on Windows, macOS, Linux).

### Residual (polish, not blockers for Windows ship)

| Area | Residual |
| --- | --- |
| I18N | Niche advanced-dialog / application validation English leftovers |
| ACCESS | Manual high-DPI check (125%/150% Windows) |
| SEC | Manual Windows reparse-point check; no support-bundle UI |
| PERF | Manual memory under real aria2; no APM |
| RELEASE | No prod signing cert; no macOS/Linux **packages** (CI verify covers win/mac/linux); no aria2 network installer |

### Explicitly deferred

Network aria2 package channels · SQLite history/analytics · Per-profile proxy/limit bags · Hot profile switch without restart · HTTP JSON-RPC as first-class transport · Pause/resume **scheduling** · Tags/categories · Browser/file associations · Extra locales · Remote path mapping · In-app auto-update productization  

→ Prioritized product roadmap: [`docs/roadmap.md`](roadmap.md)

---

## 7. ADRs (summary)

| ADR | Decision |
| --- | --- |
| 001 | Application owns use cases/store (no GPUI/RPC/SQLite) |
| 002 | Pin GPUI to Zed `v1.11.3` SHA `952d712dac48a4af2c54fb22c82d82a9d69b72d4` |
| 003 | Process path before networked core installer |
| 004 | Mutable state scoped to engine session generation |
| 005 | One actor per WebSocket; auth as transport decorator |
| 006 | Sync serialized & cancellation-aware |
| 007 | Typed JSON settings now; SQLite later for multi-entity |
| 008 | Download proxy ≠ RPC; credentials in OS keychain |
| 009 | Uncertain mutations reconciled from engine |
| 010 | Remote RPC WebSocket-only; fail closed |
| 011 | Fluent in `ariadeck-i18n`; UI maps error codes |

---

## 8. Developer map

```sh
cargo run -p ariadeck-desktop
cargo fmt --all --check && cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

| Concern | Start |
| --- | --- |
| Task / seeding | `crates/ariadeck-domain/src/task.rs` |
| Store / selection | `application/src/store.rs`, `view.rs` |
| Sync / generations | `application/src/sync.rs` |
| Commands | `application/src/commands.rs`, `ports.rs` |
| Wire / multicall | `crates/ariadeck-rpc/` |
| Process / cores | `crates/ariadeck-engine/` |
| Settings migrate | `crates/ariadeck-settings/` |
| Workspace / dialogs | `apps/ariadeck-desktop/src/workspace.rs` |
| UI shell | `crates/ariadeck-ui/src/` |
| i18n | `crates/ariadeck-i18n/` · `docs/i18n.md` |

### Invariants

- Session-bind every mutating command.
- Prefer engine refresh over optimistic multi-step writes.
- Secrets: keychain/adapter only; redact projections (`privacy`).
- FS actions: managed-local + path containment.
- Capability preflight before raw method-missing errors.
- Virtualization + Background poll when tray-hidden.
- Profile/core activate remain **restart-bound** until hot rebind exists.

### Agent rules

1. Code wins; update this doc when behavior changes.  
2. Check aria2 manual / comparable clients before UX contract changes.  
3. Provider-neutral contracts in application; option strings at RPC boundary.  
4. Engine-touching features need tests or a recorded live check.  
5. Packaging stays in `docs/release.md` unless asked to expand.

---

## 9. Risks

| Risk | Mitigation |
| --- | --- |
| GPUI churn | Pinned Zed rev; UI confined |
| Large queues | Virtualization, patches, paged stopped |
| aria2 build variance | `listMethods` capabilities |
| Session corruption | Atomic writes, backup, ownership lock |
| Remote path confusion | Capability flags; no local open/Trash |

---

## 10. History

`design.md`, `implementation-progress.md`, and `post-mvp-progress.md` were consolidated here on 2026-07-22. Long verification tables live in git history.  
2026-07-22 (later): compressed this file; product gap plan moved to `docs/roadmap.md`.
