//! Extension-based download category routing (IDM-style; D-042).

/// Normalize a file extension for matching: trim, strip leading dots, lowercase ASCII.
/// Returns `None` when empty after normalization.
#[must_use]
pub fn normalize_extension(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_dots = trimmed.trim_start_matches('.');
    if without_dots.is_empty() {
        return None;
    }
    Some(without_dots.to_ascii_lowercase())
}

/// Last path segment extension from a file name (`archive.tar.gz` → `gz`).
#[must_use]
pub fn extension_from_filename(name: &str) -> Option<String> {
    let name = name.trim().trim_end_matches(['/', '\\']);
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    // Use only the final path component if a path-like string is passed.
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .split('?')
        .next()
        .unwrap_or(name)
        .split('#')
        .next()
        .unwrap_or(name);
    let base = base.trim();
    if base.is_empty() {
        return None;
    }
    let (_, ext) = base.rsplit_once('.')?;
    if ext.is_empty() || ext.contains(['/', '\\']) {
        return None;
    }
    // Dotfiles like `.gitignore` have no extension in our model.
    if base.starts_with('.') && base[1..].chars().filter(|c| *c == '.').count() == 0 {
        return None;
    }
    normalize_extension(ext)
}

/// Best-effort file name hint from a URI or local path (query/fragment stripped).
#[must_use]
pub fn filename_hint_from_source(source: &str) -> Option<String> {
    let source = source.trim();
    if source.is_empty() {
        return None;
    }
    // magnet: has no useful file extension for routing.
    if source.len() >= 7 && source[..7].eq_ignore_ascii_case("magnet:") {
        return None;
    }
    let without_fragment = source.split('#').next().unwrap_or(source);
    let without_query = without_fragment
        .split('?')
        .next()
        .unwrap_or(without_fragment);
    let path = without_query
        .strip_prefix("file://")
        .or_else(|| without_query.strip_prefix("FILE://"))
        .unwrap_or(without_query);
    // Strip scheme://host/ if present.
    let path = if let Some(idx) = path.find("://") {
        let after = &path[idx + 3..];
        after.find('/').map(|i| &after[i + 1..]).unwrap_or("")
    } else {
        path
    };
    let segment = path
        .rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or("");
    if segment.is_empty() {
        return None;
    }
    // Percent-decode common cases lightly: leave as-is for matching (extensions rarely encoded).
    let decoded = percent_decode_minimal(segment);
    if decoded.is_empty() || decoded == "." || decoded == ".." {
        return None;
    }
    Some(decoded)
}

fn percent_decode_minimal(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Match categories by extension; first list hit wins; otherwise the fallback category.
///
/// `categories` order is significant for overlapping extensions.
/// Each item is `(id, is_fallback, extensions)`.
#[must_use]
pub fn resolve_category_id<'a>(
    categories: impl IntoIterator<Item = (&'a str, bool, &'a [String])>,
    filename_hint: Option<&str>,
) -> Option<&'a str> {
    let categories: Vec<(&str, bool, &[String])> = categories.into_iter().collect();
    if categories.is_empty() {
        return None;
    }
    let ext = filename_hint.and_then(extension_from_filename);
    if let Some(ext) = ext.as_deref() {
        for (id, _, extensions) in &categories {
            if extensions
                .iter()
                .any(|candidate| normalize_extension(candidate).as_deref() == Some(ext))
            {
                return Some(*id);
            }
        }
    }
    categories
        .iter()
        .find(|(_, is_fallback, _)| *is_fallback)
        .map(|(id, _, _)| *id)
        .or_else(|| categories.first().map(|(id, _, _)| *id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_extracts_extensions() {
        assert_eq!(normalize_extension(".MP4"), Some("mp4".into()));
        assert_eq!(extension_from_filename("film.MP4"), Some("mp4".into()));
        assert_eq!(extension_from_filename("a.tar.gz"), Some("gz".into()));
        assert_eq!(extension_from_filename("readme"), None);
        assert_eq!(extension_from_filename(".gitignore"), None);
        assert_eq!(extension_from_filename("dir/file.mkv"), Some("mkv".into()));
    }

    #[test]
    fn filename_hint_from_http_and_magnet() {
        assert_eq!(
            filename_hint_from_source("https://cdn.example/path/video.mp4?token=1"),
            Some("video.mp4".into())
        );
        assert_eq!(
            filename_hint_from_source("magnet:?xt=urn:btih:abcdef"),
            None
        );
        assert_eq!(
            filename_hint_from_source("https://example.com/a%2Eb.zip"),
            Some("a.b.zip".into())
        );
    }

    #[test]
    fn resolve_prefers_extension_then_fallback() {
        let video = "video-id".to_string();
        let general = "general-id".to_string();
        let video_ext = vec!["mp4".into(), "mkv".into()];
        let none_ext: Vec<String> = Vec::new();
        let cats = [
            (video.as_str(), false, video_ext.as_slice()),
            (general.as_str(), true, none_ext.as_slice()),
        ];
        assert_eq!(
            resolve_category_id(cats, Some("movie.mp4")),
            Some("video-id")
        );
        assert_eq!(
            resolve_category_id(cats, Some("readme")),
            Some("general-id")
        );
        assert_eq!(resolve_category_id(cats, None), Some("general-id"));
    }

    #[test]
    fn first_matching_category_wins_on_overlap() {
        let a_ext = vec!["zip".into()];
        let b_ext = vec!["zip".into()];
        let a = "a".to_string();
        let b = "b".to_string();
        let cats = [
            (a.as_str(), false, a_ext.as_slice()),
            (b.as_str(), true, b_ext.as_slice()),
        ];
        assert_eq!(resolve_category_id(cats, Some("x.zip")), Some("a"));
    }
}
