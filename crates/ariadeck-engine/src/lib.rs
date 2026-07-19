//! Local external-engine lifecycle and profile persistence.
//!
//! This crate deliberately owns only process and profile concerns. RPC
//! synchronization remains in `ariadeck-rpc`, while profile identity is shared
//! through `ariadeck-domain`.

use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use ariadeck_domain::ProfileId;
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

const CONFIG_FILE_NAME: &str = "aria2.conf";
const SESSION_FILE_NAME: &str = "aria2.session";
const LOG_FILE_NAME: &str = "aria2.log";

/// Errors returned by local engine lifecycle and profile storage operations.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("profile name cannot be empty")]
    EmptyProfileName,
    #[error("profile data directory cannot be empty")]
    EmptyDataDirectory,
    #[error("profile download directory cannot be empty")]
    EmptyDownloadDirectory,
    #[error("executable does not exist: {path}")]
    ExecutableNotFound { path: PathBuf },
    #[error("executable path is a directory: {path}")]
    ExecutableIsDirectory { path: PathBuf },
    #[error("executable path is not a regular file: {path}")]
    ExecutableIsNotFile { path: PathBuf },
    #[error("executable validation failed for {path}: {reason}")]
    ExecutableValidation { path: PathBuf, reason: String },
    #[error("failed to spawn aria2 for {path}: {source}")]
    Spawn {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("I/O error while {operation} at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid profile store path: {path}")]
    InvalidStorePath { path: PathBuf },
    #[error("failed to serialize profile metadata: {source}")]
    Serialize {
        #[source]
        source: serde_json::Error,
    },
    #[error("malformed profile JSON at {path}: {message}")]
    MalformedProfile { path: PathBuf, message: String },
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> EngineError {
    EngineError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

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

    let output = Command::new(path)
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
    let output = Command::new(path)
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
    child: Option<Child>,
    endpoint: Url,
    secret: SecretString,
    config: LocalEngineConfig,
    profile_dir: PathBuf,
    config_path: PathBuf,
    session_path: PathBuf,
    log_path: PathBuf,
    port: u16,
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

        let config_path = profile_dir.join(CONFIG_FILE_NAME);
        let session_path = profile_dir.join(SESSION_FILE_NAME);
        let log_path = profile_dir.join(LOG_FILE_NAME);
        create_runtime_file(&config_path)?;
        create_runtime_file(&session_path)?;
        create_runtime_file(&log_path)?;

        let port = reserve_loopback_port()?;
        let secret = Uuid::new_v4().to_string();
        let endpoint = Url::parse(&format!("ws://127.0.0.1:{port}/jsonrpc")).map_err(|error| {
            EngineError::ExecutableValidation {
                path: config.executable.clone(),
                reason: format!("failed to construct RPC endpoint: {error}"),
            }
        })?;

        let arguments = vec![
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
            format!("--log={}", log_path.to_string_lossy()),
            // Keep the default local profile loopback-only. These optional
            // peer-discovery listeners can otherwise trigger a firewall
            // prompt before the user has configured a network profile.
            "--enable-dht=false".to_owned(),
            "--enable-dht6=false".to_owned(),
            "--bt-enable-lpd=false".to_owned(),
        ];

        let child = Command::new(&config.executable)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|source| EngineError::Spawn {
                path: config.executable.clone(),
                source,
            })?;

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

    pub fn is_running(&mut self) -> Result<bool, EngineError> {
        Ok(self.try_wait()?.is_none())
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

fn create_runtime_file(path: &Path) -> Result<(), EngineError> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map(|_| ())
        .map_err(|error| io_error("create a runtime file", path, error))
}

/// A JSON-backed store for one external profile's non-secret metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonProfileStore {
    path: PathBuf,
}

impl JsonProfileStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Atomically replace the profile file with a fully written temporary file.
    pub fn save(&self, profile: &ExternalEngineProfile) -> Result<(), EngineError> {
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if self.path.file_name().is_none() {
            return Err(EngineError::InvalidStorePath {
                path: self.path.clone(),
            });
        }
        fs::create_dir_all(parent)
            .map_err(|error| io_error("create the profile store directory", parent, error))?;

        let payload = serde_json::to_vec_pretty(profile)
            .map_err(|source| EngineError::Serialize { source })?;
        let temp_path = parent.join(format!(
            ".{}.{}.tmp",
            self.path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("profile"),
            Uuid::new_v4()
        ));

        let write_result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp_path)
                .map_err(|error| {
                    io_error("create the temporary profile file", &temp_path, error)
                })?;
            file.write_all(&payload)
                .map_err(|error| io_error("write the profile metadata", &temp_path, error))?;
            file.write_all(b"\n")
                .map_err(|error| io_error("finish the profile metadata", &temp_path, error))?;
            file.flush()
                .map_err(|error| io_error("flush the profile metadata", &temp_path, error))?;
            file.sync_all()
                .map_err(|error| io_error("sync the profile metadata", &temp_path, error))?;
            replace_file(&temp_path, &self.path).map_err(|error| {
                io_error("atomically replace the profile metadata", &self.path, error)
            })
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        write_result
    }

    pub fn load(&self) -> Result<ExternalEngineProfile, EngineError> {
        let bytes = fs::read(&self.path)
            .map_err(|error| io_error("read the profile metadata", &self.path, error))?;
        serde_json::from_slice(&bytes).map_err(|error| EngineError::MalformedProfile {
            path: self.path.clone(),
            message: error.to_string(),
        })
    }

    pub fn save_profile(&self, profile: &ExternalEngineProfile) -> Result<(), EngineError> {
        self.save(profile)
    }

    pub fn load_profile(&self) -> Result<ExternalEngineProfile, EngineError> {
        self.load()
    }
}

