//! Local managed `aria2c` process lifecycle, restart supervisor, ownership
//! locks, and session-file recovery.

use std::{
    collections::VecDeque,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    process::{Child, ExitStatus, Stdio},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ariadeck_domain::ProfileId;
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{
    CONFIG_FILE_NAME, EngineError, LOG_FILE_NAME, OWNERSHIP_LOCK_FILE_NAME, SESSION_FILE_NAME,
    console_free_command, error::io_error, profiles::create_runtime_file,
};

/// Metadata for an externally supplied aria2 executable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExternalEngineProfile {
    pub profile_id: ProfileId,
    pub name: String,
    pub executable: PathBuf,
    pub data_dir: PathBuf,
    pub download_dir: PathBuf,
}

impl ExternalEngineProfile {
    #[must_use]
    pub fn new(
        profile_id: ProfileId,
        name: impl Into<String>,
        executable: impl Into<PathBuf>,
        data_dir: impl Into<PathBuf>,
        download_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            profile_id,
            name: name.into(),
            executable: executable.into(),
            data_dir: data_dir.into(),
            download_dir: download_dir.into(),
        }
    }

    /// Returns the profile-specific directory under `data_dir`.
    #[must_use]
    pub fn profile_dir(&self) -> PathBuf {
        self.data_dir.join(self.profile_id.to_string())
    }

    #[must_use]
    pub fn local_config(&self) -> LocalEngineConfig {
        LocalEngineConfig::from(self)
    }
}

/// Runtime configuration for one locally managed external engine.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocalEngineConfig {
    pub profile_id: ProfileId,
    pub name: String,
    pub executable: PathBuf,
    pub data_dir: PathBuf,
    pub download_dir: PathBuf,
}

impl LocalEngineConfig {
    #[must_use]
    pub fn new(
        profile_id: ProfileId,
        name: impl Into<String>,
        executable: impl Into<PathBuf>,
        data_dir: impl Into<PathBuf>,
        download_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            profile_id,
            name: name.into(),
            executable: executable.into(),
            data_dir: data_dir.into(),
            download_dir: download_dir.into(),
        }
    }

    #[must_use]
    pub fn profile_dir(&self) -> PathBuf {
        self.data_dir.join(self.profile_id.to_string())
    }

    fn validate_shape(&self) -> Result<(), EngineError> {
        if self.name.trim().is_empty() {
            return Err(EngineError::EmptyProfileName);
        }
        if self.data_dir.as_os_str().is_empty() {
            return Err(EngineError::EmptyDataDirectory);
        }
        if self.download_dir.as_os_str().is_empty() {
            return Err(EngineError::EmptyDownloadDirectory);
        }
        Ok(())
    }
}

impl From<&ExternalEngineProfile> for LocalEngineConfig {
    fn from(profile: &ExternalEngineProfile) -> Self {
        Self {
            profile_id: profile.profile_id,
            name: profile.name.clone(),
            executable: profile.executable.clone(),
            data_dir: profile.data_dir.clone(),
            download_dir: profile.download_dir.clone(),
        }
    }
}

impl From<ExternalEngineProfile> for LocalEngineConfig {
    fn from(profile: ExternalEngineProfile) -> Self {
        Self {
            profile_id: profile.profile_id,
            name: profile.name,
            executable: profile.executable,
            data_dir: profile.data_dir,
            download_dir: profile.download_dir,
        }
    }
}

