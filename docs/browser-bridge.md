# AriaDeck — Browser Bridge Contract (B3a)

**Status:** Implemented. B3a contract, B3b host + IPC, B3c reference extension, and the cross-cutting settings/installer/i18n work are all in tree. Outstanding: extension icons, a published extension ID, and the manual end-to-end check against a real gated download (§8).
**Last updated:** 2026-07-26
**Owns:** Auth model, wire protocol, confirm policy, and header/cookie handling for the browser extension path.
**Summary contract:** D-045 in [`project-context.md`](project-context.md) · **Priority:** B3 in [`roadmap.md`](roadmap.md)

Design-before-build per roadmap B3a. This file is the normative spec; B3b (host + IPC) and B3c (reference extension) implement it. Freeze changes here before changing code.

---

## 1. Goals and non-goals

**Goal:** let a browser hand a download to a running AriaDeck with the credentials the site actually requires, without opening a network port and without becoming a silent remote-control channel.

| Non-goal | Why |
| --- | --- |
| Network-reachable RPC endpoint for extensions | Any local process or page could probe it; conflicts with D-011 fail-closed posture |
| Generic aria2 option passthrough from the browser | Extension would become a remote config channel; only the whitelist in §5 is accepted |
| Setting output paths (`dir` / `out`) from the browser | Violates D-001 (engine owns filename) and D-040 category routing |
| Reading AriaDeck state back into the extension | Bridge is **write-only, one-way**; no task list, no settings, no history |
| Firefox as a first-party target | MV3 + native messaging differ enough to fork; community may port (roadmap B3c) |

---

## 2. Transport and trust

Chosen model: **native messaging host**, no listening port.

```text
Browser extension
   │  Chrome native messaging (stdio, 4-byte LE length + UTF-8 JSON)
   ▼
ariadeck-bridge(.exe)          ← launched by the browser, not by AriaDeck
   │  existing per-data-directory local socket, protocol v3 (§4)
   ▼
AriaDeck primary instance      ← already running, or the request fails
```

**Trust boundaries**

| Boundary | Enforced by |
| --- | --- |
| Only whitelisted extensions may launch the host | `allowed_origins` in the host manifest (extension ID pinned at build time) |
| Web pages cannot reach the host | Native messaging is extension-only; the reference extension **must not** declare `externally_connectable` |
| No remote reachability | No socket binds to any network interface; the local socket is the existing `GenericNamespaced` name |
| Cross-install isolation | Socket label stays `ariadeck-<sha256(data_dir)[..16]>` (`ariadeck_ipc::socket_label`) |

**Residual, accepted:** any process running as the same OS user can already connect to the local socket (this is true today for `--open-magnet`). The bridge does not widen *reachability*, but auto-submit (§6) widens *impact*. §7 bounds that.

**Host identity**

| Field | Value |
| --- | --- |
| Host name | `com.ariadeck.bridge` (Chrome host-name grammar: `[a-z0-9._]`) |
| Windows registration | `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.ariadeck.bridge` and the `Microsoft\Edge` equivalent, both pointing at the manifest path. Written by `ariadeck-bridge --register`, removed by `--unregister`. Per-user, no admin. macOS/Linux use manifest directories instead and are deferred with the Firefox port (§9) |
| Manifest `type` | `stdio` |
| Install trigger | Explicit installer opt-in, same pattern as D-037/D-038 — never enabled silently |

The host is a **separate executable** and must not link GPUI. It starts per browser port, forwards, and exits with the port.

---

## 3. Extension → host message

One JSON object per native message. Batches use `items`.

```json
{
  "version": 1,
  "items": [
    {
      "url": "https://example.test/file.bin",
      "referer": "https://example.test/page",
      "user_agent": "Mozilla/5.0 …",
      "cookie": "session=…",
      "filename": "file.bin",
      "file_size": 1048576,
      "mime": "application/octet-stream"
    }
  ]
}
```

