//! Unified ordered persistence worker for `ariadeck-desktop`.
//!
//! All mutations to `settings.json`, `profiles.json`,
//! `profiles/<id>/environment.json`, `window.json`, and OS keychain entries
//! flow through this single Tokio actor so that:
//! 1. No disk `sync_all()` or OS keychain I/O ever blocks the GPUI main thread.
//! 2. Concurrent writes from settings saves, UI preference updates, tracker
//!    refreshes, and profile switches are strictly serialized (no lost updates).
//! 3. Multi-step engine + credential + disk transactions use a single Saga
//!    rollback path instead of duplicated failure pyramids.

use std::{collections::HashMap, sync::Arc};

use ariadeck_application::{DownloadDestinationGateway, SyncHandle, TaskListQuery};
use ariadeck_domain::{ConnectionState, EngineSession, ProfileId};
use ariadeck_engine::{JsonProfileStore, ProfileCatalog};
use ariadeck_settings::{
    AppSettings, JsonSettingsStore, JsonWindowGeometryStore, ProfileEnvironment,
    ProfileEnvironmentStore, ProxyCredentialStore, TrackerListSettings, WindowGeometry,
};
use ariadeck_ui::{ProfileCatalogView, ProfileRpcSecretUpdateView, RequestId};
use secrecy::SecretString;
use tokio::{runtime::Runtime, sync::mpsc, task::JoinHandle};

use super::{
    SettingsPersistenceRequest, SettingsPersistenceResult,
    engine_setup::{
        apply_profile_secret_updates, cleanup_removed_profile_environments,
        cleanup_removed_profile_secrets,
    },
    settings_bridge::{
        CredentialMutation, apply_credential_update, load_proxy_password, map_bt_tracker_list,
        map_download_proxy_config, map_download_proxy_config_or_clear, map_speed_limit_config,
        map_transfer_policy_config, preflight_settings, rollback_credential_async,
    },
    tracker_list::TrackerListRefreshResult,
};

/// Serialized persistence commands handled off the GPUI UI thread.
pub(crate) enum PersistenceCommand {
    SaveSettings(Box<SettingsPersistenceRequest>),
    CommitTrackerRefresh(TrackerRefreshCommit),
    SwitchProfile(Box<ProfileSwitchRequest>),
    SaveProfileCatalog(Box<ProfileCatalogSaveJob>),
    SaveWindowGeometry(WindowGeometry),
}

pub(crate) struct TrackerRefreshCommit {
    pub(crate) request_id: RequestId,
    pub(crate) auto: bool,
    pub(crate) draft: Option<TrackerListSettings>,
    pub(crate) next_list: TrackerListSettings,
    pub(crate) tracker_count: usize,
}

pub(crate) struct ProfileSwitchRequest {
    pub(crate) request_id: RequestId,
    pub(crate) profile_id: ProfileId,
    pub(crate) profile_id_raw: String,
    pub(crate) current_catalog: ProfileCatalog,
    pub(crate) current_settings: AppSettings,
}

pub(crate) struct ProfileSwitchResult {
    pub(crate) request_id: RequestId,
    pub(crate) profile_id_raw: String,
    pub(crate) catalog: ProfileCatalog,
    pub(crate) next_settings: Option<AppSettings>,
    pub(crate) result: Result<(), String>,
}

pub(crate) struct ProfileCatalogSaveJob {
    pub(crate) request_id: RequestId,
    pub(crate) request_catalog: ProfileCatalogView,
    pub(crate) next_catalog: ProfileCatalog,
    pub(crate) secret_updates: HashMap<ProfileId, ProfileRpcSecretUpdateView>,
    pub(crate) previous_catalog: ProfileCatalog,
    pub(crate) current_settings: AppSettings,
}

pub(crate) struct ProfileCatalogSaveResult {
    pub(crate) request_id: RequestId,
    pub(crate) request_catalog: ProfileCatalogView,
    pub(crate) catalog: Option<ProfileCatalog>,
    pub(crate) result: Result<(), String>,
}

/// Outcomes streamed back to `DesktopRoot` on the GPUI main thread.
pub(crate) enum PersistenceEvent {
    SettingsSaved(SettingsPersistenceResult),
    TrackerListRefreshed(TrackerListRefreshResult),
    ProfileSwitched(ProfileSwitchResult),
    ProfileCatalogSaved(ProfileCatalogSaveResult),
    WindowGeometrySaved(WindowGeometry),
}

