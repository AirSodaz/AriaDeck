use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

use data_encoding::HEXLOWER;
use interprocess::local_socket::{
    GenericNamespaced, ListenerOptions,
    tokio::{Listener, Stream, prelude::*},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    runtime::Runtime,
    time,
};

const PROTOCOL_VERSION: u8 = 1;
const MAX_REQUEST_BYTES: usize = 256 * 1024;
pub(crate) const MAX_LAUNCH_PATHS: usize = 32;
const MAX_PATH_UNITS: usize = 32_768;
const IO_TIMEOUT: Duration = Duration::from_secs(2);

#[cfg(target_os = "windows")]
type EncodedPath = Vec<u16>;
#[cfg(not(target_os = "windows"))]
type EncodedPath = Vec<u8>;

#[derive(Debug)]
pub(crate) struct LaunchRequest {
    pub(crate) paths: Vec<PathBuf>,
}

pub(crate) enum InstanceRole {
    Primary(Receiver<LaunchRequest>),
    Forwarded,
}

#[derive(Debug, Deserialize, Serialize)]
struct WireRequest {
    version: u8,
    paths: Vec<EncodedPath>,
}

pub(crate) fn coordinate_instance(
    runtime: &Runtime,
    data_dir: &Path,
    initial_paths: &[PathBuf],
) -> io::Result<InstanceRole> {
    let socket_label = socket_label(data_dir);
    let name = socket_label.as_str().to_ns_name::<GenericNamespaced>()?;
    let listener = {
        let _runtime_guard = runtime.enter();
        ListenerOptions::new().name(name).create_tokio()
    };
    match listener {
        Ok(listener) => {
            let (sender, receiver) = mpsc::channel();
            runtime.spawn(serve(listener, sender));
            Ok(InstanceRole::Primary(receiver))
        }
        Err(error) if listener_name_is_occupied(&error) => {
            forward_request(runtime, &socket_label, initial_paths)?;
            Ok(InstanceRole::Forwarded)
        }
        Err(error) => Err(error),
    }
}

async fn serve(listener: Listener, sender: Sender<LaunchRequest>) {
    loop {
        match listener.accept().await {
            Ok(connection) => {
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
            Err(error) => tracing::warn!(%error, "failed to accept local launch request"),
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
    let paths = decode_request(request)?;
    sender
        .send(LaunchRequest { paths })
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "application is shutting down"))?;
    reader.get_mut().write_all(b"ok\n").await?;
    reader.get_mut().flush().await
}

fn forward_request(runtime: &Runtime, socket_label: &str, paths: &[PathBuf]) -> io::Result<()> {
    let request = encode_request(paths)?;
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

fn encode_request(paths: &[PathBuf]) -> io::Result<WireRequest> {
    if paths.len() > MAX_LAUNCH_PATHS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "too many metadata paths",
        ));
    }
    let paths = paths
        .iter()
        .map(|path| {
            let encoded = encode_path(path);
            if encoded.len() > MAX_PATH_UNITS {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "metadata path is too long",
                ));
            }
            Ok(encoded)
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(WireRequest {
        version: PROTOCOL_VERSION,
        paths,
    })
}

fn decode_request(request: WireRequest) -> io::Result<Vec<PathBuf>> {
    if request.version != PROTOCOL_VERSION || request.paths.len() > MAX_LAUNCH_PATHS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported local launch request",
        ));
    }
    request
        .paths
        .into_iter()
        .map(|path| {
            if path.len() > MAX_PATH_UNITS {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "metadata path is too long",
                ));
            }
            Ok(decode_path(path))
        })
        .collect()
}

fn socket_label(data_dir: &Path) -> String {
    let digest = Sha256::digest(data_dir.as_os_str().as_encoded_bytes());
    format!("ariadeck-{}", HEXLOWER.encode(&digest[..16]))
}

#[cfg(target_os = "windows")]
fn encode_path(path: &Path) -> EncodedPath {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().collect()
}

#[cfg(target_os = "windows")]
fn decode_path(path: EncodedPath) -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    PathBuf::from(OsString::from_wide(&path))
}

#[cfg(not(target_os = "windows"))]
fn encode_path(path: &Path) -> EncodedPath {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(target_os = "windows"))]
fn decode_path(path: EncodedPath) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(OsString::from_vec(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SOCKET_NONCE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn wire_request_round_trips_paths_without_text_conversion() {
        let paths = vec![PathBuf::from("D:/Downloads/示例 file.torrent")];
        let decoded = decode_request(encode_request(&paths).expect("request encodes"))
            .expect("request decodes");
        assert_eq!(decoded, paths);
    }

    #[test]
    fn request_bounds_reject_path_floods_and_future_versions() {
        let paths = vec![PathBuf::from("sample.torrent"); MAX_LAUNCH_PATHS + 1];
        assert_eq!(
            encode_request(&paths)
                .expect_err("path flood must fail")
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            decode_request(WireRequest {
                version: PROTOCOL_VERSION + 1,
                paths: Vec::new(),
            })
            .expect_err("future protocol must fail")
            .kind(),
            io::ErrorKind::InvalidData
        );
    }

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
        let receiver = match coordinate_instance(&runtime, &data_dir, &[]).expect("primary starts")
        {
            InstanceRole::Primary(receiver) => receiver,
            InstanceRole::Forwarded => panic!("unique socket must become primary"),
        };
        let paths = vec![
            data_dir.join("sample file.torrent"),
            data_dir.join("示例.meta4"),
        ];

        assert!(matches!(
            coordinate_instance(&runtime, &data_dir, &paths).expect("secondary forwards"),
            InstanceRole::Forwarded
        ));
        let request = receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("primary receives request");
        assert_eq!(request.paths, paths);
    }
}
