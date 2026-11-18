use std::{
    io,
    path::Path,
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

use interprocess::local_socket::{
    GenericNamespaced, ListenerOptions,
    tokio::{Listener, Stream, prelude::*},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    runtime::Runtime,
    sync::watch,
    time,
};

use crate::{
    paths::socket_label,
    protocol::{LaunchRequest, MAX_REQUEST_BYTES, WireRequest, decode_request, encode_request},
};

const IO_TIMEOUT: Duration = Duration::from_secs(2);
const ACCEPT_BACKOFF_INITIAL: Duration = Duration::from_millis(25);
const ACCEPT_BACKOFF_MAX: Duration = Duration::from_secs(1);

pub enum InstanceRole {
    Primary(Receiver<LaunchRequest>),
    Forwarded,
}

/// Handle returned by [`coordinate_instance_with_shutdown`] to gracefully stop
/// the local single-instance IPC listener task.
#[derive(Clone, Debug)]
pub struct InstanceShutdownHandle {
    tx: watch::Sender<bool>,
}

impl InstanceShutdownHandle {
    pub fn shutdown(&self) {
        let _ = self.tx.send(true);
    }
}

/// Become the primary instance, or forward `initial_request` to the one that
/// already holds the socket for `data_dir`.
pub fn coordinate_instance(
    runtime: &Runtime,
    data_dir: &Path,
    initial_request: &LaunchRequest,
) -> io::Result<InstanceRole> {
    let (role, _shutdown) = coordinate_instance_with_shutdown(runtime, data_dir, initial_request)?;
    Ok(role)
}

/// Like [`coordinate_instance`], additionally returning a shutdown handle when
/// this process became the primary instance so the listener task can be stopped
/// cleanly on exit or in tests.
pub fn coordinate_instance_with_shutdown(
    runtime: &Runtime,
    data_dir: &Path,
    initial_request: &LaunchRequest,
) -> io::Result<(InstanceRole, Option<InstanceShutdownHandle>)> {
    let socket_label = socket_label(data_dir);
    let name = socket_label.as_str().to_ns_name::<GenericNamespaced>()?;
    let listener = {
        let _runtime_guard = runtime.enter();
        ListenerOptions::new().name(name).create_tokio()
    };
    match listener {
        Ok(listener) => {
            let (sender, receiver) = mpsc::channel();
            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            runtime.spawn(serve(listener, sender, shutdown_rx));
            Ok((
                InstanceRole::Primary(receiver),
                Some(InstanceShutdownHandle { tx: shutdown_tx }),
            ))
        }
        Err(error) if listener_name_is_occupied(&error) => {
            forward_request(runtime, &socket_label, initial_request)?;
            Ok((InstanceRole::Forwarded, None))
        }
        Err(error) => Err(error),
    }
}

/// Forward a request to the primary instance for `data_dir` without attempting
/// to become primary. Used by the native messaging host, which must never take
/// ownership of the socket.
pub fn forward_to_primary(
    runtime: &Runtime,
    data_dir: &Path,
    request: &LaunchRequest,
) -> io::Result<()> {
    forward_request(runtime, &socket_label(data_dir), request)
}

async fn wait_for_shutdown(shutdown: &mut watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow() {
            return;
        }
        if shutdown.changed().await.is_err() {
            // All shutdown handles were dropped without requesting shutdown;
            // keep serving until the Tokio runtime itself shuts down.
            std::future::pending::<()>().await;
        }
    }
}

async fn serve(
    listener: Listener,
    sender: Sender<LaunchRequest>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut backoff = ACCEPT_BACKOFF_INITIAL;
    loop {
        if *shutdown.borrow() {
            break;
        }
        let accepted = tokio::select! {
            biased;
            () = wait_for_shutdown(&mut shutdown) => break,
            result = listener.accept() => result,
        };
        match accepted {
            Ok(connection) => {
                backoff = ACCEPT_BACKOFF_INITIAL;
                let sender = sender.clone();
                tokio::spawn(async move {
                    let result =
                        match time::timeout(IO_TIMEOUT, handle_connection(connection, &sender))
                            .await
                        {
                            Ok(result) => result,
                            Err(_) => Err(io::Error::new(
                                io::ErrorKind::TimedOut,
                                "local launch request timed out",
                            )),
                        };
                    if let Err(error) = result {
                        tracing::warn!(%error, "rejected local launch request");
                    }
                });
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    backoff_ms = backoff.as_millis(),
                    "failed to accept local launch request"
                );
                tokio::select! {
                    biased;
                    () = wait_for_shutdown(&mut shutdown) => break,
                    () = time::sleep(backoff) => {}
                }
                backoff = backoff.saturating_mul(2).min(ACCEPT_BACKOFF_MAX);
            }
        }
    }
}

