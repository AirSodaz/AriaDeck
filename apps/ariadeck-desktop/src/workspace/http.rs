//! Shared bounded HTTP fetch helpers for desktop network actions (updates,
//! verified core download, tracker list refresh).
//!
//! All helpers reuse a single [`reqwest::Client`] (preserving TLS root setup
//! and connection pools), reject HTTPS -> HTTP redirect downgrades, and enforce
//! response size limits while streaming chunks rather than relying solely on
//! `Content-Length`.

use std::{sync::OnceLock, time::Duration};

pub(crate) const USER_AGENT: &str = concat!("AriaDeck/", env!("CARGO_PKG_VERSION"));
const MAX_REDIRECT_HOPS: usize = 5;

static SHARED_HTTP_CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();

fn is_https_to_http_downgrade(previous_schemes: &[&str], next_scheme: &str) -> bool {
    previous_schemes.contains(&"https") && next_scheme == "http"
}

fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= MAX_REDIRECT_HOPS {
            return attempt.error("too many redirects");
        }
        let previous_schemes: Vec<&str> = attempt
            .previous()
            .iter()
            .map(reqwest::Url::scheme)
            .collect();
        if is_https_to_http_downgrade(&previous_schemes, attempt.url().scheme()) {
            return attempt.error("refused HTTPS to HTTP redirect downgrade");
        }
        attempt.follow()
    })
}

fn shared_http_client(label: &str) -> Result<&'static reqwest::Client, String> {
    let client = SHARED_HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .redirect(redirect_policy())
            .user_agent(USER_AGENT)
            .build()
            .map_err(|error| error.to_string())
    });
    client
        .as_ref()
        .map_err(|error| format!("Could not initialize {label} download: {error}"))
}

enum StreamSizeRule {
    Max(usize),
    Exact(u64),
}

enum StreamSink<'a> {
    Vec(&'a mut Vec<u8>),
    Channel(&'a tokio::sync::mpsc::Sender<Vec<u8>>),
}

async fn fetch_stream_with_sink<F>(
    url: &str,
    size_rule: StreamSizeRule,
    timeout: Duration,
    label: &str,
    headers: &[(&str, &str)],
    mut on_progress: Option<F>,
    mut sink: StreamSink<'_>,
) -> Result<(), String>
where
    F: FnMut(u64, Option<u64>),
{
    let client = shared_http_client(label)?;
    let mut request = client.get(url).timeout(timeout);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut response = request
        .send()
        .await
        .map_err(|error| format!("{label} download failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "{label} download failed with HTTP {}.",
            response.status()
        ));
    }

    let total_bytes = response.content_length();
    let max_bytes = match size_rule {
        StreamSizeRule::Max(limit) => {
            if total_bytes.is_some_and(|size| size > limit as u64) {
                return Err(format!("{label} download is too large."));
            }
            limit
        }
        StreamSizeRule::Exact(expected_bytes) => {
            if let Some(len) = total_bytes
                && len != expected_bytes
            {
                return Err(format!(
                    "{label} download has an unexpected size ({len} bytes, expected {expected_bytes})."
                ));
            }
            usize::try_from(expected_bytes)
                .map_err(|_| format!("{label} expected size exceeds addressable memory."))?
        }
    };

    let mut received_bytes = 0usize;
    let mut last_progress_report = 0u64;
    if let Some(callback) = on_progress.as_mut() {
        callback(0, total_bytes);
    }

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("{label} download failed: {error}"))?
    {
        if received_bytes.saturating_add(chunk.len()) > max_bytes {
            return match size_rule {
                StreamSizeRule::Max(_) => Err(format!("{label} download is too large.")),
                StreamSizeRule::Exact(expected_bytes) => Err(format!(
                    "{label} download has an unexpected size (exceeded {expected_bytes} bytes)."
                )),
            };
        }
        received_bytes += chunk.len();
        match &mut sink {
            StreamSink::Vec(bytes) => bytes.extend_from_slice(&chunk),
            StreamSink::Channel(tx) => {
                tx.send(chunk.to_vec())
                    .await
                    .map_err(|_| format!("{label} download consumer stopped unexpectedly."))?;
            }
        }
        if let Some(callback) = on_progress.as_mut() {
            let current_len = received_bytes as u64;
            if current_len.saturating_sub(last_progress_report) >= 256 * 1024
                || total_bytes == Some(current_len)
            {
                last_progress_report = current_len;
                callback(current_len, total_bytes);
            }
        }
    }

    if let StreamSizeRule::Exact(expected_bytes) = size_rule
        && received_bytes as u64 != expected_bytes
    {
        return Err(format!(
            "{label} download has an unexpected size ({received_bytes} bytes, expected {expected_bytes})."
        ));
    }

    Ok(())
}

async fn fetch_stream_bytes<F>(
    url: &str,
    size_rule: StreamSizeRule,
    timeout: Duration,
    label: &str,
    headers: &[(&str, &str)],
    on_progress: Option<F>,
) -> Result<Vec<u8>, String>
where
    F: FnMut(u64, Option<u64>),
{
    let mut bytes = Vec::new();
    fetch_stream_with_sink(
        url,
        size_rule,
        timeout,
        label,
        headers,
        on_progress,
        StreamSink::Vec(&mut bytes),
    )
    .await?;
    Ok(bytes)
}

