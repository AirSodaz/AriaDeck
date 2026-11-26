use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorScheme {
    /// Follow the operating-system light/dark preference (UI-001 / D-031).
    #[default]
    System,
    Light,
    Dark,
}

/// Display language preference (i18n).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguagePreference {
    /// Follow the operating-system UI language when possible.
    #[default]
    System,
    /// English (`en`).
    En,
    /// Simplified Chinese (`zh-CN`).
    ZhCn,
}

/// Last-used download list filter (UI-001). Not a named-filter library.
pub type ListFilterPreference = ariadeck_domain::DownloadFilter;

/// Last-used list sort key (UI-001).
pub type ListSortKeyPreference = ariadeck_domain::SortKey;

/// Last-used list sort direction (UI-001).
pub type ListSortDirectionPreference = ariadeck_domain::SortDirection;

/// Restored list-query preferences (filter + sort). Search text is never
/// persisted so restarts do not re-hide tasks behind a forgotten query.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UiPreferences {
    pub list_filter: ListFilterPreference,
    pub list_sort_key: ListSortKeyPreference,
    pub list_sort_direction: ListSortDirectionPreference,
    /// Automatically run lightweight HTTP probe when adding a single HTTP(S) URI.
    #[serde(default)]
    pub auto_probe_on_add: bool,
}

/// First-run guidance state (B4).
///
/// Only records what the user has already been asked, never how the engine is
/// configured — the core registry and the profile catalog remain the truth for
/// that. Client-wide (`settings.json`), not per-profile: being walked through
/// core setup is a property of this installation, not of an engine environment.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OnboardingSettings {
    /// The core-setup guide has been completed or explicitly dismissed, so it
    /// must not reopen on its own. Settings → Engine still offers the same panel.
    #[serde(default)]
    pub core_setup_dismissed: bool,
}