async fn handle_connection(connection: Stream, sender: &Sender<LaunchRequest>) -> io::Result<()> {
    let mut reader = BufReader::new(connection);
    let mut buffer = Vec::new();
    let bytes_read = {
        let mut limited = (&mut reader).take((MAX_REQUEST_BYTES + 1) as u64);
        limited.read_until(b'\n', &mut buffer).await?
    };
    if bytes_read == 0 || bytes_read > MAX_REQUEST_BYTES || buffer.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "local launch request is empty, oversized, or unterminated",
        ));
    }
    buffer.pop();
    let request: WireRequest = serde_json::from_slice(&buffer)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let request = decode_request(request)?;
    sender
        .send(request)
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "application is shutting down"))?;
    reader.get_mut().write_all(b"ok\n").await?;
    reader.get_mut().flush().await
}

fn forward_request(
    runtime: &Runtime,
    socket_label: &str,
    request: &LaunchRequest,
) -> io::Result<()> {
    let request = encode_request(request)?;
    let mut payload = serde_json::to_vec(&request).map_err(io::Error::other)?;
    payload.push(b'\n');
    if payload.len() > MAX_REQUEST_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "local launch request is too large",
        ));
    }

    runtime.block_on(async {
        time::timeout(IO_TIMEOUT, async {
            let mut last_error = None;
            for _ in 0..10 {
                let name = socket_label.to_ns_name::<GenericNamespaced>()?;
                match Stream::connect(name).await {
                    Ok(connection) => {
                        let mut reader = BufReader::new(connection);
                        reader.get_mut().write_all(&payload).await?;
                        reader.get_mut().flush().await?;
                        let mut acknowledgement = Vec::new();
                        (&mut reader)
                            .take(17)
                            .read_until(b'\n', &mut acknowledgement)
                            .await?;
                        return if acknowledgement == b"ok\n" {
                            Ok(())
                        } else {
                            Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "primary instance returned an invalid acknowledgement",
                            ))
                        };
                    }
                    Err(error) => {
                        last_error = Some(error);
                        time::sleep(Duration::from_millis(25)).await;
                    }
                }
            }
            Err(last_error.unwrap_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotConnected,
                    "primary instance is unavailable",
                )
            }))
        })
        .await
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "primary instance did not acknowledge the request",
            )
        })?
    })
}

fn listener_name_is_occupied(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::AddrInUse
        || (cfg!(target_os = "windows") && error.kind() == io::ErrorKind::PermissionDenied)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::protocol::tests::sample_download;

    static SOCKET_NONCE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn secondary_instance_forwards_paths_and_receives_acknowledgement() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime starts");
        let nonce = SOCKET_NONCE.fetch_add(1, Ordering::Relaxed);
        let data_dir = std::env::temp_dir().join(format!(
            "ariadeck-instance-test-{}-{nonce}",
            std::process::id()
        ));
        let (role, shutdown) =
            coordinate_instance_with_shutdown(&runtime, &data_dir, &LaunchRequest::default())
                .expect("primary starts");
        let receiver = match role {
            InstanceRole::Primary(receiver) => receiver,
            InstanceRole::Forwarded => panic!("unique socket must become primary"),
        };
        let request = LaunchRequest {
            metadata_paths: vec![
                data_dir.join("sample file.torrent"),
                data_dir.join("示例.meta4"),
            ],
            magnet_uris: vec![
                "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Example".into(),
            ],
            downloads: vec![sample_download()],
        };

        assert!(matches!(
            coordinate_instance(&runtime, &data_dir, &request).expect("secondary forwards"),
            InstanceRole::Forwarded
        ));
        let received = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("primary receives request");
        assert_eq!(received, request);
        if let Some(handle) = shutdown {
            handle.shutdown();
        }
    }

    #[test]
    fn forward_to_primary_reports_when_no_instance_is_listening() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime starts");
        let nonce = SOCKET_NONCE.fetch_add(1, Ordering::Relaxed);
        let data_dir = std::env::temp_dir().join(format!(
            "ariadeck-absent-test-{}-{nonce}",
            std::process::id()
        ));
        let request = LaunchRequest {
            metadata_paths: Vec::new(),
            magnet_uris: Vec::new(),
            downloads: vec![sample_download()],
        };
        assert!(
            forward_to_primary(&runtime, &data_dir, &request).is_err(),
            "forwarding without a primary must fail rather than spool"
        );
    }
}
