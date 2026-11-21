use std::{
    fs, io,
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

use ariadeck_domain::ProfileId;
use serde::{Deserialize, Serialize};

use crate::{EngineError, console_free_command, error::io_error};

/// Callback notified with the OS PID whenever a managed `aria2c` child is spawned
/// (initial launch or supervisor restart). Used by the desktop platform layer to
/// attach the child to an OS job object without introducing `unsafe` here.
pub type ChildSpawnHook = Arc<dyn Fn(u32) + Send + Sync>;

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

    pub(super) fn validate_shape(&self) -> Result<(), EngineError> {
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

/// Validate `path` (regular file or bare PATH command name), invoke `--version`
/// once, confirm it identifies `aria2`, and return its output text.
pub(crate) fn run_aria2_version(path: &Path) -> Result<String, EngineError> {
    match fs::metadata(path) {
        Ok(metadata) => {
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
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if path.components().count() != 1 {
                return Err(EngineError::ExecutableNotFound {
                    path: path.to_path_buf(),
                });
            }
        }
        Err(error) => {
            return Err(EngineError::ExecutableValidation {
                path: path.to_path_buf(),
                reason: error.to_string(),
            });
        }
    }

    let mut child = console_free_command(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| EngineError::ExecutableValidation {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;

    const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
    const MAX_PROBE_BYTES: u64 = 1024 * 1024;

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || read_bounded_pipe(stdout_pipe, MAX_PROBE_BYTES));
    let stderr_reader = std::thread::spawn(move || read_bounded_pipe(stderr_pipe, MAX_PROBE_BYTES));

    let deadline = std::time::Instant::now() + PROBE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(EngineError::ExecutableValidation {
                        path: path.to_path_buf(),
                        reason: "timed out probing --version".into(),
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(EngineError::ExecutableValidation {
                    path: path.to_path_buf(),
                    reason: error.to_string(),
                });
            }
        }
    };

    if !status.success() {
        return Err(EngineError::ExecutableValidation {
            path: path.to_path_buf(),
            reason: format!("process exited with {status}"),
        });
    }
    let stdout_bytes = stdout_reader.join().unwrap_or_default();
    let stderr_bytes = stderr_reader.join().unwrap_or_default();
    let stdout = String::from_utf8_lossy(&stdout_bytes);
    let stderr = String::from_utf8_lossy(&stderr_bytes);
    let stdout_lower = stdout.to_ascii_lowercase();
    let stderr_lower = stderr.to_ascii_lowercase();
    if !stdout_lower.contains("aria2") && !stderr_lower.contains("aria2") {
        return Err(EngineError::ExecutableValidation {
            path: path.to_path_buf(),
            reason: "--version output did not identify aria2".into(),
        });
    }
    Ok(if stdout.trim().is_empty() {
        stderr.into_owned()
    } else {
        stdout.into_owned()
    })
}

fn read_bounded_pipe(pipe: Option<impl io::Read>, limit: u64) -> Vec<u8> {
    let Some(pipe) = pipe else {
        return Vec::new();
    };
    let mut buffer = Vec::new();
    let _ = io::Read::read_to_end(&mut io::Read::take(pipe, limit), &mut buffer);
    buffer
}

/// Validate that a path names an executable that accepts `--version`.
pub fn validate_executable(path: impl AsRef<Path>) -> Result<(), EngineError> {
    let _ = run_aria2_version(path.as_ref())?;
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

pub(crate) fn local_engine_arguments(
    config: &LocalEngineConfig,
    port: u16,
    config_path: &Path,
    session_path: &Path,
    log_path: &Path,
) -> Vec<String> {
    vec![
        // Load only AriaDeck's ephemeral managed runtime config (`rpc-secret`)
        // instead of exposing the RPC secret in argv or reading `~/.aria2/aria2.conf`.
        "--no-conf=false".to_owned(),
        format!("--conf-path={}", config_path.to_string_lossy()),
        "--enable-rpc=true".to_owned(),
        "--rpc-listen-all=false".to_owned(),
        format!("--rpc-listen-port={port}"),
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
