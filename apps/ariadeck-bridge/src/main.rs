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
mod register;

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

/// What this invocation is for. The browser passes its own arguments (the host
/// manifest path and the caller's origin), so anything unrecognized means "serve
/// the port" rather than an error.
enum Mode {
    Serve,
    Register { extension_id: Option<String> },
    Unregister,
    Help,
}

fn parse_mode(arguments: &[String]) -> Mode {
    let mut mode = Mode::Serve;
    let mut extension_id = None;
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--register" => mode = Mode::Register { extension_id: None },
            "--unregister" => mode = Mode::Unregister,
            "--help" | "-h" => return Mode::Help,
            "--extension-id" => extension_id = arguments.next().cloned(),
            other => {
                if let Some(value) = other.strip_prefix("--extension-id=") {
                    extension_id = Some(value.to_owned());
                }
            }
        }
    }
    match mode {
        Mode::Register { .. } => Mode::Register { extension_id },
        other => other,
    }
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match parse_mode(&arguments) {
        Mode::Help => {
            print_usage();
            return ExitCode::SUCCESS;
        }
        Mode::Register { extension_id } => return run_register(extension_id.as_deref()),
        Mode::Unregister => {
            return match register::unregister() {
                Ok(()) => {
                    register::report_unregistered();
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    report(&format!("unregister failed: {error}"));
                    ExitCode::FAILURE
                }
            };
        }
        Mode::Serve => {}
    }

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

fn run_register(extension_id: Option<&str>) -> ExitCode {
    let extension_id = match register::resolve_extension_id(extension_id) {
        Ok(id) => id,
        Err(error) => {
            report(&error.to_string());
            return ExitCode::FAILURE;
        }
    };
    match register::register(&extension_id) {
        Ok(manifest_path) => {
            register::report_registered(&manifest_path, &extension_id);
            ExitCode::SUCCESS
        }
        Err(error) => {
            report(&format!("register failed: {error}"));
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    let _ = writeln!(
        io::stdout(),
        "ariadeck-bridge — AriaDeck browser bridge (native messaging host)\n\
         \n\
         With no arguments it serves a browser port on stdin/stdout; the browser\n\
         launches it that way and passes its own arguments.\n\
         \n\
         ariadeck-bridge --register [--extension-id <ID>]\n\
         \x20   Write the host manifest next to this executable and point Chrome and\n\
         \x20   Edge at it. Requires a pinned extension ID, from --extension-id or\n\
         \x20   ARIADECK_EXTENSION_ID.\n\
         \n\
         ariadeck-bridge --unregister\n\
         \x20   Remove both registry keys and the generated manifest.\n"
    );
}

/// Diagnostics go to stderr, which the browser surfaces in its own log. Payload
/// fields never appear here — a cookie must not leak into browser logs (D-045 §5.3).
fn report(message: &str) {
    let _ = writeln!(io::stderr(), "ariadeck-bridge: {message}");
}
