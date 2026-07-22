//! Fluent-based localization for AriaDeck.
//!
//! Catalogs live under `i18n/{locale}/*.ftl` and are embedded at compile time.
//! Missing keys fall back through the active locale → English.

use std::collections::BTreeSet;
use std::sync::Arc;

use fluent_bundle::FluentResource;
use fluent_bundle::concurrent::FluentBundle;
use rust_embed::Embed;
use unic_langid::{LanguageIdentifier, langid};

pub use fluent_bundle::{FluentArgs, FluentValue};

/// BCP-47 identifiers AriaDeck ships catalogs for.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum LocaleId {
    #[default]
    En,
    ZhCn,
}

impl LocaleId {
    #[must_use]
    pub const fn bcp47(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::ZhCn => "zh-CN",
        }
    }

    #[must_use]
    pub fn language_id(self) -> LanguageIdentifier {
        match self {
            Self::En => langid!("en"),
            Self::ZhCn => langid!("zh-CN"),
        }
    }

    #[must_use]
    pub const fn all() -> [Self; 2] {
        [Self::En, Self::ZhCn]
    }

    /// Map a language tag / env locale string onto a supported catalog.
    #[must_use]
    pub fn parse_tag(tag: &str) -> Option<Self> {
        let normalized = tag.trim().replace('_', "-");
        if normalized.is_empty() {
            return None;
        }
        let lower = normalized.to_ascii_lowercase();
        if lower == "en" || lower.starts_with("en-") {
            return Some(Self::En);
        }
        if lower == "zh"
            || lower.starts_with("zh-cn")
            || lower.starts_with("zh-hans")
            || lower.starts_with("zh-sg")
            || lower == "zh-cmn-hans"
        {
            return Some(Self::ZhCn);
        }
        // Traditional Chinese tags fall back to English until zh-TW ships.
        if lower.starts_with("zh-") {
            return Some(Self::En);
        }
        None
    }

    /// Resolve OS / environment preference for [`LanguagePreference::System`].
    ///
    /// Order:
    /// 1. Platform UI locale via `sys-locale` (covers Windows Chinese UI where
    ///    `LANG`/`LC_*` are typically unset)
    /// 2. POSIX env vars `LC_ALL` / `LC_MESSAGES` / `LANG`
    /// 3. English fallback
    #[must_use]
    pub fn from_system_env() -> Self {
        // Prefer the OS UI language list (Windows, macOS, Linux).
        for tag in sys_locale::get_locales() {
            if let Some(locale) = Self::parse_tag(&tag) {
                return locale;
            }
        }
        // Explicit env overrides remain useful for containers / CI.
        for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(value) = std::env::var(key)
                && let Some(locale) = value
                    .split('.')
                    .next()
                    .and_then(|part| part.split('@').next())
                    .and_then(Self::parse_tag)
            {
                return locale;
            }
        }
        Self::En
    }
}

/// User-facing language preference stored in settings.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum LanguagePreference {
    /// Follow the operating-system UI language when possible.
    #[default]
    System,
    English,
    ChineseSimplified,
}

impl LanguagePreference {
    #[must_use]
    pub const fn all() -> [Self; 3] {
        [Self::System, Self::English, Self::ChineseSimplified]
    }

    #[must_use]
    pub const fn as_settings_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::English => "en",
            Self::ChineseSimplified => "zh_cn",
        }
    }

    #[must_use]
    pub fn resolve(self) -> LocaleId {
        match self {
            Self::System => LocaleId::from_system_env(),
            Self::English => LocaleId::En,
            Self::ChineseSimplified => LocaleId::ZhCn,
        }
    }

    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::System => "language-system",
            Self::English => "language-english",
            Self::ChineseSimplified => "language-chinese-simplified",
        }
    }
}

#[derive(Embed)]
#[folder = "i18n/"]
struct CatalogAssets;

type Bundle = FluentBundle<FluentResource>;

/// Process-wide translator: one active locale with English fallback.
#[derive(Clone)]
pub struct Translator {
    locale: LocaleId,
    primary: Arc<Bundle>,
    fallback: Arc<Bundle>,
}

impl std::fmt::Debug for Translator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Translator")
            .field("locale", &self.locale)
            .finish_non_exhaustive()
    }
}

impl Default for Translator {
    fn default() -> Self {
        Self::new(LocaleId::En)
    }
}

impl Translator {
    /// Load embedded catalogs for `locale` (with English always available).
    #[must_use]
    pub fn new(locale: LocaleId) -> Self {
        let fallback = Arc::new(load_bundle(LocaleId::En));
        let primary = if locale == LocaleId::En {
            Arc::clone(&fallback)
        } else {
            Arc::new(load_bundle(locale))
        };
        Self {
            locale,
            primary,
            fallback,
        }
    }

    #[must_use]
    pub fn locale(&self) -> LocaleId {
        self.locale
    }

