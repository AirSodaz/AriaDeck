//! Local filesystem gateways: download destination preflight, Trash removal,
//! path containment, reparse-point guards, and opening local paths.

use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
};

use ariadeck_application::{
    DownloadDestinationFile, DownloadDestinationGateway, DownloadDestinationReport,
    DownloadDestinationRequest, GatewayError, GatewayErrorKind, TaskFileGateway,
    TaskFileRemovalPreview, TaskFileRemovalReport, TaskFileRemovalRequest, TaskOpenRequest,
    TaskOpenTarget,
};
use async_trait::async_trait;
use sysinfo::Disks;
use uuid::Uuid;

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
        if is_symlink_or_reparse_point(&metadata) {
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

fn is_symlink_or_reparse_point(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink() || is_windows_reparse_point(metadata)
}

#[cfg(windows)]
fn is_windows_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;

    // Junctions and other name-surrogate reparse points are not reported by
    // FileType::is_symlink(), but can redirect a path after containment checks.
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_windows_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
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
    pub(crate) fn with_trash(
        download_root: impl Into<PathBuf>,
        trash: Arc<dyn TrashBackend>,
    ) -> Self {
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
pub(crate) async fn run_blocking<T, F>(work: F) -> Result<T, GatewayError>
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
pub(crate) struct OpenCommandSpec {
    pub(crate) program: &'static str,
    pub(crate) arguments: Vec<String>,
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

pub(crate) fn open_command_spec(path: &Path, is_file: bool) -> OpenCommandSpec {
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

pub(crate) trait TrashBackend: Send + Sync {
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
    if is_symlink_or_reparse_point(&metadata) {
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
    if is_symlink_or_reparse_point(&metadata) {
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
