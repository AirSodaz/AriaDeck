//! Engine setup, RPC runtime configuration, local aria2 supervision, and
//! profile secret management.

use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use ariadeck_application::{
    CoordinatorConfig, ReconnectPolicy, SyncHandle, spawn_sync_coordinator,
};
use ariadeck_domain::ProfileId;
use ariadeck_engine::{
    CoreStore, LocalEngineSupervisor, ProfileCatalog, ProfileEntry, ProfileKind,
};
use ariadeck_history::SqliteHistoryStore;
use ariadeck_rpc::{
    Aria2Client, AuthenticatedTransport, RpcSecret, RpcSyncConnector, WebSocketConfig,
    WebSocketTransport,
};
use ariadeck_settings::{
    AppSettings, ProfileEnvironmentStore, ProxyCredentialRef, ProxyCredentialStore,
    SystemProxyCredentialStore,
};
use ariadeck_ui::ProfileRpcSecretUpdateView;
use secrecy::SecretString;
use tokio::runtime::Runtime;
use url::Url;

#[derive(Debug)]
pub(crate) struct RpcRuntimeConfig {
    pub(crate) connect_timeout: Duration,
    pub(crate) request_timeout: Duration,
    pub(crate) reconnect: ReconnectPolicy,
    pub(crate) allow_insecure_remote: bool,
}

impl RpcRuntimeConfig {
    pub(crate) fn from_values(
        external: bool,
        mut value: impl FnMut(&str) -> Option<String>,
    ) -> Result<Self, String> {
        let defaults = ReconnectPolicy::default();
        let connect_timeout = parse_millisecond_setting(
            "ARIADECK_RPC_CONNECT_TIMEOUT_MS",
            value("ARIADECK_RPC_CONNECT_TIMEOUT_MS"),
            if external {
                Duration::from_secs(10)
            } else {
                Duration::from_millis(750)
            },
        )?;
        let request_timeout = parse_millisecond_setting(
            "ARIADECK_RPC_REQUEST_TIMEOUT_MS",
            value("ARIADECK_RPC_REQUEST_TIMEOUT_MS"),
            if external {
                Duration::from_secs(15)
            } else {
                Duration::from_secs(5)
            },
        )?;
        let base_delay = parse_millisecond_setting(
            "ARIADECK_RPC_RECONNECT_BASE_DELAY_MS",
            value("ARIADECK_RPC_RECONNECT_BASE_DELAY_MS"),
            defaults.base_delay,
        )?;
        let max_delay = parse_millisecond_setting(
            "ARIADECK_RPC_RECONNECT_MAX_DELAY_MS",
            value("ARIADECK_RPC_RECONNECT_MAX_DELAY_MS"),
            defaults.max_delay,
        )?;
        if base_delay > max_delay {
            return Err(
                "ARIADECK_RPC_RECONNECT_BASE_DELAY_MS must not exceed ARIADECK_RPC_RECONNECT_MAX_DELAY_MS."
                    .into(),
            );
        }
        let reset_after = parse_millisecond_setting(
            "ARIADECK_RPC_RECONNECT_RESET_AFTER_MS",
            value("ARIADECK_RPC_RECONNECT_RESET_AFTER_MS"),
            defaults.reset_after,
        )?;
        let max_attempts = parse_max_attempts(value("ARIADECK_RPC_RECONNECT_MAX_ATTEMPTS"))?;
        let allow_insecure_remote = parse_boolean_setting(
            "ARIADECK_RPC_ALLOW_INSECURE_REMOTE",
            value("ARIADECK_RPC_ALLOW_INSECURE_REMOTE"),
            false,
        )?;
        Ok(Self {
            connect_timeout,
            request_timeout,
            reconnect: ReconnectPolicy {
                base_delay,
                max_delay,
                jitter_percent: defaults.jitter_percent,
                max_attempts,
                reset_after,
            },
            allow_insecure_remote,
        })
    }
}

pub(crate) fn parse_millisecond_setting(
    name: &'static str,
    value: Option<String>,
    default: Duration,
) -> Result<Duration, String> {
    let Some(value) = value else {
        return Ok(default);
    };
    let milliseconds = value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be an integer number of milliseconds."))?;
    if !(1..=3_600_000).contains(&milliseconds) {
        return Err(format!(
            "{name} must be between 1 and 3600000 milliseconds."
        ));
    }
    Ok(Duration::from_millis(milliseconds))
}

