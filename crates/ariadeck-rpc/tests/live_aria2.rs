use std::{
    env,
    error::Error,
    net::TcpListener,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

use ariadeck_rpc::{
    Aria2Client, AuthenticatedTransport, RpcSecret, TaskKey, WebSocketConfig, WebSocketTransport,
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
