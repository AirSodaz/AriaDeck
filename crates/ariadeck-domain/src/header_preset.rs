//! Domain-based HTTP request header presets (cookie-free, SEC-001 compliant).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use url::Url;

/// Maximum number of configured domain header presets.
pub const MAX_HEADER_PRESETS: usize = 64;
/// Maximum number of domain patterns per preset.
pub const MAX_PRESET_DOMAINS: usize = 32;
/// Maximum number of custom headers per preset.
pub const MAX_PRESET_HEADERS: usize = 32;

/// Reusable HTTP header identity preset matched by download URL host.
///
/// Cookie and Authorization headers are explicitly forbidden here so secrets
/// never persist in `settings.json` or export documents (SEC-001).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainHeaderPreset {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub referer: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub headers: Vec<String>,
}

const fn default_true() -> bool {
    true
}

/// Merged headers produced by matching one or more presets against a URL host.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MatchedHeaderPreset {
    pub preset_names: Vec<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub headers: Vec<String>,
}

/// Validation failure for a [`DomainHeaderPreset`].
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum HeaderPresetValidationError {
    #[error("Preset id must not be empty.")]
    EmptyId,
    #[error("Preset name must not be empty.")]
    EmptyName,
    #[error("Too many header presets (max {MAX_HEADER_PRESETS}).")]
    TooManyPresets,
    #[error("Duplicate header preset id: {0}")]
    DuplicateId(String),
    #[error("Preset must include at least one domain pattern.")]
    EmptyDomains,
    #[error("Too many domains in preset (max {MAX_PRESET_DOMAINS}).")]
    TooManyDomains,
    #[error("Invalid domain pattern: {0}")]
    InvalidDomainPattern(String),
    #[error("{0} must be a single line without control characters.")]
    InvalidControlCharacters(&'static str),
    #[error("Too many custom headers in preset (max {MAX_PRESET_HEADERS}).")]
    TooManyHeaders,
    #[error("Custom header must use `Name: value` form without control characters.")]
    InvalidHeaderLine,
    #[error("Cookie and Authorization headers cannot be persisted in domain presets.")]
    ForbiddenSensitiveHeader,
}

fn contains_control_chars(value: &str) -> bool {
    value
        .chars()
        .any(|ch| ch.is_control() || ch == '\r' || ch == '\n')
}

/// Normalize a domain pattern (`example.com` or `*.example.com`) and reject
/// paths, schemes, whitespace, or empty labels.
pub fn normalize_domain_pattern(raw: &str) -> Result<String, HeaderPresetValidationError> {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty()
        || trimmed.contains("://")
        || trimmed.contains('/')
        || trimmed.contains('?')
        || trimmed.contains('#')
        || trimmed.chars().any(char::is_whitespace)
        || contains_control_chars(&trimmed)
    {
        return Err(HeaderPresetValidationError::InvalidDomainPattern(
            raw.trim().to_owned(),
        ));
    }
    let host_part = trimmed.strip_prefix("*.").unwrap_or(&trimmed);
    if host_part.is_empty()
        || host_part.contains('*')
        || host_part.starts_with('.')
        || host_part.ends_with('.')
        || host_part.split('.').any(str::is_empty)
    {
        return Err(HeaderPresetValidationError::InvalidDomainPattern(
            raw.trim().to_owned(),
        ));
    }
    Ok(trimmed)
}

/// Validate a non-sensitive `Name: value` header line for domain presets.
pub fn validate_preset_header_line(raw: &str) -> Result<String, HeaderPresetValidationError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || contains_control_chars(trimmed) {
        return Err(HeaderPresetValidationError::InvalidHeaderLine);
    }
    let Some((name, value)) = trimmed.split_once(':') else {
        return Err(HeaderPresetValidationError::InvalidHeaderLine);
    };
    let name_trimmed = name.trim();
    let value_trimmed = value.trim();
    if name_trimmed.is_empty()
        || value_trimmed.is_empty()
        || name_trimmed.chars().any(char::is_whitespace)
    {
        return Err(HeaderPresetValidationError::InvalidHeaderLine);
    }
    let lower = name_trimmed.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "cookie" | "authorization" | "proxy-authorization"
    ) {
        return Err(HeaderPresetValidationError::ForbiddenSensitiveHeader);
    }
    Ok(format!("{name_trimmed}: {value_trimmed}"))
}

/// Validate a single [`DomainHeaderPreset`].
pub fn validate_header_preset(
    preset: &DomainHeaderPreset,
) -> Result<(), HeaderPresetValidationError> {
    if preset.id.trim().is_empty() {
        return Err(HeaderPresetValidationError::EmptyId);
    }
    if preset.name.trim().is_empty() {
        return Err(HeaderPresetValidationError::EmptyName);
    }
    if preset.domains.is_empty() {
        return Err(HeaderPresetValidationError::EmptyDomains);
    }
    if preset.domains.len() > MAX_PRESET_DOMAINS {
        return Err(HeaderPresetValidationError::TooManyDomains);
    }
    for domain in &preset.domains {
        normalize_domain_pattern(domain)?;
    }
    if let Some(referer) = &preset.referer
        && (referer.trim().is_empty() || contains_control_chars(referer))
    {
        return Err(HeaderPresetValidationError::InvalidControlCharacters(
            "Referer",
        ));
    }
    if let Some(ua) = &preset.user_agent
        && (ua.trim().is_empty() || contains_control_chars(ua))
    {
        return Err(HeaderPresetValidationError::InvalidControlCharacters(
            "User-Agent",
        ));
    }
    if preset.headers.len() > MAX_PRESET_HEADERS {
        return Err(HeaderPresetValidationError::TooManyHeaders);
    }
    for header in &preset.headers {
        validate_preset_header_line(header)?;
    }
    Ok(())
}