pub(crate) fn parse_max_attempts(value: Option<String>) -> Result<Option<u32>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let attempts = value.parse::<u32>().map_err(|_| {
        "ARIADECK_RPC_RECONNECT_MAX_ATTEMPTS must be a positive integer.".to_owned()
    })?;
    if attempts == 0 {
        return Err("ARIADECK_RPC_RECONNECT_MAX_ATTEMPTS must be at least 1.".into());
    }
    Ok(Some(attempts))
}

pub(crate) fn parse_boolean_setting(
    name: &'static str,
    value: Option<String>,
    default: bool,
) -> Result<bool, String> {
    let Some(value) = value else {
        return Ok(default);
    };
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(format!("{name} must be true, false, 1, or 0.")),
    }
}

pub(crate) fn load_profile_rpc_secret(entry: &ProfileEntry) -> Result<Option<RpcSecret>, String> {
    let Some(secret_ref) = entry.secret_ref else {
        return Ok(None);
    };
    let store = SystemProxyCredentialStore::new("AriaDeck rpc secret");
    let credential = ProxyCredentialRef::from_uuid(secret_ref.as_uuid());
    match store.load(credential) {
        Ok(Some(password)) => {
            use secrecy::ExposeSecret as _;
            Ok(Some(RpcSecret::new(password.expose_secret().to_owned())))
        }
        Ok(None) => Ok(None),
        Err(error) => Err(format!("Failed to load remote RPC secret: {error}")),
    }
}

pub(crate) fn rpc_secret_store() -> SystemProxyCredentialStore {
    SystemProxyCredentialStore::new("AriaDeck rpc secret")
}

/// Persist Set/Clear secret mutations against the keyring, keyed strictly by `ProfileId`.
pub(crate) fn apply_profile_secret_updates(
    catalog: &ProfileCatalog,
    secret_updates: &HashMap<ProfileId, ProfileRpcSecretUpdateView>,
    previous: &ProfileCatalog,
) -> Result<(), String> {
    if secret_updates.is_empty() {
        return Ok(());
    }
    let store = rpc_secret_store();
    apply_profile_secret_updates_with_store(&store, catalog, secret_updates, previous)
}

pub(crate) fn apply_profile_secret_updates_with_store(
    store: &dyn ProxyCredentialStore,
    catalog: &ProfileCatalog,
    secret_updates: &HashMap<ProfileId, ProfileRpcSecretUpdateView>,
    previous: &ProfileCatalog,
) -> Result<(), String> {
    for (profile_id, update) in secret_updates {
        match update {
            ProfileRpcSecretUpdateView::Unchanged => {}
            ProfileRpcSecretUpdateView::Clear => {
                if let Some(secret_ref) = previous
                    .get(*profile_id)
                    .and_then(|profile| profile.secret_ref)
                {
                    let credential = ProxyCredentialRef::from_uuid(secret_ref.as_uuid());
                    let _ = store.delete(credential);
                }
            }
            ProfileRpcSecretUpdateView::Set(password) => {
                let profile = catalog.get(*profile_id).ok_or_else(|| {
                    format!("Remote profile {profile_id} was not found in the mapped catalog.")
                })?;
                let secret_ref = profile.secret_ref.ok_or_else(|| {
                    format!(
                        "Remote profile {} is missing a secret reference after save mapping.",
                        profile.name
                    )
                })?;
                let credential = ProxyCredentialRef::from_uuid(secret_ref.as_uuid());
                let secret = SecretString::new(password.clone().into_inner());
                store
                    .save(credential, &secret)
                    .map_err(|error| format!("Failed to store RPC secret: {error}"))?;
            }
        }
    }
    Ok(())
}

pub(crate) fn cleanup_removed_profile_environments(
    store: &ProfileEnvironmentStore,
    previous: &ProfileCatalog,
    next: &ProfileCatalog,
) {
    let remaining: std::collections::HashSet<_> = next
        .profiles
        .iter()
        .map(|profile| profile.profile_id)
        .collect();
    for profile in &previous.profiles {
        if !remaining.contains(&profile.profile_id) {
            store.remove(profile.profile_id.as_uuid());
        }
    }
}

pub(crate) fn cleanup_removed_profile_secrets(previous: &ProfileCatalog, next: &ProfileCatalog) {
    let store = rpc_secret_store();
    let remaining: std::collections::HashSet<_> = next
        .profiles
        .iter()
        .filter_map(|profile| profile.secret_ref.map(|secret_ref| secret_ref.as_uuid()))
        .collect();
    for profile in &previous.profiles {
        if let Some(secret_ref) = profile.secret_ref
            && !remaining.contains(&secret_ref.as_uuid())
        {
            let credential = ProxyCredentialRef::from_uuid(secret_ref.as_uuid());
            let _ = store.delete(credential);
        }
    }
}

