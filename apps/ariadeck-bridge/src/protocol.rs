//! Native messaging framing and the extension → host payload contract (D-045 §3).
//!
//! Everything here is pure and stdio-free so the whitelist and the cookie gate
//! can be tested without a browser.

use std::{
    io::{self, Read, Write},
    path::Path,
};

use ariadeck_ipc::{BridgeDownload, MAX_LAUNCH_ITEMS};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};

/// Whole-message ceiling (D-045 §3). Well under Chrome's own limit and under the
/// socket broker's 256 KiB, so an accepted message always fits downstream.
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;

/// The only extension protocol version this host accepts.
pub const HOST_MESSAGE_VERSION: u32 = 1;

/// Bounded error vocabulary sent back to the extension. Deliberately carries no
/// AriaDeck state, paths, or settings values — the bridge stays one-way.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCode {
    NotRunning,
    Rejected,
    TooLarge,
    UnsupportedVersion,
    Timeout,
}

impl ErrorCode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotRunning => "not_running",
            Self::Rejected => "rejected",
            Self::TooLarge => "too_large",
            Self::UnsupportedVersion => "unsupported_version",
            Self::Timeout => "timeout",
        }
    }

    /// Map a forwarding failure onto the reply vocabulary.
    ///
    /// A validation refusal is `rejected`; anything else means the primary
    /// instance could not be reached, which is `not_running`. There is no
    /// fallback transport to try (D-045 §7).
    #[must_use]
    pub fn from_forward_error(error: &io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => Self::Rejected,
            io::ErrorKind::TimedOut => Self::Timeout,
            _ => Self::NotRunning,
        }
    }
}

/// One decoded native message from the extension.
#[derive(Deserialize)]
pub struct HostMessage {
    pub version: u32,
    pub items: Vec<HostItem>,
}

/// A single offered download.
///
/// Unknown fields are tolerated so the extension can send display-only extras
/// (`mime`) and evolve without breaking an older host; what matters is that
/// nothing outside this struct is ever read. No `Debug` derive — an item may
/// hold a cookie.
#[derive(Deserialize)]
pub struct HostItem {
    pub url: String,
    #[serde(default)]
    pub referer: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub cookie: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub file_size: Option<u64>,
}

/// Bounded ack. `accepted` on success, `error` on failure, never both.
#[derive(Debug, Serialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<&'static str>,
}

impl Reply {
    #[must_use]
    pub fn accepted(count: usize) -> Self {
        Self {
            ok: true,
            accepted: Some(count),
            error: None,
        }
    }

    #[must_use]
    pub fn failed(code: ErrorCode) -> Self {
        Self {
            ok: false,
            accepted: None,
            error: Some(code.as_str()),
        }
    }
}

/// One read from the browser port.
#[derive(Debug)]
pub enum Frame {
    Message(Vec<u8>),
    /// Length header exceeded [`MAX_MESSAGE_BYTES`]. The body is not consumed, so
    /// the stream is out of sync and the caller must reply and stop.
    Oversize,
    /// Browser closed the port.
    Eof,
}

/// Read one length-prefixed native message (4-byte little-endian length).
pub fn read_frame(reader: &mut impl Read) -> io::Result<Frame> {
    let mut header = [0_u8; 4];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(Frame::Eof),
        Err(error) => return Err(error),
    }
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > MAX_MESSAGE_BYTES {
        return Ok(Frame::Oversize);
    }
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body)?;
    Ok(Frame::Message(body))
}

/// Write one length-prefixed reply and flush it.
pub fn write_reply(writer: &mut impl Write, reply: &Reply) -> io::Result<()> {
    let body = serde_json::to_vec(reply).map_err(io::Error::other)?;
    let length = u32::try_from(body.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "reply is too large"))?;
    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(&body)?;
    writer.flush()
}