pub(crate) struct PersistenceWorkerDeps {
    pub(crate) settings_store: Option<JsonSettingsStore>,
    pub(crate) profile_store: JsonProfileStore,
    pub(crate) profile_env_store: ProfileEnvironmentStore,
    pub(crate) window_geometry_store: JsonWindowGeometryStore,
    pub(crate) destination_gateway: Option<Arc<dyn DownloadDestinationGateway>>,
    pub(crate) sync: Option<SyncHandle>,
    pub(crate) credential_store: Arc<dyn ProxyCredentialStore>,
    pub(crate) initial_active_profile_id: Option<ProfileId>,
}

pub(crate) fn spawn_persistence_worker(
    runtime: Arc<Runtime>,
    deps: PersistenceWorkerDeps,
) -> (
    mpsc::UnboundedSender<PersistenceCommand>,
    JoinHandle<()>,
    mpsc::UnboundedReceiver<PersistenceEvent>,
) {
    let (commands, mut command_rx) = mpsc::unbounded_channel::<PersistenceCommand>();
    let (events, event_rx) = mpsc::unbounded_channel::<PersistenceEvent>();
    let task = runtime.spawn(async move {
        let mut active_profile_id = deps.initial_active_profile_id;
        while let Some(command) = command_rx.recv().await {
            match command {
                PersistenceCommand::SaveSettings(request) => {
                    let request = *request;
                    let result = match deps.settings_store.as_ref() {
                        Some(store) => {
                            let mut request_with_profile = request.clone();
                            if request_with_profile.active_profile_id.is_none() {
                                request_with_profile.active_profile_id = active_profile_id;
                            }
                            persist_settings_request(
                                store.clone(),
                                deps.profile_env_store.clone(),
                                deps.destination_gateway.clone(),
                                deps.sync.clone(),
                                deps.credential_store.clone(),
                                request_with_profile,
                            )
                            .await
                        }
                        None => Err("Settings persistence is unavailable for this session.".into()),
                    };
                    let _ =
                        events.send(PersistenceEvent::SettingsSaved(SettingsPersistenceResult {
                            request_id: request.request_id,
                            settings: request.settings,
                            result,
                        }));
                }
                PersistenceCommand::CommitTrackerRefresh(commit) => {
                    let outcome = commit_tracker_refresh(
                        deps.settings_store.as_ref(),
                        &deps.profile_env_store,
                        active_profile_id,
                        deps.sync.as_ref(),
                        commit,
                    )
                    .await;
                    let _ = events.send(PersistenceEvent::TrackerListRefreshed(outcome));
                }
                PersistenceCommand::SwitchProfile(request) => {
                    let outcome = execute_profile_switch(
                        deps.settings_store.clone(),
                        deps.profile_store.clone(),
                        deps.profile_env_store.clone(),
                        *request,
                    )
                    .await;
                    if outcome.result.is_ok() {
                        active_profile_id = Some(outcome.catalog.active_profile_id);
                    }
                    let _ = events.send(PersistenceEvent::ProfileSwitched(outcome));
                }
                PersistenceCommand::SaveProfileCatalog(job) => {
                    let outcome = execute_profile_catalog_save(
                        deps.profile_store.clone(),
                        deps.profile_env_store.clone(),
                        *job,
                    )
                    .await;
                    if outcome.result.is_ok()
                        && let Some(catalog) = outcome.catalog.as_ref()
                    {
                        active_profile_id = Some(catalog.active_profile_id);
                    }
                    let _ = events.send(PersistenceEvent::ProfileCatalogSaved(outcome));
                }
                PersistenceCommand::SaveWindowGeometry(geometry) => {
                    let store = deps.window_geometry_store.clone();
                    match tokio::task::spawn_blocking(move || store.save(geometry)).await {
                        Ok(Ok(())) => {
                            let _ = events.send(PersistenceEvent::WindowGeometrySaved(geometry));
                        }
                        Ok(Err(error)) => {
                            tracing::debug!(%error, "failed to persist window geometry");
                        }
                        Err(error) => {
                            tracing::debug!(%error, "window geometry persistence task failed");
                        }
                    }
                }
            }
        }
    });
    (commands, task, event_rx)
}

