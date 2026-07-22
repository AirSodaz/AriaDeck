//! Privacy helpers for redacting credentials and tracker tokens from URIs.
//!
//! Domain and engine layers may still hold raw engine data for RPC and retry.
//! UI projections, notices, clipboard source copy, Debug formatting, and any
//! diagnostic snapshot must use these helpers so secrets never leave those
//! surfaces.

use data_encoding::BASE32_NOPAD;
use url::Url;

/// Placeholder when a source URI cannot be safely parsed or redacted.
pub const REDACTED_SOURCE_PLACEHOLDER: &str = "Source available (details redacted)";

/// Extract a BitTorrent info hash from a magnet URI (`xt=urn:btih:…`).
///
/// Accepts 40-character hex and 32-character base32 hashes. Returns lowercase
/// hex, or `None` when the URI is not a magnet with a usable `xt` value.
#[must_use]
pub fn magnet_info_hash(uri: &str) -> Option<String> {
    let parsed = Url::parse(uri.trim()).ok()?;
    if !parsed.scheme().eq_ignore_ascii_case("magnet") {
        return None;
    }
    let value = parsed
        .query_pairs()
        .find(|(key, _)| key.eq_ignore_ascii_case("xt"))?
        .1;
    let value = value.as_ref();
    const BTIH_PREFIX: &str = "urn:btih:";
    let prefix = value.get(..BTIH_PREFIX.len())?;
    if !prefix.eq_ignore_ascii_case(BTIH_PREFIX) {
        return None;
    }
    let hash = value.get(BTIH_PREFIX.len()..)?;
    if hash.len() == 40 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Some(hash.to_ascii_lowercase());
    }
    if hash.len() == 32 {
        let decoded = BASE32_NOPAD
            .decode(hash.to_ascii_uppercase().as_bytes())
            .ok()?;
        return Some(decoded.iter().map(|byte| format!("{byte:02x}")).collect());
    }
    None
}

/// Redact a download source URI for UI, clipboard, notices, and diagnostics.
///
/// - Magnets collapse to `magnet:?xt=urn:btih:<hash>` (no `tr`/`dn`/tokens).
/// - HTTP(S)/FTP URLs drop userinfo, query, and fragment.
/// - Unparseable input becomes [`REDACTED_SOURCE_PLACEHOLDER`].
#[must_use]
pub fn redact_source_uri(uri: &str) -> String {
    let trimmed = uri.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(info_hash) = magnet_info_hash(trimmed) {
        return format!("magnet:?xt=urn:btih:{info_hash}");
    }
    let Ok(mut parsed) = Url::parse(trimmed) else {
        return REDACTED_SOURCE_PLACEHOLDER.into();
    };
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    parsed.set_query(None);
    parsed.set_fragment(None);
    parsed.to_string()
}

/// Redact a BitTorrent tracker announce URI (passkeys often live in path or query).
///
/// Same rules as [`redact_source_uri`]: strip credentials, query, and fragment.
/// For trackers that embed tokens in the path, only the origin (scheme/host/port)
/// is retained so the tier list remains useful without leaking tokens.
#[must_use]
pub fn redact_tracker_uri(uri: &str) -> String {
    let trimmed = uri.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let Ok(mut parsed) = Url::parse(trimmed) else {
        return REDACTED_SOURCE_PLACEHOLDER.into();
    };
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    parsed.set_query(None);
    parsed.set_fragment(None);
    // Announce tokens are commonly path segments (`/announce/SECRET`). Keep
    // host identity only when the path is non-trivial.
    let path = parsed.path();
    let path_is_sensitive =
        path != "/" && path != "/announce" && path != "/announce/" && !path.is_empty();
    if path_is_sensitive {
        parsed.set_path("");
    }
    parsed.to_string()
}

/// Whether an aria2 option key must never leave the RPC adapter in cleartext.
#[must_use]
pub fn task_option_key_is_sensitive(key: &str) -> bool {
    let key = key.trim().to_ascii_lowercase();
    key.contains("passwd")
        || key.contains("password")
        || key.contains("cookie")
        || key.contains("secret")
        || key.contains("token")
        || key.contains("credential")
        || key.contains("private-key")
        || key.contains("certificate")
        || key.contains("netrc")
        || key.contains("auth")
        || matches!(
            key.as_str(),
            "header"
                | "referer"
                | "http-user"
                | "ftp-user"
                | "all-proxy"
                | "all-proxy-user"
                | "http-proxy"
                | "http-proxy-user"
                | "https-proxy"
                | "https-proxy-user"
                | "ftp-proxy"
                | "ftp-proxy-user"
                | "bt-tracker"
                | "bt-exclude-tracker"
                | "metalink-location"
        )
}

/// Minimal diagnostic fields safe to log or export (no secrets, raw URIs, or paths).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiagnosticSnapshot {
    pub app_version: String,
    pub settings_schema_version: Option<u32>,
    pub connection_state: String,
    pub redacted_rpc_endpoint: Option<String>,
    pub profile_kind: Option<String>,
    pub task_count: Option<u32>,
    pub capability_count: Option<u32>,
}

