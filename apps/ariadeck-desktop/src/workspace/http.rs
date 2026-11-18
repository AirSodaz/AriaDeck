//! Shared bounded HTTP fetch helpers for desktop network actions (updates,
//! verified core download, tracker list refresh).
//!
//! All helpers enforce response size limits while streaming chunks rather than
//! relying solely on `Content-Length`, preventing unbounded memory allocation
//! on chunked or lying responses.

use std::time::Duration;

pub(crate) const USER_AGENT: &str = concat!("AriaDeck/", env!("CARGO_PKG_VERSION"));

pub(crate) fn build_http_client(timeout: Duration, label: &str) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| format!("Could not initialize {label} download: {error}"))
}

/// Fetch `url` into memory while enforcing a hard upper bound of `max_bytes`.
pub(crate) async fn fetch_bounded_bytes(
    url: &str,
    max_bytes: usize,
    timeout: Duration,
    label: &str,
    headers: &[(&str, &str)],
) -> Result<Vec<u8>, String> {
    let client = build_http_client(timeout, label)?;
    let mut request = client.get(url);
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
    if response
        .content_length()
        .is_some_and(|size| size > max_bytes as u64)
    {
        return Err(format!("{label} download is too large."));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("{label} download failed: {error}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!("{label} download is too large."));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Fetch `url` into memory, rejecting early if the response exceeds or differs
/// from `expected_bytes` even when `Content-Length` is omitted (chunked).
pub(crate) async fn fetch_exact_bytes(
    url: &str,
    expected_bytes: u64,
    timeout: Duration,
    label: &str,
) -> Result<Vec<u8>, String> {
    let client = build_http_client(timeout, label)?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("{label} download failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "{label} download failed with HTTP {}.",
            response.status()
        ));
    }
    if let Some(len) = response.content_length()
        && len != expected_bytes
    {
        return Err(format!(
            "{label} download has an unexpected size ({len} bytes, expected {expected_bytes})."
        ));
    }
    let cap = usize::try_from(expected_bytes)
        .map_err(|_| format!("{label} expected size exceeds addressable memory."))?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("{label} download failed: {error}"))?
    {
        let next_len = bytes.len().saturating_add(chunk.len());
        if next_len > cap {
            return Err(format!(
                "{label} download has an unexpected size (exceeded {expected_bytes} bytes)."
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.len() as u64 != expected_bytes {
        return Err(format!(
            "{label} download has an unexpected size ({} bytes, expected {expected_bytes}).",
            bytes.len()
        ));
    }
    Ok(bytes)
}

/// Fetch `url` with streaming size limit and throttled progress callbacks.
pub(crate) async fn fetch_bounded_with_progress<F>(
    url: &str,
    max_bytes: usize,
    timeout: Duration,
    label: &str,
    mut on_progress: F,
) -> Result<Vec<u8>, String>
where
    F: FnMut(u64, Option<u64>),
{
    let client = build_http_client(timeout, label)?;
    let mut response = client
        .get(url)
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
    if total_bytes.is_some_and(|size| size > max_bytes as u64) {
        return Err(format!("{label} download is too large."));
    }

    let mut bytes = Vec::new();
    let mut last_progress_report = 0u64;
    on_progress(0, total_bytes);

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("{label} download failed: {error}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!("{label} download is too large."));
        }
        bytes.extend_from_slice(&chunk);
        let current_len = bytes.len() as u64;
        if current_len.saturating_sub(last_progress_report) >= 256 * 1024
            || total_bytes == Some(current_len)
        {
            last_progress_report = current_len;
            on_progress(current_len, total_bytes);
        }
    }

    Ok(bytes)
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
