//! Local single-instance IPC shared by `ariadeck-desktop` (server) and
//! `ariadeck-bridge` (client).
//!
//! The transport is a bounded, per-data-directory local socket. It carries three
//! kinds of launch items: metadata file paths (D-037), magnet URIs (D-038), and
//! browser-bridge downloads (D-045).
//!
//! This crate must stay free of GPUI so the native messaging host can link it
//! without pulling in the UI stack.

use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

use data_encoding::HEXLOWER;
use interprocess::local_socket::{
    GenericNamespaced, ListenerOptions,
    tokio::{Listener, Stream, prelude::*},
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    runtime::Runtime,
    time,
};

/// Highest protocol version this build emits.
const PROTOCOL_VERSION: u8 = 3;
/// Lowest protocol version this build still accepts. An older forwarder left
/// over from an in-place upgrade must keep working; v4+ stays fail-closed.
const MIN_PROTOCOL_VERSION: u8 = 2;

const MAX_REQUEST_BYTES: usize = 256 * 1024;
pub const MAX_LAUNCH_ITEMS: usize = 32;
const MAX_PATH_UNITS: usize = 32_768;
const MAX_MAGNET_URI_BYTES: usize = 16 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(2);

// Per-field bounds for bridge downloads (D-045 §3). The whole request is still
// capped by MAX_REQUEST_BYTES; these keep any single field from consuming it.
const MAX_URL_BYTES: usize = 8 * 1024;
const MAX_REFERER_BYTES: usize = 4 * 1024;
const MAX_USER_AGENT_BYTES: usize = 1024;
const MAX_COOKIE_BYTES: usize = 8 * 1024;
const MAX_FILENAME_BYTES: usize = 1024;

/// Marker file placed next to the executable to enable portable mode (RELEASE-001).
pub const PORTABLE_MARKER_FILE: &str = "ariadeck.portable";

#[cfg(target_os = "windows")]
type EncodedPath = Vec<u16>;
#[cfg(not(target_os = "windows"))]
type EncodedPath = Vec<u8>;

/// A download handed over by the browser bridge (D-045).
///
/// The field set is the exhaustive whitelist from the contract: no output path,
/// no aria2 option passthrough, no HTTP auth. `filename` and `file_size` are
/// display hints for the confirmation dialog and never become `out` (D-001).
#[derive(Clone)]
pub struct BridgeDownload {
    pub url: String,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    /// Origin-scoped cookie, opt-in only. Memory for the lifetime of the add;
    /// never persisted to settings, history, logs, or diagnostics (D-032/D-035).
    pub cookie: Option<SecretString>,
    pub filename: Option<String>,
    pub file_size: Option<u64>,
}

impl std::fmt::Debug for BridgeDownload {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BridgeDownload")
            .field("url", &self.url)
            .field("referer", &self.referer)
            .field("user_agent", &self.user_agent)
            .field("cookie", &self.cookie.as_ref().map(|_| "[REDACTED]"))
            .field("filename", &self.filename)
            .field("file_size", &self.file_size)
            .finish()
    }
}

impl PartialEq for BridgeDownload {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
            && self.referer == other.referer
            && self.user_agent == other.user_agent
            && self.cookie.as_ref().map(ExposeSecret::expose_secret)
                == other.cookie.as_ref().map(ExposeSecret::expose_secret)
            && self.filename == other.filename
            && self.file_size == other.file_size
    }
}

impl Eq for BridgeDownload {}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LaunchRequest {
    pub metadata_paths: Vec<PathBuf>,
    pub magnet_uris: Vec<String>,
    pub downloads: Vec<BridgeDownload>,
}