#[derive(Default)]
struct AppliedEngineChanges {
    credential: Option<CredentialMutation>,
    proxy: bool,
    speed_limit: bool,
    transfer_policy: bool,
    bt_tracker: bool,
}

async fn rollback_applied(
    sync: Option<&SyncHandle>,
    session: Option<EngineSession>,
    previous_settings: &AppSettings,
    previous_password: Option<SecretString>,
    credential_store: &Arc<dyn ProxyCredentialStore>,
    applied: AppliedEngineChanges,
    mut summary: String,
) -> String {
    if let (Some(sync), Some(session)) = (sync, session) {
        if applied.bt_tracker
            && let Err(error) = sync
                .apply_bt_tracker(session, map_bt_tracker_list(previous_settings))
                .await
        {
            summary.push_str(&format!(
                " Tracker-list rollback also failed: {}",
                error.summary
            ));
        }
        if applied.transfer_policy
            && let Err(error) = sync
                .apply_transfer_policy(session, map_transfer_policy_config(previous_settings))
                .await
        {
            summary.push_str(&format!(
                " Transfer-policy rollback also failed: {}",
                error.summary
            ));
        }
        if applied.speed_limit
            && let Err(error) = sync
                .apply_speed_limit(session, map_speed_limit_config(previous_settings))
                .await
        {
            summary.push_str(&format!(
                " Speed-limit rollback also failed: {}",
                error.summary
            ));
        }
        if applied.proxy {
            let prev_settings = previous_settings.clone();
            let rollback_proxy = tokio::task::spawn_blocking(move || {
                map_download_proxy_config_or_clear(&prev_settings, previous_password)
            })
            .await
            .unwrap_or_default();
            if let Err(error) = sync.apply_download_proxy(session, rollback_proxy).await {
                summary.push_str(&format!(" Proxy rollback also failed: {}", error.summary));
            }
        }
    }
    if let Some(mutation) = applied.credential
        && let Err(error) = rollback_credential_async(Arc::clone(credential_store), mutation).await
    {
        summary.push_str(&format!(" Credential rollback also failed: {error}"));
    }
    summary
}