    /// Translate `id` with no arguments.
    #[must_use]
    pub fn t(&self, id: &str) -> String {
        self.t_args(id, None)
    }

    /// Translate `id` with optional Fluent arguments.
    #[must_use]
    pub fn t_args(&self, id: &str, args: Option<&FluentArgs>) -> String {
        if let Some(value) = format_in(&self.primary, id, args) {
            return value;
        }
        if !Arc::ptr_eq(&self.primary, &self.fallback)
            && let Some(value) = format_in(&self.fallback, id, args)
        {
            return value;
        }
        id.to_owned()
    }

    /// Convenience: integer argument named `n` (plurals / relative time).
    #[must_use]
    pub fn t_count(&self, id: &str, n: u64) -> String {
        let mut args = FluentArgs::new();
        args.set("n", FluentValue::from(i64::try_from(n).unwrap_or(i64::MAX)));
        self.t_args(id, Some(&args))
    }
}

fn format_in(bundle: &Bundle, id: &str, args: Option<&FluentArgs>) -> Option<String> {
    let message = bundle.get_message(id)?;
    let pattern = message.value()?;
    let mut errors = Vec::new();
    let value = bundle.format_pattern(pattern, args, &mut errors);
    if !errors.is_empty() {
        return None;
    }
    Some(value.into_owned())
}

fn load_bundle(locale: LocaleId) -> Bundle {
    let mut bundle = FluentBundle::new_concurrent(vec![locale.language_id()]);
    // Desktop UI: avoid bidi isolation marks around interpolations.
    bundle.set_use_isolating(false);
    let prefix = format!("{}/", locale.bcp47());
    let mut files: Vec<_> = CatalogAssets::iter()
        .filter(|path| path.starts_with(&prefix) && path.ends_with(".ftl"))
        .collect();
    files.sort();
    for path in files {
        let Some(file) = CatalogAssets::get(path.as_ref()) else {
            continue;
        };
        let Ok(source) = std::str::from_utf8(file.data.as_ref()) else {
            continue;
        };
        let resource = match FluentResource::try_new(source.to_owned()) {
            Ok(resource) => resource,
            Err((resource, errors)) => {
                for error in errors {
                    eprintln!("ariadeck-i18n: parse error in {}: {error:?}", path.as_ref());
                }
                resource
            }
        };
        if let Err(errors) = bundle.add_resource(resource) {
            for error in errors {
                eprintln!(
                    "ariadeck-i18n: add_resource error in {}: {error:?}",
                    path.as_ref()
                );
            }
        }
    }
    bundle
}