impl LaunchRequest {
    #[must_use]
    pub fn len(&self) -> usize {
        self.metadata_paths.len() + self.magnet_uris.len() + self.downloads.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub enum InstanceRole {
    Primary(Receiver<LaunchRequest>),
    Forwarded,
}

#[derive(Debug, Deserialize, Serialize)]
struct WireRequest {
    version: u8,
    metadata_paths: Vec<EncodedPath>,
    magnet_uris: Vec<String>,
    #[serde(default)]
    downloads: Vec<WireDownload>,
}

#[derive(Debug, Deserialize, Serialize)]
struct WireDownload {
    url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    referer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cookie: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file_size: Option<u64>,
}

/// Become the primary instance, or forward `initial_request` to the one that
/// already holds the socket for `data_dir`.
pub fn coordinate_instance(
    runtime: &Runtime,
    data_dir: &Path,
    initial_request: &LaunchRequest,
) -> io::Result<InstanceRole> {
    let socket_label = socket_label(data_dir);
    let name = socket_label.as_str().to_ns_name::<GenericNamespaced>()?;
    let listener = {
        let _runtime_guard = runtime.enter();
        ListenerOptions::new().name(name).create_tokio()
    };
    match listener {
        Ok(listener) => {
            let (sender, receiver) = mpsc::channel();
            runtime.spawn(serve(listener, sender));
            Ok(InstanceRole::Primary(receiver))
        }
        Err(error) if listener_name_is_occupied(&error) => {
            forward_request(runtime, &socket_label, initial_request)?;
            Ok(InstanceRole::Forwarded)
        }
        Err(error) => Err(error),
    }
}

/// Forward a request to the primary instance for `data_dir` without attempting
/// to become primary. Used by the native messaging host, which must never take
/// ownership of the socket.
pub fn forward_to_primary(
    runtime: &Runtime,
    data_dir: &Path,
    request: &LaunchRequest,
) -> io::Result<()> {
    forward_request(runtime, &socket_label(data_dir), request)
}

async fn serve(listener: Listener, sender: Sender<LaunchRequest>) {
    loop {
        match listener.accept().await {
            Ok(connection) => {
                let sender = sender.clone();
                tokio::spawn(async move {
                    let result =
                        match time::timeout(IO_TIMEOUT, handle_connection(connection, &sender))
                            .await
                        {
                            Ok(result) => result,
                            Err(_) => Err(io::Error::new(
                                io::ErrorKind::TimedOut,
                                "local launch request timed out",
                            )),
                        };
                    if let Err(error) = result {
                        tracing::warn!(%error, "rejected local launch request");
                    }
                });
            }
            Err(error) => tracing::warn!(%error, "failed to accept local launch request"),
        }
    }
}

async fn handle_connection(connection: Stream, sender: &Sender<LaunchRequest>) -> io::Result<()> {
    let mut reader = BufReader::new(connection);
    let mut buffer = Vec::new();
    let bytes_read = {
        let mut limited = (&mut reader).take((MAX_REQUEST_BYTES + 1) as u64);
        limited.read_until(b'\n', &mut buffer).await?
    };
    if bytes_read == 0 || bytes_read > MAX_REQUEST_BYTES || buffer.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "local launch request is empty, oversized, or unterminated",
        ));
    }
    buffer.pop();
    let request: WireRequest = serde_json::from_slice(&buffer)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let request = decode_request(request)?;
    sender
        .send(request)
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "application is shutting down"))?;
    reader.get_mut().write_all(b"ok\n").await?;
    reader.get_mut().flush().await
}

fn forward_request(
    runtime: &Runtime,
    socket_label: &str,
    request: &LaunchRequest,
) -> io::Result<()> {
    let request = encode_request(request)?;
    let mut payload = serde_json::to_vec(&request).map_err(io::Error::other)?;
    payload.push(b'\n');
    if payload.len() > MAX_REQUEST_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "local launch request is too large",
        ));
    }

    runtime.block_on(async {
        time::timeout(IO_TIMEOUT, async {
            let mut last_error = None;
            for _ in 0..10 {
                let name = socket_label.to_ns_name::<GenericNamespaced>()?;
                match Stream::connect(name).await {
                    Ok(connection) => {
                        let mut reader = BufReader::new(connection);
                        reader.get_mut().write_all(&payload).await?;
                        reader.get_mut().flush().await?;
                        let mut acknowledgement = Vec::new();
                        (&mut reader)
                            .take(17)
                            .read_until(b'\n', &mut acknowledgement)
                            .await?;
                        return if acknowledgement == b"ok\n" {
                            Ok(())
                        } else {
                            Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "primary instance returned an invalid acknowledgement",
                            ))
                        };
                    }
                    Err(error) => {
                        last_error = Some(error);
                        time::sleep(Duration::from_millis(25)).await;
                    }
                }
            }
            Err(last_error.unwrap_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotConnected,
                    "primary instance is unavailable",
                )
            }))
        })
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "primary instance did not acknowledge the request",
            )
        })?
    })
}