pub(crate) async fn persist_settings_request(
    store: JsonSettingsStore,
    profile_env_store: ProfileEnvironmentStore,
    destination_gateway: Option<Arc<dyn DownloadDestinationGateway>>,
    sync: Option<SyncHandle>,
    credential_store: Arc<dyn ProxyCredentialStore>,
    request: SettingsPersistenceRequest,
) -> Result<(), String> {
    let settings_for_preflight = request.settings.clone();
    tokio::task::spawn_blocking(move || {
        preflight_settings(&settings_for_preflight, destination_gateway.as_deref())
    })
    .await
    .map_err(|error| format!("settings preflight task failed: {error}"))??;

    let any_engine_change = request.apply_proxy
        || request.apply_speed_limit
        || request.apply_transfer_policy
        || request.apply_bt_tracker;

    if !any_engine_change {
        let settings = request.settings;
        let profile_id = request.active_profile_id;
        return tokio::task::spawn_blocking(move || {
            save_settings_and_profile_env(&store, &profile_env_store, profile_id, &settings)
        })
        .await
        .map_err(|error| format!("settings persistence task failed: {error}"))?;
    }

    let mut applied = AppliedEngineChanges::default();
    let mut previous_password = None;
    let mut next_password = None;

    if request.apply_proxy {
        let previous_settings = request.previous_settings.clone();
        let next_settings = request.settings.clone();
        let password_update = request.proxy_password.clone();
        let credentials = Arc::clone(&credential_store);
        let (prev_pw, pw, mutation) = tokio::task::spawn_blocking(move || {
            let prev_pw = load_proxy_password(credentials.as_ref(), &previous_settings)?;
            let (pw, mutation) = apply_credential_update(
                credentials.as_ref(),
                &previous_settings,
                &next_settings,
                &password_update,
                prev_pw.clone(),
            )?;
            Ok::<_, String>((prev_pw, pw, mutation))
        })
        .await
        .map_err(|error| format!("credential update task failed: {error}"))??;
        previous_password = prev_pw;
        next_password = pw;
        applied.credential = Some(mutation);
    }

    let unavailable_subject = if request.apply_proxy {
        "Download proxy settings"
    } else {
        "Engine transfer settings"
    };

    let Some(sync) = sync else {
        return Err(rollback_applied(
            None,
            None,
            &request.previous_settings,
            previous_password,
            &credential_store,
            applied,
            format!("{unavailable_subject} cannot be applied because aria2 is unavailable."),
        )
        .await);
    };

    let Some(snapshot) = sync.snapshot(TaskListQuery::default()).await else {
        return Err(rollback_applied(
            Some(&sync),
            None,
            &request.previous_settings,
            previous_password,
            &credential_store,
            applied,
            format!(
                "{unavailable_subject} cannot be applied because the synchronization coordinator is unavailable."
            ),
        )
        .await);
    };
    let session = snapshot.session;

    if request.apply_proxy {
        let proxy_settings = request.settings.clone();
        let next_proxy_res = tokio::task::spawn_blocking(move || {
            map_download_proxy_config(&proxy_settings, next_password)
        })
        .await
        .map_err(|error| format!("proxy resolution task failed: {error}"))
        .and_then(|res| res);
        let next_proxy = match next_proxy_res {
            Ok(proxy) => proxy,
            Err(error) => {
                return Err(rollback_applied(
                    Some(&sync),
                    Some(session),
                    &request.previous_settings,
                    previous_password,
                    &credential_store,
                    applied,
                    error,
                )
                .await);
            }
        };
        if let Err(error) = sync.apply_download_proxy(session, next_proxy).await {
            return Err(rollback_applied(
                Some(&sync),
                Some(session),
                &request.previous_settings,
                previous_password,
                &credential_store,
                applied,
                error.summary,
            )
            .await);
        }
        applied.proxy = true;
    }

    if request.apply_speed_limit {
        if let Err(error) = sync
            .apply_speed_limit(session, map_speed_limit_config(&request.settings))
            .await
        {
            return Err(rollback_applied(
                Some(&sync),
                Some(session),
                &request.previous_settings,
                previous_password,
                &credential_store,
                applied,
                error.summary,
            )
            .await);
        }
        applied.speed_limit = true;
    }

    if request.apply_transfer_policy {
        if let Err(error) = sync
            .apply_transfer_policy(session, map_transfer_policy_config(&request.settings))
            .await
        {
            return Err(rollback_applied(
                Some(&sync),
                Some(session),
                &request.previous_settings,
                previous_password,
                &credential_store,
                applied,
                error.summary,
            )
            .await);
        }
        applied.transfer_policy = true;
    }

    if request.apply_bt_tracker {
        if let Err(error) = sync
            .apply_bt_tracker(session, map_bt_tracker_list(&request.settings))
            .await
        {
            return Err(rollback_applied(
                Some(&sync),
                Some(session),
                &request.previous_settings,
                previous_password,
                &credential_store,
                applied,
                error.summary,
            )
            .await);
        }
        applied.bt_tracker = true;
    }

    let settings_to_save = request.settings.clone();
    let save_store = store.clone();
    let env_store = profile_env_store.clone();
    let profile_id = request.active_profile_id;
    let save_result = tokio::task::spawn_blocking(move || {
        save_settings_and_profile_env(&save_store, &env_store, profile_id, &settings_to_save)
    })
    .await
    .map_err(|error| format!("settings persistence task failed: {error}"))?;

    if let Err(error) = save_result {
        let prefix = if request.apply_proxy {
            "Failed to persist proxy settings"
        } else {
            "Failed to persist transfer settings"
        };
        return Err(rollback_applied(
            Some(&sync),
            Some(session),
            &request.previous_settings,
            previous_password,
            &credential_store,
            applied,
            format!("{prefix}: {error}"),
        )
        .await);
    }

    Ok(())
}

