use serde::{Deserialize, Serialize};
use url::Url;

use crate::SettingsError;

/// Soft cap for persisted tracker list text (characters).
pub const MAX_TRACKER_LIST_TEXT_CHARS: usize = 2 * 1024 * 1024;
/// Soft cap for a custom tracker list URL.
pub const MAX_TRACKER_LIST_URL_CHARS: usize = 2_048;

/// Curated public tracker list URL (ngosang best list). Fetched only with
/// explicit user refresh or opt-in auto-refresh (D-041).
pub const CURATED_TRACKER_LIST_URL: &str =
    "https://cdn.jsdelivr.net/gh/ngosang/trackerslist@master/trackers_best.txt";

/// Where the extra tracker list is obtained from.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackerListSource {
    /// Built-in curated public list URL.
    #[default]
    Curated,
    /// User-supplied HTTPS URL.
    Custom,
}

/// Extra BitTorrent trackers applied as aria2 global `bt-tracker` (D1 / D-041).
///
/// Network fetch is opt-in: empty by default, no auto-refresh unless enabled,
/// and never fetched on first run without user action.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrackerListSettings {
    /// When true, push the parsed list to aria2 as `bt-tracker`.
    pub enabled: bool,
    /// Curated constant URL vs user HTTPS URL.
    pub source: TrackerListSource,
    /// Required when `source` is Custom; must be `https:` without credentials.
    pub custom_url: Option<String>,
    /// When true, refresh at most once per day while the app is running.
    pub auto_refresh: bool,
    /// Unix timestamp (seconds) of the last successful network refresh.
    pub last_refreshed_at: Option<i64>,
    /// Last applied / edited newline-separated tracker list body.
    pub list_text: String,
}

impl TrackerListSettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.list_text.chars().count() > MAX_TRACKER_LIST_TEXT_CHARS {
            return Err(SettingsError::TrackerListTextTooLarge);
        }
        if self.source == TrackerListSource::Custom {
            let Some(url) = self
                .custom_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                return Err(SettingsError::MissingCustomTrackerListUrl);
            };
            if url.chars().count() > MAX_TRACKER_LIST_URL_CHARS {
                return Err(SettingsError::TrackerListUrlTooLong);
            }
            let parsed = Url::parse(url).map_err(|_| SettingsError::InvalidTrackerListUrl {
                reason: "not a valid URL".into(),
            })?;
            if parsed.scheme() != "https" {
                return Err(SettingsError::InvalidTrackerListUrl {
                    reason: "only https URLs are allowed".into(),
                });
            }
            if !parsed.username().is_empty() || parsed.password().is_some() {
                return Err(SettingsError::InvalidTrackerListUrl {
                    reason: "credentials in the URL are not allowed".into(),
                });
            }
            if parsed.host_str().is_none() {
                return Err(SettingsError::InvalidTrackerListUrl {
                    reason: "URL must include a host".into(),
                });
            }
        }
        Ok(())
    }

    /// Resolved fetch URL for the active source (when a network refresh runs).
    #[must_use]
    pub fn fetch_url(&self) -> Option<&str> {
        match self.source {
            TrackerListSource::Curated => Some(CURATED_TRACKER_LIST_URL),
            TrackerListSource::Custom => self
                .custom_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
        }
    }
}
