# AriaDeck

AriaDeck is a native Rust desktop client for aria2. It uses GPUI for rendering
and communicates with an independent aria2 process through JSON-RPC.

The project is under active development. See
[`docs/implementation-progress.md`](docs/implementation-progress.md) for the
current milestone, completed checks, and known gaps.

## Development

Requirements:

- Rust 1.96.0 (installed automatically through `rust-toolchain.toml`)
- A supported Windows, macOS, or Linux development environment for GPUI

Run the desktop shell:

```sh
cargo run -p ariadeck-desktop
```

Run the current verification suite:

```sh
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Architecture

AriaDeck keeps domain and application behavior independent from GPUI, aria2
wire models, persistence, and process management. The desktop crate is the
composition root; application pages consume only AriaDeck-owned UI components.

The source architecture is defined in [`docs/design.md`](docs/design.md).