pub(crate) fn save_settings_and_profile_env(
    store: &JsonSettingsStore,
    profile_env_store: &ProfileEnvironmentStore,
    profile_id: Option<ProfileId>,
    settings: &AppSettings,
) -> Result<(), String> {
    store.save(settings).map_err(|error| error.to_string())?;
    if let Some(profile_id) = profile_id {
        let env = ProfileEnvironment::from_settings(settings);
        profile_env_store
            .save(profile_id.as_uuid(), &env)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Serialize tracker-list persistence with all other settings writes and
/// dual-write both `settings.json` and the active profile's `environment.json`
/// (D-043) so refreshes survive restarts and never clobber concurrent edits.
pub(crate) async fn commit_tracker_refresh(
    settings_store: Option<&JsonSettingsStore>,
    profile_env_store: &ProfileEnvironmentStore,
    active_profile_id: Option<ProfileId>,
    sync: Option<&SyncHandle>,
    commit: TrackerRefreshCommit,
) -> TrackerListRefreshResult {
    let Some(store) = settings_store.cloned() else {
        return TrackerListRefreshResult {
            request_id: commit.request_id,
            settings: None,
            tracker_count: commit.tracker_count,
            result: Err("Settings persistence is unavailable for this session.".into()),
            auto: commit.auto,
        };
    };
    let env_store = profile_env_store.clone();
    let draft = commit.draft;
    let next_list = commit.next_list;

    let saved = tokio::task::spawn_blocking(move || -> Result<AppSettings, String> {
        let mut settings = store.load().map_err(|error| error.to_string())?;
        if let Some(draft) = draft {
            settings.tracker_list.enabled = draft.enabled;
            settings.tracker_list.source = draft.source;
            settings.tracker_list.custom_url = draft.custom_url;
            settings.tracker_list.auto_refresh = draft.auto_refresh;
        }
        settings.tracker_list.list_text = next_list.list_text;
        settings.tracker_list.last_refreshed_at = next_list.last_refreshed_at;
        save_settings_and_profile_env(&store, &env_store, active_profile_id, &settings)?;
        Ok(settings)
    })
    .await
    .map_err(|error| format!("tracker settings persistence task failed: {error}"))
    .and_then(|result| result);

    let settings = match saved {
        Ok(settings) => settings,
        Err(error) => {
            return TrackerListRefreshResult {
                request_id: commit.request_id,
                settings: None,
                tracker_count: commit.tracker_count,
                result: Err(error),
                auto: commit.auto,
            };
        }
    };

    if let Some(sync) = sync
        && let Some(snapshot) = sync.snapshot(TaskListQuery::default()).await
        && matches!(snapshot.connection_state, ConnectionState::Connected)
        && let Err(error) = sync
            .apply_bt_tracker(snapshot.session, map_bt_tracker_list(&settings))
            .await
    {
        return TrackerListRefreshResult {
            request_id: commit.request_id,
            settings: Some(settings),
            tracker_count: commit.tracker_count,
            result: Err(format!(
                "Tracker list was saved but not applied to aria2: {}",
                error.summary
            )),
            auto: commit.auto,
        };
    }

    TrackerListRefreshResult {
        request_id: commit.request_id,
        settings: Some(settings),
        tracker_count: commit.tracker_count,
        result: Ok(()),
        auto: commit.auto,
    }
}

pub(crate) async fn execute_profile_switch(
    settings_store: Option<JsonSettingsStore>,
    profile_store: JsonProfileStore,
    profile_env_store: ProfileEnvironmentStore,
    request: ProfileSwitchRequest,
) -> ProfileSwitchResult {
    let request_id = request.request_id;
    let profile_id_raw = request.profile_id_raw.clone();
    let fallback_catalog = request.current_catalog.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        switch_profile_blocking(
            settings_store.as_ref(),
            &profile_store,
            &profile_env_store,
            request,
        )
    })
    .await;

    match outcome {
        Ok(result) => result,
        Err(error) => ProfileSwitchResult {
            request_id,
            profile_id_raw,
            catalog: fallback_catalog,
            next_settings: None,
            result: Err(format!("Profile switch worker failed: {error}")),
        },
    }
}