/// Message ids present in a locale's FTL files (for catalog parity tests).
#[must_use]
pub fn message_ids_for(locale: LocaleId) -> BTreeSet<String> {
    let prefix = format!("{}/", locale.bcp47());
    let mut ids = BTreeSet::new();
    for path in
        CatalogAssets::iter().filter(|path| path.starts_with(&prefix) && path.ends_with(".ftl"))
    {
        let Some(file) = CatalogAssets::get(path.as_ref()) else {
            continue;
        };
        let Ok(source) = std::str::from_utf8(file.data.as_ref()) else {
            continue;
        };
        // Line-oriented scan: Fluent message ids are `name =` at line start.
        for line in source.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
                continue;
            }
            if let Some((name, rest)) = line.split_once('=') {
                let name = name.trim();
                let rest = rest.trim_start();
                if name.is_empty() || name.starts_with('.') {
                    continue;
                }
                if name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    && !rest.is_empty()
                {
                    ids.insert(name.to_owned());
                }
            }
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_and_chinese_catalogs_share_the_same_keys() {
        let en = message_ids_for(LocaleId::En);
        let zh = message_ids_for(LocaleId::ZhCn);
        assert!(!en.is_empty(), "english catalog must not be empty");
        let missing_in_zh: Vec<_> = en.difference(&zh).cloned().collect();
        let extra_in_zh: Vec<_> = zh.difference(&en).cloned().collect();
        assert!(
            missing_in_zh.is_empty(),
            "zh-CN missing keys: {missing_in_zh:?}"
        );
        assert!(extra_in_zh.is_empty(), "zh-CN extra keys: {extra_in_zh:?}");
    }

    #[test]
    fn translator_returns_chinese_for_core_labels() {
        let t = Translator::new(LocaleId::ZhCn);
        assert_eq!(t.t("filter-active"), "下载中");
        assert_eq!(t.t("settings-nav-general"), "通用");
        assert_eq!(t.t("settings-language"), "语言");
    }

    #[test]
    fn missing_key_falls_back_to_id() {
        let t = Translator::new(LocaleId::ZhCn);
        assert_eq!(t.t("does-not-exist"), "does-not-exist");
        assert_eq!(t.t("filter-all"), "全部");
    }

    #[test]
    fn relative_time_interpolation_works() {
        let t = Translator::new(LocaleId::En);
        assert_eq!(t.t_count("time-minutes-ago", 5), "5 minutes ago");
        let zh = Translator::new(LocaleId::ZhCn);
        assert_eq!(zh.t_count("time-minutes-ago", 5), "5 分钟前");
    }

    #[test]
    fn production_operation_error_codes_are_translated() {
        let keys = [
            "error-validation-invalid-request",
            "error-validation-duplicate-task",
            "error-validation-unsupported-metadata-file",
            "error-validation-invalid-metadata",
            "error-validation-invalid-output-name",
            "error-validation-invalid-speed-limit",
            "error-validation-invalid-seed-ratio",
            "error-validation-invalid-seed-time",
            "error-validation-empty-task-options",
            "error-command-wrong-profile",
            "error-command-stale-session",
            "error-command-task-changed",
            "error-command-seed-rules-unsupported",
            "error-rpc-disconnected",
            "error-rpc-command-outcome-unknown",
            "error-rpc-add-not-observed",
            "error-rpc-retry-not-observed",
            "error-rpc-remove-not-observed",
            "error-rpc-authentication-failed",
            "error-rpc-timeout",
            "error-rpc-command-rejected",
            "error-command-unsupported",
            "error-filesystem-unsafe-path",
            "error-filesystem-operation-failed",
            "error-application-internal",
            "error-sync-unavailable",
            "error-command-no-result",
            "error-settings-invalid-download-directory",
            "error-settings-invalid-speed-limit",
            "error-settings-invalid-transfer-policy",
            "error-settings-path-picker-failed",
            "error-settings-path-picker-closed",
            "error-settings-save-failed",
            "error-profile-switch-failed",
            "error-profile-save-failed",
            "error-core-command-failed",
            "error-diagnostics-export-failed",
        ];
        for locale in LocaleId::all() {
            let translator = Translator::new(locale);
            for key in keys {
                assert_ne!(translator.t(key), key, "{locale:?} is missing {key}");
            }
        }
    }

    #[test]
    fn dialog_arguments_and_counts_render_in_chinese() {
        let translator = Translator::new(LocaleId::ZhCn);
        assert_eq!(
            translator.t_count("dialog-add-sources-detected", 3),
            "检测到 3 个来源"
        );

        let mut args = FluentArgs::new();
        args.set("kind", "Torrent");
        args.set("name", "linux.iso.torrent");
        args.set("selected", 2);
        args.set("total", 4);
        assert_eq!(
            translator.t_args("dialog-add-metadata-row-aria", Some(&args)),
            "Torrent linux.iso.torrent，已选择 2/4 个文件"
        );
    }

    #[test]
    fn language_preference_resolves_explicit_locales() {
        assert_eq!(LanguagePreference::English.resolve(), LocaleId::En);
        assert_eq!(
            LanguagePreference::ChineseSimplified.resolve(),
            LocaleId::ZhCn
        );
    }

    #[test]
    fn locale_tag_parsing_accepts_common_forms() {
        assert_eq!(LocaleId::parse_tag("en-US"), Some(LocaleId::En));
        assert_eq!(LocaleId::parse_tag("zh_CN.UTF-8"), Some(LocaleId::ZhCn));
        assert_eq!(LocaleId::parse_tag("zh-Hans-CN"), Some(LocaleId::ZhCn));
        // Windows GetUserPreferredUILanguages-style tags
        assert_eq!(LocaleId::parse_tag("zh-CN"), Some(LocaleId::ZhCn));
        assert_eq!(LocaleId::parse_tag("zh-Hans"), Some(LocaleId::ZhCn));
    }

    #[test]
    fn prints_system_locale_for_manual_debug() {
        // Not an assertion-heavy test: helps diagnose Windows UI language detection.
        let tags: Vec<_> = sys_locale::get_locales().collect();
        eprintln!("sys_locale tags: {tags:?}");
        eprintln!("from_system_env: {:?}", LocaleId::from_system_env());
    }

    #[test]
    fn system_preference_resolves_to_a_supported_catalog() {
        // On Chinese Windows UI, sys-locale returns tags like "zh-CN" and System
        // must map to ZhCn (not English). LANG/LC_* are often unset on Windows.
        assert_eq!(
            LanguagePreference::ChineseSimplified.resolve(),
            LocaleId::ZhCn
        );
        let system = LanguagePreference::System.resolve();
        assert!(
            matches!(system, LocaleId::En | LocaleId::ZhCn),
            "System resolved to unexpected locale: {system:?}"
        );
        // If the platform reports a Chinese UI language, System must pick ZhCn.
        let has_zh =
            sys_locale::get_locales().any(|tag| LocaleId::parse_tag(&tag) == Some(LocaleId::ZhCn));
        if has_zh {
            assert_eq!(system, LocaleId::ZhCn);
        }
    }
}
