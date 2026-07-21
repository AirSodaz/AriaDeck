use std::{
    env,
    error::Error,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use ariadeck_application::{
    AddDownloadRequest, AddDownloadSource, AppCommand, CommandItem, CommandOutcome,
    CoordinatorConfig, DownloadProxyConfig, DownloadProxyMode, PollIntervals, QueueMove,
    ReconnectPolicy, RefreshPolicy, RemoveTasksRequest, SetTaskOutputNameRequest, StoreSnapshot,
    SyncHandle, TaskListQuery, TaskRemovalScope, spawn_sync_coordinator,
};
use ariadeck_domain::{
    ConnectionState, DownloadStatus, EnginePath, ProfileId, TaskIdentity, TaskSourceKind,
};
use ariadeck_rpc::{
    Aria2Client, AuthenticatedTransport, RpcError, RpcSecret, RpcSyncConnector, TaskKey,
    WebSocketConfig, WebSocketTransport,
};
use lava_torrent::torrent::v1::TorrentBuilder;
use secrecy::SecretString;
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
            .arg("--rpc-save-upload-metadata=true")
            .arg("--rpc-max-request-size=32M")
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
#[ignore = "requires ARIA2C_PATH and launches a real local aria2 process"]
async fn applies_global_and_per_task_speed_limits_on_live_aria2() -> Result<(), TestError> {
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

    // Global limits use the exact option names the RATE-001 adapter emits
    // (aria2.changeGlobalOption with max-overall-*-limit). A real aria2 must
    // accept the K/M-normalized byte strings without error.
    client
        .change_global_options(&[
            (
                "max-overall-download-limit".into(),
                (2 * 1024 * 1024).to_string(),
            ),
            ("max-overall-upload-limit".into(), (512 * 1024).to_string()),
        ])
        .await?;
    // Zero must clear the limit rather than being rejected.
    client
        .change_global_options(&[
            ("max-overall-download-limit".into(), "0".into()),
            ("max-overall-upload-limit".into(), "0".into()),
        ])
        .await?;

    // Add a paused task and apply per-task limits through the same option names
    // the adapter uses (aria2.changeOption with max-*-limit), then read them
    // back from aria2's own getOption projection.
    let request = AddDownloadRequest {
        source: AddDownloadSource::Uris(vec!["http://127.0.0.1:9/limited.bin".into()]),
        destination: Some(EnginePath::new(data_dir.path().to_string_lossy())),
        file_conflict: ariadeck_application::FileConflictPolicy::default(),
        selected_file_indices: None,
        options: vec![("pause".into(), "true".into())],
    };
    let gid = client.add_uri(&request).await?;
    client
        .change_options(
            gid,
            &[
                ("max-download-limit".into(), (1024 * 1024).to_string()),
                ("max-upload-limit".into(), (256 * 1024).to_string()),
            ],
        )
        .await?;
    let options = client.get_options(gid).await?;
    assert_eq!(
        options
            .get("max-download-limit")
            .and_then(|value| value.as_str()),
        Some((1024 * 1024).to_string().as_str()),
        "aria2 must report the per-task download limit it accepted"
    );
    assert_eq!(
        options
            .get("max-upload-limit")
            .and_then(|value| value.as_str()),
        Some((256 * 1024).to_string().as_str()),
        "aria2 must report the per-task upload limit it accepted"
    );

    client.remove(gid).await.ok();
    client.remove_download_result(gid).await.ok();
    client.shutdown().await?;
    transport.close().await;
    let status = process.wait_for_exit(Duration::from_secs(5))?;
    assert!(status.success());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ARIA2C_PATH and launches a real local aria2 process"]
async fn projects_task_connection_details_on_live_aria2() -> Result<(), TestError> {
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

    let mirror_request = AddDownloadRequest {
        source: AddDownloadSource::Uris(vec![
            "http://127.0.0.1:9/mirror.bin".into(),
            "http://127.0.0.1:10/mirror.bin".into(),
        ]),
        destination: Some(EnginePath::new(data_dir.path().to_string_lossy())),
        file_conflict: ariadeck_application::FileConflictPolicy::default(),
        selected_file_indices: None,
        options: vec![
            ("pause".into(), "true".into()),
            ("max-download-limit".into(), "1M".into()),
            ("http-user".into(), "private-user".into()),
            ("http-passwd".into(), "private-password".into()),
            ("header".into(), "Cookie: private-cookie".into()),
        ],
    };
    let mirror_gid = client.add_uri(&mirror_request).await?;
    let mirror_details = client.connection_details(mirror_gid, false, false).await?;
    assert_eq!(mirror_details.uris.len(), 2);
    assert!(mirror_details.peers.is_empty());
    assert!(mirror_details.servers.is_empty());
    let limit = mirror_details
        .options
        .iter()
        .find(|entry| entry.key == "max-download-limit")
        .expect("real aria2 returns max-download-limit");
    assert!(!limit.redacted);
    assert!(matches!(limit.value.as_str(), "1M" | "1048576"));
    for key in ["header", "http-passwd", "http-user"] {
        let option = mirror_details
            .options
            .iter()
            .find(|entry| entry.key == key)
            .unwrap_or_else(|| panic!("real aria2 did not return {key}"));
        assert!(option.redacted, "{key} must be redacted before projection");
        assert!(
            option.value.is_empty(),
            "{key} value escaped the RPC adapter"
        );
    }

    let payload = vec![b'x'; 64 * 1024];
    let (slow_url, slow_server) = spawn_slow_http_fixture(payload, Duration::from_secs(5))?;
    let active_request = AddDownloadRequest {
        source: AddDownloadSource::Uris(vec![slow_url]),
        destination: Some(EnginePath::new(data_dir.path().to_string_lossy())),
        file_conflict: ariadeck_application::FileConflictPolicy::default(),
        selected_file_indices: None,
        options: vec![("split".into(), "1".into())],
    };
    let active_gid = client.add_uri(&active_request).await?;
    wait_for_active_client_task(&client, active_gid, Duration::from_secs(5)).await?;
    let active_details =
        wait_for_server_details(&client, active_gid, Duration::from_secs(3)).await?;
    assert!(active_details.peers.is_empty());
    assert!(
        active_details
            .servers
            .iter()
            .any(|server| server.file_index == 1 && !server.current_uri.is_empty()),
        "real aria2 must expose the active HTTP server projection"
    );
    slow_server
        .join()
        .map_err(|_| std::io::Error::other("slow HTTP fixture panicked"))??;

    client.remove(mirror_gid).await.ok();
    client.remove_download_result(mirror_gid).await.ok();
    client.remove(active_gid).await.ok();
    client.remove_download_result(active_gid).await.ok();
    client.shutdown().await?;
    transport.close().await;
    let status = process.wait_for_exit(Duration::from_secs(5))?;
    assert!(status.success());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ARIA2C_PATH and launches a real local aria2 process"]
async fn reorders_queue_and_applies_global_pause_resume_on_live_aria2() -> Result<(), TestError> {
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

    // Add three paused tasks so they populate the waiting queue in submit order
    // without downloading. aria2's changePosition operates on this queue.
    let mut gids = Vec::new();
    for index in 0..3 {
        let request = AddDownloadRequest {
            source: AddDownloadSource::Uris(vec![format!("http://127.0.0.1:9/queued-{index}.bin")]),
            destination: Some(EnginePath::new(data_dir.path().to_string_lossy())),
            file_conflict: ariadeck_application::FileConflictPolicy::default(),
            selected_file_indices: None,
            options: vec![("pause".into(), "true".into())],
        };
        gids.push(client.add_uri(&request).await?);
    }

    let queue_order = |waiting: &[ariadeck_domain::TaskSnapshot]| {
        waiting.iter().map(|task| task.gid).collect::<Vec<_>>()
    };
    let waiting = client.tell_waiting(0, 10, TaskKey::LIST_PROJECTION).await?;
    assert_eq!(
        queue_order(&waiting),
        gids,
        "paused tasks should queue in submit order"
    );

    // Move the last task to the top of the queue.
    client.move_in_queue(gids[2], QueueMove::Top).await?;
    let waiting = client.tell_waiting(0, 10, TaskKey::LIST_PROJECTION).await?;
    assert_eq!(
        queue_order(&waiting),
        vec![gids[2], gids[0], gids[1]],
        "move-to-top must place the task first in the authoritative queue"
    );

    // Move that same task to the bottom.
    client.move_in_queue(gids[2], QueueMove::Bottom).await?;
    let waiting = client.tell_waiting(0, 10, TaskKey::LIST_PROJECTION).await?;
    assert_eq!(
        queue_order(&waiting),
        vec![gids[0], gids[1], gids[2]],
        "move-to-bottom must place the task last in the authoritative queue"
    );

    // Engine-wide resume then pause must not error against a real aria2.
    client.resume_all().await?;
    client.pause_all().await?;

    // Clean up the queued tasks.
    for gid in &gids {
        client.remove(*gid).await.ok();
        client.remove_download_result(*gid).await.ok();
    }

    client.shutdown().await?;
    transport.close().await;
    let status = process.wait_for_exit(Duration::from_secs(5))?;
    assert!(status.success());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ARIA2C_PATH and launches a real local aria2 process"]
async fn uploads_torrent_and_metalink_metadata_to_live_aria2() -> Result<(), TestError> {
    let executable = env::var("ARIA2C_PATH")?;
    let data_dir = TempDir::new()?;
    let port = reserve_loopback_port()?;
    let secret = Uuid::new_v4().simple().to_string();
    let mut process = ChildGuard::spawn(Path::new(&executable), data_dir.path(), port, &secret)?;
    let endpoint = Url::parse(&format!("ws://127.0.0.1:{port}/jsonrpc"))?;
    let transport = connect_with_retry(endpoint, Duration::from_secs(5)).await?;
    let client = Aria2Client::new(AuthenticatedTransport::new(
        transport.clone(),
        Some(RpcSecret::new(secret)),
    ));

    let torrent = lava_torrent::torrent::v1::Torrent {
        announce: None,
        announce_list: None,
        length: 2,
        files: Some(vec![
            lava_torrent::torrent::v1::File {
                length: 1,
                path: "one.bin".into(),
                extra_fields: None,
            },
            lava_torrent::torrent::v1::File {
                length: 1,
                path: "two.bin".into(),
                extra_fields: None,
            },
        ]),
        name: "fixture".into(),
        piece_length: 16_384,
        pieces: vec![vec![0; 20]],
        extra_fields: None,
        extra_info_fields: None,
    }
    .encode()?;
    let torrent_gid = client
        .add_torrent(&AddDownloadRequest {
            source: AddDownloadSource::Torrent(Arc::<[u8]>::from(torrent)),
            destination: None,
            file_conflict: ariadeck_application::FileConflictPolicy::Reject,
            selected_file_indices: Some(vec![2]),
            options: Vec::new(),
        })
        .await?;

    let first_url = "http://127.0.0.1:9/one.bin";
    let second_url = "http://127.0.0.1:9/two.bin";
    let metalink = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><metalink xmlns=\"urn:ietf:params:xml:ns:metalink\" version=\"4.0\"><file name=\"one.bin\"><resources><url>{first_url}</url></resources></file><file name=\"two.bin\"><resources><url>{second_url}</url></resources></file></metalink>"
    );
    let metalink_gids = client
        .add_metalink(&AddDownloadRequest {
            source: AddDownloadSource::Metalink(Arc::<[u8]>::from(metalink.into_bytes())),
            destination: None,
            file_conflict: ariadeck_application::FileConflictPolicy::Reject,
            selected_file_indices: Some(vec![2]),
            options: Vec::new(),
        })
        .await?;
    assert!(!metalink_gids.is_empty());

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut torrent_seen = false;
    let mut metalink_seen = false;
    while Instant::now() < deadline && (!torrent_seen || !metalink_seen) {
        let mut tasks = client.tell_active(TaskKey::DISCOVERY_PROJECTION).await?;
        tasks.extend(
            client
                .tell_waiting(0, 20, TaskKey::DISCOVERY_PROJECTION)
                .await?,
        );
        tasks.extend(
            client
                .tell_stopped(0, 20, TaskKey::DISCOVERY_PROJECTION)
                .await?,
        );
        torrent_seen |= tasks.iter().any(|task| {
            task.gid == torrent_gid && task.metadata.source_kind == TaskSourceKind::BitTorrent
        });
        // aria2 expands a Metalink into ordinary per-file downloads, so the
        // durable contract is that every returned GID becomes observable.
        metalink_seen |= metalink_gids
            .iter()
            .all(|gid| tasks.iter().any(|task| task.gid == *gid));
        if !torrent_seen || !metalink_seen {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    assert!(torrent_seen, "uploaded torrent task was not observed");
    assert!(metalink_seen, "uploaded metalink task was not observed");
    let torrent_details = client.task_details(torrent_gid).await?;
    assert_eq!(torrent_details.files.len(), 2);
    assert!(!torrent_details.files[0].selected);
    assert!(torrent_details.files[1].selected);
    assert_eq!(
        torrent_details.files[1]
            .path
            .as_str()
            .replace('\\', "/")
            .rsplit('/')
            .next(),
        Some("two.bin")
    );
    assert_eq!(
        metalink_gids.len(),
        1,
        "partial Metalink selection should return only the selected file GID"
    );
    let metalink_details = client.task_details(metalink_gids[0]).await?;
    assert_eq!(metalink_details.files.len(), 1);
    assert!(metalink_details.files[0].selected);
    assert_eq!(
        metalink_details.files[0]
            .path
            .as_str()
            .replace('\\', "/")
            .rsplit('/')
            .next(),
        Some("two.bin")
    );

    client.shutdown().await?;
    transport.close().await;
    let _status = process.wait_for_exit(Duration::from_secs(5))?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ARIA2C_PATH and launches a real local aria2 process"]
async fn projects_explicit_seeding_state_and_stop_rules_on_live_aria2() -> Result<(), TestError> {
    let executable = env::var("ARIA2C_PATH")?;
    let data_dir = TempDir::new()?;
    let payload_path = data_dir.path().join("seed-fixture.bin");
    std::fs::write(&payload_path, b"AriaDeck live seeding fixture")?;
    let torrent = TorrentBuilder::new(&payload_path, 16_384)
        .build()?
        .encode()?;

    let port = reserve_loopback_port()?;
    let secret = Uuid::new_v4().simple().to_string();
    let mut process = ChildGuard::spawn(Path::new(&executable), data_dir.path(), port, &secret)?;
    let endpoint = Url::parse(&format!("ws://127.0.0.1:{port}/jsonrpc"))?;
    let transport = connect_with_retry(endpoint, Duration::from_secs(5)).await?;
    let client = Aria2Client::new(AuthenticatedTransport::new(
        transport.clone(),
        Some(RpcSecret::new(secret)),
    ));

    let gid = client
        .add_torrent(&AddDownloadRequest {
            source: AddDownloadSource::Torrent(Arc::<[u8]>::from(torrent)),
            destination: Some(EnginePath::new(data_dir.path().to_string_lossy())),
            file_conflict: ariadeck_application::FileConflictPolicy::Overwrite,
            selected_file_indices: None,
            options: vec![
                ("check-integrity".into(), "true".into()),
                ("seed-ratio".into(), "1000.0".into()),
                ("seed-time".into(), "60".into()),
            ],
        })
        .await?;

    let deadline = Instant::now() + Duration::from_secs(15);
    let seeding = loop {
        if let Some(task) = client
            .tell_active(TaskKey::LIST_PROJECTION)
            .await?
            .into_iter()
            .find(|task| task.gid == gid && task.status == DownloadStatus::Seeding)
        {
            break task;
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("task {gid} did not expose seeder=true as Seeding"),
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(
        seeding.upload_speed.get(),
        0,
        "a seeder with no leecher must remain Seeding at zero upload speed"
    );
    assert_eq!(seeding.completed_length, seeding.total_length);

    let options = client.get_options(gid).await?;
    assert_eq!(
        options.get("seed-ratio").and_then(|value| value.as_str()),
        Some("1000.0")
    );
    assert_eq!(
        options.get("seed-time").and_then(|value| value.as_str()),
        Some("60")
    );

    client.remove(gid).await.ok();
    client.remove_download_result(gid).await.ok();
    client.shutdown().await?;
    transport.close().await;
    let status = process.wait_for_exit(Duration::from_secs(5))?;
    assert!(status.success());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ARIA2C_PATH and launches a real local aria2 process"]
async fn rejects_a_wrong_secret_without_leaking_credentials() -> Result<(), TestError> {
    let executable = env::var("ARIA2C_PATH")?;
    let data_dir = TempDir::new()?;
    let port = reserve_loopback_port()?;
    let secret = Uuid::new_v4().simple().to_string();
    let wrong_secret = format!("wrong-{}", Uuid::new_v4().simple());
    let mut process = ChildGuard::spawn(Path::new(&executable), data_dir.path(), port, &secret)?;
    let endpoint = Url::parse(&format!("ws://127.0.0.1:{port}/jsonrpc"))?;
    let rejected_transport = connect_with_retry(endpoint.clone(), Duration::from_secs(5)).await?;
    let rejected_client = Aria2Client::new(AuthenticatedTransport::new(
        rejected_transport.clone(),
        Some(RpcSecret::new(wrong_secret.clone())),
    ));
    let error = rejected_client
        .get_version()
        .await
        .expect_err("aria2 must reject an invalid RPC secret");
    let RpcError::Remote { message, .. } = &error else {
        return Err(std::io::Error::other(format!(
            "expected an aria2 authentication error, received {error}"
        ))
        .into());
    };
    assert!(message.to_ascii_lowercase().contains("unauthorized"));
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains(&secret));
    assert!(!rendered.contains(&wrong_secret));
    rejected_transport.close().await;

    let transport = connect_with_retry(endpoint, Duration::from_secs(5)).await?;
    let authenticated =
        AuthenticatedTransport::new(transport.clone(), Some(RpcSecret::new(secret)));
    Aria2Client::new(authenticated).shutdown().await?;
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
    let handle = spawn_live_coordinator(endpoint.clone(), &secret);
    let connected = wait_for_snapshot(&handle, Duration::from_secs(5), |snapshot| {
        snapshot.connection_state == ConnectionState::Connected && !snapshot.stale
    })
    .await?;

    let payload = b"AriaDeck removal keeps completed files".to_vec();
    let (fixture_url, fixture_server) = spawn_http_fixture(payload.clone())?;
    let kept_name = "kept-after-remove.bin";
    let completed = handle
        .execute(
            connected.session,
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Uris(vec![fixture_url]),
                destination: Some(EnginePath::new(data_dir.path().to_string_lossy())),
                file_conflict: ariadeck_application::FileConflictPolicy::default(),
                selected_file_indices: None,
                options: vec![
                    ("out".into(), kept_name.into()),
                    ("split".into(), "1".into()),
                    ("max-connection-per-server".into(), "1".into()),
                ],
            }),
        )
        .await;
    let completed_identity = single_succeeded_task(completed)?;
    handle.force_refresh().await;
    wait_for_task_status(
        &handle,
        completed_identity,
        Duration::from_secs(5),
        |status| status == DownloadStatus::Complete,
    )
    .await?;
    fixture_server.join().map_err(|_| {
        std::io::Error::other("local HTTP fixture thread panicked during download")
    })??;
    let kept_path = data_dir.path().join(kept_name);
    assert_eq!(std::fs::read(&kept_path)?, payload);
    let (conflict_url, conflict_server) = spawn_http_fixture(payload.clone())?;
    let auto_renamed = handle
        .execute(
            connected.session,
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Uris(vec![conflict_url]),
                destination: Some(EnginePath::new(data_dir.path().to_string_lossy())),
                file_conflict: ariadeck_application::FileConflictPolicy::AutoRename,
                selected_file_indices: None,
                options: vec![
                    ("out".into(), kept_name.into()),
                    ("split".into(), "1".into()),
                    ("max-connection-per-server".into(), "1".into()),
                ],
            }),
        )
        .await;
    let auto_renamed_identity = single_succeeded_task(auto_renamed)?;
    handle.force_refresh().await;
    wait_for_task_status(
        &handle,
        auto_renamed_identity,
        Duration::from_secs(5),
        |status| status == DownloadStatus::Complete,
    )
    .await?;
    conflict_server.join().map_err(|_| {
        std::io::Error::other("local HTTP fixture thread panicked during conflict download")
    })??;
    let auto_renamed_path = data_dir.path().join("kept-after-remove.1.bin");
    assert_eq!(std::fs::read(&auto_renamed_path)?, payload);
    let removed_completed = handle
        .execute(
            connected.session,
            AppCommand::RemoveTasks(RemoveTasksRequest {
                tasks: vec![completed_identity, auto_renamed_identity],
                scope: TaskRemovalScope::TaskOnly,
            }),
        )
        .await;
    assert!(
        removed_completed.has_successes(),
        "completed-result removal failed: {removed_completed:?}"
    );
    handle.force_refresh().await;
    wait_for_snapshot(&handle, Duration::from_secs(5), |snapshot| {
        snapshot
            .tasks
            .iter()
            .all(|task| task.gid != completed_identity.gid && task.gid != auto_renamed_identity.gid)
    })
    .await?;
    assert_eq!(
        std::fs::read(&kept_path)?,
        payload,
        "aria2 result removal must not delete the completed file"
    );
    assert_eq!(
        std::fs::read(&auto_renamed_path)?,
        payload,
        "aria2 result removal must not delete the auto-renamed file"
    );

    let direct_added = handle
        .execute(
            connected.session,
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Uris(vec![
                    "https://example.invalid/original-live-name.bin".into(),
                ]),
                destination: None,
                file_conflict: ariadeck_application::FileConflictPolicy::default(),
                selected_file_indices: None,
                options: vec![("pause".into(), "true".into())],
            }),
        )
        .await;
    let direct_identity = single_succeeded_task(direct_added)?;
    handle.force_refresh().await;
    wait_for_snapshot(&handle, Duration::from_secs(5), |snapshot| {
        snapshot.tasks.iter().any(|task| {
            task.gid == direct_identity.gid
                && task.status == DownloadStatus::Paused
                && task.metadata.source_kind == TaskSourceKind::DirectUri
        })
    })
    .await?;

    let renamed = handle
        .execute(
            connected.session,
            AppCommand::SetTaskOutputName(SetTaskOutputNameRequest {
                task: direct_identity,
                output_name: "renamed-live.bin".into(),
            }),
        )
        .await;
    assert!(
        renamed.has_successes(),
        "output-name change failed: {renamed:?}"
    );
    handle.force_refresh().await;
    wait_for_snapshot(&handle, Duration::from_secs(5), |snapshot| {
        snapshot
            .tasks
            .iter()
            .any(|task| task.gid == direct_identity.gid && task.display_name == "renamed-live.bin")
    })
    .await?;

    let removed_direct = handle
        .execute(
            connected.session,
            AppCommand::RemoveTasks(RemoveTasksRequest {
                tasks: vec![direct_identity],
                scope: TaskRemovalScope::TaskOnly,
            }),
        )
        .await;
    assert!(
        removed_direct.has_successes(),
        "direct-task removal failed: {removed_direct:?}"
    );

    let mirrors_added = handle
        .execute(
            connected.session,
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Uris(vec![
                    "https://example.invalid/mirrored-live.bin".into(),
                    "https://mirror.invalid/mirrored-live.bin".into(),
                ]),
                destination: None,
                file_conflict: ariadeck_application::FileConflictPolicy::default(),
                selected_file_indices: None,
                options: vec![("pause".into(), "true".into())],
            }),
        )
        .await;
    let mirrors_identity = single_succeeded_task(mirrors_added)?;
    handle.force_refresh().await;
    wait_for_snapshot(&handle, Duration::from_secs(5), |snapshot| {
        snapshot.tasks.iter().any(|task| {
            task.gid == mirrors_identity.gid
                && task.status == DownloadStatus::Paused
                && task.metadata.primary_uri.as_deref()
                    == Some("https://example.invalid/mirrored-live.bin")
        })
    })
    .await?;
    let removed_mirrors = handle
        .execute(
            connected.session,
            AppCommand::RemoveTasks(RemoveTasksRequest {
                tasks: vec![mirrors_identity],
                scope: TaskRemovalScope::TaskOnly,
            }),
        )
        .await;
    assert!(
        removed_mirrors.has_successes(),
        "mirrored-task removal failed: {removed_mirrors:?}"
    );

    let added = handle
        .execute(
            connected.session,
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Uris(vec![
                    "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567".into(),
                ]),
                destination: None,
                file_conflict: ariadeck_application::FileConflictPolicy::default(),
                selected_file_indices: None,
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
    let checksum_option = format!("sha-256={}", "0".repeat(64));
    let failed = handle
        .execute(
            connected.session,
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Uris(vec![format!(
                    "http://127.0.0.1:{unavailable_port}/unreachable-test-file"
                )]),
                destination: None,
                file_conflict: ariadeck_application::FileConflictPolicy::default(),
                selected_file_indices: None,
                options: vec![
                    ("connect-timeout".into(), "1".into()),
                    ("max-tries".into(), "1".into()),
                    ("out".into(), "preserved-retry-name.bin".into()),
                    ("header".into(), "Cookie: session=preserved".into()),
                    ("http-user".into(), "retry-user".into()),
                    ("http-passwd".into(), "retry-password".into()),
                    ("max-download-limit".into(), "64K".into()),
                    ("max-connection-per-server".into(), "2".into()),
                    ("checksum".into(), checksum_option.clone()),
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
    handle.force_refresh().await;
    wait_for_snapshot(&handle, Duration::from_secs(5), |snapshot| {
        snapshot
            .tasks
            .iter()
            .any(|task| task.gid == failed_identity.gid)
            && snapshot
                .tasks
                .iter()
                .any(|task| task.gid == retried_identity.gid)
    })
    .await?;

    let query_transport = connect_with_retry(endpoint, Duration::from_secs(5)).await?;
    let query_client = Aria2Client::new(AuthenticatedTransport::new(
        query_transport.clone(),
        Some(RpcSecret::new(secret.clone())),
    ));
    let retried_options = query_client.get_options(retried_identity.gid).await?;
    for (key, expected) in [
        ("out", "preserved-retry-name.bin"),
        ("header", "Cookie: session=preserved"),
        ("http-user", "retry-user"),
        ("http-passwd", "retry-password"),
        ("max-download-limit", "65536"),
        ("max-connection-per-server", "2"),
    ] {
        assert!(
            rpc_option_contains(&retried_options, key, expected),
            "retry did not preserve {key}: {:?}",
            retried_options.get(key)
        );
    }
    assert_eq!(
        retried_options
            .get("checksum")
            .and_then(serde_json::Value::as_str),
        Some(checksum_option.as_str())
    );
    query_transport.close().await;

    handle.stop().await;
    let _ = process.terminate()?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ARIA2C_PATH and launches a real local aria2 process"]
async fn download_proxy_routes_bypasses_and_disables_live_traffic() -> Result<(), TestError> {
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

    let proxied_payload = b"download reached the configured proxy".to_vec();
    let (proxy_endpoint, proxy_server) = spawn_http_proxy_fixture(proxied_payload.clone())?;
    handle
        .apply_download_proxy(
            connected.session,
            DownloadProxyConfig {
                mode: DownloadProxyMode::Manual,
                all_proxy: Some(proxy_endpoint),
                username: Some("proxy-user".into()),
                password: Some(SecretString::new("proxy-pass".into())),
                ..DownloadProxyConfig::default()
            },
        )
        .await
        .map_err(|error| std::io::Error::other(error.summary))?;
    let proxied = handle
        .execute(
            connected.session,
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Uris(vec![
                    "http://proxy-target.invalid/proxied.bin".into(),
                ]),
                destination: Some(EnginePath::new(data_dir.path().to_string_lossy())),
                options: vec![
                    ("out".into(), "proxied.bin".into()),
                    ("split".into(), "1".into()),
                    ("max-connection-per-server".into(), "1".into()),
                ],
                ..AddDownloadRequest::default()
            }),
        )
        .await;
    let proxied_identity = single_succeeded_task(proxied)?;
    wait_for_task_status(
        &handle,
        proxied_identity,
        Duration::from_secs(5),
        |status| status == DownloadStatus::Complete,
    )
    .await?;
    let proxy_request = proxy_server
        .join()
        .map_err(|_| std::io::Error::other("proxy fixture thread panicked"))??;
    assert!(proxy_request.starts_with("GET http://proxy-target.invalid/proxied.bin "));
    assert!(
        proxy_request
            .to_ascii_lowercase()
            .contains("proxy-authorization: basic chjvehktdxnlcjpwcm94es1wyxnz"),
        "authenticated proxy request did not contain the expected Basic header: {proxy_request:?}"
    );
    assert_eq!(
        std::fs::read(data_dir.path().join("proxied.bin"))?,
        proxied_payload
    );

    let bypass_payload = b"download bypassed the configured proxy".to_vec();
    let (bypass_url, bypass_server) = spawn_http_fixture(bypass_payload.clone())?;
    let (bypass_proxy, bypass_detector) = spawn_proxy_detector(Duration::from_secs(2))?;
    handle
        .apply_download_proxy(
            connected.session,
            DownloadProxyConfig {
                mode: DownloadProxyMode::Manual,
                all_proxy: Some(bypass_proxy),
                no_proxy: vec!["127.0.0.1".into()],
                ..DownloadProxyConfig::default()
            },
        )
        .await
        .map_err(|error| std::io::Error::other(error.summary))?;
    let bypassed = handle
        .execute(
            connected.session,
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Uris(vec![bypass_url]),
                destination: Some(EnginePath::new(data_dir.path().to_string_lossy())),
                options: vec![("out".into(), "bypassed.bin".into())],
                ..AddDownloadRequest::default()
            }),
        )
        .await;
    let bypassed_identity = single_succeeded_task(bypassed)?;
    wait_for_task_status(
        &handle,
        bypassed_identity,
        Duration::from_secs(5),
        |status| status == DownloadStatus::Complete,
    )
    .await?;
    bypass_server
        .join()
        .map_err(|_| std::io::Error::other("bypass fixture thread panicked"))??;
    assert!(
        !bypass_detector
            .join()
            .map_err(|_| std::io::Error::other("bypass detector thread panicked"))??
    );
    assert_eq!(
        std::fs::read(data_dir.path().join("bypassed.bin"))?,
        bypass_payload
    );

    let disabled_payload = b"download stayed direct after disabling the proxy".to_vec();
    let (disabled_url, disabled_server) = spawn_http_fixture(disabled_payload.clone())?;
    let (stale_proxy, stale_detector) = spawn_proxy_detector(Duration::from_secs(2))?;
    handle
        .apply_download_proxy(
            connected.session,
            DownloadProxyConfig {
                mode: DownloadProxyMode::Manual,
                all_proxy: Some(stale_proxy),
                ..DownloadProxyConfig::default()
            },
        )
        .await
        .map_err(|error| std::io::Error::other(error.summary))?;
    handle
        .apply_download_proxy(connected.session, DownloadProxyConfig::default())
        .await
        .map_err(|error| std::io::Error::other(error.summary))?;
    let direct = handle
        .execute(
            connected.session,
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Uris(vec![disabled_url]),
                destination: Some(EnginePath::new(data_dir.path().to_string_lossy())),
                options: vec![("out".into(), "disabled.bin".into())],
                ..AddDownloadRequest::default()
            }),
        )
        .await;
    let direct_identity = single_succeeded_task(direct)?;
    wait_for_task_status(&handle, direct_identity, Duration::from_secs(5), |status| {
        status == DownloadStatus::Complete
    })
    .await?;
    disabled_server
        .join()
        .map_err(|_| std::io::Error::other("disabled fixture thread panicked"))??;
    assert!(
        !stale_detector
            .join()
            .map_err(|_| std::io::Error::other("disabled detector thread panicked"))??
    );
    assert_eq!(
        std::fs::read(data_dir.path().join("disabled.bin"))?,
        disabled_payload
    );

    handle.stop().await;
    let _ = process.terminate()?;
    Ok(())
}

