//! Configurable URL mirror rewrite rules (GitHub, HuggingFace, custom prefixes).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use url::Url;

/// Maximum number of configured mirror rewrite rules.
pub const MAX_MIRROR_RULES: usize = 64;

/// How a [`MirrorRewriteRule`] matches an incoming HTTP(S) download URI.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MirrorMatchKind {
    /// Replace a leading URL prefix (e.g. `https://github.com/` -> `https://mirror.example/https://github.com/`).
    #[default]
    Prefix,
    /// Replace `scheme://host[:port]` while keeping path, query, and fragment
    /// (e.g. `huggingface.co` -> `https://hf-mirror.com`).
    Host,
}

/// A user-configurable URL mirror rewrite rule.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MirrorRewriteRule {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub match_kind: MirrorMatchKind,
    pub pattern: String,
    pub replacement: String,
}

/// Outcome of applying [`MirrorRewriteRule`] entries to a candidate download URI.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MirrorRewriteResult {
    pub original: String,
    pub rewritten: String,
    pub matched_rule_name: Option<String>,
}

impl MirrorRewriteResult {
    #[must_use]
    pub fn was_rewritten(&self) -> bool {
        self.matched_rule_name.is_some() && self.rewritten != self.original
    }
}

/// Built-in opt-in mirror templates (all disabled by default).
#[must_use]
pub fn builtin_mirror_templates() -> Vec<MirrorRewriteRule> {
    vec![
        MirrorRewriteRule {
            id: "builtin-github-release".into(),
            name: "GitHub Releases".into(),
            enabled: false,
            match_kind: MirrorMatchKind::Prefix,
            pattern: "https://github.com/".into(),
            replacement: String::new(),
        },
        MirrorRewriteRule {
            id: "builtin-github-raw".into(),
            name: "GitHub Raw".into(),
            enabled: false,
            match_kind: MirrorMatchKind::Prefix,
            pattern: "https://raw.githubusercontent.com/".into(),
            replacement: String::new(),
        },
        MirrorRewriteRule {
            id: "builtin-huggingface".into(),
            name: "HuggingFace".into(),
            enabled: false,
            match_kind: MirrorMatchKind::Host,
            pattern: "huggingface.co".into(),
            replacement: "https://hf-mirror.com".into(),
        },
    ]
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MirrorRuleValidationError {
    #[error("Mirror rule id must not be empty.")]
    EmptyId,
    #[error("Mirror rule name must not be empty.")]
    EmptyName,
    #[error("Too many mirror rules (max {MAX_MIRROR_RULES}).")]
    TooManyRules,
    #[error("Duplicate mirror rule id: {0}")]
    DuplicateId(String),
    #[error("Mirror match pattern must not be empty.")]
    EmptyPattern,
    #[error("Invalid mirror pattern: {0}")]
    InvalidPattern(String),
    #[error("Enabled mirror rule must specify a valid http:// or https:// replacement URL.")]
    InvalidReplacement,
    #[error("Mirror replacement URL must not contain embedded credentials.")]
    ReplacementContainsCredentials,
}

pub fn validate_mirror_rule(rule: &MirrorRewriteRule) -> Result<(), MirrorRuleValidationError> {
    if rule.id.trim().is_empty() {
        return Err(MirrorRuleValidationError::EmptyId);
    }
    if rule.name.trim().is_empty() {
        return Err(MirrorRuleValidationError::EmptyName);
    }
    let pattern = rule.pattern.trim();
    if pattern.is_empty() || pattern.contains(['\r', '\n']) {
        return Err(MirrorRuleValidationError::EmptyPattern);
    }
    match rule.match_kind {
        MirrorMatchKind::Prefix => {
            let lower = pattern.to_ascii_lowercase();
            if !(lower.starts_with("http://") || lower.starts_with("https://")) {
                return Err(MirrorRuleValidationError::InvalidPattern(pattern.into()));
            }
        }
        MirrorMatchKind::Host => {
            if pattern.contains("://")
                || pattern.contains('/')
                || pattern.chars().any(char::is_whitespace)
            {
                return Err(MirrorRuleValidationError::InvalidPattern(pattern.into()));
            }
        }
    }
    let replacement = rule.replacement.trim();
    if !replacement.is_empty() || rule.enabled {
        let parsed =
            Url::parse(replacement).map_err(|_| MirrorRuleValidationError::InvalidReplacement)?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(MirrorRuleValidationError::InvalidReplacement);
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(MirrorRuleValidationError::ReplacementContainsCredentials);
        }
    }
    Ok(())
}

pub fn validate_mirror_rules(rules: &[MirrorRewriteRule]) -> Result<(), MirrorRuleValidationError> {
    if rules.len() > MAX_MIRROR_RULES {
        return Err(MirrorRuleValidationError::TooManyRules);
    }
    let mut ids = HashSet::with_capacity(rules.len());
    for rule in rules {
        validate_mirror_rule(rule)?;
        if !ids.insert(rule.id.trim().to_owned()) {
            return Err(MirrorRuleValidationError::DuplicateId(rule.id.clone()));
        }
    }
    Ok(())
}

