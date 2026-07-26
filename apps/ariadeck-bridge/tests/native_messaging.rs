//! End-to-end tests that drive the real host binary over stdio, the same way a
//! browser does, and assert what reaches the primary instance (D-045 §3/§4).

use std::{
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

use ariadeck_ipc::{InstanceRole, LaunchRequest, coordinate_instance};
use secrecy::ExposeSecret;
use tokio::runtime::Runtime;

const BRIDGE: &str = env!("CARGO_BIN_EXE_ariadeck-bridge");

fn frame(body: &str) -> Vec<u8> {
    let mut bytes = u32::try_from(body.len())
        .expect("test payload fits a length prefix")
        .to_le_bytes()
        .to_vec();
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

fn spawn_host(data_dir: &Path) -> Child {
    Command::new(BRIDGE)
        .env("ARIADECK_DATA_DIR", data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("host binary starts")
}

/// Send one message, close the port, and return the host's replies as JSON text.
fn exchange(data_dir: &Path, message: &str) -> String {
    let mut host = spawn_host(data_dir);
    host.stdin
        .take()
        .expect("stdin is piped")
        .write_all(&frame(message))
        .expect("message writes");
    let mut raw = Vec::new();
    host.stdout
        .take()
        .expect("stdout is piped")
        .read_to_end(&mut raw)
        .expect("reply reads");
    let _ = host.wait();

    assert!(raw.len() > 4, "reply must be length-prefixed");
    let length = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
    assert_eq!(length, raw.len() - 4, "reply length prefix must match body");
    String::from_utf8(raw[4..].to_vec()).expect("reply is UTF-8")
}

fn primary(runtime: &Runtime, data_dir: &Path) -> std::sync::mpsc::Receiver<LaunchRequest> {
    match coordinate_instance(runtime, data_dir, &LaunchRequest::default())
        .expect("primary instance starts")
    {
        InstanceRole::Primary(receiver) => receiver,
        InstanceRole::Forwarded => panic!("a fresh data dir must become primary"),
    }
}

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("test runtime starts")
}

#[test]
fn forwards_a_batch_to_the_primary_instance() {
    let dir = tempfile::tempdir().expect("tempdir");
    let runtime = runtime();
    let receiver = primary(&runtime, dir.path());

    let reply = exchange(
        dir.path(),
        r#"{"version":1,"items":[
            {"url":"https://example.test/a.bin","referer":"https://example.test/p",
             "user_agent":"UA","filename":"a.bin","file_size":7,
             "mime":"application/octet-stream"},
            {"url":"https://example.test/b.bin"}
        ]}"#,
    );
    assert_eq!(reply, r#"{"ok":true,"accepted":2}"#);

    let request = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("primary receives the batch");
    assert_eq!(request.downloads.len(), 2);
    assert!(request.metadata_paths.is_empty());
    assert!(request.magnet_uris.is_empty());
    assert_eq!(request.downloads[0].url, "https://example.test/a.bin");
    assert_eq!(
        request.downloads[0].referer.as_deref(),
        Some("https://example.test/p")
    );
    assert_eq!(request.downloads[0].filename.as_deref(), Some("a.bin"));
    assert_eq!(request.downloads[1].referer, None);
}

/// §5.1: the cookie is dropped by the host, so it never crosses the socket until
/// the user has opted in. Same message, two settings, two outcomes.
#[test]
fn cookie_crosses_the_socket_only_after_an_explicit_opt_in() {
    let dir = tempfile::tempdir().expect("tempdir");
    let runtime = runtime();
    let receiver = primary(&runtime, dir.path());
    let message = r#"{"version":1,"items":[
        {"url":"https://example.test/gated.bin","cookie":"session=e2e-secret"}]}"#;

    // No settings file at all: fail closed.
    assert_eq!(exchange(dir.path(), message), r#"{"ok":true,"accepted":1}"#);
    let denied = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("primary receives the request");
    assert!(
        denied.downloads[0].cookie.is_none(),
        "cookie must not be forwarded without an opt-in"
    );

    std::fs::write(
        dir.path().join("settings.json"),
        br#"{"browser_bridge":{"allow_cookies":true}}"#,
    )
    .expect("settings write");

    assert_eq!(exchange(dir.path(), message), r#"{"ok":true,"accepted":1}"#);
    let allowed = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("primary receives the request");
    assert_eq!(
        allowed.downloads[0]
            .cookie
            .as_ref()
            .map(|cookie| cookie.expose_secret().as_str()),
        Some("session=e2e-secret")
    );
}

#[test]
fn refused_payloads_report_a_code_and_reach_no_instance() {
    let dir = tempfile::tempdir().expect("tempdir");
    let runtime = runtime();
    let receiver = primary(&runtime, dir.path());

    let cases = [
        // Non-http(s) schemes, refused by the IPC whitelist.
        (
            r#"{"version":1,"items":[{"url":"file:///C:/Windows/win.ini"}]}"#,
            r#"{"ok":false,"error":"rejected"}"#,
        ),
        (
            r#"{"version":1,"items":[{"url":"javascript:alert(1)"}]}"#,
            r#"{"ok":false,"error":"rejected"}"#,
        ),
        // Header injection.
        (
            r#"{"version":1,"items":[{"url":"https://a.test/f","referer":"https://a.test/\r\nX: 1"}]}"#,
            r#"{"ok":false,"error":"rejected"}"#,
        ),
        // An output path dressed as a filename.
        (
            r#"{"version":1,"items":[{"url":"https://a.test/f","filename":"../../evil.exe"}]}"#,
            r#"{"ok":false,"error":"rejected"}"#,
        ),
        // Unknown extension protocol version.
        (
            r#"{"version":9,"items":[{"url":"https://a.test/f"}]}"#,
            r#"{"ok":false,"error":"unsupported_version"}"#,
        ),
        (r#"{"items":[]}"#, r#"{"ok":false,"error":"rejected"}"#),
    ];
    for (message, expected) in cases {
        assert_eq!(exchange(dir.path(), message), expected, "{message}");
    }

    assert!(
        receiver.try_recv().is_err(),
        "no refused payload may reach the primary instance"
    );
}

#[test]
fn oversized_message_is_refused_without_being_truncated() {
    let dir = tempfile::tempdir().expect("tempdir");
    let runtime = runtime();
    let receiver = primary(&runtime, dir.path());

    let mut host = spawn_host(dir.path());
    let mut stdin = host.stdin.take().expect("stdin is piped");
    // Claim a body far larger than the 64 KiB ceiling, then send nothing.
    let _ = stdin.write_all(&(64 * 1024 * 1024_u32).to_le_bytes());
    let _ = stdin.flush();

    let mut raw = Vec::new();
    host.stdout
        .take()
        .expect("stdout is piped")
        .read_to_end(&mut raw)
        .expect("reply reads");
    drop(stdin);
    let _ = host.wait();

    assert_eq!(&raw[4..], br#"{"ok":false,"error":"too_large"}"#);
    assert!(receiver.try_recv().is_err(), "nothing may be forwarded");
}

/// §6: with no instance listening the host reports `not_running`. Nothing is
/// spooled, so a later-started instance receives nothing.
#[test]
fn reports_not_running_without_spooling() {
    let dir = tempfile::tempdir().expect("tempdir");
    let message = r#"{"version":1,"items":[{"url":"https://example.test/orphan.bin"}]}"#;
    assert_eq!(
        exchange(dir.path(), message),
        r#"{"ok":false,"error":"not_running"}"#
    );

    let runtime = runtime();
    let receiver = primary(&runtime, dir.path());
    assert!(
        receiver.recv_timeout(Duration::from_millis(500)).is_err(),
        "a refused forward must not be replayed to a later instance"
    );
}

#[test]
fn handles_several_messages_on_one_port_and_exits_on_close() {
    let dir = tempfile::tempdir().expect("tempdir");
    let runtime = runtime();
    let receiver = primary(&runtime, dir.path());

    let mut host = spawn_host(dir.path());
    let mut stdin = host.stdin.take().expect("stdin is piped");
    for index in 0..3 {
        stdin
            .write_all(&frame(&format!(
                r#"{{"version":1,"items":[{{"url":"https://example.test/{index}.bin"}}]}}"#
            )))
            .expect("message writes");
    }
    drop(stdin);

    let mut raw = Vec::new();
    host.stdout
        .take()
        .expect("stdout is piped")
        .read_to_end(&mut raw)
        .expect("replies read");
    let status = host.wait().expect("host exits");
    assert!(status.success(), "a closed port is a clean exit");

    let mut cursor = 0;
    for _ in 0..3 {
        let length = u32::from_le_bytes([
            raw[cursor],
            raw[cursor + 1],
            raw[cursor + 2],
            raw[cursor + 3],
        ]) as usize;
        let body = &raw[cursor + 4..cursor + 4 + length];
        assert_eq!(body, br#"{"ok":true,"accepted":1}"#);
        cursor += 4 + length;
    }
    assert_eq!(cursor, raw.len(), "no trailing bytes");

    for index in 0..3 {
        let request = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("primary receives each message");
        assert_eq!(
            request.downloads[0].url,
            format!("https://example.test/{index}.bin")
        );
    }
}