#[cfg(windows)]
fn replace_file(temp_path: &Path, target_path: &Path) -> io::Result<()> {
    match fs::rename(temp_path, target_path) {
        Ok(()) => Ok(()),
        Err(first_error) if target_path.exists() => {
            fs::remove_file(target_path)?;
            fs::rename(temp_path, target_path).or(Err(first_error))
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(windows))]
fn replace_file(temp_path: &Path, target_path: &Path) -> io::Result<()> {
    fs::rename(temp_path, target_path)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use super::*;

    fn temporary_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!("ariadeck-engine-{}", Uuid::new_v4()));
        if let Err(error) = fs::create_dir_all(&path) {
            panic!("failed to create test directory: {error}");
        }
        path
    }

    fn sample_profile(root: &Path) -> ExternalEngineProfile {
        ExternalEngineProfile::new(
            ProfileId::new(),
            "Test profile",
            std::env::current_exe().unwrap_or_else(|error| panic!("current exe failed: {error}")),
            root.join("data"),
            root.join("downloads"),
        )
    }

    #[test]
    fn rejects_missing_or_directory_paths_before_process_execution() {
        let root = temporary_directory();
        assert!(matches!(
            validate_executable(root.join("missing")),
            Err(EngineError::ExecutableNotFound { .. })
        ));
        assert!(matches!(
            validate_executable(&root),
            Err(EngineError::ExecutableIsDirectory { .. })
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[ignore = "requires ARIA2C_PATH and launches a real local aria2 process"]
    fn process_creates_profile_paths_and_redacts_secret() {
        let root = temporary_directory();
        let sample = sample_profile(&root);
        let profile = ExternalEngineProfile::new(
            sample.profile_id,
            sample.name,
            std::env::var_os("ARIA2C_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| panic!("ARIA2C_PATH is required for this test")),
            sample.data_dir,
            sample.download_dir,
        );
        let config = profile.local_config();
        let mut process = LocalEngineProcess::spawn(&config)
            .unwrap_or_else(|error| panic!("failed to spawn test process: {error}"));

        assert_eq!(process.endpoint().host_str(), Some("127.0.0.1"));
        assert_eq!(process.endpoint().path(), "/jsonrpc");
        assert_ne!(process.port(), 0);
        assert!(!process.secret().is_empty());
        assert!(process.profile_dir().is_dir());
        assert!(process.config_path().is_file());
        assert!(process.session_path().is_file());
        assert!(process.log_path().is_file());
        assert_eq!(process.profile_dir(), profile.profile_dir());
        let debug = format!("{process:?}");
        assert!(!debug.contains(process.secret()));
        assert!(process.shutdown().is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn profile_store_round_trips_atomically_and_handles_malformed_json() {
        let root = temporary_directory();
        let profile = sample_profile(&root);
        let store = JsonProfileStore::new(root.join("profiles").join("profile.json"));
        assert!(store.save(&profile).is_ok());
        assert_eq!(
            store
                .load()
                .unwrap_or_else(|error| panic!("load failed: {error}")),
            profile
        );

        let replacement = ExternalEngineProfile::new(
            profile.profile_id,
            "Replacement",
            profile.executable.clone(),
            profile.data_dir.clone(),
            profile.download_dir.clone(),
        );
        assert!(store.save(&replacement).is_ok());
        assert_eq!(
            store
                .load()
                .unwrap_or_else(|error| panic!("reload failed: {error}")),
            replacement
        );
        let raw = fs::read_to_string(store.path())
            .unwrap_or_else(|error| panic!("profile file read failed: {error}"));
        assert!(!raw.contains("rpc-secret"));
        let temporary_files = fs::read_dir(store.path().parent().unwrap_or_else(|| Path::new(".")))
            .unwrap_or_else(|error| panic!("profile directory read failed: {error}"))
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(temporary_files, 0);

        fs::write(store.path(), b"{ definitely not json")
            .unwrap_or_else(|error| panic!("malformed profile write failed: {error}"));
        assert!(matches!(
            store.load(),
            Err(EngineError::MalformedProfile { .. })
        ));
        let _ = fs::remove_dir_all(root);
    }
}