fn apply_single_rule(uri: &str, rule: &MirrorRewriteRule) -> Option<String> {
    if !rule.enabled {
        return None;
    }
    let replacement = rule.replacement.trim();
    if replacement.is_empty() {
        return None;
    }
    let parsed = Url::parse(uri).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    // Never rewrite URLs carrying embedded credentials.
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }

    let candidate = match rule.match_kind {
        MirrorMatchKind::Prefix => {
            let pattern = rule.pattern.trim();
            let prefix = uri.get(..pattern.len())?;
            if !prefix.eq_ignore_ascii_case(pattern) {
                return None;
            }
            let suffix = &uri[pattern.len()..];
            if !pattern.ends_with(['/', '?', '#'])
                && !suffix.is_empty()
                && !suffix.starts_with(['/', '?', '#'])
            {
                return None;
            }
            match (replacement.ends_with('/'), suffix.starts_with('/')) {
                (true, true) => format!("{replacement}{}", &suffix[1..]),
                (false, false) if !suffix.is_empty() && !suffix.starts_with(['?', '#']) => {
                    format!("{replacement}/{suffix}")
                }
                _ => format!("{replacement}{suffix}"),
            }
        }
        MirrorMatchKind::Host => {
            let target_host = rule.pattern.trim();
            let host = parsed.host_str()?;
            if !host.eq_ignore_ascii_case(target_host) {
                return None;
            }
            let base = Url::parse(replacement).ok()?;
            if !matches!(base.scheme(), "http" | "https")
                || !base.username().is_empty()
                || base.password().is_some()
            {
                return None;
            }
            let mut rewritten = parsed;
            rewritten.set_scheme(base.scheme()).ok()?;
            rewritten.set_host(base.host_str()).ok()?;
            rewritten.set_port(base.port()).ok()?;
            let base_path = base.path().trim_end_matches('/');
            if !base_path.is_empty() && base_path != "/" {
                let merged = format!("{base_path}{}", rewritten.path());
                rewritten.set_path(&merged);
            }
            rewritten.to_string()
        }
    };

    let validated = Url::parse(&candidate).ok()?;
    if !matches!(validated.scheme(), "http" | "https")
        || !validated.username().is_empty()
        || validated.password().is_some()
    {
        return None;
    }
    Some(validated.to_string())
}

/// Apply the first matching enabled [`MirrorRewriteRule`] to an HTTP(S) URI.
#[must_use]
pub fn rewrite_uri_with_rules(uri: &str, rules: &[MirrorRewriteRule]) -> MirrorRewriteResult {
    let original = uri.trim().to_owned();
    for rule in rules {
        if let Some(rewritten) = apply_single_rule(&original, rule)
            && rewritten != original
        {
            return MirrorRewriteResult {
                original,
                rewritten,
                matched_rule_name: Some(rule.name.clone()),
            };
        }
    }
    MirrorRewriteResult {
        rewritten: original.clone(),
        original,
        matched_rule_name: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_mirror_rewrites_github_release_and_preserves_path() {
        let rules = vec![MirrorRewriteRule {
            id: "gh".into(),
            name: "GH Proxy".into(),
            enabled: true,
            match_kind: MirrorMatchKind::Prefix,
            pattern: "https://github.com/".into(),
            replacement: "https://mirror.example.test/https://github.com/".into(),
        }];
        let res = rewrite_uri_with_rules(
            "https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2.zip",
            &rules,
        );
        assert!(res.was_rewritten());
        assert_eq!(
            res.rewritten,
            "https://mirror.example.test/https://github.com/aria2/aria2/releases/download/release-1.37.0/aria2.zip"
        );
    }

    #[test]
    fn host_mirror_rewrites_huggingface_and_skips_magnet() {
        let mut rules = builtin_mirror_templates();
        rules[2].enabled = true;
        let res = rewrite_uri_with_rules(
            "https://huggingface.co/meta/model/resolve/main/weights.bin?download=true",
            &rules,
        );
        assert!(res.was_rewritten());
        assert_eq!(
            res.rewritten,
            "https://hf-mirror.com/meta/model/resolve/main/weights.bin?download=true"
        );

        let magnet = rewrite_uri_with_rules(
            "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567",
            &rules,
        );
        assert!(!magnet.was_rewritten());
    }

    #[test]
    fn prefix_mirror_handles_utf8_boundaries_and_host_boundaries_safely() {
        let rules = vec![MirrorRewriteRule {
            id: "gh".into(),
            name: "GH Proxy".into(),
            enabled: true,
            match_kind: MirrorMatchKind::Prefix,
            pattern: "https://github.com/org/repo-long-name/".into(),
            replacement: "https://mirror.example.test/https://github.com/org/repo-long-name/".into(),
        }];
        // Pattern length (41) lands inside a multi-byte UTF-8 sequence in the URI.
        let utf8_uri = "https://github.com/中文测试中文测试中文测试中文测试";
        let res = rewrite_uri_with_rules(utf8_uri, &rules);
        assert!(!res.was_rewritten());

        let no_slash_rules = vec![MirrorRewriteRule {
            id: "gh-noslash".into(),
            name: "GH No Slash".into(),
            enabled: true,
            match_kind: MirrorMatchKind::Prefix,
            pattern: "https://github.com".into(),
            replacement: "https://mirror.example.test/https://github.com".into(),
        }];
        // Must not match a lookalike host `github.com.evil.test`.
        let lookalike = rewrite_uri_with_rules("https://github.com.evil.test/file.zip", &no_slash_rules);
        assert!(!lookalike.was_rewritten());

        // Must not produce double slashes when joining suffix starting with '/'.
        let joined = rewrite_uri_with_rules("https://github.com/org/file.zip", &no_slash_rules);
        assert!(joined.was_rewritten());
        assert_eq!(
            joined.rewritten,
            "https://mirror.example.test/https://github.com/org/file.zip"
        );
    }
}
