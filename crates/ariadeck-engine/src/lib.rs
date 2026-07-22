//! Local external-engine lifecycle, managed core registry, and profile persistence.
//!
//! This crate deliberately owns only process, core-installation, and profile
//! concerns. RPC synchronization remains in `ariadeck-rpc`, while profile
//! identity is shared through `ariadeck-domain`.

mod cores;

pub use cores::{
    Aria2Probe, CoreInstallStatus, CoreInstallation, CoreInstallationSummary, CoreInstallationView,
    CoreRegistry, CoreSource, CoreStore, parse_aria2_version_output, probe_aria2,
};

use std::{
    collections::{HashSet, VecDeque},
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ariadeck_application::{
    DownloadDestinationFile, DownloadDestinationGateway, DownloadDestinationReport,
    DownloadDestinationRequest, GatewayError, GatewayErrorKind, TaskFileGateway,
    TaskFileRemovalPreview, TaskFileRemovalReport, TaskFileRemovalRequest, TaskOpenRequest,
    TaskOpenTarget,
};
use ariadeck_domain::ProfileId;
use async_trait::async_trait;
use secrecy::{ExposeSecret as _, SecretString};
use serde::{Deserialize, Serialize};
use sysinfo::Disks;
use thiserror::Error;
use url::Url;
use uuid::Uuid;

const CONFIG_FILE_NAME: &str = "aria2.conf";
const SESSION_FILE_NAME: &str = "aria2.session";
const LOG_FILE_NAME: &str = "aria2.log";
const OWNERSHIP_LOCK_FILE_NAME: &str = ".ariadeck-engine.lock";

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
    #[error("failed to start the local engine supervisor: {0}")]
    SpawnSupervisor(io::Error),
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
    /// Another AriaDeck instance still owns this profile directory.
    #[error(
        "profile {profile_id} is already owned by process {owner_pid}; close that AriaDeck instance or stop its managed aria2 before starting again"
    )]
    ProfileAlreadyOwned {
        profile_id: ProfileId,
        owner_pid: u32,
        lock_path: PathBuf,
    },
    #[error("profile catalog has no profiles")]
    EmptyProfileCatalog,
    #[error("active profile {profile_id} is missing from the catalog")]
    ActiveProfileMissing { profile_id: ProfileId },
    #[error("profile {profile_id} was not found")]
    ProfileNotFound { profile_id: ProfileId },
    #[error("invalid remote profile endpoint: {reason}")]
    InvalidRemoteEndpoint { reason: String },
    #[error("remote profile requires a non-empty endpoint")]
    MissingRemoteEndpoint,
    #[error("managed core {id} was not found")]
    CoreNotFound {
        id: ariadeck_domain::CoreInstallationId,
    },
    #[error("aria2 version {version} ({target}) is already installed")]
    CoreAlreadyInstalled { version: String, target: String },
    #[error("invalid managed core version label: {version}")]
    InvalidCoreVersion { version: String },
    #[error("cannot remove the active managed core {id}; activate another version first")]
    CannotRemoveActiveCore {
        id: ariadeck_domain::CoreInstallationId,
    },
    #[error("no last-working managed core is recorded for rollback")]
    NoLastWorkingCore,
    #[error("already running the last-working managed core {id}")]
    AlreadyOnLastWorkingCore {
        id: ariadeck_domain::CoreInstallationId,
    },
    #[error("cannot remove the last remaining profile")]
    CannotRemoveLastProfile,
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> EngineError {
    EngineError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

#[derive(Clone, Default)]
pub struct LocalDownloadRootRegistry {
    roots: Arc<Mutex<Vec<PathBuf>>>,
}

impl LocalDownloadRootRegistry {
    #[must_use]
    pub fn new(initial_root: impl Into<PathBuf>) -> Self {
        Self {
            roots: Arc::new(Mutex::new(vec![initial_root.into()])),
        }
    }

    fn authorize(&self, directory: &Path) -> Result<PathBuf, GatewayError> {
        let canonical = canonical_directory(directory, "download directory")?;
        let mut roots = self
            .roots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let already_authorized = roots
            .iter()
            .any(|root| fs::canonicalize(root).is_ok_and(|existing| existing == canonical));
        if !already_authorized {
            roots.push(canonical.clone());
        }
        Ok(canonical)
    }

    fn snapshot(&self) -> Vec<PathBuf> {
        self.roots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

#[derive(Default)]
pub struct LocalDownloadDestinationGateway {
    roots: LocalDownloadRootRegistry,
}

impl LocalDownloadDestinationGateway {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_roots(roots: LocalDownloadRootRegistry) -> Self {
        Self { roots }
    }
}

impl DownloadDestinationGateway for LocalDownloadDestinationGateway {
    fn preflight(
        &self,
        request: &DownloadDestinationRequest,
    ) -> Result<DownloadDestinationReport, GatewayError> {
        let path = PathBuf::from(request.directory.as_str());
        if !path.is_absolute() {
            return Err(unsafe_path_error(format!(
                "local download directory must be absolute: {}",
                path.display()
            )));
        }
        if path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        }) {
            return Err(unsafe_path_error(format!(
                "download directory contains parent or current-directory traversal: {}",
                path.display()
            )));
        }

        let directory = canonical_directory(&path, "download directory")?;
        // Do not use Permissions::readonly() on directories: on Windows the
        // FILE_ATTRIBUTE_READONLY bit is commonly set on ordinary folders and
        // does not mean they are non-writable. Probe with a real write instead.
        validate_destination_files(&directory, &request.files)?;

        verify_directory_writable(&directory)?;
        let available_bytes = available_space(&directory)?;
        if available_bytes == 0 {
            return Err(filesystem_gateway_error(
                format!(
                    "download directory has no free space: {}",
                    directory.display()
                ),
                true,
            ));
        }
        if let Some(required_bytes) = request.required_bytes
            && required_bytes > available_bytes
        {
            return Err(filesystem_gateway_error(
                format!(
                    "download requires {required_bytes} bytes but only {available_bytes} bytes are available in {}",
                    directory.display()
                ),
                true,
            ));
        }

        self.roots.authorize(&directory)?;

        Ok(DownloadDestinationReport { available_bytes })
    }
}

fn validate_destination_files(
    directory: &Path,
    files: &[DownloadDestinationFile],
) -> Result<(), GatewayError> {
    let mut unique_paths = HashSet::with_capacity(files.len());
    for file in files {
        let relative = PathBuf::from(file.relative_path.as_str());
        if relative.as_os_str().is_empty()
            || relative.is_absolute()
            || relative.components().any(|component| {
                !matches!(component, std::path::Component::Normal(part) if !part.is_empty())
            })
        {
            return Err(unsafe_path_error(format!(
                "download file path must be a safe relative path: {}",
                relative.display()
            )));
        }
        if !unique_paths.insert(destination_path_key(&relative)) {
            return Err(unsafe_path_error(format!(
                "download file path appears more than once: {}",
                relative.display()
            )));
        }

        let target = directory.join(&relative);
        if !target.starts_with(directory) {
            return Err(unsafe_path_error(format!(
                "download file escapes its destination: {}",
                relative.display()
            )));
        }
        validate_existing_destination_path(directory, &target, file.reject_existing)?;
    }
    Ok(())
}

fn destination_path_key(path: &Path) -> String {
    let key = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        key.to_ascii_lowercase()
    } else {
        key
    }
}

