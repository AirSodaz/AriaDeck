use std::{
    env,
    error::Error,
    net::TcpListener,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use ariadeck_application::{
    AddDownloadRequest, AppCommand, CommandItem, CommandOutcome, CoordinatorConfig, PollIntervals,
    ReconnectPolicy, RefreshPolicy, RemoveTasksRequest, StoreSnapshot, SyncHandle, TaskListQuery,
    TaskRemovalScope, spawn_sync_coordinator,
};
use ariadeck_domain::{ConnectionState, DownloadStatus, ProfileId, TaskIdentity};
use ariadeck_rpc::{
    Aria2Client, AuthenticatedTransport, RpcSecret, RpcSyncConnector, TaskKey, WebSocketConfig,
    WebSocketTransport,
};
use tempfile::TempDir;
use url::Url;
use uuid::Uuid;

type TestError = Box<dyn Error + Send + Sync>;

struct ChildGuard {
    child: Child,
}

impl ChildGuard {
    fn spawn(
        executable: &Path,
        data_dir: &Path,
        port: u16,
        secret: &str,
    ) -> Result<Self, TestError> {
        let child = Command::new(executable)
            .arg("--no-conf=true")
            .arg("--enable-rpc=true")
            .arg("--rpc-listen-all=false")
            .arg(format!("--rpc-listen-port={port}"))
            .arg(format!("--rpc-secret={secret}"))
            .arg(format!("--dir={}", data_dir.to_string_lossy()))
            .arg("--summary-interval=0")
            .arg("--console-log-level=warn")
            .arg("--enable-dht=false")
            .arg("--enable-dht6=false")
            .arg("--bt-enable-lpd=false")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Self { child })
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> Result<ExitStatus, TestError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "aria2 did not exit after RPC shutdown",
                )
                .into());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn terminate(&mut self) -> Result<ExitStatus, TestError> {
        if let Some(status) = self.child.try_wait()? {
            return Ok(status);
        }
        self.child.kill()?;
        self.wait_for_exit(Duration::from_secs(5))
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ARIA2C_PATH and launches a real local aria2 process"]
async fn authenticates_and_reads_live_aria2_state() -> Result<(), TestError> {
    let executable = env::var("ARIA2C_PATH")?;
    let data_dir = TempDir::new()?;
    let port = reserve_loopback_port()?;
    let secret = Uuid::new_v4().simple().to_string();
    let mut process = ChildGuard::spawn(Path::new(&executable), data_dir.path(), port, &secret)?;
    let endpoint = Url::parse(&format!("ws://127.0.0.1:{port}/jsonrpc"))?;
    let transport = connect_with_retry(endpoint, Duration::from_secs(5)).await?;
    let authenticated =
        AuthenticatedTransport::new(transport.clone(), Some(RpcSecret::new(secret)));
    let client = Aria2Client::new(authenticated);

    let version = client.get_version().await?;
    let global_stat = client.get_global_stat().await?;
    let active = client.tell_active(TaskKey::LIST_PROJECTION).await?;
    let waiting = client.tell_waiting(0, 10, TaskKey::LIST_PROJECTION).await?;
    let stopped = client.tell_stopped(0, 10, TaskKey::LIST_PROJECTION).await?;

    assert!(!version.version.is_empty());
    assert!(!version.enabled_features.is_empty());
    assert_eq!(global_stat.active_tasks, 0);
    assert!(active.is_empty());
    assert!(waiting.is_empty());
    assert!(stopped.is_empty());

    client.shutdown().await?;
    transport.close().await;
    let status = process.wait_for_exit(Duration::from_secs(5))?;
    assert!(status.success());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ARIA2C_PATH and launches and restarts a real local aria2 process"]
async fn coordinator_recovers_after_live_aria2_restart() -> Result<(), TestError> {
    let executable = env::var("ARIA2C_PATH")?;
    let executable = Path::new(&executable);
    let data_dir = TempDir::new()?;
    let port = reserve_loopback_port()?;
    let secret = Uuid::new_v4().simple().to_string();
    let mut process = ChildGuard::spawn(executable, data_dir.path(), port, &secret)?;
    let endpoint = Url::parse(&format!("ws://127.0.0.1:{port}/jsonrpc"))?;
    let handle = spawn_live_coordinator(endpoint, &secret);

    let initial = wait_for_snapshot(&handle, Duration::from_secs(5), |snapshot| {
        snapshot.connection_state == ConnectionState::Connected && !snapshot.stale
    })
    .await?;
    let _ = process.terminate()?;
    let stale =
        wait_for_snapshot(&handle, Duration::from_secs(5), |snapshot| snapshot.stale).await?;
    assert_ne!(stale.connection_state, ConnectionState::Connected);

    process = ChildGuard::spawn(executable, data_dir.path(), port, &secret)?;
    let reconnected = wait_for_snapshot(&handle, Duration::from_secs(5), |snapshot| {
        snapshot.connection_state == ConnectionState::Connected && !snapshot.stale
    })
    .await?;

    assert!(reconnected.session.generation > initial.session.generation);
    handle.stop().await;
    let _ = process.terminate()?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ARIA2C_PATH and launches a real local aria2 process"]
async fn session_bound_command_flow_handles_both_removal_contracts() -> Result<(), TestError> {
    let executable = env::var("ARIA2C_PATH")?;
    let data_dir = TempDir::new()?;
    let port = reserve_loopback_port()?;
    let secret = Uuid::new_v4().simple().to_string();
    let mut process = ChildGuard::spawn(Path::new(&executable), data_dir.path(), port, &secret)?;
    let endpoint = Url::parse(&format!("ws://127.0.0.1:{port}/jsonrpc"))?;
    let handle = spawn_live_coordinator(endpoint, &secret);
    let connected = wait_for_snapshot(&handle, Duration::from_secs(5), |snapshot| {
        snapshot.connection_state == ConnectionState::Connected && !snapshot.stale
    })
    .await?;

    let added = handle
        .execute(
            connected.session,
            AppCommand::AddDownload(AddDownloadRequest {
                uris: vec!["magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567".into()],
                destination: None,
                options: Vec::new(),
            }),
        )
        .await;
    let identity = single_succeeded_task(added)?;
    handle.force_refresh().await;
    wait_for_task_status(&handle, identity, Duration::from_secs(5), |status| {
        !status.is_terminal()
    })
    .await?;

    let paused = handle
        .execute(connected.session, AppCommand::PauseTasks(vec![identity]))
        .await;
    assert!(paused.has_successes(), "pause failed: {paused:?}");
    handle.force_refresh().await;
    wait_for_task_status(&handle, identity, Duration::from_secs(5), |status| {
        status == DownloadStatus::Paused
    })
    .await?;

    let resumed = handle
        .execute(connected.session, AppCommand::ResumeTasks(vec![identity]))
        .await;
    assert!(resumed.has_successes(), "resume failed: {resumed:?}");
    handle.force_refresh().await;
    wait_for_task_status(&handle, identity, Duration::from_secs(5), |status| {
        status != DownloadStatus::Paused && !status.is_terminal()
    })
    .await?;

    let removed_live = handle
        .execute(
            connected.session,
            AppCommand::RemoveTasks(RemoveTasksRequest {
                tasks: vec![identity],
                scope: TaskRemovalScope::TaskOnly,
            }),
        )
        .await;
    assert!(
        removed_live.has_successes(),
        "live-task removal failed: {removed_live:?}"
    );
    handle.force_refresh().await;
    wait_for_task_status(&handle, identity, Duration::from_secs(5), |status| {
        status == DownloadStatus::Removed
    })
    .await?;

    let removed_result = handle
        .execute(
            connected.session,
            AppCommand::RemoveTasks(RemoveTasksRequest {
                tasks: vec![identity],
                scope: TaskRemovalScope::TaskOnly,
            }),
        )
        .await;
    assert!(
        removed_result.has_successes(),
        "stopped-result removal failed: {removed_result:?}"
    );
    handle.force_refresh().await;
    wait_for_snapshot(&handle, Duration::from_secs(5), |snapshot| {
        snapshot.tasks.iter().all(|task| task.gid != identity.gid)
    })
    .await?;

    let unavailable_port = reserve_loopback_port()?;
    let failed = handle
        .execute(
            connected.session,
            AppCommand::AddDownload(AddDownloadRequest {
                uris: vec![format!(
                    "http://127.0.0.1:{unavailable_port}/unreachable-test-file"
                )],
                destination: None,
                options: vec![
                    ("connect-timeout".into(), "1".into()),
                    ("max-tries".into(), "1".into()),
                ],
            }),
        )
        .await;
    let failed_identity = single_succeeded_task(failed)?;
    handle.force_refresh().await;
    wait_for_task_status(
        &handle,
        failed_identity,
        Duration::from_secs(10),
        |status| status == DownloadStatus::Error,
    )
    .await?;

    let retried = handle
        .execute(
            connected.session,
            AppCommand::RetryTasks(vec![failed_identity]),
        )
        .await;
    let retried_identity = single_succeeded_task(retried)?;
    assert_ne!(retried_identity.gid, failed_identity.gid);

    handle.stop().await;
    let _ = process.terminate()?;
    Ok(())
}

fn spawn_live_coordinator(endpoint: Url, secret: &str) -> SyncHandle {
    let mut websocket = WebSocketConfig::new(endpoint);
    websocket.connect_timeout = Duration::from_millis(150);
    websocket.request_timeout = Duration::from_millis(500);
    let connector = Arc::new(RpcSyncConnector::new(
        websocket,
        Some(RpcSecret::new(secret.to_owned())),
    ));
    let intervals = PollIntervals {
        global_stat: Duration::from_millis(100),
        live_tasks: Duration::from_millis(100),
        stopped_tasks: Duration::from_millis(250),
    };
    let mut coordinator = CoordinatorConfig::new(ProfileId::new());
    coordinator.refresh = RefreshPolicy {
        foreground: intervals,
        background: intervals,
        notification_debounce: Duration::from_millis(20),
    };
    coordinator.reconnect = ReconnectPolicy {
        base_delay: Duration::from_millis(50),
        max_delay: Duration::from_millis(200),
        jitter_percent: 0,
        max_attempts: Some(30),
        reset_after: Duration::from_secs(2),
    };
    coordinator.stopped_page_size = 10;
    spawn_sync_coordinator(connector, coordinator)
}

fn single_succeeded_task(outcome: CommandOutcome) -> Result<TaskIdentity, TestError> {
    let CommandOutcome::Success { succeeded } = outcome else {
        return Err(std::io::Error::other(format!("add command failed: {outcome:?}")).into());
    };
    let Some(CommandItem::Task(identity)) = succeeded.into_iter().next() else {
        return Err(std::io::Error::other("add command returned no task identity").into());
    };
    Ok(identity)
}

async fn wait_for_task_status(
    handle: &SyncHandle,
    identity: TaskIdentity,
    timeout: Duration,
    mut predicate: impl FnMut(DownloadStatus) -> bool,
) -> Result<StoreSnapshot, TestError> {
    wait_for_snapshot(handle, timeout, |snapshot| {
        snapshot
            .tasks
            .iter()
            .find(|task| task.gid == identity.gid)
            .is_some_and(|task| predicate(task.status))
    })
    .await
}

fn reserve_loopback_port() -> Result<u16, TestError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

async fn connect_with_retry(
    endpoint: Url,
    timeout: Duration,
) -> Result<WebSocketTransport, TestError> {
    let deadline = Instant::now() + timeout;
    loop {
        let mut config = WebSocketConfig::new(endpoint.clone());
        config.connect_timeout = Duration::from_millis(250);
        match WebSocketTransport::connect(config).await {
            Ok(transport) => return Ok(transport),
            Err(error) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(50)).await;
                tracing::debug!(%error, "waiting for live aria2 RPC readiness");
            }
            Err(error) => return Err(error.into()),
        }
    }
}

async fn wait_for_snapshot(
    handle: &SyncHandle,
    timeout: Duration,
    mut predicate: impl FnMut(&StoreSnapshot) -> bool,
) -> Result<StoreSnapshot, TestError> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "coordinator did not reach the expected state",
            )
            .into());
        }
        if let Ok(Some(snapshot)) = tokio::time::timeout(
            remaining.min(Duration::from_millis(500)),
            handle.snapshot(TaskListQuery::default()),
        )
        .await
            && predicate(&snapshot)
        {
            return Ok(snapshot);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
