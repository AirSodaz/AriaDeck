//! Parse and format public BitTorrent tracker list text for aria2 `bt-tracker`.
//!
//! Lists are newline-oriented (Motrix / ngosang style). Comments and blank lines
//! are ignored. The engine option value is a single comma-separated string.

/// Soft cap on accepted tracker lines after parse (settings + RPC size hygiene).
pub const MAX_TRACKER_LIST_ENTRIES: usize = 2_048;
/// Soft cap on a single tracker URI length.
pub const MAX_TRACKER_URI_LEN: usize = 2_048;
/// Soft cap on raw fetch/import body size (bytes of text).
pub const MAX_TRACKER_LIST_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Parse a tracker list body into ordered, de-duplicated announce URIs.
///
/// Rules:
/// - Trim each line; skip empty lines and `#` comments.
/// - Accept `http://`, `https://`, and `udp://` schemes only.
/// - Drop lines that exceed [`MAX_TRACKER_URI_LEN`].
/// - De-duplicate while preserving first-seen order (case-sensitive).
/// - Stop accepting new entries once [`MAX_TRACKER_LIST_ENTRIES`] is reached.
#[must_use]
pub fn parse_tracker_list(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in text.lines() {
        if out.len() >= MAX_TRACKER_LIST_ENTRIES {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.len() > MAX_TRACKER_URI_LEN {
            continue;
        }
        if !is_allowed_tracker_uri(trimmed) {
            continue;
        }
        if seen.insert(trimmed.to_owned()) {
            out.push(trimmed.to_owned());
        }
    }
    out
}

/// Format trackers for aria2's global `bt-tracker` option (comma-separated).
#[must_use]
pub fn format_bt_tracker_option(trackers: &[String]) -> String {
    trackers.join(",")
}

/// Rebuild a stable newline-separated list for settings persistence / UI.
#[must_use]
pub fn format_tracker_list_text(trackers: &[String]) -> String {
    trackers.join("\n")
}

fn is_allowed_tracker_uri(uri: &str) -> bool {
    let lower = uri.to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("udp://"))
        && !uri.chars().any(|c| c.is_control() || c == ',' || c == ' ')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_skips_comments_blanks_and_invalid_lines() {
        let text = r#"
# curated list
https://tracker.example/announce

udp://open.tracker.example:80/announce
ftp://not-allowed.example/announce
not-a-uri
http://ok.example/announce
http://ok.example/announce
"#;
        assert_eq!(
            parse_tracker_list(text),
            vec![
                "https://tracker.example/announce".to_owned(),
                "udp://open.tracker.example:80/announce".to_owned(),
                "http://ok.example/announce".to_owned(),
            ]
        );
    }

    #[test]
    fn format_bt_tracker_is_comma_separated() {
        let trackers = vec![
            "https://a.example/announce".to_owned(),
            "udp://b.example:80/announce".to_owned(),
        ];
        assert_eq!(
            format_bt_tracker_option(&trackers),
            "https://a.example/announce,udp://b.example:80/announce"
        );
        assert_eq!(
            format_tracker_list_text(&trackers),
            "https://a.example/announce\nudp://b.example:80/announce"
        );
    }

    #[test]
    fn parse_drops_overlong_and_commas() {
        let long = format!("https://x.example/{}", "a".repeat(MAX_TRACKER_URI_LEN));
        let with_comma = "https://a.example/announce,extra";
        let text = format!("{long}\n{with_comma}\nhttps://ok.example/announce");
        assert_eq!(
            parse_tracker_list(&text),
            vec!["https://ok.example/announce".to_owned()]
        );
    }

    #[test]
    fn parse_respects_entry_cap() {
        let mut body = String::new();
        for i in 0..(MAX_TRACKER_LIST_ENTRIES + 10) {
            body.push_str(&format!("https://t{i}.example/announce\n"));
        }
        assert_eq!(parse_tracker_list(&body).len(), MAX_TRACKER_LIST_ENTRIES);
    }
}
