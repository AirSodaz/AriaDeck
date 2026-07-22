# AriaDeck — Product Roadmap

**Last updated:** 2026-07-22  
**Purpose:** Competitive gaps → prioritized development direction. Complements [`project-context.md`](project-context.md) (architecture & contracts).

Core download-manager surface is **done**. Remaining work is distribution polish, OS integration users expect from Motrix-class apps, and selective depth features—without becoming a web UI, cloud product, or engine reimplementation.

---

## 1. Positioning

| | AriaDeck | Typical competitor |
| --- | --- | --- |
| Runtime | Native Rust + GPUI | Electron / Tauri+Web / Python Qt / pure HTML |
| Engine | External aria2 only | Bundled aria2 or fork (Motrix Next: Aria2 Next) |
| Feel | Dense, keyboard-first DM | Consumer polish + browser capture |
| Multi-engine | Profiles (local + remote RPC) | Often single local + optional remote |

**Differentiation to keep:** native footprint, remote/multi-profile RPC, capability-gated advanced ops, privacy redaction, virtualized large queues, restart-safe managed cores.

**Do not chase:** video site extractors (yt-dlp product), arbitrary cloud sync, full remote file browser, embedding a new download engine.

---

## 2. Competitor snapshot

Sources: public READMEs / feature lists of representative open-source clients (2026).

