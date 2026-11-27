//! Pure-Rust clipboard text parsing and download link recognition.
//!
//! Used by the opt-in clipboard monitor to turn copied URI lines into
//! pre-filled confirmation candidates while avoiding false positives on
//! ordinary web pages or mixed prose.

use std::collections::HashSet;

use url::Url;

use crate::{extension_from_filename, filename_hint_from_source, magnet_info_hash};

/// Upper bound on clipboard text inspected by the watcher (64 KiB).
pub const MAX_CLIPBOARD_TEXT_BYTES: usize = 64 * 1024;

/// Maximum number of deduplicated links extracted from a single clipboard copy.
pub const MAX_CLIPBOARD_LINKS: usize = 32;

#[rustfmt::skip]
const DEFAULT_CLIPBOARD_DOWNLOAD_EXTENSIONS: &[&str] = &[
    // Archives & compressed containers
    "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "tgz", "tbz2", "txz", "lz4", "cab",
    // Disk images, firmware & binaries
    "iso", "img", "bin", "dmg", "pkg", "exe", "msi", "msix", "appx", "deb", "rpm", "apk", "ipa", "appimage",
    // BitTorrent & Metalink metadata
    "torrent", "metalink", "meta4",
    // Video
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "ts",
    // Audio
    "mp3", "flac", "aac", "wav", "ogg", "m4a", "opus", "wma",
    // Documents
    "pdf", "epub", "mobi", "azw3", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
];

/// Returns `true` when `uri` is a single downloadable link eligible for
/// clipboard capture.
///
/// Matches:
/// - valid `magnet:` links (verified via [`magnet_info_hash`]),
/// - valid `ftp://` / `sftp://` URLs with a non-empty host,
/// - valid `http://` / `https://` URLs with a non-empty host and non-directory
///   path whose filename hint ends with a built-in downloadable extension or
///   one in `extra_extensions`.
#[must_use]
pub fn is_clipboard_download_uri(uri: &str, extra_extensions: &[&str]) -> bool {
    let trimmed = uri.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return false;
    }
    if magnet_info_hash(trimmed).is_some() {
        return true;
    }
    let Ok(parsed) = Url::parse(trimmed) else {
        return false;
    };
    if parsed.host_str().is_none_or(str::is_empty) {
        return false;
    }
    match parsed.scheme() {
        "ftp" | "sftp" => true,
        "http" | "https" => {
            if parsed.path().ends_with('/') {
                return false;
            }
            let Some(filename) = filename_hint_from_source(trimmed) else {
                return false;
            };
            let Some(ext) = extension_from_filename(&filename) else {
                return false;
            };
            DEFAULT_CLIPBOARD_DOWNLOAD_EXTENSIONS.contains(&ext.as_str())
                || extra_extensions
                    .iter()
                    .filter_map(|candidate| crate::normalize_extension(candidate))
                    .any(|candidate| candidate == ext)
        }
        _ => false,
    }
}

/// Extract downloadable links from clipboard `text` when every non-empty line is
/// a recognized download URI.
///
/// Returns an empty vector when:
/// - `text` is empty or exceeds [`MAX_CLIPBOARD_TEXT_BYTES`],
/// - any non-empty line is not a recognized download link (rejecting mixed
///   prose, comments, or ordinary web page URLs).
///
/// Accepted URIs are deduplicated in original order and capped at
/// [`MAX_CLIPBOARD_LINKS`].
#[must_use]
pub fn extract_clipboard_download_uris(text: &str, extra_extensions: &[&str]) -> Vec<String> {
    if text.len() > MAX_CLIPBOARD_TEXT_BYTES {
        return Vec::new();
    }

    let mut seen = HashSet::new();
    let mut uris = Vec::new();
    let mut non_empty_lines = 0usize;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        non_empty_lines += 1;
        if !is_clipboard_download_uri(line, extra_extensions) {
            return Vec::new();
        }
        if seen.insert(line) && uris.len() < MAX_CLIPBOARD_LINKS {
            uris.push(line.to_owned());
        }
    }

    if non_empty_lines == 0 {
        return Vec::new();
    }
    uris
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_valid_magnets_ftp_and_downloadable_http_urls() {
        assert!(is_clipboard_download_uri(
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=linux.iso",
            &[],
        ));
        assert!(is_clipboard_download_uri(
            "ftp://mirrors.example.test/pub/releases/README",
            &[],
        ));
        assert!(is_clipboard_download_uri(
            "sftp://files.example.test/archive/build",
            &[],
        ));
        assert!(is_clipboard_download_uri(
            "https://cdn.example.test/releases/app-1.0.0.zip?token=abc#frag",
            &[],
        ));
        assert!(is_clipboard_download_uri(
            "http://dl.example.test/installer.EXE",
            &[],
        ));
    }

    #[test]
    fn rejects_web_pages_trailing_slash_urls_invalid_magnets_and_prose() {
        assert!(!is_clipboard_download_uri(
            "https://example.test/docs/getting-started",
            &[],
        ));
        assert!(!is_clipboard_download_uri(
            "https://example.test/releases/v1.0.zip/",
            &[],
        ));
        assert!(!is_clipboard_download_uri(
            "https://example.test/index.html",
            &[],
        ));
        assert!(!is_clipboard_download_uri(
            "magnet:?xt=urn:btih:invalid",
            &[]
        ));
        assert!(!is_clipboard_download_uri(
            "file:///C:/Downloads/archive.zip",
            &[]
        ));
        assert!(!is_clipboard_download_uri(
            "https://example.test/a.zip extra",
            &[]
        ));
    }

    #[test]
    fn honors_custom_category_extensions_without_affecting_other_urls() {
        assert!(!is_clipboard_download_uri(
            "https://models.example.test/weights/model.safetensors",
            &[],
        ));
        assert!(is_clipboard_download_uri(
            "https://models.example.test/weights/model.safetensors",
            &[".safetensors"],
        ));
    }

    #[test]
    fn extracts_deduplicated_multiline_links_and_rejects_mixed_prose() {
        let text = "\n  https://cdn.example.test/a.zip  \r\n\
                    https://cdn.example.test/a.zip\n\
                    magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567\n";
        assert_eq!(
            extract_clipboard_download_uris(text, &[]),
            vec![
                "https://cdn.example.test/a.zip",
                "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
            ]
        );

        let mixed = "Download from:\nhttps://cdn.example.test/a.zip\n";
        assert!(extract_clipboard_download_uris(mixed, &[]).is_empty());

        let mixed_webpage = "https://cdn.example.test/a.zip\nhttps://example.test/release-notes\n";
        assert!(extract_clipboard_download_uris(mixed_webpage, &[]).is_empty());
    }

    #[test]
    fn enforces_max_text_bytes_and_max_links_cap() {
        let oversized = format!(
            "https://example.test/{}.zip",
            "a".repeat(MAX_CLIPBOARD_TEXT_BYTES)
        );
        assert!(extract_clipboard_download_uris(&oversized, &[]).is_empty());

        let many = (0..(MAX_CLIPBOARD_LINKS + 5))
            .map(|i| format!("https://example.test/file-{i}.zip"))
            .collect::<Vec<_>>()
            .join("\n");
        let extracted = extract_clipboard_download_uris(&many, &[]);
        assert_eq!(extracted.len(), MAX_CLIPBOARD_LINKS);
    }
}