fn switch_profile_blocking(
    settings_store: Option<&JsonSettingsStore>,
    profile_store: &JsonProfileStore,
    profile_env_store: &ProfileEnvironmentStore,
    request: ProfileSwitchRequest,
) -> ProfileSwitchResult {
    let mut catalog = request.current_catalog;
    let previous_id = catalog.active_profile_id;
    let previous_settings = request.current_settings;

    if previous_id != request.profile_id {
        let env = ProfileEnvironment::from_settings(&previous_settings);
        if let Err(error) = profile_env_store.save(previous_id.as_uuid(), &env) {
            return ProfileSwitchResult {
                request_id: request.request_id,
                profile_id_raw: request.profile_id_raw,
                catalog,
                next_settings: None,
                result: Err(format!(
                    "Failed to save environment for the current profile: {error}"
                )),
            };
        }
    }

    if let Err(error) = catalog.set_active(request.profile_id) {
        return ProfileSwitchResult {
            request_id: request.request_id,
            profile_id_raw: request.profile_id_raw,
            catalog,
            next_settings: None,
            result: Err(error.to_string()),
        };
    }

    let mut next_settings = previous_settings.clone();
    match profile_env_store.load_or_initialize(request.profile_id.as_uuid(), &next_settings) {
        Ok(env) => env.apply_to(&mut next_settings),
        Err(error) => {
            let _ = catalog.set_active(previous_id);
            return ProfileSwitchResult {
                request_id: request.request_id,
                profile_id_raw: request.profile_id_raw,
                catalog,
                next_settings: None,
                result: Err(format!(
                    "Failed to load environment for the selected profile: {error}"
                )),
            };
        }
    }

    if let Some(entry) = catalog.active_mut() {
        entry.download_dir = next_settings.download_directory.clone();
    }

    if let Err(error) = profile_store.save_catalog(&catalog) {
        let _ = catalog.set_active(previous_id);
        return ProfileSwitchResult {
            request_id: request.request_id,
            profile_id_raw: request.profile_id_raw,
            catalog,
            next_settings: None,
            result: Err(format!("Failed to save active profile: {error}")),
        };
    }

    // Dual-write: settings.json env slice mirrors the active bag (D-043).
    if let Some(store) = settings_store
        && let Err(error) = store.save(&next_settings)
    {
        let _ = catalog.set_active(previous_id);
        let _ = profile_store.save_catalog(&catalog);
        return ProfileSwitchResult {
            request_id: request.request_id,
            profile_id_raw: request.profile_id_raw,
            catalog,
            next_settings: None,
            result: Err(format!(
                "Failed to mirror profile environment into settings: {error}"
            )),
        };
    }

    // Ensure the activated bag is persisted even if it was just seeded.
    let env = ProfileEnvironment::from_settings(&next_settings);
    if let Err(error) = profile_env_store.save(request.profile_id.as_uuid(), &env) {
        let _ = catalog.set_active(previous_id);
        let _ = profile_store.save_catalog(&catalog);
        if let Some(store) = settings_store {
            let _ = store.save(&previous_settings);
        }
        return ProfileSwitchResult {
            request_id: request.request_id,
            profile_id_raw: request.profile_id_raw,
            catalog,
            next_settings: None,
            result: Err(format!(
                "Failed to save environment for the selected profile: {error}"
            )),
        };
    }

    ProfileSwitchResult {
        request_id: request.request_id,
        profile_id_raw: request.profile_id_raw,
        catalog,
        next_settings: Some(next_settings),
        result: Ok(()),
    }
}

