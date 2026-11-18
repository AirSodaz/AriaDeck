//! Local external-engine lifecycle, managed core registry, and profile persistence.
//!
//! This crate deliberately owns only process, core-installation, and profile
//! concerns. RPC synchronization remains in `ariadeck-rpc`, while profile
//! identity is shared through `ariadeck-domain`.

mod cores;
pub mod discovery;
mod error;
mod fs_gateway;
mod profiles;
mod supervisor;

pub use cores::{
    Aria2Probe, CoreInstallStatus, CoreInstallation, CoreInstallationSummary, CoreInstallationView,
    CoreRegistry, CoreSource, CoreStore, parse_aria2_version_output, probe_aria2,
};
pub use discovery::{CoreDiscoveryOrigin, DiscoveredCore, aria2_executable_name, discover_cores};
pub use error::EngineError;
pub(crate) use error::io_error;
pub use fs_gateway::{
    LocalDownloadDestinationGateway, LocalDownloadRootRegistry, LocalTaskFileGateway,
};
pub use profiles::{
    JsonProfileStore, LoadedCatalog, LoadedProfile, PROFILE_CATALOG_SCHEMA_VERSION, ProfileCatalog,
    ProfileEntry, ProfileKind, ProfileStoreRecovery, RpcSecretRef,
};
pub use supervisor::{
    ExternalEngineProfile, LocalEngineConfig, LocalEngineHealth, LocalEngineHealthHandle,
    LocalEngineProcess, LocalEngineSupervisor, LocalRestartPolicy, ProfileOwnershipLock,
    SessionFilePreparation, prepare_session_file, reserve_loopback_port, validate_executable,
};

use std::{ffi::OsStr, process::Command};

#[cfg(test)]
use {
    ariadeck_application::{
        DownloadDestinationFile, DownloadDestinationGateway, DownloadDestinationRequest,
        GatewayError, GatewayErrorKind, TaskFileGateway, TaskFileRemovalRequest,
    },
    ariadeck_domain::ProfileId,
    fs_gateway::{TrashBackend, open_command_spec, run_blocking},
    std::{
        process::Child,
        sync::Arc,
        thread,
        time::{Duration, Instant},
    },
    supervisor::{local_engine_arguments, process_appears_alive},
    uuid::Uuid,
};

pub(crate) const CONFIG_FILE_NAME: &str = "aria2.conf";
pub(crate) const SESSION_FILE_NAME: &str = "aria2.session";
pub(crate) const LOG_FILE_NAME: &str = "aria2.log";
pub(crate) const OWNERSHIP_LOCK_FILE_NAME: &str = ".ariadeck-engine.lock";

/// `CREATE_NO_WINDOW` — the child gets no console, inherits none, and keeps the
/// redirected stdio handles it is spawned with.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Build a command for a console program that must not show a console window.
///
/// The desktop binary is `windows_subsystem = "windows"`, so it owns no console;
/// spawning a console-subsystem child like `aria2c` makes Windows allocate a
/// fresh one. That is a black window sitting next to the app for the managed
/// engine, and a flash for every `--version` probe. Every command this crate
/// runs is such a program, so route them all through here.
#[must_use]
pub fn console_free_command(program: impl AsRef<OsStr>) -> Command {
    let command = Command::new(program);
    #[cfg(windows)]
    let command = {
        use std::os::windows::process::CommandExt as _;
        let mut command = command;
        command.creation_flags(CREATE_NO_WINDOW);
        command
    };
    command
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
    fn profile_catalog_round_trips_current_schema() {
        let root = temporary_directory();
        let defaults = sample_profile(&root);
        let store = JsonProfileStore::new(root.join("profiles.json"));
        let mut catalog = ProfileCatalog::from_single(defaults.clone());
        assert_eq!(catalog.schema_version, PROFILE_CATALOG_SCHEMA_VERSION);

        let remote = ProfileEntry::remote_rpc(
            ProfileId::new(),
            "NAS",
            "wss://aria2.example.invalid/jsonrpc",
            root.join("nas-downloads"),
            None,
        )
        .expect("remote entry");
        catalog.upsert(remote.clone()).expect("upsert remote");
        catalog
            .set_active(remote.profile_id)
            .expect("activate remote");
        store.save_catalog(&catalog).expect("save catalog");

        let reloaded = store
            .load_catalog_or_recover(&defaults)
            .expect("reload catalog");
        assert!(!reloaded.migrated);
        assert_eq!(
            reloaded.catalog.schema_version,
            PROFILE_CATALOG_SCHEMA_VERSION
        );
        assert_eq!(reloaded.catalog.profiles.len(), 2);
        assert_eq!(reloaded.catalog.active_profile_id, remote.profile_id);
        assert_eq!(
            reloaded.catalog.active().map(|p| p.kind),
            Some(ProfileKind::RemoteRpc)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unsupported_profile_catalog_schema_is_rejected_without_replace() {
        let root = temporary_directory();
        let defaults = sample_profile(&root);
        let path = root.join("profiles.json");
        let store = JsonProfileStore::new(&path);
        fs::write(
            &path,
            r#"{"schema_version":99,"active_profile_id":"00000000-0000-4000-8000-000000000001","profiles":[]}"#,
        )
        .expect("seed unsupported schema");
        let err = store
            .load_catalog_or_recover(&defaults)
            .expect_err("unsupported schema");
        assert!(matches!(
            err,
            EngineError::UnsupportedSchemaVersion {
                found: 99,
                supported: PROFILE_CATALOG_SCHEMA_VERSION
            }
        ));
        let on_disk = fs::read_to_string(&path).expect("read");
        assert!(on_disk.contains("\"schema_version\":99"));
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
