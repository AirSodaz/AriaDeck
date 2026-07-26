# ariadeck-bridge

Chrome/Edge **native messaging host** for the AriaDeck browser bridge.
Normative spec: [`docs/browser-bridge.md`](../../docs/browser-bridge.md) (contract **D-045**).

The browser launches this executable; AriaDeck never does. It reads
length-prefixed JSON on stdin, validates each offered download against the
contract whitelist, and forwards it to the already-running primary AriaDeck
instance over the existing per-data-directory local socket. Nothing about
AriaDeck's state travels back — only a bounded ack.

Two invariants, both asserted rather than assumed:

- **No network listener.** The only transport is stdio in and the local socket out.
- **No GPUI.** CI fails the build if a dependency edge reaches the UI stack.

## Registering the host

The host registers itself, so the recorded path is always the real install path
and a moved portable copy can re-register without a reinstall:

```text
ariadeck-bridge --register --extension-id <32-char ID>
ariadeck-bridge --unregister
```

`--register` writes `com.ariadeck.bridge.json` next to the executable and points
both browsers at it:

```text
HKCU\Software\Google\Chrome\NativeMessagingHosts\com.ariadeck.bridge   (default) = <manifest path>
HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.ariadeck.bridge  (default) = <manifest path>
```

Per-user (HKCU), no admin rights. `--unregister` removes both keys and the
manifest, touching nothing else under the shared `NativeMessagingHosts` key.

**A pinned extension ID is mandatory.** Without one (`--extension-id`, or
`ARIADECK_EXTENSION_ID` in the environment or at build time) `--register` refuses
and writes nothing: a manifest with an empty `allowed_origins` would let any
extension launch the host, which is the trust boundary the whole design rests on.
The ID must be exactly 32 characters in `a`–`p`, so a typo fails loudly instead of
pinning to an extension nobody controls.

The installer offers this as an opt-in task, defaulted off, matching D-037/D-038 —
and only when the build was given an extension ID (`ARIADECK_EXTENSION_ID` when
running `scripts/package-windows-installer.ps1`). Uninstall always runs
`--unregister`, including when the host was registered by hand.

Registration is Windows-only for now; Chrome on macOS and Linux uses per-user
manifest directories instead, deferred alongside the Firefox port
([`browser-bridge.md`](../../docs/browser-bridge.md) §9).

## Manual smoke test

The host speaks the raw native messaging framing, so it can be driven from a
shell without a browser. AriaDeck must already be running against the same data
directory (`ARIADECK_DATA_DIR`, or the resolved default — see
`ariadeck_ipc::default_data_dir`).

```bash
python - <<'PY' | ./target/debug/ariadeck-bridge
import json, struct, sys
msg = json.dumps({"version": 1, "items": [
    {"url": "https://example.test/file.bin",
     "referer": "https://example.test/page",
     "filename": "file.bin"}
]}).encode()
sys.stdout.buffer.write(struct.pack("<I", len(msg)) + msg)
PY
```

A successful forward replies `{"ok":true,"accepted":1}` (also length-prefixed)
and fills the Add Download dialog in the running instance. With no instance
listening the reply is `{"ok":false,"error":"not_running"}` — the bridge never
spools to disk.

## Cookies

Cookies are dropped here unless `browser_bridge.allow_cookies` is `true` in
`<data_dir>/settings.json`, so a secret does not cross the socket at all until
the user opts in. The probe fails closed: missing file, malformed JSON, or absent
key all resolve to "not allowed".