/// Stream chunks up to `max_bytes` into a bounded channel without buffering
/// the entire payload in memory.
pub(crate) async fn fetch_bounded_to_channel<F>(
    url: &str,
    max_bytes: usize,
    timeout: Duration,
    label: &str,
    on_progress: F,
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
) -> Result<(), String>
where
    F: FnMut(u64, Option<u64>),
{
    fetch_stream_with_sink(
        url,
        StreamSizeRule::Max(max_bytes),
        timeout,
        label,
        &[],
        Some(on_progress),
        StreamSink::Channel(&tx),
    )
    .await
}

/// Fetch `url` into memory while enforcing a hard upper bound of `max_bytes`.
pub(crate) async fn fetch_bounded_bytes(
    url: &str,
    max_bytes: usize,
    timeout: Duration,
    label: &str,
    headers: &[(&str, &str)],
) -> Result<Vec<u8>, String> {
    fetch_stream_bytes::<fn(u64, Option<u64>)>(
        url,
        StreamSizeRule::Max(max_bytes),
        timeout,
        label,
        headers,
        None,
    )
    .await
}

/// Fetch `url` into memory, rejecting early if the response exceeds or differs
/// from `expected_bytes` even when `Content-Length` is omitted (chunked).
pub(crate) async fn fetch_exact_bytes(
    url: &str,
    expected_bytes: u64,
    timeout: Duration,
    label: &str,
) -> Result<Vec<u8>, String> {
    fetch_stream_bytes::<fn(u64, Option<u64>)>(
        url,
        StreamSizeRule::Exact(expected_bytes),
        timeout,
        label,
        &[],
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::TcpListener,
    };

    async fn serve_chunked(chunks: Vec<Vec<u8>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut req_buf = [0u8; 1024];
                let _ = stream.read(&mut req_buf).await;
                let header =
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
                if stream.write_all(header).await.is_err() {
                    return;
                }
                for chunk in chunks {
                    let prefix = format!("{:x}\r\n", chunk.len());
                    if stream.write_all(prefix.as_bytes()).await.is_err() {
                        return;
                    }
                    if stream.write_all(&chunk).await.is_err() {
                        return;
                    }
                    if stream.write_all(b"\r\n").await.is_err() {
                        return;
                    }
                }
                let _ = stream.write_all(b"0\r\n\r\n").await;
            }
        });
        format!("http://{addr}/payload")
    }

    async fn serve_redirect_loop(hops: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            for hop in 0..=hops {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut req_buf = [0u8; 1024];
                let _ = stream.read(&mut req_buf).await;
                let next = hop + 1;
                let response = format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://{addr}/hop-{next}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        format!("http://{addr}/hop-0")
    }

    #[test]
    fn redirect_downgrade_check_rejects_https_to_http_and_allows_https_to_https() {
        assert!(is_https_to_http_downgrade(&["https"], "http"));
        assert!(is_https_to_http_downgrade(&["http", "https"], "http"));
        assert!(!is_https_to_http_downgrade(&["https"], "https"));
        assert!(!is_https_to_http_downgrade(&["http"], "https"));
        assert!(!is_https_to_http_downgrade(&["http"], "http"));
    }

    #[tokio::test]
    async fn fetch_bounded_bytes_enforces_redirect_hop_limit() {
        let url = serve_redirect_loop(MAX_REDIRECT_HOPS + 2).await;
        let err = fetch_bounded_bytes(&url, 1024, Duration::from_secs(5), "Redirect", &[])
            .await
            .expect_err("excessive redirects must fail");
        assert!(
            err.contains("too many redirects") || err.contains("download failed"),
            "unexpected redirect error: {err}"
        );
    }

    #[tokio::test]
    async fn fetch_bounded_bytes_rejects_oversized_chunked_response() {
        let url = serve_chunked(vec![vec![b'a'; 64], vec![b'b'; 64]]).await;
        let err = fetch_bounded_bytes(&url, 100, Duration::from_secs(5), "Test", &[])
            .await
            .expect_err("oversized chunked body must fail");
        assert!(err.contains("too large"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn fetch_exact_bytes_rejects_oversized_chunked_response() {
        let url = serve_chunked(vec![vec![b'x'; 32], vec![b'y'; 32]]).await;
        let err = fetch_exact_bytes(&url, 40, Duration::from_secs(5), "Archive")
            .await
            .expect_err("oversized chunked exact fetch must fail");
        assert!(err.contains("unexpected size"), "unexpected error: {err}");
    }

    #[tokio::test]
    async fn fetch_exact_bytes_accepts_exact_chunked_response() {
        let url = serve_chunked(vec![b"hello ".to_vec(), b"world".to_vec()]).await;
        let bytes = fetch_exact_bytes(&url, 11, Duration::from_secs(5), "Archive")
            .await
            .expect("exact chunked response should succeed");
        assert_eq!(bytes, b"hello world");
    }
}
