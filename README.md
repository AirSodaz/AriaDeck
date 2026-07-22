# AriaDeck

<img src="apps/ariadeck-desktop/assets/icon.svg" width="80" height="80" align="right" alt="AriaDeck icon"/>

AriaDeck is a native Rust desktop client for aria2. It uses GPUI for rendering
and communicates with an independent aria2 process through JSON-RPC.

The project is under active development. See
[`docs/project-context.md`](docs/project-context.md) for architecture,
accepted decisions, current capability, and remaining work.

## Development

Requirements:

- Rust 1.96.0 (installed automatically through `rust-toolchain.toml`)
- A supported Windows, macOS, or Linux development environment for GPUI

Run the desktop shell:

```sh
cargo run -p ariadeck-desktop
```

### External aria2 RPC

Set `ARIADECK_RPC_URL` to connect to an existing aria2 instance instead of
starting a managed local process. AriaDeck accepts only the explicit aria2
WebSocket endpoint path, for example `wss://downloads.example:6800/jsonrpc`.
Plain `ws://` is restricted to loopback by default. HTTP is not used as an
automatic fallback because it does not provide aria2 server notifications.

The RPC secret must be supplied separately through `ARIADECK_RPC_SECRET`.
Credentials, query strings, and fragments are rejected in `ARIADECK_RPC_URL`.
WSS certificates are validated against the operating-system trust store and
there is no certificate-validation bypass.

Startup-only connection controls:

| Variable | Default |
| --- | --- |
| `ARIADECK_RPC_CONNECT_TIMEOUT_MS` | local `750`, external `10000` |
| `ARIADECK_RPC_REQUEST_TIMEOUT_MS` | local `5000`, external `15000` |
| `ARIADECK_RPC_RECONNECT_BASE_DELAY_MS` | `250` |
| `ARIADECK_RPC_RECONNECT_MAX_DELAY_MS` | `30000` |
| `ARIADECK_RPC_RECONNECT_RESET_AFTER_MS` | `10000` |
| `ARIADECK_RPC_RECONNECT_MAX_ATTEMPTS` | unlimited when unset |

`ARIADECK_RPC_ALLOW_INSECURE_REMOTE=true` explicitly permits remote plaintext
WebSocket for a trusted network. It does not disable WSS certificate checks.

Run the current verification suite:

```sh
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Releases (Windows portable)

AriaDeck ships a Windows x64 **portable** package and an optional Inno Setup
installer. Managed aria2 is not bundled—import a core in Settings or use
`ARIADECK_RPC_URL`.

```powershell
python scripts/gen_third_party_notices.py
powershell -ExecutionPolicy Bypass -File scripts/package-windows-portable.ps1
```

Portable mode: place `ariadeck.portable` next to the executable (the package
script does this) so settings live under `./data`. Installed builds use
`%LOCALAPPDATA%\AriaDeck` and keep that data when uninstalling.

Full packaging, signing hooks, and data-retention policy:
[`docs/release.md`](docs/release.md).

## Architecture

AriaDeck keeps domain and application behavior independent from GPUI, aria2
wire models, persistence, and process management. The desktop crate is the
composition root; application pages consume only AriaDeck-owned UI components.

Current package boundaries:

- `ariadeck-domain`: strong identifiers, engine/task state, and transfer values.
- `ariadeck-application`: incremental state, derived views, command services, and ports.
- `ariadeck-engine`: external aria2 process lifecycle, runtime isolation, and profile metadata.
- `ariadeck-rpc`: authenticated JSON-RPC transport and typed aria2 adapter.
- `ariadeck-ui`: semantic design tokens and GPUI-owned components.
- `ariadeck-telemetry`: structured diagnostics setup.
- `ariadeck-desktop`: process bootstrap and composition root.
- `ariadeck-i18n`: Fluent catalogs (en, zh-CN).
- `ariadeck-settings`: versioned settings and migrations.

Architecture and product context: [`docs/project-context.md`](docs/project-context.md).
Release packaging: [`docs/release.md`](docs/release.md).
License: [`LICENSE`](LICENSE) (MIT); third-party: [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