fn validate_existing_destination_path(
    directory: &Path,
    target: &Path,
    reject_existing: bool,
) -> Result<(), GatewayError> {
    let relative = target.strip_prefix(directory).map_err(|_| {
        unsafe_path_error(format!(
            "download file escapes its destination: {}",
            target.display()
        ))
    })?;
    let component_count = relative.components().count();
    let mut current = directory.to_path_buf();
    for (offset, component) in relative.components().enumerate() {
        current.push(component.as_os_str());
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(filesystem_error(
                    "inspect download file path",
                    &current,
                    error,
                    true,
                ));
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(unsafe_path_error(format!(
                "download file path cannot traverse a symlink or reparse point: {}",
                current.display()
            )));
        }
        let canonical = fs::canonicalize(&current).map_err(|error| {
            filesystem_error("canonicalize download file path", &current, error, true)
        })?;
        if !canonical.starts_with(directory) {
            return Err(unsafe_path_error(format!(
                "download file path resolves outside its destination: {}",
                current.display()
            )));
        }

        let is_target = offset + 1 == component_count;
        if !is_target && !metadata.is_dir() {
            return Err(unsafe_path_error(format!(
                "download file parent is not a directory: {}",
                current.display()
            )));
        }
        if is_target {
            if reject_existing {
                return Err(GatewayError::new(
                    GatewayErrorKind::Rejected,
                    format!("download file already exists: {}", current.display()),
                    false,
                ));
            }
            if !metadata.is_file() {
                return Err(unsafe_path_error(format!(
                    "download file target is not a regular file: {}",
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

fn verify_directory_writable(directory: &Path) -> Result<(), GatewayError> {
    let probe = directory.join(format!(".ariadeck-write-test-{}", Uuid::new_v4()));
    let write_result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)?;
        file.write_all(&[0])?;
        file.sync_all()
    })();
    let remove_result = fs::remove_file(&probe);

    if let Err(error) = write_result {
        return Err(filesystem_error(
            "write to download directory",
            directory,
            error,
            true,
        ));
    }
    remove_result.map_err(|error| {
        filesystem_error("remove download-directory write probe", &probe, error, true)
    })
}

fn available_space(directory: &Path) -> Result<u64, GatewayError> {
    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|disk| {
            fs::canonicalize(disk.mount_point())
                .is_ok_and(|mount_point| directory.starts_with(mount_point))
        })
        .max_by_key(|disk| disk.mount_point().components().count())
        .map(|disk| disk.available_space())
        .ok_or_else(|| {
            filesystem_gateway_error(
                format!(
                    "could not determine free space for download directory: {}",
                    directory.display()
                ),
                true,
            )
        })
}

/// Local-only adapter that moves exact aria2 task files to the OS Trash.
pub struct LocalTaskFileGateway {
    download_roots: LocalDownloadRootRegistry,
    trash: Arc<dyn TrashBackend>,
}

impl LocalTaskFileGateway {
    #[must_use]
    pub fn new(download_root: impl Into<PathBuf>) -> Self {
        Self {
            download_roots: LocalDownloadRootRegistry::new(download_root),
            trash: Arc::new(SystemTrash),
        }
    }

    #[must_use]
    pub fn with_roots(download_roots: LocalDownloadRootRegistry) -> Self {
        Self {
            download_roots,
            trash: Arc::new(SystemTrash),
        }
    }

    fn collect_safe_paths(
        &self,
        request: &TaskFileRemovalRequest,
    ) -> Result<SafeTaskPaths, GatewayError> {
        if request.files.is_empty() {
            return Err(unsafe_path_error(
                "aria2 did not report any exact task file paths; no local files were touched",
            ));
        }

        let (root, raw_task_dir, task_dir) =
            resolve_authorized_task_directory(&self.download_roots, &request.directory)?;
        // Skip Permissions::readonly() here for the same Windows directory-attribute
        // reason as destination preflight; actual Trash failures surface below.

        let raw_files = request
            .files
            .iter()
            .map(|path| resolve_engine_path(&raw_task_dir, path.as_str()))
            .collect::<Result<Vec<_>, _>>()?;

        let mut candidates = raw_files
            .iter()
            .cloned()
            .map(|path| (path, CandidateKind::Content))
            .collect::<Vec<_>>();
        if request.include_control_files {
            candidates.extend(
                raw_files
                    .iter()
                    .map(|path| (append_aria2_suffix(path), CandidateKind::Control)),
            );
            if let Some(common_root) = common_top_level_path(&raw_task_dir, &raw_files) {
                candidates.push((append_aria2_suffix(&common_root), CandidateKind::Control));
            }
        }

        let mut seen = HashSet::new();
        let mut paths = Vec::new();
        let mut content_files = 0;
        let mut control_files = 0;
        let mut missing_paths = 0;
        for (raw_path, kind) in candidates {
            match safe_existing_file(&raw_path, &root, &task_dir)? {
                Some(path) if seen.insert(path.clone()) => {
                    match kind {
                        CandidateKind::Content => content_files += 1,
                        CandidateKind::Control => control_files += 1,
                    }
                    paths.push(path);
                }
                Some(_) => {}
                None => missing_paths += 1,
            }
        }
        Ok(SafeTaskPaths {
            paths,
            preview: TaskFileRemovalPreview {
                content_files,
                control_files,
                missing_paths,
            },
        })
    }

    #[cfg(test)]
    fn with_trash(download_root: impl Into<PathBuf>, trash: Arc<dyn TrashBackend>) -> Self {
        Self {
            download_roots: LocalDownloadRootRegistry::new(download_root),
            trash,
        }
    }
}

#[async_trait]
impl TaskFileGateway for LocalTaskFileGateway {
    fn preflight(
        &self,
        request: &TaskFileRemovalRequest,
    ) -> Result<TaskFileRemovalPreview, GatewayError> {
        self.collect_safe_paths(request).map(|paths| paths.preview)
    }

    async fn move_to_trash(
        &self,
        request: &TaskFileRemovalRequest,
    ) -> Result<TaskFileRemovalReport, GatewayError> {
        // Path collection does filesystem I/O; keep it off async executors that
        // are not a Tokio runtime (e.g. GPUI task threads).
        let request = request.clone();
        let roots = self.download_roots.clone();
        let trash = self.trash.clone();
        run_blocking(move || {
            let gateway = LocalTaskFileGateway {
                download_roots: roots,
                trash: trash.clone(),
            };
            let safe = gateway.collect_safe_paths(&request)?;
            let missing_paths = safe.preview.missing_paths;
            let mut moved_to_trash = 0;
            for path in safe.paths {
                trash.move_to_trash(&path).map_err(|error| {
                    filesystem_gateway_error(
                        format!(
                            "moved {moved_to_trash} task file(s) before Trash failed for {}: {error}",
                            path.display()
                        ),
                        true,
                    )
                })?;
                moved_to_trash += 1;
            }
            Ok(TaskFileRemovalReport {
                moved_to_trash,
                missing_paths,
            })
        })
        .await
    }

    async fn open(&self, request: &TaskOpenRequest) -> Result<(), GatewayError> {
        let roots = self.download_roots.clone();
        let request = request.clone();
        run_blocking(move || {
            let (root, raw_task_dir, task_dir) =
                resolve_authorized_task_directory(&roots, &request.directory)?;
            let (target, is_file) = match request.target {
                TaskOpenTarget::Folder => (task_dir, false),
                TaskOpenTarget::Download if request.files.len() == 1 => {
                    let raw_path = resolve_engine_path(&raw_task_dir, request.files[0].as_str())?;
                    let path =
                        safe_existing_file(&raw_path, &root, &task_dir)?.ok_or_else(|| {
                            filesystem_gateway_error(
                                format!("downloaded file does not exist: {}", raw_path.display()),
                                false,
                            )
                        })?;
                    (path, true)
                }
                TaskOpenTarget::Download => (task_dir, false),
            };
            open_local_path(&target, is_file)
        })
        .await
    }
}

/// Run blocking filesystem work without requiring a current Tokio runtime.
///
/// GPUI spawns task-command futures on its own executor; `tokio::task::spawn_blocking`
/// panics there without an entered runtime, which left delete-with-files hanging.
async fn run_blocking<T, F>(work: F) -> Result<T, GatewayError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, GatewayError> + Send + 'static,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle.spawn_blocking(work).await.map_err(|error| {
            filesystem_gateway_error(format!("local file worker stopped: {error}"), true)
        })?,
        Err(_) => {
            // No Tokio context (e.g. GPUI async task): offload to a thread and await it.
            let (sender, receiver) = tokio::sync::oneshot::channel();
            thread::spawn(move || {
                let _ = sender.send(work());
            });
            receiver.await.map_err(|_| {
                filesystem_gateway_error("local file worker stopped unexpectedly", true)
            })?
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OpenCommandSpec {
    program: &'static str,
    arguments: Vec<String>,
}

fn open_local_path(path: &Path, is_file: bool) -> Result<(), GatewayError> {
    let spec = open_command_spec(path, is_file);
    Command::new(spec.program)
        .args(&spec.arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| filesystem_error("open task path", path, error, true))
}

fn open_command_spec(path: &Path, is_file: bool) -> OpenCommandSpec {
    let value = path.to_string_lossy().into_owned();
    #[cfg(target_os = "windows")]
    {
        if is_file {
            OpenCommandSpec {
                program: "rundll32.exe",
                arguments: vec!["url.dll,FileProtocolHandler".into(), value],
            }
        } else {
            OpenCommandSpec {
                program: "explorer.exe",
                arguments: vec![value],
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = is_file;
        OpenCommandSpec {
            program: "open",
            arguments: vec![value],
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = is_file;
        OpenCommandSpec {
            program: "xdg-open",
            arguments: vec![value],
        }
    }
}

struct SafeTaskPaths {
    paths: Vec<PathBuf>,
    preview: TaskFileRemovalPreview,
}

#[derive(Clone, Copy)]
enum CandidateKind {
    Content,
    Control,
}

trait TrashBackend: Send + Sync {
    fn move_to_trash(&self, path: &Path) -> Result<(), String>;
}

struct SystemTrash;

impl TrashBackend for SystemTrash {
    fn move_to_trash(&self, path: &Path) -> Result<(), String> {
        trash::delete(path).map_err(|error| error.to_string())
    }
}

fn resolve_authorized_task_directory(
    roots: &LocalDownloadRootRegistry,
    directory: &ariadeck_domain::EnginePath,
) -> Result<(PathBuf, PathBuf, PathBuf), GatewayError> {
    for configured_root in roots.snapshot() {
        let Ok(root) = canonical_directory(&configured_root, "download root") else {
            continue;
        };
        let raw_task_dir = resolve_engine_path(&root, directory.as_str())?;
        let Ok(task_dir) = canonical_directory(&raw_task_dir, "task directory") else {
            continue;
        };
        if task_dir.starts_with(&root) {
            return Ok((root, raw_task_dir, task_dir));
        }
    }
    Err(unsafe_path_error(format!(
        "task directory is outside the authorized local download roots: {}",
        directory
    )))
}

fn resolve_engine_path(base: &Path, value: &str) -> Result<PathBuf, GatewayError> {
    let path = PathBuf::from(value);
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    }) {
        return Err(unsafe_path_error(format!(
            "task path contains parent or current-directory traversal: {}",
            path.display()
        )));
    }
    Ok(if path.is_absolute() {
        path
    } else {
        base.join(path)
    })
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, GatewayError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| filesystem_error(&format!("inspect {label}"), path, error, true))?;
    if metadata.file_type().is_symlink() {
        return Err(unsafe_path_error(format!(
            "{label} cannot be a symlink or reparse point: {}",
            path.display()
        )));
    }
    if !metadata.is_dir() {
        return Err(unsafe_path_error(format!(
            "{label} is not a directory: {}",
            path.display()
        )));
    }
    fs::canonicalize(path)
        .map_err(|error| filesystem_error(&format!("canonicalize {label}"), path, error, true))
}

fn safe_existing_file(
    path: &Path,
    root: &Path,
    task_dir: &Path,
) -> Result<Option<PathBuf>, GatewayError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(filesystem_error("inspect task file", path, error, true)),
    };
    if metadata.file_type().is_symlink() {
        return Err(unsafe_path_error(format!(
            "task file cannot be a symlink or reparse point: {}",
            path.display()
        )));
    }
    if !metadata.is_file() {
        return Err(unsafe_path_error(format!(
            "task deletion only accepts exact files, not directories: {}",
            path.display()
        )));
    }
    let canonical = fs::canonicalize(path)
        .map_err(|error| filesystem_error("canonicalize task file", path, error, true))?;
    if canonical == root || !canonical.starts_with(root) || !canonical.starts_with(task_dir) {
        return Err(unsafe_path_error(format!(
            "task file is outside the permitted local roots: {}",
            path.display()
        )));
    }
    Ok(Some(canonical))
}

