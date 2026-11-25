use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Child, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use secrecy::{ExposeSecret as _, SecretString};
use url::Url;
use uuid::Uuid;

use crate::{
    CONFIG_FILE_NAME, EngineError, LOG_FILE_NAME, SESSION_FILE_NAME, console_free_command,
    error::io_error, profiles::create_runtime_file,
};

use super::{
    config::{
        ChildSpawnHook, LocalEngineConfig, local_engine_arguments, reserve_loopback_port,
        validate_executable,
    },
    lock::ProfileOwnershipLock,
    session::prepare_session_file,
};

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
    graceful_shutdown_requested: bool,
    on_child_spawned: Option<ChildSpawnHook>,
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
        Self::spawn_with_child_hook(config, None)
    }

    /// Start aria2 and invoke `on_child_spawned` with the child PID after spawn and restart.
    pub fn spawn_with_child_hook(
        config: &LocalEngineConfig,
        on_child_spawned: Option<ChildSpawnHook>,
    ) -> Result<Self, EngineError> {
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
        let session_prep = prepare_session_file(&session_path)?;
        create_runtime_file(&log_path)?;

        let port = reserve_loopback_port()?;
        let secret = Uuid::new_v4().to_string();
        write_runtime_config_file(&config_path, &secret)?;
        let endpoint = Url::parse(&format!("ws://127.0.0.1:{port}/jsonrpc")).map_err(|error| {
            EngineError::ExecutableValidation {
                path: config.executable.clone(),
                reason: format!("failed to construct RPC endpoint: {error}"),
            }
        })?;

        let child = match spawn_child(config, port, &config_path, &session_path, &log_path) {
            Ok(child) => child,
            Err(error) => {
                let _ = fs::remove_file(&config_path);
                return Err(error);
            }
        };
        if let Some(hook) = on_child_spawned.as_ref() {
            hook(child.id());
        }

        Ok(Self {
            child: Some(child),
            endpoint,
            secret: SecretString::from(secret),
            config: config.clone(),
            profile_dir,
            config_path,
            session_path,
            log_path,
            port,
            ownership_lock,
            session_recovery_backup: session_prep.backup_path,
            graceful_shutdown_requested: false,
            on_child_spawned,
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
        write_runtime_config_file(&self.config_path, self.secret.expose_secret())?;
        let child = spawn_child(
            &self.config,
            self.port,
            &self.config_path,
            &self.session_path,
            &self.log_path,
        )?;
        if let Some(hook) = self.on_child_spawned.as_ref() {
            hook(child.id());
        }
        self.graceful_shutdown_requested = false;
        self.child = Some(child);
        Ok(())
    }

    /// Record that an RPC `aria2.shutdown` was issued so `shutdown()` grants a
    /// brief grace period for session flushing before hard termination.
    pub fn mark_graceful_shutdown_requested(&mut self) {
        self.graceful_shutdown_requested = true;
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
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        if child
            .try_wait()
            .map_err(|source| io_error("check the aria2 process", &self.config.executable, source))?
            .is_some()
        {
            return Ok(());
        }
        // Only wait for self-exit when an RPC `aria2.shutdown` was actually sent;
        // otherwise `child` has not been asked to stop and sleeping 2s is pure waste.
        if self.graceful_shutdown_requested {
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
        }
        match child.kill() {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                self.child = Some(child);
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
        let _ = fs::remove_file(&self.config_path);
    }
}

fn write_runtime_config_file(path: &Path, secret: &str) -> Result<(), EngineError> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| io_error("open the managed aria2 runtime config", path, error))?;
    file.write_all(format!("rpc-secret={secret}\n").as_bytes())
        .and_then(|()| file.flush())
        .map_err(|error| io_error("write the managed aria2 runtime config", path, error))
}

fn spawn_child(
    config: &LocalEngineConfig,
    port: u16,
    config_path: &Path,
    session_path: &Path,
    log_path: &Path,
) -> Result<Child, EngineError> {
    let arguments = local_engine_arguments(config, port, config_path, session_path, log_path);

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