| Field | Required | Rule |
| --- | --- | --- |
| `version` | yes | Must be `1`. Unknown → reject whole message |
| `items` | yes | 1..=32 entries (matches `MAX_LAUNCH_ITEMS`) |
| `url` | yes | `http`/`https` only. `file:`, `data:`, `blob:`, `javascript:`, `ftp:` rejected |
| `referer` | no | Single line, no CRLF |
| `user_agent` | no | Single line, no CRLF |
| `cookie` | no | Single line, no CRLF. Accepted **only** under §5; otherwise dropped by the host before forwarding |
| `filename` | no | **Display hint only** — see §5 |
| `file_size` | no | u64, display hint only |
| `mime` | no | Display hint only |

**Bounds:** whole message ≤ 64 KiB (well under Chrome's limit and the broker's 256 KiB). Oversize → reject, do not truncate.

**Host → extension reply** is a bounded ack only, so the bridge stays one-way:

```json
{"ok": true, "accepted": 1}
{"ok": false, "error": "not_running"}
```

Error codes: `not_running`, `rejected`, `too_large`, `unsupported_version`, `timeout`. No AriaDeck state, no paths, no settings values.

---

## 4. Host → AriaDeck: local socket protocol v3

Extends the existing `WireRequest`, now in `crates/ariadeck-ipc/src/lib.rs`.

```rust
struct WireRequest {
    version: u8,                       // 2 or 3
    metadata_paths: Vec<EncodedPath>,
    magnet_uris: Vec<String>,
    #[serde(default)]
    downloads: Vec<WireDownload>,      // new in v3
}

struct WireDownload {
    url: String,
    referer: Option<String>,
    user_agent: Option<String>,
    cookie: Option<String>,            // memory only; never logged or persisted
    filename: Option<String>,          // hint
    file_size: Option<u64>,            // hint
}
```

**Version negotiation.** v2 rejected anything `!= PROTOCOL_VERSION`. v3 accepts **`2..=3`**, defaulting `downloads` to empty for v2, so an in-flight older forwarder during an upgrade still works. Future versions stay rejected (fail-closed, unchanged).

Existing bounds carry over unchanged: 256 KiB per request, 32 items total across all three vectors, 2 s IO timeout, `ok\n` ack.

**Structural resolution (done):** the old `instance.rs` was `pub(crate)` inside `ariadeck-desktop`, and the host must not depend on GPUI. The wire types and codec now live in the leaf crate `crates/ariadeck-ipc`, consumed by both `apps/ariadeck-desktop` and `apps/ariadeck-bridge`. Socket-label **and** data-dir derivation moved with them so the two sides cannot drift.

---

## 5. Header, cookie, and filename policy

Bridge items map onto the **existing** `AddDownloadAdvancedOptions` (`crates/ariadeck-application/src/commands.rs:52`). No new option modeling, and no new flattening path — `validate()` and `to_option_pairs()` are reused as-is, which is what keeps CRLF injection and secret redaction consistent with manual advanced add (D-022).

| Bridge field | Maps to | Default |
| --- | --- | --- |
| `referer` | `AddDownloadAdvancedOptions::referer` | **Sent** |
| `user_agent` | `…::user_agent` | **Sent** |
| `cookie` | `…::cookie` (`SecretString`) | **Dropped unless opted in** |
| `filename` / `file_size` / `mime` | none — confirm dialog display only | Never becomes `out` |

**Cookie rules (all mandatory):**

1. Off by default. Enabled only by an explicit AriaDeck setting **and** the extension's own host-permission grant — both, not either.
2. Scoped to the download URL's origin. The extension must not attach cookies from any other origin.
3. Memory-only for the lifetime of the add. Never written to `settings.json`, the profile env bag, `history.sqlite`, logs, or the diagnostic ZIP (D-032, D-035).
4. Carried as `SecretString`, redacted in `Debug`, flattened to `header: Cookie: …` only at the RPC adapter boundary — the path that already exists.
5. Retry (D-006) replays the stored options for the task; a bridge-added task with a cookie retains it in memory for that session only and does **not** persist it across restart. A cookie-bearing task retried after restart may fail auth; that is intended, not a bug.

**Filename:** shown in the confirm dialog so the user knows what is coming, then discarded. aria2 resolves the real name from `Content-Disposition`/URL (D-001), and category routing uses that resolved name (D-042). Setting `out` from a browser-supplied string would let a page influence the on-disk name — rejected.

