//! Shared bounded HTTP fetch helpers for desktop network actions (updates,
//! verified core download, tracker list refresh).
//!
//! All helpers reuse a single [`reqwest::Client`] (preserving TLS root setup
//! and connection pools), reject HTTPS -> HTTP redirect downgrades, and enforce
//! response size limits while streaming chunks rather than relying solely on
//! `Content-Length`.

use std::{path::Path, sync::OnceLock, time::Duration};

use ariadeck_application::{DownloadProxyConfig, DownloadProxyMode};
use secrecy::ExposeSecret;

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

/// Metadata resolved by a lightweight pre-add / relink HTTP probe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UrlProbeResult {
    pub(crate) final_url: String,
    pub(crate) file_name: Option<String>,
    pub(crate) total_bytes: Option<u64>,
    pub(crate) etag: Option<String>,
    pub(crate) accept_ranges: bool,
    pub(crate) status_code: u16,
}

fn build_probe_client(proxy: Option<&DownloadProxyConfig>) -> Result<reqwest::Client, String> {
    let Some(proxy) = proxy.filter(|cfg| cfg.mode != DownloadProxyMode::Disabled) else {
        return shared_http_client("probe").cloned();
    };
    let mut builder = reqwest::Client::builder()
        .redirect(redirect_policy())
        .user_agent(USER_AGENT);

    let no_proxy = (!proxy.no_proxy.is_empty())
        .then(|| reqwest::NoProxy::from_string(&proxy.no_proxy.join(",")))
        .flatten();
    let apply_auth = |mut req_proxy: reqwest::Proxy| -> reqwest::Proxy {
        if let Some(user) = proxy.username.as_deref().filter(|u| !u.is_empty()) {
            let pass = proxy
                .password
                .as_ref()
                .map(ExposeSecret::expose_secret)
                .unwrap_or("");
            req_proxy = req_proxy.basic_auth(user, pass);
        }
        if let Some(ref bypass) = no_proxy {
            req_proxy = req_proxy.no_proxy(Some(bypass.clone()));
        }
        req_proxy
    };

    let mut configured = false;
    if let Some(all) = proxy.all_proxy.as_deref().filter(|v| !v.is_empty()) {
        let p = reqwest::Proxy::all(all).map_err(|e| format!("Invalid proxy URL: {e}"))?;
        builder = builder.proxy(apply_auth(p));
        configured = true;
    } else {
        if let Some(http) = proxy.http_proxy.as_deref().filter(|v| !v.is_empty()) {
            let p =
                reqwest::Proxy::http(http).map_err(|e| format!("Invalid HTTP proxy URL: {e}"))?;
            builder = builder.proxy(apply_auth(p));
            configured = true;
        }
        if let Some(https) = proxy.https_proxy.as_deref().filter(|v| !v.is_empty()) {
            let p = reqwest::Proxy::https(https)
                .map_err(|e| format!("Invalid HTTPS proxy URL: {e}"))?;
            builder = builder.proxy(apply_auth(p));
            configured = true;
        }
    }
    if !configured {
        return shared_http_client("probe").cloned();
    }
    builder
        .build()
        .map_err(|error| format!("Could not initialize probe HTTP client: {error}"))
}

/// Probe an HTTP(S) URL with `HEAD` (falling back to `GET` + `Range: bytes=0-0`
/// when servers reject `HEAD` or omit filename/size) to discover final URL,
/// sanitized filename, total byte size, and ETag.
pub(crate) async fn probe_download_url(
    url: &str,
    headers: &[(String, String)],
    proxy: Option<&DownloadProxyConfig>,
    timeout: Duration,
) -> Result<UrlProbeResult, String> {
    let parsed =
        reqwest::Url::parse(url.trim()).map_err(|error| format!("Invalid URL: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "HTTP probe supports only http:// and https:// URLs (got {}).",
            parsed.scheme()
        ));
    }
    let client = build_probe_client(proxy)?;
    let send = |method: reqwest::Method, range_zero: bool| {
        let mut req = client.request(method, parsed.clone()).timeout(timeout);
        for (name, value) in headers {
            if name.eq_ignore_ascii_case("cookie") || name.eq_ignore_ascii_case("authorization") {
                continue;
            }
            req = req.header(name.as_str(), value.as_str());
        }
        if range_zero {
            req = req.header(reqwest::header::RANGE, "bytes=0-0");
        }
        req.send()
    };

    let head_resp = send(reqwest::Method::HEAD, false)
        .await
        .map_err(|error| format!("Probe request failed: {error}"))?;
    let head_status = head_resp.status();
    let head_result = extract_probe_from_response(&head_resp);

    let needs_range_fallback = matches!(head_status.as_u16(), 403 | 405 | 501)
        || (head_status.is_success()
            && head_result.file_name.is_none()
            && head_result.total_bytes.is_none());

    if !needs_range_fallback {
        if !head_status.is_success() {
            return Err(format!("Probe returned HTTP {head_status}."));
        }
        return Ok(finalize_probe(head_result));
    }

    let range_resp = match send(reqwest::Method::GET, true).await {
        Ok(resp) => resp,
        Err(_) if head_status.is_success() => return Ok(finalize_probe(head_result)),
        Err(error) => return Err(format!("Probe request failed: {error}")),
    };
    let range_status = range_resp.status();
    if !range_status.is_success() {
        if head_status.is_success() {
            return Ok(finalize_probe(head_result));
        }
        return Err(format!("Probe returned HTTP {range_status}."));
    }
    Ok(finalize_probe(extract_probe_from_response(&range_resp)))
}

