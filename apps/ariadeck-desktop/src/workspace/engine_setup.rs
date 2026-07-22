//! Split from workspace.rs — engine_setup.

use super::*;

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

/// Persist Set/Clear secret mutations against the keyring using catalog secret_ref values.
pub(crate) fn apply_profile_secret_updates(
    catalog: &ProfileCatalog,
    view: &ProfileCatalogView,
    secret_updates: &std::collections::HashMap<String, ProfileRpcSecretUpdateView>,
    previous: &ProfileCatalog,
) -> Result<(), String> {
    if secret_updates.is_empty() {
        return Ok(());
    }
    let store = rpc_secret_store();
    for entry in &view.profiles {
        let Some(update) = secret_updates.get(&entry.profile_id) else {
            continue;
        };
        // Resolve the catalog entry for this view row (draft ids already mapped to UUIDs).
        let profile = catalog
            .profiles
            .iter()
            .find(|profile| {
                entry.profile_id.parse::<ProfileId>().ok() == Some(profile.profile_id)
                    || profile.name == entry.name.trim()
            })
            .or_else(|| {
                // Fallback: match by endpoint for remotes.
                catalog.profiles.iter().find(|profile| {
                    profile.kind == ProfileKind::RemoteRpc
                        && profile.endpoint.as_deref() == Some(entry.endpoint.trim())
                })
            });
        match update {
            ProfileRpcSecretUpdateView::Unchanged => {}
            ProfileRpcSecretUpdateView::Clear => {
                let refs = previous
                    .profiles
                    .iter()
                    .filter(|profile| {
                        entry.profile_id.parse::<ProfileId>().ok() == Some(profile.profile_id)
                            || profile
                                .endpoint
                                .as_deref()
                                .is_some_and(|endpoint| endpoint == entry.endpoint.trim())
                    })
                    .filter_map(|profile| profile.secret_ref);
                for secret_ref in refs {
                    let credential = ProxyCredentialRef::from_uuid(secret_ref.as_uuid());
                    let _ = store.delete(credential);
                }
            }
            ProfileRpcSecretUpdateView::Set(password) => {
                let secret_ref = profile.and_then(|profile| profile.secret_ref).ok_or_else(
                    || {
                        format!(
                            "Remote profile {} is missing a secret reference after save mapping.",
                            entry.name
                        )
                    },
                )?;
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
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .or_else(|| {
            Command::new("aria2c")
                .arg("--version")
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|_| PathBuf::from("aria2c"))
        })
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

/// Hide the GPUI window without destroying it so tray sessions can restore it.
pub(crate) fn hide_native_window(window: &Window) -> bool {
    #[cfg(target_os = "windows")]
    {
        // Use the HasWindowHandle trait explicitly; Window::window_handle()
        // returns the GPUI AnyWindowHandle id, not the OS handle.
        if let Ok(handle) = HasWindowHandle::window_handle(window)
            && let RawWindowHandle::Win32(win32) = handle.as_raw()
        {
            return hide_show_win32(win32.hwnd.get(), false);
        }
    }
    // Fallback on non-Windows or when the raw handle is unavailable: minimize
    // keeps the process alive and the tray menu can still restore/activate.
    window.minimize_window();
    true
}

pub(crate) fn show_native_window(window: &Window) -> bool {
    #[cfg(target_os = "windows")]
    {
        if let Ok(handle) = HasWindowHandle::window_handle(window)
            && let RawWindowHandle::Win32(win32) = handle.as_raw()
        {
            let restored = hide_show_win32(win32.hwnd.get(), true);
            window.activate_window();
            return restored;
        }
    }
    window.activate_window();
    true
}

#[cfg(target_os = "windows")]
pub(crate) fn hide_show_win32(hwnd: isize, show: bool) -> bool {
    // Narrow platform boundary: user32 ShowWindow for true hide-to-tray.
    #[link(name = "user32")]
    unsafe extern "system" {
        fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
    }
    const SW_HIDE: i32 = 0;
    const SW_RESTORE: i32 = 9;
    // SAFETY: hwnd is the live GPUI window handle for this process.
    unsafe {
        ShowWindow(hwnd, if show { SW_RESTORE } else { SW_HIDE });
    }
    true
}

/// Marker file placed next to the executable to enable portable mode (RELEASE-001).
pub(crate) const PORTABLE_MARKER_FILE: &str = "ariadeck.portable";

/// Application data directory used for settings, profiles, cores, and window geometry.
///
/// Resolution order:
/// 1. `ARIADECK_DATA_DIR` when set
/// 2. `<exe_dir>/data` when `<exe_dir>/ariadeck.portable` exists
/// 3. `%LOCALAPPDATA%/AriaDeck` (Windows)
/// 4. `$XDG_DATA_HOME/ariadeck` or `~/.local/share/ariadeck`
/// 5. `./.ariadeck` fallback
#[must_use]
pub(crate) fn default_data_dir() -> PathBuf {
    resolve_data_dir(|key| env::var_os(key), current_exe_dir().as_deref())
}

fn current_exe_dir() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

/// Testable data-dir resolver (RELEASE-001).
#[must_use]
pub(crate) fn resolve_data_dir(
    mut env_var: impl FnMut(&str) -> Option<std::ffi::OsString>,
    exe_dir: Option<&Path>,
) -> PathBuf {
    if let Some(path) = env_var("ARIADECK_DATA_DIR") {
        return PathBuf::from(path);
    }
    if let Some(exe_dir) = exe_dir {
        let marker = exe_dir.join(PORTABLE_MARKER_FILE);
        if marker.is_file() {
            return exe_dir.join("data");
        }
    }
    if let Some(path) = env_var("LOCALAPPDATA") {
        return PathBuf::from(path).join("AriaDeck");
    }
    if let Some(path) = env_var("XDG_DATA_HOME") {
        return PathBuf::from(path).join("ariadeck");
    }
    if let Some(path) = env_var("HOME") {
        return PathBuf::from(path).join(".local/share/ariadeck");
    }
    PathBuf::from(".ariadeck")
}
