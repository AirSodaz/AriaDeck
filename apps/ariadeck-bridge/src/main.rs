//! `ariadeck-bridge` — Chrome/Edge native messaging host for the browser bridge
//! (D-045, contract in `docs/browser-bridge.md`).
//!
//! Launched by the browser, never by AriaDeck. It reads length-prefixed JSON on
//! stdin, validates each offered download against the contract whitelist, and
//! forwards it to the already-running primary instance over the existing local
//! socket. It is write-only: nothing about AriaDeck's state travels back, only a
//! bounded ack.
//!
//! Two things this binary must never grow: a network listener, and a dependency
//! on the UI stack.

mod protocol;

use std::{
    io::{self, Write},
    path::Path,
    process::ExitCode,
};

use ariadeck_ipc::{BridgeDownload, LaunchRequest, default_data_dir, forward_to_primary};
use tokio::runtime::Builder;

use crate::protocol::{
    ErrorCode, Frame, Reply, cookies_allowed, decode_items, read_frame, write_reply,
};

fn main() -> ExitCode {
    // Single-threaded is sufficient: the host handles one browser port and each
    // forward is a bounded, short-lived socket write.
    let runtime = match Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(_) => {
            report("failed to start the bridge runtime");
            return ExitCode::FAILURE;
        }
    };

    let data_dir = default_data_dir();
    match serve(&runtime, &data_dir) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => {
            // Framing is broken or the port died mid-message; there is nothing
            // useful to say to a stream we can no longer trust.
            report("bridge port closed unexpectedly");
            ExitCode::FAILURE
        }
    }
}

/// Serve the browser port until it closes.
fn serve(runtime: &tokio::runtime::Runtime, data_dir: &Path) -> io::Result<()> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();

    loop {
        match read_frame(&mut input)? {
            Frame::Eof => return Ok(()),
            Frame::Oversize => {
                // The body was deliberately not consumed, so the stream is out
                // of sync — answer once and let the browser reopen the port.
                write_reply(&mut output, &Reply::failed(ErrorCode::TooLarge))?;
                return Ok(());
            }
            Frame::Message(body) => {
                // Resolved per message so toggling the setting takes effect on
                // the next download rather than the next browser restart.
                let reply = match decode_items(&body, cookies_allowed(data_dir)) {
                    Ok(downloads) => forward(runtime, data_dir, downloads),
                    Err(code) => Reply::failed(code),
                };
                write_reply(&mut output, &reply)?;
            }
        }
    }
}

/// Hand the batch to the primary instance. No spooling, no retry on another
/// transport: if the socket is not there, the answer is `not_running` (D-045 §6).
fn forward(
    runtime: &tokio::runtime::Runtime,
    data_dir: &Path,
    downloads: Vec<BridgeDownload>,
) -> Reply {
    let count = downloads.len();
    let request = LaunchRequest {
        metadata_paths: Vec::new(),
        magnet_uris: Vec::new(),
        downloads,
    };
    match forward_to_primary(runtime, data_dir, &request) {
        Ok(()) => Reply::accepted(count),
        Err(error) => Reply::failed(ErrorCode::from_forward_error(&error)),
    }
}

/// Diagnostics go to stderr, which the browser surfaces in its own log. Payload
/// fields never appear here — a cookie must not leak into browser logs (D-045 §5.3).
fn report(message: &str) {
    let _ = writeln!(io::stderr(), "ariadeck-bridge: {message}");
}
