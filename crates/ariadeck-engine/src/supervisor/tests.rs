use super::*;
use crate::EngineError;
use ariadeck_domain::ProfileId;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Child,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

fn temporary_directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!("ariadeck-engine-{}", Uuid::new_v4()));
    fs::create_dir_all(&path).expect("create test dir");
    path
}

fn sample_profile(root: &Path) -> ExternalEngineProfile {
    ExternalEngineProfile::new(
        ProfileId::new(),
        "Test profile",
        std::env::current_exe().expect("current exe"),
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
        &root.join("aria2.conf"),
        &root.join("aria2.session"),
        &root.join("aria2.log"),
    );

    assert!(arguments.contains(&"--no-conf=false".to_owned()));
    assert!(arguments.contains(&"--rpc-save-upload-metadata=true".to_owned()));
    assert!(arguments.contains(&"--rpc-max-request-size=32M".to_owned()));
    assert!(arguments.contains(&"--max-download-result=5000".to_owned()));
    assert!(
        !arguments.iter().any(|arg| arg.starts_with("--rpc-secret")),
        "RPC secret must be passed via managed runtime conf, not argv"
    );
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
    let mut supervisor = LocalEngineSupervisor::spawn_with_policy(&profile.local_config(), policy)
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
fn ownership_lock_remains_readable_to_peers_while_held() {
    let root = temporary_directory();
    let profile_id = ProfileId::new();
    let lock = ProfileOwnershipLock::acquire(profile_id, &root).expect("acquire lock");
    let contents = fs::read_to_string(lock.path())
        .expect("peer should be able to read owner_pid under FILE_SHARE_READ");
    assert!(contents.contains(&std::process::id().to_string()));
    drop(lock);
    let _ = fs::remove_dir_all(root);
}