fn append_aria2_suffix(path: &Path) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(".aria2");
    PathBuf::from(value)
}

fn common_top_level_path(task_dir: &Path, files: &[PathBuf]) -> Option<PathBuf> {
    let mut components = files.iter().filter_map(|path| {
        path.strip_prefix(task_dir)
            .ok()?
            .components()
            .next()
            .map(|component| component.as_os_str().to_os_string())
    });
    let first = components.next()?;
    components
        .all(|component| component == first)
        .then(|| task_dir.join(first))
}

fn unsafe_path_error(message: impl Into<String>) -> GatewayError {
    GatewayError::new(GatewayErrorKind::UnsafePath, message, false)
}

fn filesystem_gateway_error(message: impl Into<String>, retryable: bool) -> GatewayError {
    GatewayError::new(GatewayErrorKind::Filesystem, message, retryable)
}

fn filesystem_error(
    operation: &str,
    path: &Path,
    error: io::Error,
    retryable: bool,
) -> GatewayError {
    filesystem_gateway_error(
        format!("failed to {operation} at {}: {error}", path.display()),
        retryable,
    )
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

    Command::new(&config.executable)
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

fn local_engine_arguments(
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
        // page history without a separate SQLite store (HISTORY-001). aria2's
        // default is 1000; raise the managed-local budget to 5000.
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

struct SupervisorShared {
    process: Mutex<LocalEngineProcess>,
    health: Mutex<LocalEngineHealth>,
    stop: AtomicBool,
    policy: LocalRestartPolicy,
}

/// Monitors a local aria2 child and restarts short-lived crashes in place.
pub struct LocalEngineSupervisor {
    shared: Arc<SupervisorShared>,
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

fn process_appears_alive(pid: u32) -> bool {
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

fn backup_corrupt_file(path: &Path, kind: &str) -> Result<PathBuf, EngineError> {
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

/// Recovery metadata when a corrupt profile document is replaced with defaults.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileStoreRecovery {
    pub backup_path: PathBuf,
    pub reason: String,
}

/// Result of loading or recovering the single-profile store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedProfile {
    pub profile: ExternalEngineProfile,
    pub recovery: Option<ProfileStoreRecovery>,
}

/// How a profile reaches aria2 (PROFILE-001).
///
/// Local managed profiles spawn aria2 under AriaDeck ownership. Remote RPC
/// profiles connect only and never share a managed session file.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileKind {
    LocalManaged,
    RemoteRpc,
}

impl ProfileKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::LocalManaged => "Local managed",
            Self::RemoteRpc => "Remote RPC",
        }
    }

    #[must_use]
    pub const fn is_local(self) -> bool {
        matches!(self, Self::LocalManaged)
    }
}

/// Opaque keyring reference for a remote RPC secret (never persisted as plaintext).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RpcSecretRef(Uuid);