impl DiagnosticSnapshot {
    /// Format a single-line support-friendly summary with only redacted fields.
    #[must_use]
    pub fn summary_line(&self) -> String {
        format!(
            "AriaDeck diagnostics: version={} settings_schema={:?} connection={} endpoint={:?} profile={:?} tasks={:?} capabilities={:?}",
            self.app_version,
            self.settings_schema_version,
            self.connection_state,
            self.redacted_rpc_endpoint,
            self.profile_kind,
            self.task_count,
            self.capability_count
        )
    }
}

/// Redact an RPC endpoint URL for Debug/diagnostics (strip userinfo/query/fragment).
#[must_use]
pub fn redact_endpoint_url(endpoint: &str) -> String {
    let Ok(mut parsed) = Url::parse(endpoint.trim()) else {
        return REDACTED_SOURCE_PLACEHOLDER.into();
    };
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    parsed.set_query(None);
    parsed.set_fragment(None);
    parsed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_userinfo_query_and_fragment_from_http_uris() {
        assert_eq!(
            redact_source_uri(
                "https://user:secret@example.test/archive.iso?token=private#fragment"
            ),
            "https://example.test/archive.iso"
        );
    }

    #[test]
    fn magnets_collapse_to_info_hash_only() {
        assert_eq!(
            redact_source_uri(
                "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&tr=https%3A%2F%2Ftracker.test&dn=private"
            ),
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567"
        );
    }

    #[test]
    fn magnet_base32_normalizes_to_hex() {
        let bytes = [0x01u8; 20];
        let encoded = BASE32_NOPAD.encode(&bytes);
        let magnet = format!("magnet:?xt=urn:btih:{encoded}&dn=leak");
        let hash = magnet_info_hash(&magnet).expect("base32 magnet");
        assert_eq!(hash.len(), 40);
        assert_eq!(
            redact_source_uri(&magnet),
            format!("magnet:?xt=urn:btih:{hash}")
        );
        assert!(!redact_source_uri(&magnet).contains("leak"));
    }

    #[test]
    fn unparseable_source_becomes_placeholder() {
        assert_eq!(redact_source_uri("not a uri"), REDACTED_SOURCE_PLACEHOLDER);
    }

    #[test]
    fn tracker_path_tokens_are_stripped_to_origin() {
        assert_eq!(
            redact_tracker_uri("https://tracker.example/announce/passkey-abc?token=x"),
            "https://tracker.example/"
        );
        assert_eq!(
            redact_tracker_uri("https://tracker.example/announce"),
            "https://tracker.example/announce"
        );
    }

    #[test]
    fn sensitive_option_keys_match_known_secrets_and_skip_limits() {
        for key in [
            "http-passwd",
            "all-proxy",
            "header",
            "bt-tracker",
            "ftp-passwd",
            "rpc-secret",
            "all-proxy-user",
        ] {
            assert!(task_option_key_is_sensitive(key), "{key} must be sensitive");
        }
        for key in ["max-download-limit", "dir", "out", "split", "user-agent"] {
            assert!(
                !task_option_key_is_sensitive(key),
                "{key} must not be redacted"
            );
        }
    }

    #[test]
    fn diagnostic_summary_never_embeds_planted_secrets() {
        let snapshot = DiagnosticSnapshot {
            app_version: "0.1.0".into(),
            settings_schema_version: Some(8),
            connection_state: "Connected".into(),
            redacted_rpc_endpoint: Some(redact_endpoint_url(
                "wss://user:never-log-this@rpc.example:6800/jsonrpc?token=private",
            )),
            profile_kind: Some("Remote".into()),
            task_count: Some(3),
            capability_count: Some(12),
        };
        let line = snapshot.summary_line();
        assert!(!line.contains("never-log-this"));
        assert!(!line.contains("token=private"));
        assert!(line.contains("wss://rpc.example:6800/jsonrpc"));
    }

    #[test]
    fn empty_uris_stay_empty_and_userinfo_without_password_is_stripped() {
        assert_eq!(redact_source_uri(""), "");
        assert_eq!(redact_source_uri("   "), "");
        assert_eq!(
            redact_source_uri("https://only-user@example.test/file.bin"),
            "https://example.test/file.bin"
        );
    }

    #[test]
    fn ipv6_literals_keep_host_after_redaction() {
        assert_eq!(
            redact_source_uri("http://[2001:db8::1]:8080/path?token=x#frag"),
            "http://[2001:db8::1]:8080/path"
        );
    }

    #[test]
    fn percent_encoded_credentials_are_not_left_in_output() {
        let redacted =
            redact_source_uri("https://user%40mail:s3cret%2Fpass@cdn.example/dl?sig=abc");
        assert_eq!(redacted, "https://cdn.example/dl");
        assert!(!redacted.contains("s3cret"));
        assert!(!redacted.contains("user"));
    }
}