pub(crate) async fn request_local_engine_shutdown(
    process: &LocalEngineSupervisor,
) -> Result<(), String> {
    let mut websocket = WebSocketConfig::new(process.endpoint().clone());
    websocket.connect_timeout = Duration::from_millis(500);
    websocket.request_timeout = Duration::from_millis(750);
    let transport = WebSocketTransport::connect(websocket)
        .await
        .map_err(|error| error.to_string())?;
    let authenticated = AuthenticatedTransport::new(
        transport.clone(),
        Some(RpcSecret::new(process.secret().to_owned())),
    );
    let client = Aria2Client::new(authenticated);
    let result = client.shutdown().await.map_err(|error| error.to_string());
    transport.close().await;
    result
}

pub(crate) fn discover_aria2_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(user_profile) = env::var_os("USERPROFILE") {
        candidates.push(PathBuf::from(user_profile).join("scoop/apps/aria2/current/aria2c.exe"));
    }
    if let Some(home) = env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join("scoop/apps/aria2/current/aria2c.exe"));
    }
    // Do not spawn `aria2c --version` during window construction. The actual
    // engine start reports a useful error, while the first-run availability
    // probe already runs after the first frame.
    candidates.into_iter().find(|candidate| candidate.is_file())
}

/// Resolve the aria2 binary for a local managed profile.
///
/// Order: `ARIADECK_ARIA2C_PATH` → non-empty profile pin → active managed core
/// → PATH/scoop discovery → bare `aria2c` name.
pub(crate) fn resolve_local_executable(
    data_dir: &Path,
    profile_executable: &Path,
) -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("ARIADECK_ARIA2C_PATH") {
        return Ok(PathBuf::from(path));
    }
    if !profile_executable.as_os_str().is_empty() {
        return Ok(profile_executable.to_path_buf());
    }
    if let Ok(Some(managed)) = CoreStore::new(data_dir).resolve_active_executable() {
        return Ok(managed);
    }
    Ok(discover_aria2_executable().unwrap_or_else(|| PathBuf::from("aria2c")))
}

/// True when a local managed profile has no aria2 it could actually start (B4).
///
/// Resolves the same way startup does, then runs `--version` on the result: a
/// bare `aria2c` fallback that is not really on `PATH` must count as missing,
/// and only executing it can tell. Callers must keep this off the render path.
#[must_use]
pub(crate) fn local_engine_executable_missing(data_dir: &Path, profile_executable: &Path) -> bool {
    resolve_local_executable(data_dir, profile_executable)
        .is_ok_and(|path| ariadeck_engine::probe_aria2(&path).is_err())
}

// Data-dir resolution lives in `ariadeck-ipc` so the native messaging host
// addresses the same instance socket (D-045 §4).
pub(crate) use ariadeck_ipc::default_data_dir;

/// Default download root for new installs (category fallback / General).
///
/// Resolution order:
/// 1. ARIADECK_DOWNLOAD_DIR when set
/// 2. System Downloads folder (USERPROFILE/Downloads on Windows, HOME/Downloads elsewhere)
/// 3. <data_dir>/downloads fallback
#[must_use]
pub(crate) fn default_download_dir() -> PathBuf {
    resolve_download_dir(|key| env::var_os(key), default_data_dir())
}