async fn execute_profile_catalog_save(
    profile_store: JsonProfileStore,
    profile_env_store: ProfileEnvironmentStore,
    job: ProfileCatalogSaveJob,
) -> ProfileCatalogSaveResult {
    let request_id = job.request_id;
    let request_catalog = job.request_catalog.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        if let Err(error) = apply_profile_secret_updates(
            &job.next_catalog,
            &job.secret_updates,
            &job.previous_catalog,
        ) {
            return ProfileCatalogSaveResult {
                request_id: job.request_id,
                request_catalog: job.request_catalog,
                catalog: None,
                result: Err(error),
            };
        }
        if let Err(error) = profile_store.save_catalog(&job.next_catalog) {
            return ProfileCatalogSaveResult {
                request_id: job.request_id,
                request_catalog: job.request_catalog,
                catalog: None,
                result: Err(format!("Failed to save profiles: {error}")),
            };
        }
        cleanup_removed_profile_secrets(&job.previous_catalog, &job.next_catalog);
        cleanup_removed_profile_environments(
            &profile_env_store,
            &job.previous_catalog,
            &job.next_catalog,
        );
        for entry in &job.next_catalog.profiles {
            let _ = profile_env_store
                .load_or_initialize(entry.profile_id.as_uuid(), &job.current_settings);
        }
        ProfileCatalogSaveResult {
            request_id: job.request_id,
            request_catalog: job.request_catalog,
            catalog: Some(job.next_catalog),
            result: Ok(()),
        }
    })
    .await;

    match outcome {
        Ok(result) => result,
        Err(error) => ProfileCatalogSaveResult {
            request_id,
            request_catalog,
            catalog: None,
            result: Err(format!("Profile catalog save worker failed: {error}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use ariadeck_engine::{PROFILE_CATALOG_SCHEMA_VERSION, ProfileEntry};
    use ariadeck_settings::{
        ColorScheme, DownloadProxyMode, ProxyCredentialRef, TrackerListSource,
    };

    use super::*;
    use crate::workspace::{
        ProxyPasswordUpdate,
        test_support::{
            MemoryProxyCredentialStore as MemoryCredentialStore, RecordingGateway, connected_sync,
        },
    };

    #[tokio::test]
    async fn saga_rolls_back_all_previous_engine_steps_and_credential_on_tracker_failure() {
        let root = tempfile::tempdir().expect("tempdir");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        let env_store = ProfileEnvironmentStore::new(root.path());
        let profile_id = ProfileId::new();
        let previous = AppSettings::new(root.path().join("downloads"));
        store.save(&previous).expect("seed settings");

        let mut next = previous.clone();
        let cred_ref = ProxyCredentialRef::new();
        next.download_proxy.mode = DownloadProxyMode::Manual;
        next.download_proxy.all_proxy = Some("http://proxy.example:8080".into());
        next.download_proxy.username = Some("alice".into());
        next.download_proxy.credential = Some(cred_ref);
        next.speed_limits.download_limit = 500_000;
        next.transfer_policy.max_concurrent_downloads = 8;
        next.tracker_list.enabled = true;
        next.tracker_list.list_text = "https://tracker.example/announce".into();

        let gateway = Arc::new(RecordingGateway {
            fail_on: Mutex::new(Some("tracker")),
            calls: Mutex::new(Vec::new()),
        });
        let sync = connected_sync(gateway.clone(), profile_id).await;
        let creds = Arc::new(MemoryCredentialStore::default());

        let err = persist_settings_request(
            store.clone(),
            env_store,
            None,
            Some(sync.clone()),
            creds.clone(),
            SettingsPersistenceRequest {
                request_id: RequestId::from_u64(1),
                settings: next,
                previous_settings: previous.clone(),
                proxy_password: ProxyPasswordUpdate::Set(SecretString::new("s3cr3t".into())),
                apply_proxy: true,
                apply_speed_limit: true,
                apply_transfer_policy: true,
                apply_bt_tracker: true,
                active_profile_id: Some(profile_id),
            },
        )
        .await
        .expect_err("tracker step must fail and trigger saga rollback");
        assert!(err.contains("tracker boom"), "unexpected error: {err}");

        // Forward: proxy -> speed -> policy -> tracker (fails)
        // Reverse rollback: policy -> speed -> proxy, plus credential deleted.
        let calls = gateway.calls.lock().expect("lock").clone();
        assert_eq!(
            calls,
            vec![
                "proxy:Manual",
                "speed:500000",
                "policy:8",
                "tracker:1",
                "policy:5",
                "speed:0",
                "proxy:Disabled",
            ]
        );
        assert!(creds.load(cred_ref).expect("cred load").is_none());
        assert_eq!(store.load().expect("settings unchanged"), previous);
        sync.stop().await;
    }

    #[tokio::test]
    async fn tracker_refresh_commit_dual_writes_profile_env_and_preserves_other_settings() {
        let root = tempfile::tempdir().expect("tempdir");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        let env_store = ProfileEnvironmentStore::new(root.path());
        let profile_id = ProfileId::new();
        let mut settings = AppSettings::new(root.path().join("downloads"));
        // Simulate a concurrent user edit to color_scheme committed before tracker finishes.
        settings.color_scheme = ColorScheme::Dark;
        store.save(&settings).expect("seed settings");

        let refreshed = TrackerListSettings {
            enabled: true,
            source: TrackerListSource::Curated,
            custom_url: None,
            auto_refresh: true,
            last_refreshed_at: Some(1_700_000_000),
            list_text: "https://a.example/announce\nudp://b.example:80/announce".into(),
        };

        let result = commit_tracker_refresh(
            Some(&store),
            &env_store,
            Some(profile_id),
            None,
            TrackerRefreshCommit {
                request_id: RequestId::from_u64(7),
                auto: false,
                draft: Some(refreshed.clone()),
                next_list: refreshed.clone(),
                tracker_count: 2,
            },
        )
        .await;
        assert!(result.result.is_ok(), "commit failed: {:?}", result.result);

        let on_disk = store.load().expect("reload settings.json");
        assert_eq!(on_disk.color_scheme, ColorScheme::Dark);
        assert_eq!(on_disk.tracker_list.last_refreshed_at, Some(1_700_000_000));
        assert_eq!(on_disk.tracker_list.list_text, refreshed.list_text);

        // D-043: active profile environment.json must also have the refreshed list.
        let env = env_store
            .load(profile_id.as_uuid())
            .expect("reload environment.json");
        assert_eq!(env.tracker_list.last_refreshed_at, Some(1_700_000_000));
        assert_eq!(env.tracker_list.list_text, refreshed.list_text);
    }

    #[test]
    fn profile_switch_rolls_back_settings_json_when_target_env_save_fails() {
        let root = tempfile::tempdir().expect("tempdir");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        let profile_store = JsonProfileStore::new(root.path().join("profiles.json"));
        let env_store = ProfileEnvironmentStore::new(root.path());

        let profile_a = ProfileEntry::local_managed(
            ProfileId::new(),
            "Local A",
            std::path::PathBuf::from("aria2c"),
            root.path().to_path_buf(),
            root.path().join("dl-a"),
        );
        let profile_b = ProfileEntry::local_managed(
            ProfileId::new(),
            "Local B",
            std::path::PathBuf::from("aria2c"),
            root.path().to_path_buf(),
            root.path().join("dl-b"),
        );
        let catalog = ProfileCatalog {
            schema_version: PROFILE_CATALOG_SCHEMA_VERSION,
            active_profile_id: profile_a.profile_id,
            profiles: vec![profile_a.clone(), profile_b.clone()],
        };
        profile_store.save_catalog(&catalog).expect("save catalog");

        let mut settings_a = AppSettings::new(root.path().join("dl-a"));
        settings_a.speed_limits.download_limit = 1_111;
        store.save(&settings_a).expect("save settings_a");
        env_store
            .save(
                profile_a.profile_id.as_uuid(),
                &ProfileEnvironment::from_settings(&settings_a),
            )
            .expect("save env a");

        // Inject a real failure when persisting target profile B's environment:
        // place a directory at `profiles/<b_id>/environment.json` so atomic
        // replace fails after `profiles.json` and `settings.json` were updated,
        // then verify both stores roll back to profile A.
        let target_env_path = env_store.path_for(profile_b.profile_id.as_uuid());
        std::fs::create_dir_all(&target_env_path).expect("block target env path with directory");

        let switched = switch_profile_blocking(
            Some(&store),
            &profile_store,
            &env_store,
            ProfileSwitchRequest {
                request_id: RequestId::from_u64(1),
                profile_id: profile_b.profile_id,
                profile_id_raw: profile_b.profile_id.to_string(),
                current_catalog: catalog,
                current_settings: settings_a,
            },
        );
        assert!(
            switched.result.is_err(),
            "switch must fail when target environment cannot be persisted"
        );
        assert_eq!(
            switched.catalog.active_profile_id, profile_a.profile_id,
            "returned catalog must remain on profile A"
        );
        let reloaded_catalog: ProfileCatalog = serde_json::from_slice(
            &std::fs::read(root.path().join("profiles.json")).expect("read profiles.json"),
        )
        .expect("parse reloaded catalog");
        assert_eq!(
            reloaded_catalog.active_profile_id, profile_a.profile_id,
            "persisted profiles.json must roll back to profile A"
        );
        assert_eq!(
            store
                .load()
                .expect("reloaded settings")
                .speed_limits
                .download_limit,
            1_111,
            "persisted settings.json must roll back to profile A"
        );
    }
}