/// Decode a message body into forwardable downloads.
///
/// `allow_cookies` is the resolved AriaDeck-side gate; when false, cookies are
/// dropped here so a secret never crosses the socket at all (D-045 §3/§5).
/// Field-level validation (scheme, CRLF, bounds) is left to the IPC codec, which
/// is the single gate both the host and the primary instance go through.
pub fn decode_items(body: &[u8], allow_cookies: bool) -> Result<Vec<BridgeDownload>, ErrorCode> {
    let message: HostMessage = serde_json::from_slice(body).map_err(|_| ErrorCode::Rejected)?;
    if message.version != HOST_MESSAGE_VERSION {
        return Err(ErrorCode::UnsupportedVersion);
    }
    if message.items.is_empty() || message.items.len() > MAX_LAUNCH_ITEMS {
        return Err(ErrorCode::Rejected);
    }
    Ok(message
        .items
        .into_iter()
        .map(|item| BridgeDownload {
            url: item.url,
            referer: item.referer,
            user_agent: item.user_agent,
            cookie: item.cookie.filter(|_| allow_cookies).map(SecretString::new),
            filename: item.filename,
            file_size: item.file_size,
        })
        .collect())
}

/// Minimal, lenient probe of the `browser_bridge` section of `settings.json`.
///
/// The bridge intentionally does not link `ariadeck-settings`: it needs one
/// boolean and must not fight the desktop over schema migration. Anything
/// missing, malformed, or unreadable resolves to "not allowed" (fail closed,
/// D-011).
#[derive(Default, Deserialize)]
struct SettingsProbe {
    #[serde(default)]
    browser_bridge: BrowserBridgeProbe,
}

#[derive(Default, Deserialize)]
struct BrowserBridgeProbe {
    #[serde(default)]
    allow_cookies: bool,
}