**Never accepted from the bridge:** `dir`, `out`, `http-user`, `http-passwd`, `checksum`, arbitrary `header` lines, and every other aria2 option key. The whitelist in the table above is exhaustive.

---

## 6. Confirm policy

Default is **confirm**, extending D-038's "fill, don't submit" to the bridge.

| Mode | Behavior | Default |
| --- | --- | --- |
| Confirm | Forwarded items open/fill the Add Download dialog with URL, referer, size, and suggested name; user submits | **on** |
| Auto-submit | Items are added directly, using Auto category routing (D-042); a grouped toast + activity entry records each add (D-025) | off, explicit opt-in |

**Rules**

- Auto-submit is a client-wide preference (`browser_bridge` section of `settings.json`, not the per-profile engine env bag — it is UI behavior, not engine state, per D-043).
- Auto-submit **never** suppresses the notice/activity trail; a silent add is not an option.
- With cookies enabled *and* auto-submit enabled, the first add per session still requires one confirmation. That combination is the highest-impact configuration and does not get a fully silent path.
- Window hidden to tray: forwarded items follow the existing activation path used by D-037/D-038 (raise and focus for Confirm mode; stay hidden and toast for Auto-submit).
- Engine not connected: **never spool to disk.** Forwarded items wait in the primary instance's memory under the same 32-item cap as D-037/D-038 and open the dialog once the engine can accept them; they are lost on exit, like a queued magnet. `not_running` is reserved for the case the bridge can actually detect — no primary instance holding the socket.

  *Amended 2026-07-26 (B3b).* The original rule said "queue nothing, reply `not_running`" when the engine is disconnected. The bridge cannot see engine state — it is write-only by §1 — and the socket ack is written before the UI handles the request, so honoring that literally would require making the socket protocol synchronous on UI acceptance. The in-memory wait is what D-037/D-038 already do for torrents and magnets, and it keeps the disk-spooling prohibition (the part that actually matters for a cookie-bearing request) intact.

---

## 7. Threat model

| Threat | Mitigation |
| --- | --- |
| Malicious web page drives the bridge | Native messaging is unreachable from page context; extension must not expose `externally_connectable` |
| Non-whitelisted extension connects | `allowed_origins` pins the extension ID; browser enforces before launching the host |
| Compromised whitelisted extension | Blast radius bounded by the §5 whitelist: it can add http(s) downloads with headers, but cannot choose paths, set arbitrary options, read state, or exfiltrate settings |
| Local process forges socket traffic | Pre-existing exposure (D-037/D-038). Bounded by the same whitelist; Confirm mode keeps a human in the loop; auto-submit still cannot pick paths |
| Cookie exfiltration to disk | Opt-in, memory-only, `SecretString`, excluded from settings/history/logs/diagnostics (§5) |
| Header injection via `referer`/`cookie` | Existing `AddDownloadAdvancedOptions::validate()` rejects CRLF on every field |
| Oversized / malformed payload flood | 64 KiB per native message, 256 KiB per socket request, 32 items, 2 s timeout — all existing bounds |
| Downgrade to an unauthenticated path | Bridge has no fallback transport; if the socket fails it reports `not_running` and stops |

**Not mitigated (documented, accepted):** a same-user local process can reach the socket; OS-level user isolation is the boundary. Unchanged from today.

---

## 8. Acceptance criteria

**B3b (host + IPC)** — done unless noted

- [x] `crates/ariadeck-ipc` holds wire types, codec, and socket-label derivation; `ariadeck-desktop` and `ariadeck-bridge` both consume it, neither duplicates it. Data-dir resolution moved with them: the label is derived from the data dir, so a divergence there would silently address a different socket.
- [x] `ariadeck-bridge` links no GPUI (asserted in CI via `cargo tree`; its whole dependency set is `ariadeck-ipc`, `secrecy`, `serde`, `serde_json`, `tokio`)
- [x] Protocol v3 accepts v2 requests with empty `downloads`; rejects v4+
- [x] Round-trip tests for `WireDownload`, including non-ASCII and CRLF-bearing rejects
- [x] Rejection tests: non-http(s) scheme, >32 items, >64 KiB message, unknown `version`. Validation runs on **decode** as well as encode — the raw-socket path is the actual attack surface — and unmodelled option keys (`dir`, `out`, `http-user`, `checksum`, `header`) are asserted to be dropped rather than tunnelled.
- [x] Cookie absent from `Debug`, logs, `settings.json`, `history.sqlite`, and diagnostic ZIP — asserted, not assumed. Structurally, the cookie only ever lives in `AddDownloadAdvancedOptions` for the lifetime of one add; nothing in `ariadeck-history` models advanced options, and no rejection message includes a field value.
- [x] `not_running` path returns cleanly with no spooling