/// Validate that a path names an executable that accepts `--version`.
pub fn validate_executable(path: impl AsRef<Path>) -> Result<(), EngineError> {
    let path = path.as_ref();
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if path.components().count() == 1 {
                return validate_command_name(path);
            }
            return Err(EngineError::ExecutableNotFound {
                path: path.to_path_buf(),
            });
        }
        Err(error) => {
            return Err(EngineError::ExecutableValidation {
                path: path.to_path_buf(),
                reason: error.to_string(),
            });
        }
    };
    if metadata.is_dir() {
        return Err(EngineError::ExecutableIsDirectory {
            path: path.to_path_buf(),
        });
    }
    if !metadata.is_file() {
        return Err(EngineError::ExecutableIsNotFile {
            path: path.to_path_buf(),
        });
    }

    let output = console_free_command(path)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| EngineError::ExecutableValidation {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(EngineError::ExecutableValidation {
            path: path.to_path_buf(),
            reason: format!("process exited with {}", output.status),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    if !stdout.contains("aria2") && !stderr.contains("aria2") {
        return Err(EngineError::ExecutableValidation {
            path: path.to_path_buf(),
            reason: "--version output did not identify aria2".into(),
        });
    }
    Ok(())
}

fn validate_command_name(path: &Path) -> Result<(), EngineError> {
    let output = console_free_command(path)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| EngineError::ExecutableValidation {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(EngineError::ExecutableValidation {
            path: path.to_path_buf(),
            reason: format!("process exited with {}", output.status),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    if !stdout.contains("aria2") && !stderr.contains("aria2") {
        return Err(EngineError::ExecutableValidation {
            path: path.to_path_buf(),
            reason: "--version output did not identify aria2".into(),
        });
    }
    Ok(())
}

/// Reserve a currently available loopback TCP port.
pub fn reserve_loopback_port() -> Result<u16, EngineError> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .map_err(|error| io_error("reserve a loopback port", Path::new("127.0.0.1"), error))?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| {
            io_error(
                "read the reserved loopback port",
                Path::new("127.0.0.1"),
                error,
            )
        })
}

/// A running local aria2 process and the ephemeral RPC credentials for it.
pub struct LocalEngineProcess {
    pub(crate) child: Option<Child>,
    endpoint: Url,
    secret: SecretString,
    config: LocalEngineConfig,
    profile_dir: PathBuf,
    config_path: PathBuf,
    session_path: PathBuf,
    log_path: PathBuf,
    port: u16,
    /// Exclusive ownership of this profile directory for the process lifetime.
    ownership_lock: ProfileOwnershipLock,
    session_recovery_backup: Option<PathBuf>,
}

impl fmt::Debug for LocalEngineProcess {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalEngineProcess")
            .field("endpoint", &self.endpoint)
            .field("secret", &"[REDACTED]")
            .field("config", &self.config)
            .field("profile_dir", &self.profile_dir)
            .field("config_path", &self.config_path)
            .field("session_path", &self.session_path)
            .field("log_path", &self.log_path)
            .field("port", &self.port)
            .field("running", &self.child.is_some())
            .field("ownership_lock", &self.ownership_lock.path())
            .field(
                "session_recovery_backup",
                &self
                    .session_recovery_backup
                    .as_ref()
                    .map(|path| path.display().to_string()),
            )
            .finish()
    }
}

impl LocalEngineProcess {
    /// Start aria2 with an ephemeral loopback RPC endpoint.
    pub fn spawn(config: &LocalEngineConfig) -> Result<Self, EngineError> {
        config.validate_shape()?;
        validate_executable(&config.executable)?;

        let profile_dir = config.profile_dir();
        fs::create_dir_all(&profile_dir)
            .map_err(|error| io_error("create the profile directory", &profile_dir, error))?;
        fs::create_dir_all(&config.download_dir).map_err(|error| {
            io_error("create the download directory", &config.download_dir, error)
        })?;

        // Fail closed if another AriaDeck instance still owns this profile dir.
        let ownership_lock = ProfileOwnershipLock::acquire(config.profile_id, &profile_dir)?;

        let config_path = profile_dir.join(CONFIG_FILE_NAME);
        let session_path = profile_dir.join(SESSION_FILE_NAME);
        let log_path = profile_dir.join(LOG_FILE_NAME);
        create_runtime_file(&config_path)?;
        let session_prep = prepare_session_file(&session_path)?;
        create_runtime_file(&log_path)?;

        let port = reserve_loopback_port()?;
        let secret = Uuid::new_v4().to_string();
        let endpoint = Url::parse(&format!("ws://127.0.0.1:{port}/jsonrpc")).map_err(|error| {
            EngineError::ExecutableValidation {
                path: config.executable.clone(),
                reason: format!("failed to construct RPC endpoint: {error}"),
            }
        })?;

        let child = spawn_child(
            config,
            port,
            &secret,
            &config_path,
            &session_path,
            &log_path,
        )?;

        Ok(Self {
            child: Some(child),
            endpoint,
            secret: SecretString::new(secret),
            config: config.clone(),
            profile_dir,
            config_path,
            session_path,
            log_path,
            port,
            ownership_lock,
            session_recovery_backup: session_prep.backup_path,
        })
    }

    /// Start from an owned configuration without requiring a borrow at call sites.
    pub fn spawn_owned(config: LocalEngineConfig) -> Result<Self, EngineError> {
        Self::spawn(&config)
    }

    #[must_use]
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    #[must_use]
    pub fn rpc_endpoint(&self) -> &Url {
        self.endpoint()
    }

    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    #[must_use]
    pub fn secret(&self) -> &str {
        self.secret.expose_secret()
    }

    #[must_use]
    pub fn rpc_secret(&self) -> &str {
        self.secret()
    }

    #[must_use]
    pub fn config(&self) -> &LocalEngineConfig {
        &self.config
    }

    #[must_use]
    pub fn profile_dir(&self) -> &Path {
        &self.profile_dir
    }

    #[must_use]
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    #[must_use]
    pub fn session_path(&self) -> &Path {
        &self.session_path
    }

    #[must_use]
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// True when the previous aria2 session file was corrupt and replaced.
    #[must_use]
    pub fn session_was_recovered(&self) -> bool {
        self.session_recovery_backup.is_some()
    }

    #[must_use]
    pub fn session_recovery_backup(&self) -> Option<&Path> {
        self.session_recovery_backup.as_deref()
    }

    #[must_use]
    pub fn ownership_lock_path(&self) -> &Path {
        self.ownership_lock.path()
    }

    pub fn is_running(&mut self) -> Result<bool, EngineError> {
        if self.child.is_none() {
            return Ok(false);
        }
        Ok(self.try_wait()?.is_none())
    }

    /// Restart an unexpectedly exited process without changing its RPC endpoint.
    pub fn restart(&mut self) -> Result<(), EngineError> {
        if self.is_running()? {
            return Ok(());
        }
        let child = spawn_child(
            &self.config,
            self.port,
            self.secret.expose_secret(),
            &self.config_path,
            &self.session_path,
            &self.log_path,
        )?;
        self.child = Some(child);
        Ok(())
    }

    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, EngineError> {
        let Some(child) = self.child.as_mut() else {
            return Ok(None);
        };
        child
            .try_wait()
            .map_err(|source| io_error("check the aria2 process", &self.config.executable, source))
    }

    /// Best-effort synchronous termination. It is safe to call repeatedly.
    pub fn shutdown(&mut self) -> Result<(), EngineError> {
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        if child
            .try_wait()
            .map_err(|source| io_error("check the aria2 process", &self.config.executable, source))?
            .is_some()
        {
            return Ok(());
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if child
                .try_wait()
                .map_err(|source| {
                    io_error(
                        "wait for the aria2 process",
                        &self.config.executable,
                        source,
                    )
                })?
                .is_some()
            {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(25));
        }
        match child.kill() {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(io_error(
                    "terminate the aria2 process",
                    &self.config.executable,
                    source,
                ));
            }
        }
        child.wait().map(|_| ()).map_err(|source| {
            io_error(
                "wait for the aria2 process",
                &self.config.executable,
                source,
            )
        })
    }
}

impl Drop for LocalEngineProcess {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn spawn_child(
    config: &LocalEngineConfig,
    port: u16,
    secret: &str,
    config_path: &Path,
    session_path: &Path,
    log_path: &Path,
) -> Result<Child, EngineError> {
    let arguments =
        local_engine_arguments(config, port, secret, config_path, session_path, log_path);

    console_free_command(&config.executable)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|source| EngineError::Spawn {
            path: config.executable.clone(),
            source,
        })
}

pub(crate) fn local_engine_arguments(
    config: &LocalEngineConfig,
    port: u16,
    secret: &str,
    config_path: &Path,
    session_path: &Path,
    log_path: &Path,
) -> Vec<String> {
    vec![
        "--no-conf=true".to_owned(),
        "--enable-rpc=true".to_owned(),
        "--rpc-listen-all=false".to_owned(),
        format!("--rpc-listen-port={port}"),
        format!("--rpc-secret={secret}"),
        format!("--conf-path={}", config_path.to_string_lossy()),
        format!("--dir={}", config.download_dir.to_string_lossy()),
        format!("--input-file={}", session_path.to_string_lossy()),
        format!("--save-session={}", session_path.to_string_lossy()),
        "--save-session-interval=60".to_owned(),
        // Keep more completed/error/removed results in memory so the UI can
        // page recent stopped results in-session (HISTORY-001 / D-021). Durable
        // completed/failed summaries also live in local SQLite history (B6 / D-039).
        // aria2's default is 1000; raise the managed-local budget to 5000.
        "--max-download-result=5000".to_owned(),
        "--rpc-save-upload-metadata=true".to_owned(),
        "--rpc-max-request-size=32M".to_owned(),
        format!("--log={}", log_path.to_string_lossy()),
        // Keep the default local profile loopback-only. These optional
        // peer-discovery listeners can otherwise trigger a firewall prompt
        // before the user has configured a network profile.
        "--enable-dht=false".to_owned(),
        "--enable-dht6=false".to_owned(),
        "--bt-enable-lpd=false".to_owned(),
    ]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalRestartPolicy {
    pub max_restarts: u32,
    pub window: Duration,
    pub poll_interval: Duration,
}

impl Default for LocalRestartPolicy {
    fn default() -> Self {
        Self {
            max_restarts: 2,
            window: Duration::from_secs(30),
            poll_interval: Duration::from_millis(250),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalEngineHealth {
    Running { restarts: u32 },
    Restarting { attempt: u32 },
    Failed { restarts: u32, reason: String },
}

/// Read-only weak handle for observing a supervisor without extending its lifetime.
#[derive(Clone, Default)]
pub struct LocalEngineHealthHandle {
    shared: Weak<SupervisorShared>,
}

impl fmt::Debug for LocalEngineHealthHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalEngineHealthHandle")
            .field("health", &self.health())
            .finish_non_exhaustive()
    }
}

impl LocalEngineHealthHandle {
    #[must_use]
    pub fn health(&self) -> Option<LocalEngineHealth> {
        let shared = self.shared.upgrade()?;
        Some(
            shared
                .health
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
        )
    }
}

pub(crate) struct SupervisorShared {
    pub(crate) process: Mutex<LocalEngineProcess>,
    health: Mutex<LocalEngineHealth>,
    stop: AtomicBool,
    policy: LocalRestartPolicy,
}

/// Monitors a local aria2 child and restarts short-lived crashes in place.
pub struct LocalEngineSupervisor {
    pub(crate) shared: Arc<SupervisorShared>,
    monitor: Option<JoinHandle<()>>,
    endpoint: Url,
    secret: SecretString,
    profile_id: ProfileId,
}

impl fmt::Debug for LocalEngineSupervisor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalEngineSupervisor")
            .field("endpoint", &self.endpoint)
            .field("secret", &"[REDACTED]")
            .field("profile_id", &self.profile_id)
            .field("health", &self.health())
            .finish_non_exhaustive()
    }
}

impl LocalEngineSupervisor {
    pub fn spawn(config: &LocalEngineConfig) -> Result<Self, EngineError> {
        Self::spawn_with_policy(config, LocalRestartPolicy::default())
    }

    pub fn spawn_with_policy(
        config: &LocalEngineConfig,
        policy: LocalRestartPolicy,
    ) -> Result<Self, EngineError> {
        let process = LocalEngineProcess::spawn(config)?;
        let endpoint = process.endpoint().clone();
        let secret = SecretString::new(process.secret().to_owned());
        let profile_id = process.config().profile_id;
        let shared = Arc::new(SupervisorShared {
            process: Mutex::new(process),
            health: Mutex::new(LocalEngineHealth::Running { restarts: 0 }),
            stop: AtomicBool::new(false),
            policy,
        });
        let monitor_shared = shared.clone();
        let monitor = thread::Builder::new()
            .name("ariadeck-engine-supervisor".into())
            .spawn(move || monitor_local_engine(&monitor_shared))
            .map_err(EngineError::SpawnSupervisor)?;

        Ok(Self {
            shared,
            monitor: Some(monitor),
            endpoint,
            secret,
            profile_id,
        })
    }

    #[must_use]
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    #[must_use]
    pub fn secret(&self) -> &str {
        self.secret.expose_secret()
    }

    #[must_use]
    pub fn profile_id(&self) -> ProfileId {
        self.profile_id
    }

    /// True when startup replaced a corrupt aria2 session file.
    #[must_use]
    pub fn session_was_recovered(&self) -> bool {
        self.shared
            .process
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .session_was_recovered()
    }

    #[must_use]
    pub fn session_recovery_backup(&self) -> Option<PathBuf> {
        self.shared
            .process
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .session_recovery_backup()
            .map(Path::to_path_buf)
    }

    #[must_use]
    pub fn health(&self) -> LocalEngineHealth {
        self.shared
            .health
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    #[must_use]
    pub fn health_handle(&self) -> LocalEngineHealthHandle {
        LocalEngineHealthHandle {
            shared: Arc::downgrade(&self.shared),
        }
    }

    /// Stop crash monitoring before the composition root requests RPC shutdown.
    pub fn stop_monitoring(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(monitor) = self.monitor.take() {
            let _ = monitor.join();
        }
    }

    pub fn shutdown(&mut self) -> Result<(), EngineError> {
        self.stop_monitoring();
        self.shared
            .process
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .shutdown()
    }
}

impl Drop for LocalEngineSupervisor {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn monitor_local_engine(shared: &SupervisorShared) {
    let mut restarts = VecDeque::new();
    loop {
        thread::sleep(shared.policy.poll_interval);
        if shared.stop.load(Ordering::Acquire) {
            break;
        }

        let now = Instant::now();
        let previous_restart_count = restarts.len();
        while restarts
            .front()
            .is_some_and(|restart| now.duration_since(*restart) > shared.policy.window)
        {
            restarts.pop_front();
        }
        let restart_count_changed = restarts.len() != previous_restart_count;
        let is_running = restart_count_changed && {
            let health = shared
                .health
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            matches!(&*health, LocalEngineHealth::Running { .. })
        };
        if is_running {
            set_supervisor_health(
                shared,
                LocalEngineHealth::Running {
                    restarts: u32::try_from(restarts.len()).unwrap_or(u32::MAX),
                },
            );
        }

        let mut process = shared
            .process
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let exit = match process.try_wait() {
            Ok(exit) => exit,
            Err(error) => {
                set_supervisor_health(
                    shared,
                    LocalEngineHealth::Failed {
                        restarts: u32::try_from(restarts.len()).unwrap_or(u32::MAX),
                        reason: error.to_string(),
                    },
                );
                break;
            }
        };
        let Some(status) = exit else {
            continue;
        };
        let restart_count = u32::try_from(restarts.len()).unwrap_or(u32::MAX);
        if restart_count >= shared.policy.max_restarts {
            set_supervisor_health(
                shared,
                LocalEngineHealth::Failed {
                    restarts: restart_count,
                    reason: format!("aria2 exited unexpectedly with {status}"),
                },
            );
            break;
        }

        let attempt = restart_count.saturating_add(1);
        set_supervisor_health(shared, LocalEngineHealth::Restarting { attempt });
        match process.restart() {
            Ok(()) => {
                restarts.push_back(now);
                set_supervisor_health(
                    shared,
                    LocalEngineHealth::Running {
                        restarts: u32::try_from(restarts.len()).unwrap_or(u32::MAX),
                    },
                );
            }
            Err(error) => {
                restarts.push_back(now);
                if attempt >= shared.policy.max_restarts {
                    set_supervisor_health(
                        shared,
                        LocalEngineHealth::Failed {
                            restarts: attempt,
                            reason: error.to_string(),
                        },
                    );
                    break;
                }
            }
        }
    }
}

fn set_supervisor_health(shared: &SupervisorShared, health: LocalEngineHealth) {
    *shared
        .health
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = health;
}

/// Record written into `.ariadeck-engine.lock` while a managed local engine is live.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct OwnershipLockRecord {
    profile_id: ProfileId,
    owner_pid: u32,
    acquired_unix_secs: u64,
}

/// Held exclusive ownership of one managed profile directory.
///
/// The lock file prevents two AriaDeck instances from sharing the same
/// `--input-file` / `--save-session` path. Stale locks from crashed processes
/// are reclaimed; live owners fail closed with [`EngineError::ProfileAlreadyOwned`].
#[derive(Debug)]
pub struct ProfileOwnershipLock {
    path: PathBuf,
    profile_id: ProfileId,
    owner_pid: u32,
    /// Keeps the exclusive open handle alive on platforms that honor share modes.
    file: Option<File>,
}

impl ProfileOwnershipLock {
    /// Acquire exclusive ownership for `profile_dir`, reclaiming a dead owner.
    pub fn acquire(profile_id: ProfileId, profile_dir: &Path) -> Result<Self, EngineError> {
        fs::create_dir_all(profile_dir)
            .map_err(|error| io_error("create the profile directory", profile_dir, error))?;
        let path = profile_dir.join(OWNERSHIP_LOCK_FILE_NAME);
        match try_create_lock_file(&path) {
            Ok(file) => write_lock_record(file, profile_id, &path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                reclaim_or_reject_existing_lock(profile_id, &path)
            }
            Err(error) => Err(io_error("create the profile ownership lock", &path, error)),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn owner_pid(&self) -> u32 {
        self.owner_pid
    }

    #[must_use]
    pub fn profile_id(&self) -> ProfileId {
        self.profile_id
    }
}

impl Drop for ProfileOwnershipLock {
    fn drop(&mut self) {
        // Close the exclusive handle first so Windows allows unlinking.
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

fn try_create_lock_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        // Deny other readers/writers while this AriaDeck instance is alive.
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(0);
    }
    options.open(path)
}

fn write_lock_record(
    mut file: File,
    profile_id: ProfileId,
    path: &Path,
) -> Result<ProfileOwnershipLock, EngineError> {
    let owner_pid = std::process::id();
    let record = OwnershipLockRecord {
        profile_id,
        owner_pid,
        acquired_unix_secs: unix_now_secs(),
    };
    let payload =
        serde_json::to_vec_pretty(&record).map_err(|source| EngineError::Serialize { source })?;
    file.set_len(0)
        .map_err(|error| io_error("truncate the profile ownership lock", path, error))?;
    file.write_all(&payload)
        .map_err(|error| io_error("write the profile ownership lock", path, error))?;
    file.write_all(b"\n")
        .map_err(|error| io_error("finish the profile ownership lock", path, error))?;
    file.flush()
        .map_err(|error| io_error("flush the profile ownership lock", path, error))?;
    Ok(ProfileOwnershipLock {
        path: path.to_path_buf(),
        profile_id,
        owner_pid,
        file: Some(file),
    })
}

fn reclaim_or_reject_existing_lock(
    profile_id: ProfileId,
    path: &Path,
) -> Result<ProfileOwnershipLock, EngineError> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let owner_pid = serde_json::from_str::<OwnershipLockRecord>(&existing)
        .ok()
        .map(|record| record.owner_pid)
        .or_else(|| parse_legacy_lock_pid(&existing));
    let this_pid = std::process::id();

    // Live foreign owner, or a second acquire in this same process (re-entry),
    // must fail closed. Same-pid happens when Windows exclusive share mode
    // still holds the first lock handle open.
    if let Some(owner_pid) = owner_pid
        && (owner_pid == this_pid || process_appears_alive(owner_pid))
    {
        return Err(EngineError::ProfileAlreadyOwned {
            profile_id,
            owner_pid,
            lock_path: path.to_path_buf(),
        });
    }

    // Stale or unreadable lock: remove and recreate.
    // If another process still has an exclusive handle, remove/create fails closed.
    if let Err(_error) = fs::remove_file(path) {
        return Err(EngineError::ProfileAlreadyOwned {
            profile_id,
            owner_pid: owner_pid.unwrap_or(this_pid),
            lock_path: path.to_path_buf(),
        });
    }
    let file = try_create_lock_file(path).map_err(|_error| EngineError::ProfileAlreadyOwned {
        profile_id,
        owner_pid: owner_pid.unwrap_or(this_pid),
        lock_path: path.to_path_buf(),
    })?;
    write_lock_record(file, profile_id, path)
}

fn parse_legacy_lock_pid(contents: &str) -> Option<u32> {
    contents
        .lines()
        .find_map(|line| line.trim().parse::<u32>().ok())
}

pub(crate) fn process_appears_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // sysinfo is already a dependency for disk checks. Refresh only the target
    // pid so startup stays cheap.
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]));
    system.process(Pid::from_u32(pid)).is_some()
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Outcome of preparing the aria2 session file before spawn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionFilePreparation {
    pub path: PathBuf,
    pub recovered_from_corruption: bool,
    pub backup_path: Option<PathBuf>,
}