/// Whether the user has opted into forwarding cookies (D-045 §5.1).
#[must_use]
pub fn cookies_allowed(data_dir: &Path) -> bool {
    let Ok(bytes) = std::fs::read(data_dir.join("settings.json")) else {
        return false;
    };
    serde_json::from_slice::<SettingsProbe>(&bytes)
        .map(|probe| probe.browser_bridge.allow_cookies)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;
    use std::io::Cursor;

    fn frame(body: &str) -> Vec<u8> {
        let mut bytes = (body.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(body.as_bytes());
        bytes
    }

    fn sample(cookie: &str) -> String {
        format!(
            r#"{{"version":1,"items":[{{"url":"https://example.test/f.bin",
               "referer":"https://example.test/p","user_agent":"UA",
               "cookie":"{cookie}","filename":"f.bin","file_size":7,
               "mime":"application/octet-stream"}}]}}"#
        )
    }

    #[test]
    fn reads_a_length_prefixed_message() {
        let mut reader = Cursor::new(frame(r#"{"version":1,"items":[]}"#));
        let Frame::Message(body) = read_frame(&mut reader).expect("frame reads") else {
            panic!("expected a message");
        };
        assert_eq!(body, br#"{"version":1,"items":[]}"#);
    }

    #[test]
    fn closed_port_reads_as_eof() {
        let mut reader = Cursor::new(Vec::new());
        assert!(matches!(
            read_frame(&mut reader).expect("frame reads"),
            Frame::Eof
        ));
    }

    #[test]
    fn oversized_length_header_is_refused_without_reading_the_body() {
        let mut bytes = ((MAX_MESSAGE_BYTES + 1) as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(b"{}");
        let mut reader = Cursor::new(bytes);
        assert!(matches!(
            read_frame(&mut reader).expect("frame reads"),
            Frame::Oversize
        ));
    }

    #[test]
    fn zero_length_message_is_refused() {
        let mut reader = Cursor::new(0_u32.to_le_bytes().to_vec());
        assert!(matches!(
            read_frame(&mut reader).expect("frame reads"),
            Frame::Oversize
        ));
    }

    #[test]
    fn truncated_body_is_an_error_not_a_short_message() {
        let mut bytes = 32_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(b"{\"version\":1}");
        let mut reader = Cursor::new(bytes);
        let error = read_frame(&mut reader).expect_err("truncated body fails");
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn replies_are_length_prefixed_and_bounded() {
        let mut buffer = Vec::new();
        write_reply(&mut buffer, &Reply::accepted(2)).expect("reply writes");
        let length = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
        assert_eq!(length, buffer.len() - 4);
        assert_eq!(&buffer[4..], br#"{"ok":true,"accepted":2}"#);

        let mut buffer = Vec::new();
        write_reply(&mut buffer, &Reply::failed(ErrorCode::NotRunning)).expect("reply writes");
        assert_eq!(&buffer[4..], br#"{"ok":false,"error":"not_running"}"#);
    }

    #[test]
    fn decodes_items_and_ignores_display_only_extras() {
        let items = decode_items(sample("session=secret").as_bytes(), true).expect("decodes");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].url, "https://example.test/f.bin");
        assert_eq!(items[0].referer.as_deref(), Some("https://example.test/p"));
        assert_eq!(items[0].filename.as_deref(), Some("f.bin"));
        assert_eq!(items[0].file_size, Some(7));
    }

    #[test]
    fn cookie_is_dropped_unless_opted_in() {
        let denied = decode_items(sample("session=secret").as_bytes(), false).expect("decodes");
        assert!(denied[0].cookie.is_none());
        let allowed = decode_items(sample("session=secret").as_bytes(), true).expect("decodes");
        assert_eq!(
            allowed[0]
                .cookie
                .as_ref()
                .map(|cookie| cookie.expose_secret().as_str()),
            Some("session=secret")
        );
    }

    #[test]
    fn unknown_extension_version_is_reported_distinctly() {
        let body = br#"{"version":2,"items":[{"url":"https://example.test/f"}]}"#;
        assert_eq!(
            decode_items(body, false).expect_err("version rejected"),
            ErrorCode::UnsupportedVersion
        );
    }

    #[test]
    fn empty_and_flooded_batches_are_refused() {
        let empty = br#"{"version":1,"items":[]}"#;
        assert_eq!(
            decode_items(empty, false).expect_err("empty rejected"),
            ErrorCode::Rejected
        );

        let items = (0..=MAX_LAUNCH_ITEMS)
            .map(|index| format!(r#"{{"url":"https://example.test/{index}"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let flooded = format!(r#"{{"version":1,"items":[{items}]}}"#);
        assert_eq!(
            decode_items(flooded.as_bytes(), false).expect_err("flood rejected"),
            ErrorCode::Rejected
        );
    }

    #[test]
    fn malformed_json_and_missing_url_are_refused() {
        assert_eq!(
            decode_items(b"not json", false).expect_err("garbage rejected"),
            ErrorCode::Rejected
        );
        assert_eq!(
            decode_items(br#"{"version":1,"items":[{"referer":"x"}]}"#, false)
                .expect_err("missing url rejected"),
            ErrorCode::Rejected
        );
    }

    #[test]
    fn cookie_gate_fails_closed_on_missing_or_malformed_settings() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!cookies_allowed(dir.path()));

        std::fs::write(dir.path().join("settings.json"), b"{ not json").expect("write");
        assert!(!cookies_allowed(dir.path()));

        std::fs::write(dir.path().join("settings.json"), br#"{"schema_version":1}"#)
            .expect("write");
        assert!(!cookies_allowed(dir.path()));
    }

    #[test]
    fn cookie_gate_opens_only_on_an_explicit_opt_in() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");

        std::fs::write(&path, br#"{"browser_bridge":{"allow_cookies":false}}"#).expect("write");
        assert!(!cookies_allowed(dir.path()));

        std::fs::write(&path, br#"{"browser_bridge":{"allow_cookies":true}}"#).expect("write");
        assert!(cookies_allowed(dir.path()));
    }

    #[test]
    fn forward_errors_map_onto_the_reply_vocabulary() {
        let cases = [
            (io::ErrorKind::InvalidInput, ErrorCode::Rejected),
            (io::ErrorKind::InvalidData, ErrorCode::Rejected),
            (io::ErrorKind::TimedOut, ErrorCode::Timeout),
            (io::ErrorKind::NotFound, ErrorCode::NotRunning),
            (io::ErrorKind::ConnectionRefused, ErrorCode::NotRunning),
        ];
        for (kind, expected) in cases {
            let error = io::Error::new(kind, "boom");
            assert_eq!(ErrorCode::from_forward_error(&error), expected, "{kind:?}");
        }
    }
}
