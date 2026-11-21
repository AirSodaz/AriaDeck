//! Fluent-based localization for AriaDeck.
//!
//! Catalogs live under `i18n/{locale}/*.ftl` and are embedded at compile time.
//! Missing keys fall back through the active locale → English.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};

use fluent_bundle::FluentResource;
use fluent_bundle::concurrent::FluentBundle;
use fluent_syntax::ast;
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
        let base = tag.split('.').next().unwrap_or(tag);
        let base = base.split('@').next().unwrap_or(base);
        let normalized = base.trim().replace('_', "-");
        if normalized.is_empty() {
            return None;
        }
        let lower = normalized.to_ascii_lowercase();
        if lower == "en" || lower.starts_with("en-") {
            return Some(Self::En);
        }
        if lower == "zh"
            || lower == "zh-cn"
            || lower.starts_with("zh-cn-")
            || lower == "zh-hans"
            || lower.starts_with("zh-hans-")
            || lower == "zh-sg"
            || lower.starts_with("zh-sg-")
            || lower == "zh-cmn-hans"
        {
            return Some(Self::ZhCn);
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

static EN_BUNDLE: OnceLock<Arc<Bundle>> = OnceLock::new();
static ZH_CN_BUNDLE: OnceLock<Arc<Bundle>> = OnceLock::new();

fn cached_bundle(locale: LocaleId) -> Arc<Bundle> {
    let slot = match locale {
        LocaleId::En => &EN_BUNDLE,
        LocaleId::ZhCn => &ZH_CN_BUNDLE,
    };
    Arc::clone(slot.get_or_init(|| Arc::new(load_bundle(locale).0)))
}

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
    ///
    /// Parsed bundles are cached once per process via `OnceLock`.
    #[must_use]
    pub fn new(locale: LocaleId) -> Self {
        let fallback = cached_bundle(LocaleId::En);
        let primary = if locale == LocaleId::En {
            Arc::clone(&fallback)
        } else {
            cached_bundle(locale)
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

fn locale_ftl_files(locale: LocaleId) -> Vec<String> {
    let prefix = format!("{}/", locale.bcp47());
    let mut files: Vec<String> = CatalogAssets::iter()
        .filter(|path| path.starts_with(&prefix) && path.ends_with(".ftl"))
        .map(|path| path.into_owned())
        .collect();
    files.sort();
    files
}

fn load_bundle(locale: LocaleId) -> (Bundle, Vec<String>) {
    let mut bundle = FluentBundle::new_concurrent(vec![locale.language_id()]);
    // Desktop UI: avoid bidi isolation marks around interpolations.
    bundle.set_use_isolating(false);
    let mut diagnostics = Vec::new();
    for path in locale_ftl_files(locale) {
        let Some(file) = CatalogAssets::get(&path) else {
            continue;
        };
        let Ok(source) = std::str::from_utf8(file.data.as_ref()) else {
            diagnostics.push(format!("{path}: invalid UTF-8"));
            continue;
        };
        let resource = match FluentResource::try_new(source.to_owned()) {
            Ok(resource) => resource,
            Err((resource, errors)) => {
                for error in errors {
                    diagnostics.push(format!("{path}: parse error: {error:?}"));
                }
                resource
            }
        };
        if let Err(errors) = bundle.add_resource(resource) {
            for error in errors {
                diagnostics.push(format!("{path}: add_resource error: {error:?}"));
            }
        }
    }
    (bundle, diagnostics)
}

/// Message ids present in a locale's FTL files (parsed via `fluent-syntax` AST).
#[must_use]
pub fn message_ids_for(locale: LocaleId) -> BTreeSet<String> {
    message_variables_for(locale).into_keys().collect()
}

/// Map of every message id in `locale` to the set of `$variable` names it references.
#[must_use]
pub fn message_variables_for(locale: LocaleId) -> BTreeMap<String, BTreeSet<String>> {
    let mut map = BTreeMap::new();
    for path in locale_ftl_files(locale) {
        let Some(file) = CatalogAssets::get(&path) else {
            continue;
        };
        let Ok(source) = std::str::from_utf8(file.data.as_ref()) else {
            continue;
        };
        let resource = match FluentResource::try_new(source.to_owned()) {
            Ok(resource) | Err((resource, _)) => resource,
        };
        for entry in resource.entries() {
            if let ast::Entry::Message(message) = entry {
                let mut vars = BTreeSet::new();
                if let Some(pattern) = &message.value {
                    collect_pattern_vars(pattern, &mut vars);
                }
                for attr in &message.attributes {
                    collect_pattern_vars(&attr.value, &mut vars);
                }
                map.insert(message.id.name.to_owned(), vars);
            }
        }
    }
    map
}

fn collect_pattern_vars(pattern: &ast::Pattern<&str>, vars: &mut BTreeSet<String>) {
    for element in &pattern.elements {
        if let ast::PatternElement::Placeable { expression } = element {
            collect_expression_vars(expression, vars);
        }
    }
}

fn collect_expression_vars(expression: &ast::Expression<&str>, vars: &mut BTreeSet<String>) {
    match expression {
        ast::Expression::Select { selector, variants } => {
            collect_inline_vars(selector, vars);
            for variant in variants {
                collect_pattern_vars(&variant.value, vars);
            }
        }
        ast::Expression::Inline(inline) => collect_inline_vars(inline, vars),
    }
}

fn collect_inline_vars(inline: &ast::InlineExpression<&str>, vars: &mut BTreeSet<String>) {
    match inline {
        ast::InlineExpression::VariableReference { id } => {
            vars.insert(id.name.to_owned());
        }
        ast::InlineExpression::FunctionReference { arguments, .. }
        | ast::InlineExpression::TermReference {
            arguments: Some(arguments),
            ..
        } => {
            for arg in &arguments.positional {
                collect_inline_vars(arg, vars);
            }
            for named in &arguments.named {
                collect_inline_vars(&named.value, vars);
            }
        }
        ast::InlineExpression::Placeable { expression } => {
            collect_expression_vars(expression, vars);
        }
        ast::InlineExpression::StringLiteral { .. }
        | ast::InlineExpression::NumberLiteral { .. }
        | ast::InlineExpression::MessageReference { .. }
        | ast::InlineExpression::TermReference {
            arguments: None, ..
        } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_embedded_ftl_files_parse_and_load_without_errors() {
        for locale in LocaleId::all() {
            let (_, diagnostics) = load_bundle(locale);
            assert!(
                diagnostics.is_empty(),
                "FTL errors in {locale:?}: {diagnostics:#?}"
            );
        }
    }

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
    fn english_and_chinese_catalogs_share_identical_variables() {
        let en = message_variables_for(LocaleId::En);
        let zh = message_variables_for(LocaleId::ZhCn);
        let mut mismatches = Vec::new();
        for (key, en_vars) in &en {
            if let Some(zh_vars) = zh.get(key)
                && en_vars != zh_vars
            {
                mismatches.push(format!("{key}: en={en_vars:?} vs zh-CN={zh_vars:?}"));
            }
        }
        assert!(
            mismatches.is_empty(),
            "variable mismatches across locales:\n{}",
            mismatches.join("\n")
        );
    }

    #[test]
    fn translator_reuses_cached_bundles() {
        let first = Translator::new(LocaleId::ZhCn);
        let second = Translator::new(LocaleId::ZhCn);
        assert!(Arc::ptr_eq(&first.primary, &second.primary));
        assert!(Arc::ptr_eq(&first.fallback, &second.fallback));
    }

    #[test]
    fn translator_returns_chinese_for_core_labels() {
        let t = Translator::new(LocaleId::ZhCn);
        assert_eq!(t.t("filter-active"), "下载中");
        assert_eq!(t.t("settings-nav-general"), "通用");
        assert_eq!(t.t("settings-language"), "语言");
        assert_eq!(t.t("task-overview-progress"), "进度");
        assert_eq!(t.t("task-overview-share-ratio"), "分享率");
        assert_eq!(t.t("notice-core-verified"), "aria2 内核校验通过。");
    }

    #[test]
    fn missing_key_falls_back_to_id() {
        let t = Translator::new(LocaleId::ZhCn);
        assert_eq!(t.t("does-not-exist"), "does-not-exist");
        assert_eq!(t.t("filter-all"), "全部");
    }

    #[test]
    fn count_interpolation_works() {
        let t = Translator::new(LocaleId::En);
        assert_eq!(
            t.t_count("dialog-details-files-aria", 5),
            "Task files, 5 items"
        );
        assert_eq!(
            t.t_count("notice-engine-recovered", 1),
            "Local aria2 recovered after 1 restart attempt."
        );
        assert_eq!(
            t.t_count("notice-engine-recovered", 3),
            "Local aria2 recovered after 3 restart attempts."
        );
        assert_eq!(
            t.t_count("notice-add-accepted", 1),
            "1 download accepted by aria2."
        );
        let zh = Translator::new(LocaleId::ZhCn);
        assert_eq!(
            zh.t_count("dialog-details-files-aria", 5),
            "任务文件，共 5 项"
        );
        assert_eq!(
            zh.t_count("notice-engine-recovered", 1),
            "本地 aria2 已在 1 次重启尝试后恢复。"
        );
        assert_eq!(
            zh.t_count("notice-add-accepted", 2),
            "aria2 已接受 2 个下载任务。"
        );
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
            "error-settings-empty-categories",
            "error-settings-invalid-speed-limit",
            "error-settings-invalid-transfer-policy",
            "error-settings-path-picker-failed",
            "error-settings-path-picker-closed",
            "error-settings-save-failed",
            "error-settings-export-failed",
            "error-settings-import-failed",
            "error-settings-import-busy",
            "error-settings-tracker-refresh-unavailable",
            "error-settings-tracker-refresh-failed",
            "error-profile-switch-failed",
            "error-profile-save-failed",
            "error-core-command-failed",
            "error-diagnostics-export-failed",
            "error-persistence-unavailable",
            "error-core-download-unsupported-platform",
            "error-core-download-unavailable",
            "error-core-download-failed",
            "error-power-action-failed",
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
        assert_eq!(LocaleId::parse_tag("zh-TW"), None);
        assert_eq!(LocaleId::parse_tag("zh-HK"), None);
        assert_eq!(LocaleId::parse_tag("zh-Hant"), None);
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