/// Ensure `session_path` exists and is a plausible aria2 session text file.
///
/// Corrupt contents are moved aside and replaced with an empty session so the
/// managed engine can start. The original bytes are preserved for diagnosis.
pub fn prepare_session_file(session_path: &Path) -> Result<SessionFilePreparation, EngineError> {
    if !session_path.exists() {
        create_runtime_file(session_path)?;
        return Ok(SessionFilePreparation {
            path: session_path.to_path_buf(),
            recovered_from_corruption: false,
            backup_path: None,
        });
    }

    let bytes = fs::read(session_path)
        .map_err(|error| io_error("read the aria2 session file", session_path, error))?;
    if session_bytes_are_plausible(&bytes) {
        return Ok(SessionFilePreparation {
            path: session_path.to_path_buf(),
            recovered_from_corruption: false,
            backup_path: None,
        });
    }

    let backup_path = backup_corrupt_file(session_path, "session")?;
    // Replace with a fresh empty session aria2 can rewrite.
    fs::write(session_path, b"")
        .map_err(|error| io_error("reset the aria2 session file", session_path, error))?;
    Ok(SessionFilePreparation {
        path: session_path.to_path_buf(),
        recovered_from_corruption: true,
        backup_path: Some(backup_path),
    })
}

fn session_bytes_are_plausible(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    // aria2 session files are line-oriented text. Reject NULs and invalid UTF-8.
    if bytes.contains(&0) {
        return false;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    // Reject control characters other than tab/CR/LF.
    !text.chars().any(|ch| {
        let code = ch as u32;
        code < 0x20 && ch != '\t' && ch != '\n' && ch != '\r'
    })
}

pub(crate) fn backup_corrupt_file(path: &Path, kind: &str) -> Result<PathBuf, EngineError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(kind);
    let backup_path = parent.join(format!("{file_name}.corrupt-{}.bak", Uuid::new_v4()));
    fs::rename(path, &backup_path)
        .map_err(|error| io_error("preserve the corrupt runtime file", &backup_path, error))?;
    Ok(backup_path)
}