fn rpc_option_contains(
    options: &serde_json::Map<String, serde_json::Value>,
    key: &str,
    expected: &str,
) -> bool {
    match options.get(key) {
        Some(serde_json::Value::String(value)) => value.trim() == expected,
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .any(|value| value.trim() == expected),
        _ => false,
    }
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

fn spawn_http_fixture(
    payload: Vec<u8>,
) -> Result<(String, thread::JoinHandle<Result<(), std::io::Error>>), TestError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let address = listener.local_addr()?;
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept()?;
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request)?;
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        stream.write_all(header.as_bytes())?;
        stream.write_all(&payload)?;
        stream.flush()
    });
    Ok((format!("http://{address}/fixture.bin"), handle))
}

fn spawn_slow_http_fixture(
    payload: Vec<u8>,
    hold: Duration,
) -> Result<(String, thread::JoinHandle<Result<(), std::io::Error>>), TestError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let address = listener.local_addr()?;
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept()?;
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request)?;
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        );
        stream.write_all(header.as_bytes())?;
        let first_chunk = payload.len().min(1);
        stream.write_all(&payload[..first_chunk])?;
        stream.flush()?;
        thread::sleep(hold);
        stream.write_all(&payload[first_chunk..])?;
        stream.flush()
    });
    Ok((format!("http://{address}/slow.bin"), handle))
}