| Product | Stack | Stars (order) | Signature features |
| --- | --- | --- | --- |
| [Motrix](https://github.com/agalwood/Motrix) | Electron + Vue + aria2 | ~52k | Clean UI, BT selective, daily trackers, UPnP, tray, i18n, file associations |
| [Motrix Next](https://github.com/AnInsomniacy/motrix-next) | Tauri 2 + Vue 3 + Aria2 Next | ~9k | Browser extension API, SQLite history, schedule limits, protocol handlers, auto-update, ~20MB, ED2K via fork |
| [AriaNg](https://github.com/mayswind/AriaNg) | Static web | ~13k | Full aria2 option surface, multi-RPC, import/export settings, speed charts, drag queue, peer detail |
| [Persepolis](https://github.com/persepolisdm/persepolis) | Python Qt + aria2 | ~7k | Queues, **scheduling**, browser add-ons, video helpers |
| Classic DMs (FDM, IDM-class) | Proprietary / mixed | — | Browser intercept, categories, scheduler, shutdown-after-complete |

AriaDeck already covers much of the **task lifecycle** these apps share (add URI/torrent/metalink, batch, limits, details, tray, themes, multi-profile, notifications). Gaps are mostly **OS integration**, **history/organization**, **browser bridge**, and **shipping surface**.

---

## 3. Gap matrix

Legend: **Have** · **Partial** · **Missing** · **Won’t** (non-goal)

| Capability | AriaDeck | Motrix / Next | AriaNg | Notes for us |
| --- | --- | --- | --- | --- |
| HTTP/FTP/BT/Magnet/Metalink | Have | Have | Have | Core done |
| Selective BT files | Have | Have | Have | Preview at add; live per-file later = P2 |
| Speed limit global/task | Have | Have | Have | Done |
| Multi-RPC / profiles | Have | Partial | Have | Strength |
| Capability gating | Have | Weak | Weak | Strength |
| Virtualized large list | Have | Partial | Weak | Strength |
| Privacy redaction | Have | Weak | Weak | Strength |
| System tray + close-to-tray | Have | Have | N/A | Done |
| Themes + en/zh-CN | Have | Have (more langs) | Have | Extra locales later |
| Windows portable/installer | Have | Have | N/A | macOS/Linux CI-verified; packages = next distro (Phase E) |
| Bundled aria2 | Partial (import) | Have | N/A | Optional offline pack later; no forced network channel |
| Browser extension / intercept | Missing | Have (Next strong) | 3rd party | High user expectation |
| Protocol handlers (magnet/torrent file) | Missing | Have | N/A | High for “default DM” |
| Tags / categories / folders | Missing | Next: categories | Filters | Organization |
| Download scheduling | Missing | Next: time windows | Weak | Persepolis-class |
| Queue named groups | Partial (aria2 wait order) | Have | Partial | Productize queues |
| SQLite history beyond aria2 memory | Missing | Next: Have | N/A | D-021 still aria2-owned |
| Auto tracker list update | Missing | Have | Manual | BT quality of life |
| UPnP / port map UI | Missing | Have | Options raw | Gate on capabilities |
| Keep-awake / shutdown after done | Missing | Next | N/A | Power integration |
| In-app auto-update | Missing | Have | N/A | Deferred productization |
| Export/import settings | Missing | AriaNg | Have | Ops convenience |
| Diagnostic zip | Missing | Next | N/A | SEC-safe export |
| Remote path mapping | Won’t (near term) | Rare | N/A | Keep non-goal |
| Web UI / mobile | Won’t | N/A | Is web | Keep non-goal |
| yt-dlp / site extractors | Won’t | Rare | N/A | Out of scope |

---

## 4. Recommended phases

Priorities assume **Windows-first users** who already have (or import) aria2, then multi-platform and “daily driver” stickiness.

### Phase A — Ship & trust (now → near)

**Goal:** Safe to recommend as a daily Windows client.

| ID | Work | Why |
| --- | --- | --- |
| A1 | Prod code-signing + release checklist hardening | SmartScreen; `docs/release.md` residual |
| A2 | **Done** — dialogs, details, and stable validation/error codes localized | en/zh-CN parity; English summaries remain unknown-code fallback |
| A3 | High-DPI + Windows reparse manual QA | ACCESS/SEC residuals |
| A4 | **Done** — optional diagnostic export (redacted ZIP) | Support without leaking secrets; excludes URLs, paths, settings, credentials, and logs |
| A5 | **Done** — settings export/import (versioned JSON, no credentials) | AriaNg parity; strict validation, local keychain secrets unchanged |
| A6 | Screen reader baseline (NVDA / Narrator): interactive controls labeled, focus order correct | a11y beyond high-DPI; Windows-first |

**Exit:** Signed Windows portable + installer; no known P0 privacy/a11y holes.

### Phase B — OS “default download manager” (high impact)

**Goal:** Capture downloads the way Motrix-class apps do—without building a browser.

| ID | Work | Why |
| --- | --- | --- |
| B1 | File associations: `.torrent`, `.metalink` | Double-click → add dialog |
| B2 | Protocol handlers: `magnet:`, optional custom `ariadeck:` | System integration |
| B3a | Browser extension: define API contract (auth model, confirm policy, referer/cookie handling) | Design before build; minimize attack surface |
| B3b | Native messaging host + local RPC endpoint for browser add-on | Motrix Next differentiator |
| B3c | Reference browser extension (Chrome/Edge); community can fork for Firefox | Validate contract end-to-end |
| B4 | First-run: discover/import aria2 or guided core import | Onboarding; still no mandatory network install |
| B5 | Tray speed meter (at least Windows + optional) | Motrix-class glanceability |
| B6 | SQLite (or equivalent) **local history** of completed/failed (paths, hashes, times) | Heavy users exhaust aria2 memory fast; ADR-007 deferred storage |

**Exit:** User can set AriaDeck as magnet/torrent handler, push links from a browser add-on, and history survives aria2 restarts.

### Phase C — Organization & retention (medium)

**Goal:** Organize and categorize downloads; manage history retention. *(SQLite history baseline moved to B6.)*

| ID | Work | Why |
| --- | --- | --- |
| C1 | Tags/categories or favorite output folders | Motrix Next / FDM organization |
| C2 | Named queues + simple schedule (start/pause windows) | Persepolis / Next scheduling |
| C3 | Stale history cleanup policies | Storage hygiene |

**Exit:** Downloads are categorized; stale history cleaned up without remote FS features.

### Phase D — BT & network depth (selective)

**Goal:** Match power users where aria2 already can—UI + safe defaults only.

| ID | Work | Why |
| --- | --- | --- |
| D1 | Tracker list refresh (user URL or curated list, explicit consent) | Motrix daily trackers |
| D2 | Richer peer/tracker presentation (already partial details) | AriaNg depth |
| D3 | UPnP/NAT-PMP **if** engine exposes/supports—capability gated | Motrix feature |
| D4 | Live per-file progress after add (D-013 extension) | Selective download UX |
| D5 | Keep-awake while active; optional “action after all complete” | Power users |

**Exit:** BT sessions need fewer external tools; still no ED2K unless upstream aria2 does.

### Phase E — Multi-platform distribution

| ID | Work | Why |
| --- | --- | --- |
| E1 | macOS app bundle + notarization path (when signing ready) | Motrix matrix |
| E2 | Linux AppImage and/or deb/rpm or Flatpak | Reach |
| E3 | CI matrix for non-Windows smoke | **Done (verify)** — `ci.yml` runs fmt/test/clippy/desktop release on windows-latest, macos-latest, ubuntu-latest; portable artifact still Windows-only |
| E4 | Optional offline **aria2 core pack** (checksummed, user-initiated) | Still not silent network channel |

**Exit:** Primary artifacts documented per OS; Windows remains best-supported until E1–E2 land.

### Phase F — Optional / later

| ID | Work | Guardrail |
| --- | --- | --- |
| F1 | In-app auto-update | Explicit product decision; signing required |
| F2 | Hot profile switch without restart | Hard; needs session rebind design |
| F3 | Per-profile proxy/limit bags | After C/history model stable |
| — | **System download proxy mode** (OS/env static; no PAC) | **Done** (settings + apply path; Manual/Disabled retained) |
| F4 | HTTP JSON-RPC transport | Only if remote ops demand; WS remains default |
| F5 | More locales (zh-TW, ja, …) | After A2 string freeze |
| F6 | Bundled maintained aria2 **fork** | Only if stock aria2 blocks critical fixes—prefer stock |

---

## 5. What we will not prioritize

- Replacing aria2 with a custom engine  
- Web UI or mobile app  
- Cloud account / cross-device sync product  
- Built-in yt-dlp / site video product  
- Silent download of binaries without user action  
- Treating remote engine paths as local filesystem  
- Feature parity for parity’s sake with Electron memory cost  

---

## 6. Suggested sequence (next 2–3 milestones)

```text
M1  Ship trust     → A1–A6 (+ roadmap doc hygiene)
M2  OS hooks       → B1–B2, B4–B6  then B3a–B3c (extension)
M3  Org/cleanup    → C1–C2  (C3 if demand)
M4  BT depth / dist → D1–D2, E1 or E2 as capacity allows
```

Each milestone should land with: tests or live-check notes for engine paths, `project-context` contract updates when user-visible rules change, and release notes under `docs/release.md` when packaging changes.

---

## 7. Success metrics (lightweight)

| Signal | Target |
| --- | --- |
| Windows install friction | Signed build; portable + installer both documented |
| Time-to-first-download | First-run → add URL in under 2 minutes with imported core |
| Large queue | 10k stopped remains interactive (PERF-001) |
| Trust | No secret leakage in copy/logs/diagnostics |
| Sticky usage | Magnet/torrent open from OS; optional browser push works |

---

## 8. Doc ownership

| Doc | Owns |
| --- | --- |
| `project-context.md` | Architecture, contracts D-xxx, invariants |
| `roadmap.md` (this) | Priority & competitive direction |
| `release.md` | Packaging acceptance |
| `i18n.md` | Locale workflow |
| `README.md` | Clone/run/env surface |

When a roadmap item ships, mark it done here and fold permanent contracts into `project-context.md` (new D-IDs if needed).
