//! Tracker list network refresh (D1 / D-041).
//!
//! Fetch is explicit (Refresh now) or opt-in auto-refresh. Defaults never
//! download on first run. TLS uses OS trust via reqwest rustls native roots.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ariadeck_domain::{MAX_TRACKER_LIST_BODY_BYTES, format_tracker_list_text, parse_tracker_list};
use ariadeck_settings::{AppSettings, JsonSettingsStore, TrackerListSettings};

use super::settings_bridge::{map_bt_tracker_list, spawn_proxy_settings_load};
use super::*;

/// Auto-refresh interval when enabled (once per day).
pub(crate) const TRACKER_AUTO_REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
const USER_AGENT: &str = concat!("AriaDeck/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug)]
pub(crate) struct TrackerListRefreshRequest {
    pub(crate) request_id: ariadeck_ui::RequestId,
    /// When true, only refresh if auto_refresh is on and the interval elapsed.
    pub(crate) auto: bool,
    /// Optional form snapshot for manual refresh (source/url/enabled/auto).
    pub(crate) draft: Option<TrackerListSettings>,
}

#[derive(Clone, Debug)]
pub(crate) struct TrackerListRefreshResult {
    pub(crate) request_id: ariadeck_ui::RequestId,
    pub(crate) settings: Option<AppSettings>,
    pub(crate) tracker_count: usize,
    pub(crate) result: Result<(), String>,
    pub(crate) auto: bool,
}

pub(crate) async fn fetch_tracker_list_body(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| format!("Failed to build HTTP client: {error}"))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("Tracker list download failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Tracker list download failed with HTTP {}.",
            response.status()
        ));
    }
    // Prefer content-length when present; still bound the read below.
    if let Some(len) = response.content_length()
        && len > MAX_TRACKER_LIST_BODY_BYTES as u64
    {
        return Err("Tracker list response is too large.".into());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("Tracker list download failed: {error}"))?;
    if bytes.len() > MAX_TRACKER_LIST_BODY_BYTES {
        return Err("Tracker list response is too large.".into());
    }
    String::from_utf8(bytes.to_vec())
        .map_err(|_| "Tracker list response is not valid UTF-8.".to_owned())
}