/// Testable download-dir resolver.
#[must_use]
pub(crate) fn resolve_download_dir(
    mut env_var: impl FnMut(&str) -> Option<std::ffi::OsString>,
    data_dir: PathBuf,
) -> PathBuf {
    if let Some(path) = env_var("ARIADECK_DOWNLOAD_DIR") {
        return PathBuf::from(path);
    }
    // Windows Known Folder is not required for MVP; USERPROFILE\Downloads is the
    // conventional shell Downloads location and matches Explorer for typical profiles.
    if let Some(home) = env_var("USERPROFILE").or_else(|| env_var("HOME")) {
        let downloads = PathBuf::from(home).join("Downloads");
        return downloads;
    }
    data_dir.join("downloads")
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EngineSessionRecovery {
    pub(crate) backup_path: Option<PathBuf>,
}

pub(crate) type EngineBootstrapOutput = (
    SyncHandle,
    Option<LocalEngineSupervisor>,
    Option<EngineSessionRecovery>,
);

pub(crate) fn create_sync_handle(
    runtime: &Runtime,
    data_dir: &Path,
    settings: &AppSettings,
    catalog: &ProfileCatalog,
    child_spawn_hook: Option<ariadeck_engine::ChildSpawnHook>,
) -> Result<EngineBootstrapOutput, String> {
    // One-shot env override for smoke tests still wins over the catalog.
    let external_endpoint = env::var("ARIADECK_RPC_URL")
        .ok()
        .filter(|endpoint| !endpoint.trim().is_empty());
    let active = catalog
        .active()
        .ok_or_else(|| "Profile catalog has no active profile.".to_owned())?;
    let remote_from_catalog = active.kind == ProfileKind::RemoteRpc;
    let is_remote = external_endpoint.is_some() || remote_from_catalog;
    let rpc_runtime = RpcRuntimeConfig::from_values(is_remote, |name| env::var(name).ok())?;
    let mut session_recovery = None;
    let (endpoint, secret, local_engine, profile_id) = if let Some(endpoint) = external_endpoint {
        if endpoint.trim() != endpoint {
            return Err("ARIADECK_RPC_URL must not contain surrounding whitespace.".into());
        }
        let endpoint =
            Url::parse(&endpoint).map_err(|error| format!("Invalid ARIADECK_RPC_URL: {error}"))?;
        let profile_id = env::var("ARIADECK_PROFILE_ID")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(active.profile_id);
        let secret = env::var("ARIADECK_RPC_SECRET")
            .ok()
            .filter(|secret| !secret.is_empty())
            .map(RpcSecret::new);
        (endpoint, secret, None, profile_id)
    } else if remote_from_catalog {
        let endpoint_str = active
            .endpoint
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "Active remote profile is missing an endpoint.".to_owned())?;
        let endpoint = Url::parse(endpoint_str)
            .map_err(|error| format!("Invalid remote profile endpoint: {error}"))?;
        let secret = load_profile_rpc_secret(active)?;
        (endpoint, secret, None, active.profile_id)
    } else {
        let config = active
            .as_local_config()
            .ok_or_else(|| "Active local profile is missing a data directory.".to_owned())?;
        // Prefer settings download directory for the live session.
        let mut config = config;
        config.download_dir = settings.download_directory.clone();
        if config.data_dir.as_os_str().is_empty() {
            config.data_dir = data_dir.to_path_buf();
        }
        // Executable resolution: env override → profile pin → managed core → discovery.
        config.executable = resolve_local_executable(data_dir, &config.executable)?;
        let process = LocalEngineSupervisor::spawn_with_child_hook(&config, child_spawn_hook)
            .map_err(|error| format!("Failed to start local aria2: {error}"))?;
        // Successful start: remember the active managed core as last working when present.
        let _ = CoreStore::new(data_dir).mark_active_as_last_working();
        if process.session_was_recovered() {
            let backup_path = process.session_recovery_backup();
            tracing::warn!(
                backup = ?backup_path,
                "local aria2 session file was recovered"
            );
            session_recovery = Some(EngineSessionRecovery { backup_path });
        }
        let endpoint = process.endpoint().clone();
        let secret = Some(RpcSecret::new(process.secret().to_owned()));
        (endpoint, secret, Some(process), config.profile_id)
    };

    let mut websocket = WebSocketConfig::new(endpoint.clone());
    websocket.connect_timeout = rpc_runtime.connect_timeout;
    websocket.request_timeout = rpc_runtime.request_timeout;
    websocket.allow_insecure_remote = rpc_runtime.allow_insecure_remote;
    websocket.validate().map_err(|error| error.to_string())?;
    let connector = Arc::new(RpcSyncConnector::new(websocket, secret));
    let mut coordinator = CoordinatorConfig::new(profile_id);
    coordinator.reconnect = rpc_runtime.reconnect;
    match SqliteHistoryStore::open(data_dir.join("history.sqlite")) {
        Ok(store) => {
            coordinator.history = std::sync::Arc::new(store);
        }
        Err(error) => {
            tracing::warn!(%error, "local task history is unavailable");
        }
    }
    tracing::info!(
        scheme = endpoint.scheme(),
        host = endpoint.host_str().unwrap_or("unknown"),
        port = endpoint.port_or_known_default(),
        connect_timeout_ms = rpc_runtime.connect_timeout.as_millis(),
        request_timeout_ms = rpc_runtime.request_timeout.as_millis(),
        reconnect_base_ms = rpc_runtime.reconnect.base_delay.as_millis(),
        reconnect_max_ms = rpc_runtime.reconnect.max_delay.as_millis(),
        reconnect_max_attempts = ?rpc_runtime.reconnect.max_attempts,
        "configured external aria2 RPC profile"
    );
    let _runtime_guard = runtime.enter();
    Ok((
        spawn_sync_coordinator(connector, coordinator),
        local_engine,
        session_recovery,
    ))
}