fn listener_name_is_occupied(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::AddrInUse
        || (cfg!(target_os = "windows") && error.kind() == io::ErrorKind::PermissionDenied)
}

fn encode_request(request: &LaunchRequest) -> io::Result<WireRequest> {
    if request.len() > MAX_LAUNCH_ITEMS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "too many launch items",
        ));
    }
    let metadata_paths = request
        .metadata_paths
        .iter()
        .map(|path| {
            let encoded = encode_path(path);
            if encoded.len() > MAX_PATH_UNITS {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "metadata path is too long",
                ));
            }
            Ok(encoded)
        })
        .collect::<io::Result<Vec<_>>>()?;
    validate_magnet_uris(&request.magnet_uris, io::ErrorKind::InvalidInput)?;
    let downloads = request
        .downloads
        .iter()
        .map(|download| {
            validate_download(download, io::ErrorKind::InvalidInput)?;
            Ok(WireDownload {
                url: download.url.clone(),
                referer: download.referer.clone(),
                user_agent: download.user_agent.clone(),
                cookie: download
                    .cookie
                    .as_ref()
                    .map(|cookie| cookie.expose_secret().to_owned()),
                filename: download.filename.clone(),
                file_size: download.file_size,
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(WireRequest {
        version: PROTOCOL_VERSION,
        metadata_paths,
        magnet_uris: request.magnet_uris.clone(),
        downloads,
    })
}

fn decode_request(request: WireRequest) -> io::Result<LaunchRequest> {
    let supported = (MIN_PROTOCOL_VERSION..=PROTOCOL_VERSION).contains(&request.version);
    if !supported
        || request.metadata_paths.len() + request.magnet_uris.len() + request.downloads.len()
            > MAX_LAUNCH_ITEMS
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported local launch request",
        ));
    }
    // v2 forwarders never send downloads; serde's default already produced an
    // empty vector, so nothing extra is required for the older version.
    validate_magnet_uris(&request.magnet_uris, io::ErrorKind::InvalidData)?;
    let metadata_paths = request
        .metadata_paths
        .into_iter()
        .map(|path| {
            if path.len() > MAX_PATH_UNITS {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "metadata path is too long",
                ));
            }
            Ok(decode_path(path))
        })
        .collect::<io::Result<Vec<_>>>()?;
    let downloads = request
        .downloads
        .into_iter()
        .map(|download| {
            let download = BridgeDownload {
                url: download.url,
                referer: download.referer,
                user_agent: download.user_agent,
                cookie: download.cookie.map(SecretString::new),
                filename: download.filename,
                file_size: download.file_size,
            };
            validate_download(&download, io::ErrorKind::InvalidData)?;
            Ok(download)
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(LaunchRequest {
        metadata_paths,
        magnet_uris: request.magnet_uris,
        downloads,
    })
}

fn validate_magnet_uris(uris: &[String], error_kind: io::ErrorKind) -> io::Result<()> {
    if uris.iter().any(|uri| !is_supported_magnet_uri(uri)) {
        return Err(io::Error::new(
            error_kind,
            "invalid or oversized magnet URI",
        ));
    }
    Ok(())
}

#[must_use]
pub fn is_supported_magnet_uri(uri: &str) -> bool {
    uri.len() <= MAX_MAGNET_URI_BYTES && ariadeck_domain::magnet_info_hash(uri).is_some()
}

/// Enforce the D-045 payload rules at the IPC boundary.
///
/// The application layer validates again before the options reach aria2
/// (`AddDownloadAdvancedOptions::validate`); this is the outer, fail-closed
/// gate so a malformed payload never reaches the UI at all.
fn validate_download(download: &BridgeDownload, error_kind: io::ErrorKind) -> io::Result<()> {
    let invalid = |message: &str| io::Error::new(error_kind, message.to_owned());

    if download.url.len() > MAX_URL_BYTES {
        return Err(invalid("bridge download URL is too long"));
    }
    let url = url::Url::parse(&download.url)
        .map_err(|_| invalid("bridge download URL is not a valid absolute URL"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(invalid("bridge download URL must use http or https"));
    }

    check_header_field(
        download.referer.as_deref(),
        MAX_REFERER_BYTES,
        "referer",
        &invalid,
    )?;
    check_header_field(
        download.user_agent.as_deref(),
        MAX_USER_AGENT_BYTES,
        "user agent",
        &invalid,
    )?;
    check_header_field(
        download
            .cookie
            .as_ref()
            .map(|cookie| cookie.expose_secret().as_str()),
        MAX_COOKIE_BYTES,
        "cookie",
        &invalid,
    )?;

    if let Some(filename) = &download.filename {
        check_header_field(
            Some(filename.as_str()),
            MAX_FILENAME_BYTES,
            "filename",
            &invalid,
        )?;
        // Display hint only, but keep it incapable of expressing a path so it
        // can never be mistaken for an output location (D-001).
        if filename.contains(['/', '\\']) || matches!(filename.as_str(), "." | "..") {
            return Err(invalid("bridge download filename must not contain a path"));
        }
    }
    Ok(())
}

fn check_header_field(
    value: Option<&str>,
    max_bytes: usize,
    label: &str,
    invalid: &impl Fn(&str) -> io::Error,
) -> io::Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.trim().is_empty() {
        return Err(invalid(&format!(
            "bridge download {label} must not be empty when present"
        )));
    }
    if value.len() > max_bytes {
        return Err(invalid(&format!("bridge download {label} is too long")));
    }
    // Rejecting all C0 controls and DEL covers CRLF header injection plus NUL
    // and friends in one check.
    if value.chars().any(|ch| ch.is_control()) {
        return Err(invalid(&format!(
            "bridge download {label} must not contain control characters"
        )));
    }
    Ok(())
}

fn socket_label(data_dir: &Path) -> String {
    let digest = Sha256::digest(data_dir.as_os_str().as_encoded_bytes());
    format!("ariadeck-{}", HEXLOWER.encode(&digest[..16]))
}

/// Application data directory used for settings, profiles, cores, and window
/// geometry — and, since the socket label is derived from it, the address of the
/// primary instance.
///
/// This lives here rather than in the desktop crate so the native messaging host
/// resolves the exact same directory; a divergence would silently address a
/// different socket and every forward would report "not running".
///
/// Resolution order:
/// 1. `ARIADECK_DATA_DIR` when set
/// 2. `<exe_dir>/data` when `<exe_dir>/ariadeck.portable` exists
/// 3. `%LOCALAPPDATA%/AriaDeck` (Windows)
/// 4. `$XDG_DATA_HOME/ariadeck` or `~/.local/share/ariadeck`
/// 5. `./.ariadeck` fallback
#[must_use]
pub fn default_data_dir() -> PathBuf {
    resolve_data_dir(|key| std::env::var_os(key), current_exe_dir().as_deref())
}

fn current_exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

/// Testable data-dir resolver (RELEASE-001).
#[must_use]
pub fn resolve_data_dir(
    mut env_var: impl FnMut(&str) -> Option<OsString>,
    exe_dir: Option<&Path>,
) -> PathBuf {
    if let Some(path) = env_var("ARIADECK_DATA_DIR") {
        return PathBuf::from(path);
    }
    if let Some(exe_dir) = exe_dir {
        let marker = exe_dir.join(PORTABLE_MARKER_FILE);
        if marker.is_file() {
            return exe_dir.join("data");
        }
    }
    if let Some(path) = env_var("LOCALAPPDATA") {
        return PathBuf::from(path).join("AriaDeck");
    }
    if let Some(path) = env_var("XDG_DATA_HOME") {
        return PathBuf::from(path).join("ariadeck");
    }
    if let Some(path) = env_var("HOME") {
        return PathBuf::from(path).join(".local/share/ariadeck");
    }
    PathBuf::from(".ariadeck")
}

#[cfg(target_os = "windows")]
fn encode_path(path: &Path) -> EncodedPath {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().collect()
}

#[cfg(target_os = "windows")]
fn decode_path(path: EncodedPath) -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    PathBuf::from(OsString::from_wide(&path))
}