§6's "engine not connected" rule was amended during B3b to match the D-037/D-038 in-memory wait; see the note under §6 for why.

**B3c (reference extension)** — `apps/ariadeck-extension/`

- [x] Chrome/Edge MV3; no `externally_connectable`; minimum host permissions. Install-time permissions are `activeTab`, `contextMenus`, `nativeMessaging`, `storage` — no host access to any site. `cookies` is optional and host access is `optional_host_permissions`, requested one origin at a time.
- [x] Cookie attachment gated behind an in-extension toggle, origin-scoped. Three independent grants are all required: the extension's per-site opt-in, the browser's permission prompt for that one origin, and AriaDeck's own `allow_cookies`. `chrome.cookies.getAll({ url })` does the domain/path matching, so another site's cookies cannot ride along; an allow-list entry whose browser permission was revoked sends nothing.
- [x] Interception is opt-in per download — a context-menu item on links and media. No `downloads` permission and no `onDeterminingFilename` hook exist in v1, so blanket interception is not merely unused but unavailable.
- [ ] End-to-end: gated download (referer+cookie required) succeeds via the bridge and fails without it. **Needs a browser and a real gated URL; not automatable in CI.** The manual procedure is in [`apps/ariadeck-extension/README.md`](../apps/ariadeck-extension/README.md).

Icons are still missing (the toolbar shows the default puzzle piece) and the extension ID is unpublished, so the installer's opt-in task stays hidden until a build supplies `ARIADECK_EXTENSION_ID`. Both are listed in that README.

**Cross-cutting**

- [x] Installer opt-in checkbox, defaulted off, matching D-037/D-038 wording. The host registers *itself* (`ariadeck-bridge --register --extension-id <ID>`) rather than having the installer write the keys, so the recorded path is always the real install path and a moved portable copy can re-register without a reinstall. `--register` refuses without a pinned, well-formed extension ID (32 chars, `a`–`p`) and writes nothing — an empty `allowed_origins` would let any extension launch the host. The installer omits the task entirely unless the build was given an ID via `ARIADECK_EXTENSION_ID`.
- [x] Uninstall removes both native-messaging registry keys, plus the generated manifest. Run unconditionally, not gated on the install-time task, because the host may also have been registered by hand; `--unregister` is a no-op when nothing is registered and only touches AriaDeck's own subkey under the shared `NativeMessagingHosts` key.
- [x] All new user-facing strings in en + zh-CN (i18n parity test already enforces id sets)
- [x] `browser_bridge` settings section (`allow_cookies` / `auto_submit`), both defaulting off. Additive on disk — schema stays v1 and a pre-D-045 `settings.json` loads with the all-off default instead of tripping corrupt-document recovery. `auto_submit` travels in a settings export; `allow_cookies` deliberately does not, so importing a document never turns cookie forwarding on (same reasoning that keeps proxy credentials out of a transfer document).
- [x] Auto-submit reuses the confirm path's fill-then-submit route, so category routing (D-042), validation, and the notice/activity trail cannot drift between the two modes.

---

## 9. Open items deferred past B3

| Item | Note |
| --- | --- |
| Firefox port | Different native-messaging manifest location and MV3 semantics; community fork per roadmap |
| Blanket download interception | v1 is opt-in per download/site; taking over all browser downloads needs its own UX pass |
| Cookie persistence across restart | Deliberately unsupported (§5.5) |
| Extension-visible task state | Would make the bridge two-way; out of scope by §1 |
