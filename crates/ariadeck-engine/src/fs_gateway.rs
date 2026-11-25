//! Local filesystem gateways: download destination preflight, Trash removal,
//! path containment, reparse-point guards, and opening local paths.

use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::Stdio,
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
        Self::with_initial_roots([initial_root.into()])
    }

    #[must_use]
    pub fn with_initial_roots(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut deduped: Vec<PathBuf> = Vec::new();
        for root in roots {
            if root.as_os_str().is_empty() || deduped.iter().any(|existing| existing == &root) {
                continue;
            }
            deduped.push(root);
        }
        Self {
            roots: Arc::new(Mutex::new(deduped)),
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
    let disks = Disks::new_with_refreshed_list().map_err(|error| {
        filesystem_gateway_error(
            format!("could not enumerate storage devices: {error}"),
            true,
        )
    })?;
    disks
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
    crate::console_free_command(spec.program)
        .args(&spec.arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| filesystem_error("open task path", path, error, true))
}

#[cfg(target_os = "windows")]
fn strip_windows_verbatim_prefix(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if let Some(unc_tail) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc_tail}")
    } else if let Some(drive_tail) = raw.strip_prefix(r"\\?\") {
        drive_tail.to_owned()
    } else {
        raw.into_owned()
    }
}

pub(crate) fn open_command_spec(path: &Path, is_file: bool) -> OpenCommandSpec {
    #[cfg(target_os = "windows")]
    {
        let value = strip_windows_verbatim_prefix(path);
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
        let value = path.to_string_lossy().into_owned();
        let _ = is_file;
        OpenCommandSpec {
            program: "open",
            arguments: vec![value],
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let value = path.to_string_lossy().into_owned();
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
            if task_dir != root {
                validate_no_reparse_ancestors(&root, &raw_task_dir, "task directory")?;
            }
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
    validate_no_reparse_ancestors(task_dir, path, "task file")?;
    Ok(Some(canonical))
}

fn validate_no_reparse_ancestors(
    canonical_base: &Path,
    raw_target: &Path,
    label: &str,
) -> Result<(), GatewayError> {
    let mut current = raw_target.parent();
    while let Some(ancestor) = current {
        let metadata = fs::symlink_metadata(ancestor).map_err(|error| {
            filesystem_error(&format!("inspect {label} parent"), ancestor, error, true)
        })?;
        if is_symlink_or_reparse_point(&metadata) {
            return Err(unsafe_path_error(format!(
                "{label} cannot traverse a symlink or reparse point: {}",
                ancestor.display()
            )));
        }
        let canonical = fs::canonicalize(ancestor).map_err(|error| {
            filesystem_error(
                &format!("canonicalize {label} parent"),
                ancestor,
                error,
                true,
            )
        })?;
        if canonical == canonical_base {
            return Ok(());
        }
        if !canonical.starts_with(canonical_base) {
            return Err(unsafe_path_error(format!(
                "{label} parent resolves outside its permitted root: {}",
                ancestor.display()
            )));
        }
        current = ancestor.parent();
    }
    Err(unsafe_path_error(format!(
        "{label} does not descend from its permitted root: {}",
        raw_target.display()
    )))
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

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use crate::console_free_command;
    use ariadeck_application::{
        DownloadDestinationFile, DownloadDestinationGateway, DownloadDestinationRequest,
        GatewayErrorKind, TaskFileGateway, TaskFileRemovalRequest,
    };
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
        time::Duration,
    };
    use uuid::Uuid;

    fn temporary_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!("ariadeck-engine-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create test dir");
        path
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

    #[cfg(windows)]
    fn create_directory_junction(link: &Path, target: &Path) {
        let output = console_free_command("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .expect("run mklink for junction fixture");
        assert!(
            output.status.success(),
            "mklink failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_reparse_points_are_rejected_by_destination_preflight() {
        let root = temporary_directory();
        let downloads = root.join("downloads");
        let actual = downloads.join("actual");
        let junction = downloads.join("junction");
        fs::create_dir_all(&actual).expect("create junction target");
        create_directory_junction(&junction, &actual);

        let gateway = LocalDownloadDestinationGateway::new();
        let error = gateway
            .preflight(&DownloadDestinationRequest {
                directory: ariadeck_domain::EnginePath::new(downloads.to_string_lossy()),
                required_bytes: Some(1),
                files: vec![DownloadDestinationFile {
                    relative_path: ariadeck_domain::EnginePath::new("junction/new.bin"),
                    reject_existing: false,
                }],
            })
            .expect_err("junction components must be rejected even within the download root");
        assert_eq!(error.kind, GatewayErrorKind::UnsafePath);

        fs::remove_dir(&junction).expect("remove junction fixture");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn windows_reparse_points_are_rejected_by_file_gateway() {
        let root = temporary_directory();
        let downloads = root.join("downloads");
        let actual = downloads.join("actual");
        let junction = downloads.join("task");
        let content = actual.join("item.bin");
        fs::create_dir_all(&actual).expect("create junction target");
        fs::write(&content, b"content").expect("create content file");
        create_directory_junction(&junction, &actual);

        let gateway = LocalTaskFileGateway::new(&downloads);
        let error = gateway
            .preflight(&TaskFileRemovalRequest {
                directory: ariadeck_domain::EnginePath::new(junction.to_string_lossy()),
                files: vec![ariadeck_domain::EnginePath::new(
                    junction.join("item.bin").to_string_lossy(),
                )],
                include_control_files: false,
            })
            .expect_err("junction task directories must never reach Trash");
        assert_eq!(error.kind, GatewayErrorKind::UnsafePath);

        fs::remove_dir(&junction).expect("remove junction fixture");
        let _ = fs::remove_dir_all(root);
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
    fn secondary_initial_root_authorizes_files_without_prior_preflight() {
        let root = temporary_directory();
        let fallback_dir = root.join("fallback");
        let video_dir = root.join("videos");
        let task_dir = video_dir.join("movie");
        let content = task_dir.join("clip.mkv");
        fs::create_dir_all(&fallback_dir).expect("create fallback dir");
        fs::create_dir_all(&task_dir).expect("create video task dir");
        fs::write(&content, b"video").expect("write video file");

        let registry = LocalDownloadRootRegistry::with_initial_roots([
            fallback_dir.clone(),
            video_dir.clone(),
            video_dir,
        ]);
        let gateway = LocalTaskFileGateway::with_roots(registry);
        let request = TaskFileRemovalRequest {
            directory: ariadeck_domain::EnginePath::new(task_dir.to_string_lossy()),
            files: vec![ariadeck_domain::EnginePath::new(content.to_string_lossy())],
            include_control_files: false,
        };

        let preview = gateway
            .preflight(&request)
            .expect("secondary category root must be authorized");
        assert_eq!(preview.content_files, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn windows_intermediate_junction_inside_task_dir_is_rejected() {
        let root = temporary_directory();
        let downloads = root.join("downloads");
        let task_dir = downloads.join("task");
        let actual = task_dir.join("actual");
        let junction = task_dir.join("nested_junction");
        let content = actual.join("item.bin");
        fs::create_dir_all(&actual).expect("create actual subdir");
        fs::write(&content, b"content").expect("create content file");
        create_directory_junction(&junction, &actual);

        let gateway = LocalTaskFileGateway::new(&downloads);
        let error = gateway
            .preflight(&TaskFileRemovalRequest {
                directory: ariadeck_domain::EnginePath::new(task_dir.to_string_lossy()),
                files: vec![ariadeck_domain::EnginePath::new(
                    junction.join("item.bin").to_string_lossy(),
                )],
                include_control_files: false,
            })
            .expect_err("intermediate junction inside task_dir must be rejected");
        assert_eq!(error.kind, GatewayErrorKind::UnsafePath);

        fs::remove_dir(&junction).expect("remove junction fixture");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn windows_open_command_spec_strips_verbatim_prefixes() {
        let drive = open_command_spec(Path::new(r"\\?\C:\Downloads\a.bin"), true);
        assert_eq!(
            drive.arguments,
            vec!["url.dll,FileProtocolHandler", r"C:\Downloads\a.bin"]
        );

        let unc = open_command_spec(Path::new(r"\\?\UNC\server\share\a.bin"), false);
        assert_eq!(unc.arguments, vec![r"\\server\share\a.bin"]);
    }
}