fn extract_probe_from_response(response: &reqwest::Response) -> UrlProbeResult {
    let status = response.status();
    let status_code = status.as_u16();
    let final_url = response.url().to_string();
    let headers = response.headers();

    let disposition_name = headers
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_content_disposition_filename);

    let total_bytes = if status == reqwest::StatusCode::PARTIAL_CONTENT {
        headers
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_range_total)
    } else {
        response.content_length()
    };

    let etag = headers
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());

    let accept_ranges = status == reqwest::StatusCode::PARTIAL_CONTENT
        || headers
            .get(reqwest::header::ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("bytes"));

    UrlProbeResult {
        final_url,
        file_name: disposition_name,
        total_bytes,
        etag,
        accept_ranges,
        status_code,
    }
}

fn finalize_probe(mut result: UrlProbeResult) -> UrlProbeResult {
    if result.file_name.is_none() {
        result.file_name = ariadeck_domain::filename_hint_from_source(&result.final_url)
            .and_then(|name| sanitize_probe_filename(&name));
    }
    result
}

fn parse_content_range_total(value: &str) -> Option<u64> {
    let (_, total) = value.trim().rsplit_once('/')?;
    let total = total.trim();
    if total == "*" {
        return None;
    }
    total.parse::<u64>().ok()
}

fn parse_content_disposition_filename(header: &str) -> Option<String> {
    // Prefer RFC 5987 `filename*=` over `filename=`.
    let mut plain_candidate = None;
    for part in header.split(';') {
        let part = part.trim();
        let Some((key, val)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let val = val.trim().trim_matches('"');
        if key == "filename*" {
            let encoded = val.split_once("''").map(|(_, rest)| rest).unwrap_or(val);
            let decoded = percent_decode_utf8(encoded);
            if let Some(safe) = sanitize_probe_filename(&decoded) {
                return Some(safe);
            }
        } else if key == "filename" && plain_candidate.is_none() {
            plain_candidate = sanitize_probe_filename(val);
        }
    }
    plain_candidate
}

fn sanitize_probe_filename(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return None;
    }
    let name = Path::new(trimmed)
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::trim)?;
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    Some(name.to_owned())
}

fn percent_decode_utf8(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&input[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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

    #[tokio::test]
    async fn probe_download_url_falls_back_from_405_to_range_get_and_parses_rfc5987() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            for _ in 0..2 {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let resp = if req.starts_with("HEAD ") {
                    "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_owned()
                } else {
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-0/4096\r\nContent-Length: 1\r\nContent-Disposition: attachment; filename=\"fallback.bin\"; filename*=UTF-8''release%20pack.zip\r\nETag: \"v1\"\r\nConnection: close\r\n\r\nx"
                        .to_owned()
                };
                let _ = stream.write_all(resp.as_bytes()).await;
            }
        });

        let res = probe_download_url(
            &format!("http://{addr}/download.aspx?id=42"),
            &[("Referer".into(), "https://example.test/".into())],
            None,
            Duration::from_secs(5),
        )
        .await
        .expect("probe should succeed via range fallback");
        assert_eq!(res.file_name.as_deref(), Some("release pack.zip"));
        assert_eq!(res.total_bytes, Some(4096));
        assert_eq!(res.etag.as_deref(), Some("\"v1\""));
        assert!(res.accept_ranges);
    }
}