impl RpcSecretRef {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    #[must_use]
    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for RpcSecretRef {
    fn default() -> Self {
        Self::new()
    }
}

/// One catalog entry: local managed engine or remote RPC endpoint.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileEntry {
    pub profile_id: ProfileId,
    pub name: String,
    pub kind: ProfileKind,
    /// Local managed: path to aria2c. Remote: unused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<PathBuf>,
    /// Shared application data root (local managed profile_dir = data_dir/id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_dir: Option<PathBuf>,
    /// Default download directory for this profile.
    pub download_dir: PathBuf,
    /// Remote RPC WebSocket endpoint (`ws`/`wss`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Whether a secret is stored in the system credential store.
    #[serde(default)]
    pub has_secret: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_ref: Option<RpcSecretRef>,
}

impl ProfileEntry {
    #[must_use]
    pub fn local_managed(
        profile_id: ProfileId,
        name: impl Into<String>,
        executable: impl Into<PathBuf>,
        data_dir: impl Into<PathBuf>,
        download_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            profile_id,
            name: name.into(),
            kind: ProfileKind::LocalManaged,
            executable: Some(executable.into()),
            data_dir: Some(data_dir.into()),
            download_dir: download_dir.into(),
            endpoint: None,
            has_secret: false,
            secret_ref: None,
        }
    }

    pub fn remote_rpc(
        profile_id: ProfileId,
        name: impl Into<String>,
        endpoint: impl Into<String>,
        download_dir: impl Into<PathBuf>,
        secret_ref: Option<RpcSecretRef>,
    ) -> Result<Self, EngineError> {
        let endpoint = endpoint.into();
        validate_remote_endpoint(&endpoint)?;
        Ok(Self {
            profile_id,
            name: name.into(),
            kind: ProfileKind::RemoteRpc,
            executable: None,
            data_dir: None,
            download_dir: download_dir.into(),
            endpoint: Some(endpoint),
            has_secret: secret_ref.is_some(),
            secret_ref,
        })
    }

    pub fn validate(&self) -> Result<(), EngineError> {
        if self.name.trim().is_empty() {
            return Err(EngineError::EmptyProfileName);
        }
        if self.download_dir.as_os_str().is_empty() {
            return Err(EngineError::EmptyDownloadDirectory);
        }
        match self.kind {
            ProfileKind::LocalManaged => {
                // Empty executable means "resolve from the active managed core
                // (or PATH discovery) at spawn time" — not a validation error.
                if self
                    .data_dir
                    .as_ref()
                    .is_none_or(|path| path.as_os_str().is_empty())
                {
                    return Err(EngineError::EmptyDataDirectory);
                }
            }
            ProfileKind::RemoteRpc => {
                let endpoint = self
                    .endpoint
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or(EngineError::MissingRemoteEndpoint)?;
                validate_remote_endpoint(endpoint)?;
            }
        }
        Ok(())
    }

    /// Local profiles with no explicit executable path use the active managed
    /// core (or discovery) at process start.
    #[must_use]
    pub fn uses_managed_core(&self) -> bool {
        self.kind == ProfileKind::LocalManaged
            && self
                .executable
                .as_ref()
                .is_none_or(|path| path.as_os_str().is_empty())
    }

    #[must_use]
    pub fn profile_dir(&self) -> Option<PathBuf> {
        match self.kind {
            ProfileKind::LocalManaged => self
                .data_dir
                .as_ref()
                .map(|data_dir| data_dir.join(self.profile_id.to_string())),
            ProfileKind::RemoteRpc => None,
        }
    }

    #[must_use]
    pub fn as_local_config(&self) -> Option<LocalEngineConfig> {
        if self.kind != ProfileKind::LocalManaged {
            return None;
        }
        Some(LocalEngineConfig {
            profile_id: self.profile_id,
            name: self.name.clone(),
            // Empty path is intentional for managed-core resolution.
            executable: self.executable.clone().unwrap_or_default(),
            data_dir: self.data_dir.clone()?,
            download_dir: self.download_dir.clone(),
        })
    }

    #[must_use]
    pub fn as_external_engine_profile(&self) -> Option<ExternalEngineProfile> {
        let config = self.as_local_config()?;
        Some(ExternalEngineProfile {
            profile_id: config.profile_id,
            name: config.name,
            executable: config.executable,
            data_dir: config.data_dir,
            download_dir: config.download_dir,
        })
    }
}

fn validate_remote_endpoint(endpoint: &str) -> Result<(), EngineError> {
    if endpoint.trim() != endpoint || endpoint.is_empty() {
        return Err(EngineError::InvalidRemoteEndpoint {
            reason: "value must be non-empty and must not have surrounding whitespace".into(),
        });
    }
    let parsed = Url::parse(endpoint).map_err(|error| EngineError::InvalidRemoteEndpoint {
        reason: error.to_string(),
    })?;
    if !matches!(parsed.scheme(), "ws" | "wss") {
        return Err(EngineError::InvalidRemoteEndpoint {
            reason: "endpoint must use ws or wss".into(),
        });
    }
    if parsed.host_str().is_none() {
        return Err(EngineError::InvalidRemoteEndpoint {
            reason: "endpoint must include a host".into(),
        });
    }
    Ok(())
}

/// Versioned multi-profile catalog (PROFILE-001). Schema 1 was a single
/// ExternalEngineProfile object; schema 2 is this catalog.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileCatalog {
    pub schema_version: u32,
    pub active_profile_id: ProfileId,
    pub profiles: Vec<ProfileEntry>,
}

pub const PROFILE_CATALOG_SCHEMA_VERSION: u32 = 2;

impl ProfileCatalog {
    #[must_use]
    pub fn from_single(profile: ExternalEngineProfile) -> Self {
        let entry = ProfileEntry::local_managed(
            profile.profile_id,
            profile.name,
            profile.executable,
            profile.data_dir,
            profile.download_dir,
        );
        Self {
            schema_version: PROFILE_CATALOG_SCHEMA_VERSION,
            active_profile_id: entry.profile_id,
            profiles: vec![entry],
        }
    }

    pub fn validate(&self) -> Result<(), EngineError> {
        if self.profiles.is_empty() {
            return Err(EngineError::EmptyProfileCatalog);
        }
        for profile in &self.profiles {
            profile.validate()?;
        }
        if !self
            .profiles
            .iter()
            .any(|profile| profile.profile_id == self.active_profile_id)
        {
            return Err(EngineError::ActiveProfileMissing {
                profile_id: self.active_profile_id,
            });
        }
        Ok(())
    }

    #[must_use]
    pub fn active(&self) -> Option<&ProfileEntry> {
        self.profiles
            .iter()
            .find(|profile| profile.profile_id == self.active_profile_id)
    }

    pub fn active_mut(&mut self) -> Option<&mut ProfileEntry> {
        let id = self.active_profile_id;
        self.profiles
            .iter_mut()
            .find(|profile| profile.profile_id == id)
    }

    pub fn set_active(&mut self, profile_id: ProfileId) -> Result<(), EngineError> {
        if !self
            .profiles
            .iter()
            .any(|profile| profile.profile_id == profile_id)
        {
            return Err(EngineError::ProfileNotFound { profile_id });
        }
        self.active_profile_id = profile_id;
        Ok(())
    }

    pub fn upsert(&mut self, entry: ProfileEntry) -> Result<(), EngineError> {
        entry.validate()?;
        if let Some(existing) = self
            .profiles
            .iter_mut()
            .find(|profile| profile.profile_id == entry.profile_id)
        {
            *existing = entry;
        } else {
            self.profiles.push(entry);
        }
        self.validate()
    }

    pub fn remove(&mut self, profile_id: ProfileId) -> Result<ProfileEntry, EngineError> {
        let index = self
            .profiles
            .iter()
            .position(|profile| profile.profile_id == profile_id)
            .ok_or(EngineError::ProfileNotFound { profile_id })?;
        if self.profiles.len() == 1 {
            return Err(EngineError::CannotRemoveLastProfile);
        }
        let removed = self.profiles.remove(index);
        if self.active_profile_id == profile_id {
            self.active_profile_id = self.profiles[0].profile_id;
        }
        Ok(removed)
    }

