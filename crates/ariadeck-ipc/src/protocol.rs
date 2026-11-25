use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

/// Highest protocol version this build emits.
pub(crate) const PROTOCOL_VERSION: u8 = 3;
/// Lowest protocol version this build still accepts. An older forwarder left
/// over from an in-place upgrade must keep working; v4+ stays fail-closed.
const MIN_PROTOCOL_VERSION: u8 = 2;

pub(crate) const MAX_REQUEST_BYTES: usize = 256 * 1024;
pub const MAX_LAUNCH_ITEMS: usize = 32;
const MAX_PATH_UNITS: usize = 32_768;
const MAX_MAGNET_URI_BYTES: usize = 16 * 1024;

// Per-field bounds for bridge downloads (D-045 §3). The whole request is still
// capped by MAX_REQUEST_BYTES; these keep any single field from consuming it.
pub const MAX_URL_BYTES: usize = 8 * 1024;
pub const MAX_REFERER_BYTES: usize = 4 * 1024;
pub const MAX_USER_AGENT_BYTES: usize = 1024;
pub const MAX_COOKIE_BYTES: usize = 8 * 1024;
pub const MAX_FILENAME_BYTES: usize = 1024;

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

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct WireRequest {
    pub(crate) version: u8,
    pub(crate) metadata_paths: Vec<EncodedPath>,
    pub(crate) magnet_uris: Vec<String>,
    #[serde(default)]
    pub(crate) downloads: Vec<WireDownload>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct WireDownload {
    pub(crate) url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) referer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) cookie: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) file_size: Option<u64>,
}

pub(crate) fn encode_request(request: &LaunchRequest) -> io::Result<WireRequest> {
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

pub(crate) fn decode_request(request: WireRequest) -> io::Result<LaunchRequest> {
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
                cookie: download.cookie.map(SecretString::from),
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
        download.cookie.as_ref().map(ExposeSecret::expose_secret),
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
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample_download() -> BridgeDownload {
        BridgeDownload {
            url: "https://example.test/file.bin".into(),
            referer: Some("https://example.test/page".into()),
            user_agent: Some("Mozilla/5.0".into()),
            cookie: Some(SecretString::from("session=secret")),
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
                .map(ExposeSecret::expose_secret),
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
                cookie: Some(SecretString::from("会话=秘密")),
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
            cookie: Some(SecretString::from("a=b\r\nX-Injected: 1")),
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
            cookie: Some(SecretString::from("x".repeat(MAX_COOKIE_BYTES + 1))),
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
}