async fn wait_for_active_client_task<T>(
    client: &Aria2Client<T>,
    gid: ariadeck_domain::Gid,
    timeout: Duration,
) -> Result<(), TestError>
where
    T: ariadeck_rpc::RpcTransport,
{
    let deadline = Instant::now() + timeout;
    loop {
        if client
            .tell_active(TaskKey::LIST_PROJECTION)
            .await?
            .iter()
            .any(|task| task.gid == gid)
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("task {gid} did not become active"),
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_server_details<T>(
    client: &Aria2Client<T>,
    gid: ariadeck_domain::Gid,
    timeout: Duration,
) -> Result<ariadeck_domain::TaskConnectionDetails, TestError>
where
    T: ariadeck_rpc::RpcTransport,
{
    let deadline = Instant::now() + timeout;
    loop {
        let details = client.connection_details(gid, true, false).await?;
        if !details.servers.is_empty() {
            return Ok(details);
        }
        if Instant::now() >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("task {gid} did not expose an active server"),
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn spawn_http_proxy_fixture(
    payload: Vec<u8>,
) -> Result<(String, thread::JoinHandle<Result<String, std::io::Error>>), TestError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let address = listener.local_addr()?;
    let handle = thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept()?;
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request)?;
            let request = String::from_utf8_lossy(&request[..read]).into_owned();
            if request
                .to_ascii_lowercase()
                .contains("proxy-authorization: basic chjvehktdxnlcjpwcm94es1wyxnz")
            {
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                );
                stream.write_all(header.as_bytes())?;
                stream.write_all(&payload)?;
                stream.flush()?;
                return Ok(request);
            }
            stream.write_all(
                b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"AriaDeck test\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )?;
            stream.flush()?;
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "aria2 did not retry the proxy request with credentials",
        ))
    });
    Ok((format!("http://{address}"), handle))
}

fn spawn_proxy_detector(
    timeout: Duration,
) -> Result<(String, thread::JoinHandle<Result<bool, std::io::Error>>), TestError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let address = listener.local_addr()?;
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + timeout;
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut request = [0_u8; 512];
                    let _ = stream.read(&mut request)?;
                    stream.write_all(
                        b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )?;
                    return Ok(true);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Ok(false);
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(error) => return Err(error),
            }
        }
    });
    Ok((format!("http://{address}"), handle))
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