pub(crate) fn refresh_tracker_list_settings(
    current: &TrackerListSettings,
    body: &str,
) -> Result<(TrackerListSettings, usize), String> {
    current.validate().map_err(|error| error.to_string())?;
    let trackers = parse_tracker_list(body);
    if trackers.is_empty() {
        return Err("Tracker list contained no valid announce URLs.".into());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut next = current.clone();
    next.list_text = format_tracker_list_text(&trackers);
    next.last_refreshed_at = Some(now);
    next.validate().map_err(|error| error.to_string())?;
    Ok((next, trackers.len()))
}

fn should_auto_refresh(settings: &TrackerListSettings, now: i64) -> bool {
    if !settings.auto_refresh || !settings.enabled {
        return false;
    }
    match settings.last_refreshed_at {
        None => true,
        Some(last) => now.saturating_sub(last) >= TRACKER_AUTO_REFRESH_INTERVAL.as_secs() as i64,
    }
}

pub(crate) async fn run_tracker_list_refresh(
    store: JsonSettingsStore,
    sync: Option<SyncHandle>,
    credential_store: Arc<dyn ProxyCredentialStore>,
    request: TrackerListRefreshRequest,
) -> TrackerListRefreshResult {
    let loaded = spawn_proxy_settings_load(
        &tokio::runtime::Handle::current(),
        store.clone(),
        credential_store,
    )
    .await
    .map_err(|error| format!("settings load task failed: {error}"))
    .and_then(|result| result);

    let (mut settings, _password) = match loaded {
        Ok(pair) => pair,
        Err(error) => {
            return TrackerListRefreshResult {
                request_id: request.request_id,
                settings: None,
                tracker_count: 0,
                result: Err(error),
                auto: request.auto,
            };
        }
    };

    if let Some(draft) = request.draft {
        // Manual refresh uses the form snapshot so Save is not required first.
        settings.tracker_list.enabled = draft.enabled;
        settings.tracker_list.source = draft.source;
        settings.tracker_list.custom_url = draft.custom_url;
        settings.tracker_list.auto_refresh = draft.auto_refresh;
        // Keep last list_text/timestamp until fetch succeeds.
    }

    if request.auto {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if !should_auto_refresh(&settings.tracker_list, now) {
            let tracker_count = parse_tracker_list(&settings.tracker_list.list_text).len();
            return TrackerListRefreshResult {
                request_id: request.request_id,
                settings: Some(settings),
                tracker_count,
                result: Ok(()),
                auto: true,
            };
        }
    }

    if let Err(error) = settings.tracker_list.validate() {
        return TrackerListRefreshResult {
            request_id: request.request_id,
            settings: Some(settings),
            tracker_count: 0,
            result: Err(error.to_string()),
            auto: request.auto,
        };
    }

    let Some(url) = settings.tracker_list.fetch_url().map(str::to_owned) else {
        return TrackerListRefreshResult {
            request_id: request.request_id,
            settings: Some(settings),
            tracker_count: 0,
            result: Err("Tracker list URL is not configured.".into()),
            auto: request.auto,
        };
    };

    let body = match fetch_tracker_list_body(&url).await {
        Ok(body) => body,
        Err(error) => {
            let tracker_count = parse_tracker_list(&settings.tracker_list.list_text).len();
            return TrackerListRefreshResult {
                request_id: request.request_id,
                settings: Some(settings),
                tracker_count,
                result: Err(error),
                auto: request.auto,
            };
        }
    };

    let (next_list, count) = match refresh_tracker_list_settings(&settings.tracker_list, &body) {
        Ok(pair) => pair,
        Err(error) => {
            return TrackerListRefreshResult {
                request_id: request.request_id,
                settings: Some(settings),
                tracker_count: 0,
                result: Err(error),
                auto: request.auto,
            };
        }
    };
    settings.tracker_list = next_list;

    if let Err(error) = tokio::task::spawn_blocking({
        let store = store.clone();
        let settings = settings.clone();
        move || store.save(&settings).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("settings persistence task failed: {error}"))
    .and_then(|result| result)
    {
        return TrackerListRefreshResult {
            request_id: request.request_id,
            settings: None,
            tracker_count: count,
            result: Err(error),
            auto: request.auto,
        };
    }

    // Apply to the live engine when connected; save already succeeded so a
    // failed apply is reported but settings retain the new list.
    if let Some(sync) = sync
        && let Some(snapshot) = sync.snapshot(TaskListQuery::default()).await
        && matches!(snapshot.connection_state, ConnectionState::Connected)
        && let Err(error) = sync
            .apply_bt_tracker(snapshot.session, map_bt_tracker_list(&settings))
            .await
    {
        return TrackerListRefreshResult {
            request_id: request.request_id,
            settings: Some(settings),
            tracker_count: count,
            result: Err(format!(
                "Tracker list was saved but not applied to aria2: {}",
                error.summary
            )),
            auto: request.auto,
        };
    }

    TrackerListRefreshResult {
        request_id: request.request_id,
        settings: Some(settings),
        tracker_count: count,
        result: Ok(()),
        auto: request.auto,
    }
}

pub(crate) fn spawn_tracker_list_refresh_bridge(
    runtime: tokio::runtime::Handle,
    store: JsonSettingsStore,
    sync: Option<SyncHandle>,
    credential_store: Arc<dyn ProxyCredentialStore>,
    mut requests: mpsc::UnboundedReceiver<TrackerListRefreshRequest>,
    results: mpsc::UnboundedSender<TrackerListRefreshResult>,
) {
    // Must run on the Tokio runtime: mpsc recv, HTTP fetch, and engine apply all
    // need a reactor. GPUI's cx.spawn executor does not provide one.
    runtime.spawn(async move {
        while let Some(request) = requests.recv().await {
            let store = store.clone();
            let sync = sync.clone();
            let credentials = credential_store.clone();
            let result = run_tracker_list_refresh(store, sync, credentials, request).await;
            if results.send(result).is_err() {
                break;
            }
        }
    });
}

/// Background auto-refresh tick (checks every hour; refresh logic enforces 24h).
pub(crate) fn spawn_tracker_list_auto_refresh(
    runtime: tokio::runtime::Handle,
    requests: mpsc::UnboundedSender<TrackerListRefreshRequest>,
) {
    runtime.spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60 * 60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Skip the immediate first tick; wait one period so startup reapply wins.
        interval.tick().await;
        loop {
            interval.tick().await;
            let request = TrackerListRefreshRequest {
                request_id: ariadeck_ui::RequestId::from_u64(0),
                auto: true,
                draft: None,
            };
            if requests.send(request).is_err() {
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariadeck_settings::TrackerListSource;

    #[test]
    fn refresh_updates_list_text_and_timestamp() {
        let current = TrackerListSettings {
            enabled: true,
            source: TrackerListSource::Curated,
            custom_url: None,
            auto_refresh: false,
            last_refreshed_at: None,
            list_text: String::new(),
        };
        let body = "# comment\nhttps://a.example/announce\nudp://b.example:80/announce\n";
        let (next, count) = refresh_tracker_list_settings(&current, body).expect("refresh");
        assert_eq!(count, 2);
        assert!(next.list_text.contains("https://a.example/announce"));
        assert!(next.last_refreshed_at.is_some());
    }

    #[test]
    fn auto_refresh_requires_interval() {
        let mut settings = TrackerListSettings {
            enabled: true,
            auto_refresh: true,
            last_refreshed_at: Some(1_700_000_000),
            ..TrackerListSettings::default()
        };
        assert!(!should_auto_refresh(&settings, 1_700_000_000 + 60));
        assert!(should_auto_refresh(
            &settings,
            1_700_000_000 + TRACKER_AUTO_REFRESH_INTERVAL.as_secs() as i64
        ));
        settings.auto_refresh = false;
        assert!(!should_auto_refresh(
            &settings,
            1_700_000_000 + TRACKER_AUTO_REFRESH_INTERVAL.as_secs() as i64
        ));
    }
}
