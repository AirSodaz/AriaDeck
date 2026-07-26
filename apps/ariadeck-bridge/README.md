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

`com.ariadeck.bridge.template.json` is the manifest template. Substitute:

| Placeholder | Value |
| --- | --- |
| `{{BRIDGE_EXE_PATH}}` | Absolute path to `ariadeck-bridge.exe`, JSON-escaped (`C:\\Program Files\\AriaDeck\\ariadeck-bridge.exe`) |
| `{{ARIADECK_EXTENSION_ID}}` | The published extension ID, pinned at build time |

Then point the browser at the written manifest:

```text
HKCU\Software\Google\Chrome\NativeMessagingHosts\com.ariadeck.bridge  (default) = <manifest path>
HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.ariadeck.bridge  (default) = <manifest path>
```

Registration is an explicit installer opt-in, defaulted off, matching D-037/D-038.
Uninstall removes both keys.

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