    #[must_use]
    pub fn get(&self, profile_id: ProfileId) -> Option<&ProfileEntry> {
        self.profiles
            .iter()
            .find(|profile| profile.profile_id == profile_id)
    }

    pub fn get_mut(&mut self, profile_id: ProfileId) -> Option<&mut ProfileEntry> {
        self.profiles
            .iter_mut()
            .find(|profile| profile.profile_id == profile_id)
    }
}

/// Result of loading a multi-profile catalog (with optional migration/recovery).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedCatalog {
    pub catalog: ProfileCatalog,
    pub migrated: bool,
    pub recovery: Option<ProfileStoreRecovery>,
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

    /// Load the stored profile, or preserve a corrupt document and install defaults.
    ///
    /// When the corrupt document still has a readable profile_id, that identity
    /// is preserved so existing profile directories remain addressable. Otherwise a
    /// new id from defaults is used.
    pub fn load_or_recover(
        &self,
        defaults: &ExternalEngineProfile,
    ) -> Result<LoadedProfile, EngineError> {
        if !self.path.exists() {
            self.save(defaults)?;
            return Ok(LoadedProfile {
                profile: defaults.clone(),
                recovery: None,
            });
        }

        // Read raw bytes first so identity can be recovered before rename.
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) => {
                return Err(io_error("read the profile metadata", &self.path, error));
            }
        };
        match serde_json::from_slice::<ExternalEngineProfile>(&bytes) {
            Ok(profile) => Ok(LoadedProfile {
                profile,
                recovery: None,
            }),
            Err(error) => {
                let reason = error.to_string();
                let recovered_id = serde_json::from_slice::<serde_json::Value>(&bytes)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("profile_id")
                            .and_then(|id| id.as_str())
                            .and_then(|id| id.parse::<ProfileId>().ok())
                    });
                let backup_path = self.preserve_corrupt_document_bytes(&bytes)?;
                let mut profile = defaults.clone();
                if let Some(profile_id) = recovered_id {
                    profile.profile_id = profile_id;
                }
                self.save(&profile)?;
                Ok(LoadedProfile {
                    profile,
                    recovery: Some(ProfileStoreRecovery {
                        backup_path,
                        reason,
                    }),
                })
            }
        }
    }

    /// Load the multi-profile catalog, migrating a legacy single-profile document.
    pub fn load_catalog_or_recover(
        &self,
        defaults: &ExternalEngineProfile,
    ) -> Result<LoadedCatalog, EngineError> {
        if !self.path.exists() {
            let catalog = ProfileCatalog::from_single(defaults.clone());
            self.save_catalog(&catalog)?;
            return Ok(LoadedCatalog {
                catalog,
                migrated: false,
                recovery: None,
            });
        }

        let bytes = fs::read(&self.path)
            .map_err(|error| io_error("read the profile catalog", &self.path, error))?;

        if let Ok(catalog) = serde_json::from_slice::<ProfileCatalog>(&bytes) {
            catalog.validate()?;
            return Ok(LoadedCatalog {
                catalog,
                migrated: false,
                recovery: None,
            });
        }

        if let Ok(profile) = serde_json::from_slice::<ExternalEngineProfile>(&bytes) {
            let catalog = ProfileCatalog::from_single(profile);
            self.save_catalog(&catalog)?;
            return Ok(LoadedCatalog {
                catalog,
                migrated: true,
                recovery: None,
            });
        }

        let reason = "profile catalog JSON is malformed".to_owned();
        let recovered_id = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|value| {
                value
                    .get("active_profile_id")
                    .or_else(|| value.get("profile_id"))
                    .and_then(|id| id.as_str())
                    .and_then(|id| id.parse::<ProfileId>().ok())
            });
        let backup_path = self.preserve_corrupt_document_bytes(&bytes)?;
        let mut defaults = defaults.clone();
        if let Some(profile_id) = recovered_id {
            defaults.profile_id = profile_id;
        }
        let catalog = ProfileCatalog::from_single(defaults);
        self.save_catalog(&catalog)?;
        Ok(LoadedCatalog {
            catalog,
            migrated: true,
            recovery: Some(ProfileStoreRecovery {
                backup_path,
                reason,
            }),
        })
    }

    pub fn save_catalog(&self, catalog: &ProfileCatalog) -> Result<(), EngineError> {
        catalog.validate()?;
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

        let payload = serde_json::to_vec_pretty(catalog)
            .map_err(|source| EngineError::Serialize { source })?;
        let temp_path = parent.join(format!(
            ".{}.{}.tmp",
            self.path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("profiles"),
            Uuid::new_v4()
        ));

        let write_result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp_path)
                .map_err(|error| {
                    io_error("create the temporary profile catalog", &temp_path, error)
                })?;
            file.write_all(&payload)
                .map_err(|error| io_error("write the profile catalog", &temp_path, error))?;
            file.write_all(b"\n")
                .map_err(|error| io_error("finish the profile catalog", &temp_path, error))?;
            file.flush()
                .map_err(|error| io_error("flush the profile catalog", &temp_path, error))?;
            file.sync_all()
                .map_err(|error| io_error("sync the profile catalog", &temp_path, error))?;
            replace_file(&temp_path, &self.path).map_err(|error| {
                io_error("atomically replace the profile catalog", &self.path, error)
            })
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        write_result
    }

    fn preserve_corrupt_document_bytes(&self, bytes: &[u8]) -> Result<PathBuf, EngineError> {
        let parent = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|error| io_error("create the profile store directory", parent, error))?;
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("profiles.json");
        let backup_path = parent.join(format!("{file_name}.corrupt-{}.bak", Uuid::new_v4()));
        // Prefer rename of the live path; fall back to writing the captured bytes.
        match fs::rename(&self.path, &backup_path) {
            Ok(()) => Ok(backup_path),
            Err(_) => {
                fs::write(&backup_path, bytes).map_err(|error| {
                    io_error("preserve the corrupt profile document", &backup_path, error)
                })?;
                let _ = fs::remove_file(&self.path);
                Ok(backup_path)
            }
        }
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
        sync::Mutex,
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
    fn managed_aria2_arguments_persist_uploaded_metadata_and_allow_large_rpc_requests() {
        let root = temporary_directory();
        let profile = sample_profile(&root);
        let arguments = local_engine_arguments(
            &profile.local_config(),
            4_321,
            "secret",
            &root.join("aria2.conf"),
            &root.join("aria2.session"),
            &root.join("aria2.log"),
        );

        assert!(arguments.contains(&"--rpc-save-upload-metadata=true".to_owned()));
        assert!(arguments.contains(&"--rpc-max-request-size=32M".to_owned()));
        assert!(arguments.contains(&"--max-download-result=5000".to_owned()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_path_opening_passes_the_path_as_one_process_argument() {
        let path = Path::new(r"C:\Downloads\name & more.bin");
        let file = open_command_spec(path, true);
        let folder = open_command_spec(path, false);

        #[cfg(target_os = "windows")]
        {
            assert_eq!(file.program, "rundll32.exe");
            assert_eq!(file.arguments[0], "url.dll,FileProtocolHandler");
            assert_eq!(file.arguments[1], path.to_string_lossy());
            assert_eq!(folder.program, "explorer.exe");
            assert_eq!(folder.arguments, vec![path.to_string_lossy()]);
        }
        #[cfg(not(target_os = "windows"))]
        {
            assert_eq!(file.arguments, vec![path.to_string_lossy()]);
            assert_eq!(folder.arguments, vec![path.to_string_lossy()]);
        }
    }

    #[derive(Default)]
    struct RecordingTrash {
        paths: Mutex<Vec<PathBuf>>,
    }

    impl TrashBackend for RecordingTrash {
        fn move_to_trash(&self, path: &Path) -> Result<(), String> {
            self.paths
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(path.to_path_buf());
            Ok(())
        }
    }

    #[test]
    fn local_download_destination_preflight_checks_writability_and_free_space() {
        let root = temporary_directory();
        let downloads = root.join("downloads");
        fs::create_dir_all(&downloads).expect("create download directory");
        let gateway = LocalDownloadDestinationGateway::new();
        let request = DownloadDestinationRequest {
            directory: ariadeck_domain::EnginePath::new(downloads.to_string_lossy()),
            required_bytes: None,
            files: Vec::new(),
        };

        let report = gateway.preflight(&request).expect("destination preflight");

        assert!(report.available_bytes > 0);
        assert!(
            fs::read_dir(&downloads)
                .expect("list download directory")
                .next()
                .is_none(),
            "the writability probe must be removed"
        );
        let _ = fs::remove_dir_all(root);
    }

    /// On Windows, FILE_ATTRIBUTE_READONLY on a directory does not mean the
    /// folder is non-writable; only a real write probe is authoritative.
    #[cfg(windows)]
    #[test]
    // Clearing the fixture attribute is intentional Windows test cleanup (not Unix mode bits).
    #[allow(clippy::permissions_set_readonly_false)]
    fn local_download_destination_ignores_directory_readonly_attribute() {
        let root = temporary_directory();
        let downloads = root.join("downloads");
        fs::create_dir_all(&downloads).expect("create download directory");
        let mut permissions = fs::metadata(&downloads).expect("metadata").permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&downloads, permissions).expect("mark directory readonly attr");
        assert!(
            fs::metadata(&downloads)
                .expect("metadata after attr")
                .permissions()
                .readonly(),
            "fixture must expose the readonly attribute"
        );

        let gateway = LocalDownloadDestinationGateway::new();
        let report = gateway
            .preflight(&DownloadDestinationRequest {
                directory: ariadeck_domain::EnginePath::new(downloads.to_string_lossy()),
                required_bytes: None,
                files: Vec::new(),
            })
            .expect("writable directory with readonly attr must pass");
        assert!(report.available_bytes > 0);

        let mut permissions = fs::metadata(&downloads).expect("metadata").permissions();
        permissions.set_readonly(false);
        let _ = fs::set_permissions(&downloads, permissions);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_download_destination_rejects_relative_paths_and_insufficient_space() {
        let gateway = LocalDownloadDestinationGateway::new();
        let relative = gateway
            .preflight(&DownloadDestinationRequest {
                directory: ariadeck_domain::EnginePath::new("downloads"),
                required_bytes: None,
                files: Vec::new(),
            })
            .expect_err("relative local path must fail");
        assert_eq!(relative.kind, GatewayErrorKind::UnsafePath);

        let root = temporary_directory();
        let insufficient = gateway
            .preflight(&DownloadDestinationRequest {
                directory: ariadeck_domain::EnginePath::new(root.to_string_lossy()),
                required_bytes: Some(u64::MAX),
                files: Vec::new(),
            })
            .expect_err("oversized download must fail");
        assert_eq!(insufficient.kind, GatewayErrorKind::Filesystem);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_download_destination_validates_metadata_paths_and_reject_conflicts() {
        let root = temporary_directory();
        let gateway = LocalDownloadDestinationGateway::new();
        let request = |relative_path: ariadeck_domain::EnginePath| DownloadDestinationRequest {
            directory: ariadeck_domain::EnginePath::new(root.to_string_lossy()),
            required_bytes: Some(30),
            files: vec![DownloadDestinationFile {
                relative_path,
                reject_existing: true,
            }],
        };

        gateway
            .preflight(&request(ariadeck_domain::EnginePath::new("nested/new.bin")))
            .expect("safe relative metadata path");

        let existing = root.join("existing.bin");
        fs::write(&existing, b"content").expect("create conflicting destination");
        let conflict = gateway
            .preflight(&request(ariadeck_domain::EnginePath::new("existing.bin")))
            .expect_err("Reject policy must catch an existing destination");
        assert_eq!(conflict.kind, GatewayErrorKind::Rejected);

        for unsafe_path in [
            ariadeck_domain::EnginePath::new("../escape.bin"),
            ariadeck_domain::EnginePath::new(existing.to_string_lossy()),
        ] {
            let error = gateway
                .preflight(&request(unsafe_path))
                .expect_err("unsafe metadata path must fail");
            assert_eq!(error.kind, GatewayErrorKind::UnsafePath);
        }

        let parent_file = root.join("not-a-directory");
        fs::write(&parent_file, b"content").expect("create non-directory parent");
        let error = gateway
            .preflight(&request(ariadeck_domain::EnginePath::new(
                "not-a-directory/child.bin",
            )))
            .expect_err("file parent must fail containment validation");
        assert_eq!(error.kind, GatewayErrorKind::UnsafePath);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[cfg(unix)]
    fn local_download_destination_rejects_symlink_components() {
        use std::os::unix::fs::symlink;

        let root = temporary_directory();
        let outside = temporary_directory();
        let nested = root.join("nested");
        fs::create_dir_all(&nested).expect("create nested");
        let link = nested.join("escape");
        symlink(&outside, &link).expect("create symlink escape");

        let gateway = LocalDownloadDestinationGateway::new();
        let error = gateway
            .preflight(&DownloadDestinationRequest {
                directory: ariadeck_domain::EnginePath::new(root.to_string_lossy()),
                required_bytes: Some(1),
                files: vec![DownloadDestinationFile {
                    relative_path: ariadeck_domain::EnginePath::new("nested/escape/secret.bin"),
                    reject_existing: false,
                }],
            })
            .expect_err("symlink path components must be rejected");
        assert_eq!(error.kind, GatewayErrorKind::UnsafePath);

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn successful_destination_preflight_accumulates_authorized_removal_roots() {
        let root = temporary_directory();
        let first = root.join("first");
        let second = root.join("second");
        let unconfigured = root.join("unconfigured");
        for directory in [&first, &second, &unconfigured] {
            fs::create_dir_all(directory).expect("create download root");
            fs::write(directory.join("item.bin"), b"content").expect("create task file");
        }
        let roots = LocalDownloadRootRegistry::new(&first);
        let destination = LocalDownloadDestinationGateway::with_roots(roots.clone());
        let files = LocalTaskFileGateway::with_roots(roots);
        destination
            .preflight(&DownloadDestinationRequest {
                directory: ariadeck_domain::EnginePath::new(second.to_string_lossy()),
                required_bytes: None,
                files: Vec::new(),
            })
            .expect("authorize second root");
        let removal = |directory: &Path| TaskFileRemovalRequest {
            directory: ariadeck_domain::EnginePath::new(directory.to_string_lossy()),
            files: vec![ariadeck_domain::EnginePath::new(
                directory.join("item.bin").to_string_lossy(),
            )],
            include_control_files: false,
        };

        assert_eq!(
            files
                .preflight(&removal(&first))
                .expect("first root")
                .content_files,
            1
        );
        assert_eq!(
            files
                .preflight(&removal(&second))
                .expect("second root")
                .content_files,
            1
        );
        let outside = files
            .preflight(&removal(&unconfigured))
            .expect_err("unconfigured root must remain forbidden");
        assert_eq!(outside.kind, GatewayErrorKind::UnsafePath);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_file_gateway_move_to_trash_works_without_current_tokio_runtime() {
        // Mirrors GPUI task threads: no entered Tokio runtime must not panic.
        let root = temporary_directory();
        let downloads = root.join("downloads");
        let content = downloads.join("item.bin");
        fs::create_dir_all(&downloads).expect("create download directory");
        fs::write(&content, b"content").expect("create content file");
        let trash = Arc::new(RecordingTrash::default());
        let gateway = LocalTaskFileGateway::with_trash(&downloads, trash.clone());
        let request = TaskFileRemovalRequest {
            directory: ariadeck_domain::EnginePath::new(downloads.to_string_lossy()),
            files: vec![ariadeck_domain::EnginePath::new(content.to_string_lossy())],
            include_control_files: false,
        };

        let report = std::thread::spawn(move || {
            // Dedicated single-thread runtime only to drive the Future; the
            // worker path must not require Handle::current() at the call site.
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("test runtime");
            // Drop any ambient handle by not calling enter() before the future
            // body checks try_current — instead drive via block_on which does
            // enter, so also exercise the spawn_blocking branch. A true
            // no-runtime drive uses a raw poll below via oneshot join.
            runtime.block_on(gateway.move_to_trash(&request))
        })
        .join()
        .expect("worker thread")
        .expect("move without ambient panic");
        assert_eq!(report.moved_to_trash, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn run_blocking_falls_back_when_no_tokio_handle_is_available() {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // Poll the future with a noop waker so try_current() is Err.
            let future = run_blocking(|| Ok::<_, GatewayError>(42_u32));
            futures_executor_block_on(future, &sender);
        });
        let value = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("run_blocking completed");
        assert_eq!(value.expect("fallback work"), 42);
    }

    fn futures_executor_block_on<T>(
        future: impl std::future::Future<Output = T>,
        sender: &std::sync::mpsc::Sender<T>,
    ) {
        use std::{
            pin::pin,
            sync::Arc,
            task::{Context, Poll, Wake, Waker},
            thread,
        };
        struct ThreadWaker(thread::Thread);
        impl Wake for ThreadWaker {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
        }
        let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => {
                    let _ = sender.send(value);
                    return;
                }
                Poll::Pending => thread::park(),
            }
        }
    }

    #[tokio::test]
    async fn local_file_gateway_moves_only_exact_files_and_control_files() {
        let root = temporary_directory();
        let downloads = root.join("downloads");
        let task_dir = downloads.join("task");
        fs::create_dir_all(&task_dir).expect("create task directory");
        let content = task_dir.join("item.bin");
        let direct_control = task_dir.join("item.bin.aria2");
        let task_control = downloads.join("task.aria2");
        let unrelated = task_dir.join("unrelated.txt");
        for path in [&content, &direct_control, &task_control, &unrelated] {
            fs::write(path, b"test").expect("create task fixture");
        }
        let trash = Arc::new(RecordingTrash::default());
        let gateway = LocalTaskFileGateway::with_trash(&downloads, trash.clone());
        let request = TaskFileRemovalRequest {
            directory: ariadeck_domain::EnginePath::new(downloads.to_string_lossy()),
            files: vec![ariadeck_domain::EnginePath::new(content.to_string_lossy())],
            include_control_files: true,
        };

        let preview = gateway.preflight(&request).expect("safe preflight");
        assert_eq!(preview.content_files, 1);
        assert_eq!(preview.control_files, 2);
        let report = gateway
            .move_to_trash(&request)
            .await
            .expect("move exact files to trash");
        assert_eq!(report.moved_to_trash, 3);
        let moved = trash
            .paths
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(moved.contains(&fs::canonicalize(content).expect("content path")));
        assert!(moved.contains(&fs::canonicalize(direct_control).expect("control path")));
        assert!(moved.contains(&fs::canonicalize(task_control).expect("task control path")));
        assert!(!moved.contains(&fs::canonicalize(unrelated).expect("unrelated path")));
        drop(moved);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_file_gateway_rejects_outside_traversal_and_directories() {
        let root = temporary_directory();
        let downloads = root.join("downloads");
        let task_dir = downloads.join("task");
        let outside = root.join("outside.bin");
        fs::create_dir_all(&task_dir).expect("create task directory");
        fs::write(&outside, b"outside").expect("create outside file");
        let gateway = LocalTaskFileGateway::new(&downloads);
        let request = |file: ariadeck_domain::EnginePath| TaskFileRemovalRequest {
            directory: ariadeck_domain::EnginePath::new(downloads.to_string_lossy()),
            files: vec![file],
            include_control_files: false,
        };

        for file in [
            ariadeck_domain::EnginePath::new(outside.to_string_lossy()),
            ariadeck_domain::EnginePath::new("../outside.bin"),
            ariadeck_domain::EnginePath::new(task_dir.to_string_lossy()),
        ] {
            let error = gateway
                .preflight(&request(file))
                .expect_err("unsafe path must be rejected");
            assert_eq!(error.kind, GatewayErrorKind::UnsafePath);
        }
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn local_file_gateway_accepts_windows_paths_with_different_casing() {
        let root = temporary_directory();
        let downloads = root.join("downloads");
        let task_dir = downloads.join("task");
        let content = task_dir.join("item.bin");
        fs::create_dir_all(&task_dir).expect("create task directory");
        fs::write(&content, b"content").expect("create content file");
        let gateway = LocalTaskFileGateway::new(&downloads);
        let request = TaskFileRemovalRequest {
            directory: ariadeck_domain::EnginePath::new(
                downloads.to_string_lossy().to_ascii_uppercase(),
            ),
            files: vec![ariadeck_domain::EnginePath::new(
                content.to_string_lossy().to_ascii_uppercase(),
            )],
            include_control_files: false,
        };

        let preview = gateway
            .preflight(&request)
            .expect("case-insensitive Windows path");

        assert_eq!(preview.content_files, 1);
        let _ = fs::remove_dir_all(root);
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
        let endpoint = process.endpoint().clone();
        let secret = process.secret().to_owned();
        assert!(process.restart().is_ok());
        assert_eq!(process.endpoint(), &endpoint);
        assert_eq!(process.secret(), secret);
        assert!(process.is_running().unwrap_or(false));
        assert!(process.shutdown().is_ok());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[ignore = "requires ARIA2C_PATH and launches a real local aria2 process"]
    fn supervisor_restarts_once_then_stops_at_the_crash_budget() {
        fn kill_current_child(supervisor: &LocalEngineSupervisor) -> u32 {
            let mut process = supervisor
                .shared
                .process
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let child = process.child.as_mut().expect("supervised child must exist");
            let pid = child.id();
            child
                .kill()
                .unwrap_or_else(|error| panic!("failed to terminate supervised child: {error}"));
            child
                .wait()
                .unwrap_or_else(|error| panic!("failed to reap supervised child: {error}"));
            pid
        }

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
        let policy = LocalRestartPolicy {
            max_restarts: 1,
            window: Duration::from_secs(10),
            poll_interval: Duration::from_millis(25),
        };
        let mut supervisor =
            LocalEngineSupervisor::spawn_with_policy(&profile.local_config(), policy)
                .unwrap_or_else(|error| panic!("failed to spawn supervisor: {error}"));
        let health_handle = supervisor.health_handle();
        let endpoint = supervisor.endpoint().clone();
        let secret = supervisor.secret().to_owned();
        let original_pid = kill_current_child(&supervisor);

        let deadline = Instant::now() + Duration::from_secs(5);
        let restarted_pid = loop {
            let pid = {
                let process = supervisor
                    .shared
                    .process
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                process.child.as_ref().map(Child::id)
            };
            if supervisor.health() == (LocalEngineHealth::Running { restarts: 1 })
                && pid.is_some_and(|pid| pid != original_pid)
            {
                break pid.expect("restarted child must exist");
            }
            assert!(
                Instant::now() < deadline,
                "supervisor did not restart aria2"
            );
            thread::sleep(Duration::from_millis(25));
        };

        assert_ne!(restarted_pid, original_pid);
        assert_eq!(supervisor.endpoint(), &endpoint);
        assert_eq!(supervisor.secret(), secret);

        assert_eq!(kill_current_child(&supervisor), restarted_pid);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let LocalEngineHealth::Failed { restarts, reason } = supervisor.health() {
                assert_eq!(restarts, 1);
                assert!(!reason.is_empty());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "supervisor did not stop at its restart budget"
            );
            thread::sleep(Duration::from_millis(25));
        }

        assert!(supervisor.shutdown().is_ok());
        drop(supervisor);
        assert_eq!(health_handle.health(), None);
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

    #[test]
    fn prepare_session_file_recovers_corrupt_contents() {
        let root = temporary_directory();
        let session_path = root.join("aria2.session");
        fs::write(&session_path, b"not\x00utf8\xff session").expect("seed corrupt session");

        let prepared = prepare_session_file(&session_path)
            .unwrap_or_else(|error| panic!("prepare failed: {error}"));
        assert!(prepared.recovered_from_corruption);
        let backup = prepared
            .backup_path
            .expect("corrupt session must be preserved");
        assert!(backup.is_file());
        assert_eq!(fs::read(&session_path).expect("read reset session"), b"");

        let clean = prepare_session_file(&session_path)
            .unwrap_or_else(|error| panic!("second prepare failed: {error}"));
        assert!(!clean.recovered_from_corruption);
        assert!(clean.backup_path.is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn prepare_session_file_accepts_plausible_text() {
        let root = temporary_directory();
        let session_path = root.join("aria2.session");
        fs::write(
            &session_path,
            b"http://example.invalid/file\n\tout=file.bin\n",
        )
        .expect("seed session");
        let prepared = prepare_session_file(&session_path)
            .unwrap_or_else(|error| panic!("prepare failed: {error}"));
        assert!(!prepared.recovered_from_corruption);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn profile_ownership_lock_is_exclusive_and_reclaims_stale_pid() {
        let root = temporary_directory();
        let profile_id = ProfileId::new();
        let profile_dir = root.join(profile_id.to_string());
        fs::create_dir_all(&profile_dir).expect("profile dir");

        let first = ProfileOwnershipLock::acquire(profile_id, &profile_dir)
            .unwrap_or_else(|error| panic!("first lock failed: {error}"));
        assert!(first.path().is_file());
        assert_eq!(first.owner_pid(), std::process::id());

        let second = ProfileOwnershipLock::acquire(profile_id, &profile_dir);
        assert!(
            matches!(
                second,
                Err(EngineError::ProfileAlreadyOwned {
                    owner_pid,
                    ..
                }) if owner_pid == std::process::id()
            ),
            "live owner must fail closed: {second:?}"
        );

        drop(first);
        // Stale lock file with a dead pid should be reclaimable.
        // Avoid pid 1: on Linux/macOS CI it is always the live init process.
        let stale_pid = unused_process_id();
        let stale_path = profile_dir.join(".ariadeck-engine.lock");
        fs::write(
            &stale_path,
            format!(
                "{{\"profile_id\":\"{profile_id}\",\"owner_pid\":{stale_pid},\"acquired_unix_secs\":0}}\n"
            ),
        )
        .expect("seed stale lock");
        let reclaimed = ProfileOwnershipLock::acquire(profile_id, &profile_dir)
            .unwrap_or_else(|error| panic!("stale reclaim failed: {error}"));
        assert_eq!(reclaimed.owner_pid(), std::process::id());
        drop(reclaimed);
        let _ = fs::remove_dir_all(root);
    }

    /// Pick a pid that does not appear alive so lock reclaim tests stay portable.
    fn unused_process_id() -> u32 {
        // High end of 32-bit pid space is unused on typical desktop/CI hosts.
        for candidate in (1_000_000_u32..=1_000_050).chain([u32::MAX - 1, u32::MAX]) {
            if candidate != std::process::id() && !process_appears_alive(candidate) {
                return candidate;
            }
        }
        // Last resort: current pid + large offset (still must not appear alive).
        std::process::id().wrapping_add(50_000).max(2)
    }

    #[test]
    fn profile_store_load_or_recover_preserves_identity_from_corrupt_document() {
        let root = temporary_directory();
        let profile_id = ProfileId::new();
        let store = JsonProfileStore::new(root.join("profiles.json"));
        let defaults = ExternalEngineProfile::new(
            ProfileId::new(),
            "Recovered",
            std::env::current_exe().expect("exe"),
            root.join("data"),
            root.join("downloads"),
        );
        fs::write(
            store.path(),
            format!(
                "{{\"profile_id\":\"{profile_id}\",\"name\":true,\"executable\":\"x\",\"data_dir\":\"y\",\"download_dir\":\"z\"}}"
            ),
        )
        .expect("seed corrupt profile");

        let loaded = store
            .load_or_recover(&defaults)
            .unwrap_or_else(|error| panic!("recover failed: {error}"));
        assert_eq!(loaded.profile.profile_id, profile_id);
        assert_eq!(loaded.profile.name, "Recovered");
        let recovery = loaded.recovery.expect("recovery metadata");
        assert!(recovery.backup_path.is_file());
        assert_eq!(
            store
                .load()
                .unwrap_or_else(|error| panic!("load recovered failed: {error}"))
                .profile_id,
            profile_id
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn profile_catalog_migrates_legacy_single_profile_and_round_trips() {
        let root = temporary_directory();
        let legacy = sample_profile(&root);
        let store = JsonProfileStore::new(root.join("profiles.json"));
        store.save(&legacy).expect("save legacy");

        let loaded = store
            .load_catalog_or_recover(&legacy)
            .expect("migrate catalog");
        assert!(loaded.migrated);
        assert_eq!(loaded.catalog.profiles.len(), 1);
        assert_eq!(loaded.catalog.active_profile_id, legacy.profile_id);
        assert_eq!(
            loaded.catalog.active().map(|p| p.name.as_str()),
            Some(legacy.name.as_str())
        );

        let remote = ProfileEntry::remote_rpc(
            ProfileId::new(),
            "NAS",
            "wss://aria2.example.invalid/jsonrpc",
            root.join("nas-downloads"),
            None,
        )
        .expect("remote entry");
        let mut catalog = loaded.catalog;
        catalog.upsert(remote.clone()).expect("upsert remote");
        catalog
            .set_active(remote.profile_id)
            .expect("activate remote");
        store.save_catalog(&catalog).expect("save catalog");

        let reloaded = store
            .load_catalog_or_recover(&legacy)
            .expect("reload catalog");
        assert!(!reloaded.migrated);
        assert_eq!(reloaded.catalog.profiles.len(), 2);
        assert_eq!(reloaded.catalog.active_profile_id, remote.profile_id);
        assert_eq!(
            reloaded.catalog.active().map(|p| p.kind),
            Some(ProfileKind::RemoteRpc)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remote_profile_endpoint_validation_rejects_http_and_empty() {
        let root = temporary_directory();
        assert!(matches!(
            ProfileEntry::remote_rpc(
                ProfileId::new(),
                "Bad",
                "http://example.invalid/jsonrpc",
                root.join("d"),
                None,
            ),
            Err(EngineError::InvalidRemoteEndpoint { .. })
        ));
        assert!(matches!(
            ProfileEntry::remote_rpc(ProfileId::new(), "Bad", "", root.join("d"), None),
            Err(EngineError::InvalidRemoteEndpoint { .. })
                | Err(EngineError::MissingRemoteEndpoint)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn local_profile_allows_empty_executable_for_managed_core() {
        let root = temporary_directory();
        let mut entry = ProfileEntry::local_managed(
            ProfileId::new(),
            "Managed",
            PathBuf::new(),
            root.clone(),
            root.join("downloads"),
        );
        entry.executable = None;
        entry
            .validate()
            .expect("empty executable is managed-core opt-in");
        assert!(entry.uses_managed_core());
        let config = entry.as_local_config().expect("local config");
        assert!(config.executable.as_os_str().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn catalog_refuses_to_remove_the_last_profile() {
        let root = temporary_directory();
        let profile = sample_profile(&root);
        let mut catalog = ProfileCatalog::from_single(profile.clone());
        assert!(matches!(
            catalog.remove(profile.profile_id),
            Err(EngineError::CannotRemoveLastProfile)
        ));
        let _ = fs::remove_dir_all(root);
    }
}