/// Validate a list of [`DomainHeaderPreset`] entries.
pub fn validate_header_presets(
    presets: &[DomainHeaderPreset],
) -> Result<(), HeaderPresetValidationError> {
    if presets.len() > MAX_HEADER_PRESETS {
        return Err(HeaderPresetValidationError::TooManyPresets);
    }
    let mut ids = HashSet::with_capacity(presets.len());
    for preset in presets {
        validate_header_preset(preset)?;
        if !ids.insert(preset.id.trim().to_owned()) {
            return Err(HeaderPresetValidationError::DuplicateId(preset.id.clone()));
        }
    }
    Ok(())
}

fn domain_match_score(host: &str, pattern: &str) -> u8 {
    let Ok(normalized) = normalize_domain_pattern(pattern) else {
        return 0;
    };
    if let Some(suffix) = normalized.strip_prefix("*.") {
        if host
            .strip_suffix(suffix)
            .is_some_and(|prefix| prefix.ends_with('.'))
        {
            1
        } else {
            0
        }
    } else if host == normalized {
        2
    } else {
        0
    }
}

/// Match enabled presets against an HTTP/HTTPS URL's host.
///
/// Exact domain matches take priority over `*.suffix` wildcard matches; ties
/// preserve the configured list order. Duplicate header names keep the higher
/// priority value.
#[must_use]
pub fn match_header_presets(
    uri: &str,
    presets: &[DomainHeaderPreset],
) -> Option<MatchedHeaderPreset> {
    let parsed = Url::parse(uri.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    let mut matches: Vec<(u8, usize, &DomainHeaderPreset)> = presets
        .iter()
        .enumerate()
        .filter(|(_, preset)| preset.enabled)
        .filter_map(|(idx, preset)| {
            let best_score = preset
                .domains
                .iter()
                .map(|pattern| domain_match_score(&host, pattern))
                .max()
                .unwrap_or(0);
            (best_score > 0).then_some((best_score, idx, preset))
        })
        .collect();
    if matches.is_empty() {
        return None;
    }
    matches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let mut result = MatchedHeaderPreset::default();
    let mut seen_header_names = HashSet::new();
    for (_, _, preset) in matches {
        result.preset_names.push(preset.name.clone());
        if result.referer.is_none()
            && let Some(referer) = &preset.referer
        {
            let trimmed = referer.trim();
            if !trimmed.is_empty() {
                result.referer = Some(trimmed.to_owned());
            }
        }
        if result.user_agent.is_none()
            && let Some(ua) = &preset.user_agent
        {
            let trimmed = ua.trim();
            if !trimmed.is_empty() {
                result.user_agent = Some(trimmed.to_owned());
            }
        }
        for raw_header in &preset.headers {
            if let Ok(normalized) = validate_preset_header_line(raw_header)
                && let Some((name, _)) = normalized.split_once(':')
            {
                let key = name.trim().to_ascii_lowercase();
                if seen_header_names.insert(key) {
                    result.headers.push(normalized);
                }
            }
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_cookie_and_authorization_headers() {
        let mut preset = DomainHeaderPreset {
            id: "p1".into(),
            name: "Example".into(),
            enabled: true,
            domains: vec!["example.com".into()],
            referer: None,
            user_agent: None,
            headers: vec!["Cookie: a=b".into()],
        };
        assert_eq!(
            validate_header_preset(&preset),
            Err(HeaderPresetValidationError::ForbiddenSensitiveHeader)
        );
        preset.headers = vec!["Authorization: Bearer xyz".into()];
        assert_eq!(
            validate_header_preset(&preset),
            Err(HeaderPresetValidationError::ForbiddenSensitiveHeader)
        );
    }

    #[test]
    fn exact_domain_outranks_wildcard_and_dedups_headers() {
        let presets = vec![
            DomainHeaderPreset {
                id: "wild".into(),
                name: "CDN".into(),
                enabled: true,
                domains: vec!["*.example.com".into()],
                referer: Some("https://wild.example.com/".into()),
                user_agent: Some("WildUA".into()),
                headers: vec!["X-Mode: wild".into(), "X-Shared: 1".into()],
            },
            DomainHeaderPreset {
                id: "exact".into(),
                name: "DL".into(),
                enabled: true,
                domains: vec!["dl.example.com".into()],
                referer: Some("https://exact.example.com/".into()),
                user_agent: None,
                headers: vec!["X-Mode: exact".into()],
            },
        ];
        let matched = match_header_presets("https://dl.example.com/file.zip", &presets)
            .expect("preset should match");
        assert_eq!(matched.preset_names, vec!["DL", "CDN"]);
        assert_eq!(
            matched.referer.as_deref(),
            Some("https://exact.example.com/")
        );
        assert_eq!(matched.user_agent.as_deref(), Some("WildUA"));
        assert_eq!(matched.headers, vec!["X-Mode: exact", "X-Shared: 1"]);
    }
}