#[cfg(not(target_os = "windows"))]
fn encode_path(path: &Path) -> EncodedPath {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(target_os = "windows"))]
fn decode_path(path: EncodedPath) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(OsString::from_vec(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SOCKET_NONCE: AtomicU64 = AtomicU64::new(0);

    fn sample_download() -> BridgeDownload {
        BridgeDownload {
            url: "https://example.test/file.bin".into(),
            referer: Some("https://example.test/page".into()),
            user_agent: Some("Mozilla/5.0".into()),
            cookie: Some(SecretString::new("session=secret".into())),
            filename: Some("file.bin".into()),
            file_size: Some(1024),
        }
    }

    #[test]
    fn wire_request_round_trips_paths_and_magnets_without_text_conversion() {
        let request = LaunchRequest {
            metadata_paths: vec![PathBuf::from("D:/Downloads/示例 file.torrent")],
            magnet_uris: vec![
                "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example".into(),
            ],
            downloads: Vec::new(),
        };
        let decoded = decode_request(encode_request(&request).expect("request encodes"))
            .expect("request decodes");
        assert_eq!(decoded, request);
    }

    #[test]
    fn wire_request_round_trips_bridge_downloads_including_cookie() {
        let request = LaunchRequest {
            metadata_paths: Vec::new(),
            magnet_uris: Vec::new(),
            downloads: vec![
                sample_download(),
                BridgeDownload {
                    url: "http://example.test/plain".into(),
                    referer: None,
                    user_agent: None,
                    cookie: None,
                    filename: None,
                    file_size: None,
                },
            ],
        };
        let decoded = decode_request(encode_request(&request).expect("request encodes"))
            .expect("request decodes");
        assert_eq!(decoded, request);
        assert_eq!(
            decoded.downloads[0]
                .cookie
                .as_ref()
                .map(|cookie| cookie.expose_secret().as_str()),
            Some("session=secret")
        );
    }

    #[test]
    fn wire_request_round_trips_non_ascii_download_fields() {
        let request = LaunchRequest {
            metadata_paths: Vec::new(),
            magnet_uris: Vec::new(),
            downloads: vec![BridgeDownload {
                url: "https://example.test/下载/文件%20one.bin?q=示例".into(),
                referer: Some("https://example.test/页面".into()),
                user_agent: Some("Mozilla/5.0 (Ünïcodé)".into()),
                cookie: Some(SecretString::new("会话=秘密".into())),
                filename: Some("示例 file.bin".into()),
                file_size: Some(u64::MAX),
            }],
        };
        let decoded = decode_request(encode_request(&request).expect("request encodes"))
            .expect("request decodes");
        assert_eq!(decoded, request);
    }

    #[test]
    fn bridge_download_debug_redacts_the_cookie() {
        let rendered = format!("{:?}", sample_download());
        assert!(rendered.contains("[REDACTED]"), "{rendered}");
        assert!(!rendered.contains("session=secret"), "{rendered}");
    }

    #[test]
    fn protocol_v2_is_accepted_and_v4_is_rejected() {
        let v2 = WireRequest {
            version: 2,
            metadata_paths: Vec::new(),
            magnet_uris: Vec::new(),
            downloads: Vec::new(),
        };
        assert_eq!(
            decode_request(v2).expect("v2 stays supported"),
            LaunchRequest::default()
        );

        let v4 = WireRequest {
            version: PROTOCOL_VERSION + 1,
            metadata_paths: Vec::new(),
            magnet_uris: Vec::new(),
            downloads: Vec::new(),
        };
        assert_eq!(
            decode_request(v4)
                .expect_err("future protocol must fail")
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn v2_payload_without_downloads_field_still_deserializes() {
        let raw = br#"{"version":2,"metadata_paths":[],"magnet_uris":[]}"#;
        let request: WireRequest = serde_json::from_slice(raw).expect("v2 payload parses");
        assert!(request.downloads.is_empty());
        assert!(decode_request(request).is_ok());
    }

    #[test]
    fn bridge_downloads_reject_unsupported_schemes() {
        for url in [
            "file:///C:/Windows/System32/config",
            "data:text/html,<script>",
            "javascript:alert(1)",
            "ftp://example.test/file.bin",
            "blob:https://example.test/abc",
            "not-a-url",
        ] {
            let request = LaunchRequest {
                metadata_paths: Vec::new(),
                magnet_uris: Vec::new(),
                downloads: vec![BridgeDownload {
                    url: url.into(),
                    referer: None,
                    user_agent: None,
                    cookie: None,
                    filename: None,
                    file_size: None,
                }],
            };
            assert_eq!(
                encode_request(&request)
                    .expect_err("scheme must be rejected: {url}")
                    .kind(),
                io::ErrorKind::InvalidInput,
                "{url} must be rejected"
            );
        }
    }

    #[test]
    fn bridge_downloads_reject_header_injection_and_path_filenames() {
        let base = sample_download();

        let crlf_referer = BridgeDownload {
            referer: Some("https://example.test/\r\nX-Injected: 1".into()),
            ..base.clone()
        };
        let crlf_cookie = BridgeDownload {
            cookie: Some(SecretString::new("a=b\r\nX-Injected: 1".into())),
            ..base.clone()
        };
        let path_filename = BridgeDownload {
            filename: Some("../../escape.bin".into()),
            ..base.clone()
        };
        let empty_referer = BridgeDownload {
            referer: Some("   ".into()),
            ..base.clone()
        };

        for download in [crlf_referer, crlf_cookie, path_filename, empty_referer] {
            let request = LaunchRequest {
                metadata_paths: Vec::new(),
                magnet_uris: Vec::new(),
                downloads: vec![download],
            };
            assert_eq!(
                encode_request(&request)
                    .expect_err("malformed download must fail")
                    .kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }

    /// The decode side is the actual attack surface: a hostile forwarder writes
    /// raw JSON to the socket and never goes through `encode_request`.
    #[test]
    fn raw_wire_payloads_are_validated_on_decode() {
        for raw in [
            br#"{"version":3,"metadata_paths":[],"magnet_uris":[],"downloads":[{"url":"file:///C:/secrets"}]}"#.as_slice(),
            br#"{"version":3,"metadata_paths":[],"magnet_uris":[],"downloads":[{"url":"https://a.test/f","cookie":"a=b\r\nX-Injected: 1"}]}"#,
            br#"{"version":3,"metadata_paths":[],"magnet_uris":[],"downloads":[{"url":"https://a.test/f","filename":"../escape.bin"}]}"#,
            br#"{"version":3,"metadata_paths":[],"magnet_uris":[],"downloads":[{"url":"https://a.test/f","referer":"https://a.test/\nX-Injected: 1"}]}"#,
        ] {
            let request: WireRequest = serde_json::from_slice(raw).expect("payload parses");
            assert_eq!(
                decode_request(request)
                    .expect_err("hostile payload must be refused on decode")
                    .kind(),
                io::ErrorKind::InvalidData,
                "{}",
                String::from_utf8_lossy(raw)
            );
        }
    }

    /// Options the bridge must never be able to express: unknown keys are
    /// discarded by the wire struct rather than tunnelled through to aria2.
    #[test]
    fn raw_wire_downloads_drop_unmodelled_option_keys() {
        let raw = br#"{"version":3,"metadata_paths":[],"magnet_uris":[],"downloads":[
            {"url":"https://a.test/f","dir":"C:/evil","out":"evil.exe",
             "http-user":"a","http-passwd":"b","checksum":"sha-256=00","header":"X: 1"}]}"#;
        let request: WireRequest = serde_json::from_slice(raw).expect("payload parses");
        let decoded = decode_request(request).expect("known fields still decode");
        assert_eq!(decoded.downloads.len(), 1);
        let rendered = format!("{decoded:?}");
        for leaked in ["C:/evil", "evil.exe", "sha-256=00", "X: 1"] {
            assert!(!rendered.contains(leaked), "{leaked} survived decode");
        }
    }

    #[test]
    fn bridge_downloads_reject_oversized_fields() {
        let oversized_url = BridgeDownload {
            url: format!("https://example.test/{}", "x".repeat(MAX_URL_BYTES)),
            ..sample_download()
        };
        let oversized_cookie = BridgeDownload {
            cookie: Some(SecretString::new("x".repeat(MAX_COOKIE_BYTES + 1))),
            ..sample_download()
        };
        for download in [oversized_url, oversized_cookie] {
            let request = LaunchRequest {
                metadata_paths: Vec::new(),
                magnet_uris: Vec::new(),
                downloads: vec![download],
            };
            assert!(encode_request(&request).is_err());
        }
    }

    #[test]
    fn request_bounds_reject_path_floods_and_mixed_item_floods() {
        let request = LaunchRequest {
            metadata_paths: vec![PathBuf::from("sample.torrent"); MAX_LAUNCH_ITEMS + 1],
            magnet_uris: Vec::new(),
            downloads: Vec::new(),
        };
        assert_eq!(
            encode_request(&request)
                .expect_err("path flood must fail")
                .kind(),
            io::ErrorKind::InvalidInput
        );

        // Downloads count toward the same budget as paths and magnets.
        let mixed = LaunchRequest {
            metadata_paths: vec![PathBuf::from("sample.torrent"); MAX_LAUNCH_ITEMS],
            magnet_uris: Vec::new(),
            downloads: vec![sample_download()],
        };
        assert_eq!(
            encode_request(&mixed)
                .expect_err("mixed flood must fail")
                .kind(),
            io::ErrorKind::InvalidInput
        );

        let invalid_magnet = LaunchRequest {
            metadata_paths: Vec::new(),
            magnet_uris: vec!["https://example.test/not-a-magnet".into()],
            downloads: Vec::new(),
        };
        assert_eq!(
            encode_request(&invalid_magnet)
                .expect_err("non-magnet URI must fail")
                .kind(),
            io::ErrorKind::InvalidInput
        );

        let oversized_magnet = LaunchRequest {
            metadata_paths: Vec::new(),
            magnet_uris: vec![format!(
                "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn={}",
                "x".repeat(MAX_MAGNET_URI_BYTES)
            )],
            downloads: Vec::new(),
        };
        assert_eq!(
            encode_request(&oversized_magnet)
                .expect_err("oversized magnet URI must fail")
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn secondary_instance_forwards_paths_and_receives_acknowledgement() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime starts");
        let nonce = SOCKET_NONCE.fetch_add(1, Ordering::Relaxed);
        let data_dir = std::env::temp_dir().join(format!(
            "ariadeck-instance-test-{}-{nonce}",
            std::process::id()
        ));
        let receiver = match coordinate_instance(&runtime, &data_dir, &LaunchRequest::default())
            .expect("primary starts")
        {
            InstanceRole::Primary(receiver) => receiver,
            InstanceRole::Forwarded => panic!("unique socket must become primary"),
        };
        let request = LaunchRequest {
            metadata_paths: vec![
                data_dir.join("sample file.torrent"),
                data_dir.join("示例.meta4"),
            ],
            magnet_uris: vec![
                "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example".into(),
            ],
            downloads: vec![sample_download()],
        };

        assert!(matches!(
            coordinate_instance(&runtime, &data_dir, &request).expect("secondary forwards"),
            InstanceRole::Forwarded
        ));
        let received = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("primary receives request");
        assert_eq!(received, request);
    }

    #[test]
    fn forward_to_primary_reports_when_no_instance_is_listening() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime starts");
        let nonce = SOCKET_NONCE.fetch_add(1, Ordering::Relaxed);
        let data_dir = std::env::temp_dir().join(format!(
            "ariadeck-absent-test-{}-{nonce}",
            std::process::id()
        ));
        let request = LaunchRequest {
            metadata_paths: Vec::new(),
            magnet_uris: Vec::new(),
            downloads: vec![sample_download()],
        };
        assert!(
            forward_to_primary(&runtime, &data_dir, &request).is_err(),
            "forwarding without a primary must fail rather than spool"
        );
    }

    #[test]
    fn resolve_data_dir_prefers_explicit_env_over_portable_and_os() {
        let root = tempfile::tempdir().expect("temp");
        let explicit = root.path().join("custom-data");
        let exe_dir = root.path().join("app");
        std::fs::create_dir_all(&exe_dir).expect("exe dir");
        std::fs::write(exe_dir.join(PORTABLE_MARKER_FILE), b"").expect("marker");
        let local_app = root.path().join("LocalAppData");

        let resolved = resolve_data_dir(
            |key| match key {
                "ARIADECK_DATA_DIR" => Some(explicit.as_os_str().to_owned()),
                "LOCALAPPDATA" => Some(local_app.as_os_str().to_owned()),
                _ => None,
            },
            Some(exe_dir.as_path()),
        );
        assert_eq!(resolved, explicit);
    }

    #[test]
    fn resolve_data_dir_uses_portable_data_when_marker_present() {
        let root = tempfile::tempdir().expect("temp");
        let exe_dir = root.path().join("portable-app");
        std::fs::create_dir_all(&exe_dir).expect("exe dir");
        std::fs::write(exe_dir.join(PORTABLE_MARKER_FILE), b"").expect("marker");
        let local_app = root.path().join("LocalAppData");

        let resolved = resolve_data_dir(
            |key| match key {
                "LOCALAPPDATA" => Some(local_app.as_os_str().to_owned()),
                _ => None,
            },
            Some(exe_dir.as_path()),
        );
        assert_eq!(resolved, exe_dir.join("data"));
    }

    #[test]
    fn resolve_data_dir_falls_back_to_localappdata_without_marker() {
        let root = tempfile::tempdir().expect("temp");
        let exe_dir = root.path().join("installed-app");
        std::fs::create_dir_all(&exe_dir).expect("exe dir");
        let local_app = root.path().join("LocalAppData");

        let resolved = resolve_data_dir(
            |key| match key {
                "LOCALAPPDATA" => Some(local_app.as_os_str().to_owned()),
                _ => None,
            },
            Some(exe_dir.as_path()),
        );
        assert_eq!(resolved, local_app.join("AriaDeck"));
    }

    /// The host and the desktop must land on the same socket for the same data
    /// directory; a divergence would look like "AriaDeck is not running".
    #[test]
    fn socket_label_is_stable_and_data_dir_scoped() {
        let first = Path::new("/tmp/ariadeck-a");
        let second = Path::new("/tmp/ariadeck-b");
        assert_eq!(socket_label(first), socket_label(first));
        assert_ne!(socket_label(first), socket_label(second));
        assert_eq!(socket_label(first).len(), "ariadeck-".len() + 32);
    }
}
