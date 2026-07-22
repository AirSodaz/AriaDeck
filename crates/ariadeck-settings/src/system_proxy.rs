//! Resolve the operating-system / environment download proxy.
//!
//! Scope (MVP):
//! - Windows: Internet Settings registry (`ProxyEnable` / `ProxyServer` / `ProxyOverride`)
//! - All platforms: `ALL_PROXY` / `HTTP_PROXY` / `HTTPS_PROXY` / `FTP_PROXY` / `NO_PROXY`
//!   (and lowercase variants), used as the primary source on non-Windows and as a
//!   fallback on Windows when the registry has no static proxy.
//!
//! Non-goals for this module:
//! - PAC / WPAD script evaluation
//! - Auto-fill of proxy credentials from the OS
//! - Per-URL proxy decisions (aria2 only accepts global endpoints)

use std::env;

use thiserror::Error;

use crate::{DownloadProxyMode, DownloadProxySettings};

/// Errors while reading platform proxy configuration.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SystemProxyError {
    #[error("failed to read Windows Internet Settings proxy configuration: {0}")]
    WindowsRegistry(String),
    #[error(
        "system proxy is configured via a PAC/auto-config script, which AriaDeck does not evaluate; switch to Manual or disable auto-proxy"
    )]
    PacUnsupported,
}

/// Snapshot of static system proxy values suitable for aria2 global options.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedSystemProxy {
    pub all_proxy: Option<String>,
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub ftp_proxy: Option<String>,
    pub no_proxy: Vec<String>,
}

impl ResolvedSystemProxy {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.all_proxy.is_none()
            && self.http_proxy.is_none()
            && self.https_proxy.is_none()
            && self.ftp_proxy.is_none()
    }

    /// Map into persisted settings shape with mode kept as [`DownloadProxyMode::System`].
    /// Manual endpoint / credential fields from the previous settings are not carried
    /// forward into the runtime config (they remain on disk for when the user switches back).
    #[must_use]
    pub fn into_runtime_settings(self) -> DownloadProxySettings {
        DownloadProxySettings {
            mode: DownloadProxyMode::System,
            all_proxy: self.all_proxy,
            http_proxy: self.http_proxy,
            https_proxy: self.https_proxy,
            ftp_proxy: self.ftp_proxy,
            no_proxy: self.no_proxy,
            username: None,
            credential: None,
            check_certificate: true,
        }
    }
}

/// Resolve the current system / environment proxy.
///
/// Returns an empty snapshot when the OS is configured for direct connections.
pub fn resolve_system_proxy() -> Result<ResolvedSystemProxy, SystemProxyError> {
    #[cfg(windows)]
    {
        match resolve_windows_internet_settings() {
            Ok(resolved) if !resolved.is_empty() => {
                return Ok(coalesce_identical_protocol_proxies(resolved));
            }
            Ok(_) => {}
            Err(SystemProxyError::PacUnsupported) => return Err(SystemProxyError::PacUnsupported),
            // Registry read failures fall through to environment variables so
            // CI / portable setups still work.
            Err(_error) => {}
        }
    }
    Ok(coalesce_identical_protocol_proxies(resolve_from_env()))
}

/// Pure helper for tests: parse env-style maps without touching the process environment.
#[must_use]
pub fn resolve_from_env_map(get: impl Fn(&str) -> Option<String>) -> ResolvedSystemProxy {
    let pick = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| get(name))
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .map(normalize_proxy_endpoint)
    };
    let all_proxy = pick(&["ALL_PROXY", "all_proxy"]);
    let http_proxy = pick(&["HTTP_PROXY", "http_proxy"]);
    let https_proxy = pick(&["HTTPS_PROXY", "https_proxy"]);
    let ftp_proxy = pick(&["FTP_PROXY", "ftp_proxy"]);
    // Bypass list is hostnames/CIDRs — never run through proxy URL normalization.
    let no_proxy = ["NO_PROXY", "no_proxy"]
        .iter()
        .find_map(|name| get(name))
        .map(|value| parse_no_proxy_list(value.trim()))
        .unwrap_or_default();
    coalesce_identical_protocol_proxies(ResolvedSystemProxy {
        all_proxy,
        http_proxy,
        https_proxy,
        ftp_proxy,
        no_proxy,
    })
}

fn resolve_from_env() -> ResolvedSystemProxy {
    resolve_from_env_map(|name| env::var(name).ok())
}

/// Normalize a system/env proxy endpoint for aria2.
///
/// Critical rules (TLS handshake failures):
/// 1. aria2 speaks **HTTP CONNECT** (or SOCKS) to the proxy. An `https://` proxy
///    URL makes aria2 open TLS **to the proxy process itself**. Local tools
///    (Clash / V2Ray / system "HTTPS proxy" slots) almost always expose a plain
///    HTTP proxy on the same port — `HTTPS_PROXY=https://127.0.0.1:7890` is a
///    common misconfiguration that yields `SSL/TLS handshake failure`.
///    We rewrite proxy-side `https://` → `http://`.
/// 2. Bare `host:port` is rewritten to `http://host:port` so the scheme is explicit.
/// 3. SOCKS endpoints keep a `socks5://` / `socks4://` scheme (required by aria2).
/// 4. Credentials embedded in the URL are stripped (D-004 / keychain only).
fn normalize_proxy_endpoint(raw: String) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Ok(url) = url::Url::parse(trimmed)
        && matches!(
            url.scheme(),
            "http" | "https" | "socks" | "socks5" | "socks4" | "socks4a" | "socks5h"
        )
    {
        return format_aria2_proxy_url(&url);
    }

    // `user:pass@host:port` or `user@host:port` without a supported scheme.
    let hostport = if let Some((userinfo, hostport)) = trimmed.rsplit_once('@')
        && !userinfo.is_empty()
        && !hostport.is_empty()
    {
        hostport
    } else {
        trimmed
    };

    if hostport.is_empty() {
        return String::new();
    }
    // Default bare endpoints to HTTP proxy (Windows Internet Settings style).
    format!("http://{hostport}")
}

fn format_aria2_proxy_url(url: &url::Url) -> String {
    let host = url.host_str().unwrap_or("").to_owned();
    if host.is_empty() {
        return String::new();
    }
    let port = url
        .port_or_known_default()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    let scheme = match url.scheme() {
        // Proxy transport is plain HTTP; HTTPS downloads still use CONNECT.
        "http" | "https" => "http",
        "socks" | "socks5" | "socks5h" => "socks5",
        "socks4" | "socks4a" => "socks4",
        other => other,
    };
    format!("{scheme}://{host}{port}")
}

/// After slot fill-in, prefer a single `all-proxy` when every non-empty protocol
/// slot points at the same endpoint (typical Clash "system proxy" layout).
fn coalesce_identical_protocol_proxies(mut resolved: ResolvedSystemProxy) -> ResolvedSystemProxy {
    let slots = [
        resolved.http_proxy.as_deref(),
        resolved.https_proxy.as_deref(),
        resolved.ftp_proxy.as_deref(),
    ];
    let non_empty: Vec<&str> = slots.into_iter().flatten().collect();
    if resolved.all_proxy.is_none()
        && !non_empty.is_empty()
        && non_empty.iter().all(|endpoint| *endpoint == non_empty[0])
    {
        // One shared HTTP proxy for all protocols — aria2 all-proxy is the
        // simplest surface and avoids empty https-proxy falling back oddly.
        resolved.all_proxy = Some(non_empty[0].to_owned());
        resolved.http_proxy = None;
        resolved.https_proxy = None;
        resolved.ftp_proxy = None;
    } else if resolved.https_proxy.is_none() {
        // OS often only fills the HTTP proxy slot; browsers still use it for
        // HTTPS (CONNECT). Mirror into https-proxy so aria2 does the same when
        // all-proxy is unset.
        if let Some(http) = resolved.http_proxy.clone() {
            resolved.https_proxy = Some(http);
        }
    }
    resolved
}

fn parse_no_proxy_list(value: &str) -> Vec<String> {
    value
        .split([',', ';', '\n', '\r'])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        // Windows uses `<local>` for loopback/intranet; map to common no-proxy tokens.
        .map(|entry| {
            if entry.eq_ignore_ascii_case("<local>") {
                "localhost,127.0.0.1,::1".to_owned()
            } else {
                entry.to_owned()
            }
        })
        .flat_map(|entry| {
            entry
                .split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(windows)]
fn resolve_windows_internet_settings() -> Result<ResolvedSystemProxy, SystemProxyError> {
    resolve_windows_internet_settings_via_winreg()
}

#[cfg(windows)]
fn resolve_windows_internet_settings_via_winreg() -> Result<ResolvedSystemProxy, SystemProxyError> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let internet_settings = hkcu
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .map_err(|error| SystemProxyError::WindowsRegistry(error.to_string()))?;

    // AutoConfigURL indicates a PAC script — we refuse rather than silently ignore.
    if let Ok(auto_config) = internet_settings.get_value::<String, _>("AutoConfigURL")
        && !auto_config.trim().is_empty()
    {
        // If ProxyEnable is also set with a static server, prefer static (common dual setup).
        let proxy_enable = internet_settings
            .get_value::<u32, _>("ProxyEnable")
            .unwrap_or(0);
        let proxy_server = internet_settings
            .get_value::<String, _>("ProxyServer")
            .unwrap_or_default();
        if proxy_enable == 0 || proxy_server.trim().is_empty() {
            return Err(SystemProxyError::PacUnsupported);
        }
    }

    let proxy_enable = internet_settings
        .get_value::<u32, _>("ProxyEnable")
        .unwrap_or(0);
    if proxy_enable == 0 {
        return Ok(ResolvedSystemProxy::default());
    }

    let proxy_server = internet_settings
        .get_value::<String, _>("ProxyServer")
        .map_err(|error| SystemProxyError::WindowsRegistry(error.to_string()))?;
    let proxy_override = internet_settings
        .get_value::<String, _>("ProxyOverride")
        .unwrap_or_default();

    Ok(coalesce_identical_protocol_proxies(
        parse_windows_proxy_server(&proxy_server, &proxy_override),
    ))
}

/// Parse Windows `ProxyServer` / `ProxyOverride` strings into protocol slots.
///
/// Formats observed in the wild:
/// - `host:port` (applies to all)
/// - `http=host:port;https=host:port;ftp=host:port;socks=host:port`
#[must_use]
pub fn parse_windows_proxy_server(proxy_server: &str, proxy_override: &str) -> ResolvedSystemProxy {
    let mut resolved = ResolvedSystemProxy {
        no_proxy: parse_no_proxy_list(proxy_override),
        ..ResolvedSystemProxy::default()
    };
    let trimmed = proxy_server.trim();
    if trimmed.is_empty() {
        return resolved;
    }

    if !trimmed.contains('=') {
        resolved.all_proxy = Some(normalize_proxy_endpoint(trimmed.to_owned()));
        return resolved;
    }

    for part in trimmed.split([';', ' ']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((scheme, endpoint)) = part.split_once('=') else {
            continue;
        };
        let endpoint = normalize_proxy_endpoint(endpoint.trim().to_owned());
        if endpoint.is_empty() {
            continue;
        }
        match scheme.trim().to_ascii_lowercase().as_str() {
            "http" => resolved.http_proxy = Some(endpoint),
            // Windows "https=" names the *target traffic* protocol, not TLS-to-proxy.
            // Endpoint is already normalized to http:// (or socks5://).
            "https" => resolved.https_proxy = Some(endpoint),
            "ftp" => resolved.ftp_proxy = Some(endpoint),
            // SOCKS-only system configs: keep socks5:// so aria2 does not try HTTP.
            "socks" | "socks5" if resolved.all_proxy.is_none() => {
                let socks = if endpoint.starts_with("socks") {
                    endpoint
                } else {
                    format!(
                        "socks5://{}",
                        endpoint
                            .trim_start_matches("http://")
                            .trim_start_matches("https://")
                    )
                };
                resolved.all_proxy = Some(socks);
            }
            _ => {}
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn env_map_reads_uppercase_and_lowercase() {
        let mut map = HashMap::new();
        map.insert("http_proxy".to_owned(), "http://proxy.example:8080".into());
        map.insert("NO_PROXY".to_owned(), "localhost, 10.0.0.0/8".into());
        let resolved = resolve_from_env_map(|name| map.get(name).cloned());
        // Single HTTP_PROXY coalesces into all-proxy for aria2.
        assert_eq!(
            resolved.all_proxy.as_deref(),
            Some("http://proxy.example:8080")
        );
        assert_eq!(
            resolved.no_proxy,
            vec!["localhost".to_owned(), "10.0.0.0/8".to_owned()]
        );
    }

    #[test]
    fn normalize_strips_embedded_credentials_and_defaults_scheme() {
        assert_eq!(
            normalize_proxy_endpoint("http://user:secret@proxy.example:8080/".into()),
            "http://proxy.example:8080"
        );
        assert_eq!(
            normalize_proxy_endpoint("user:secret@proxy.example:8080".into()),
            "http://proxy.example:8080"
        );
        assert_eq!(
            normalize_proxy_endpoint("proxy.example:8080".into()),
            "http://proxy.example:8080"
        );
    }

    #[test]
    fn normalize_rewrites_https_proxy_scheme_to_http() {
        // HTTPS_PROXY=https://… is a common env mistake; aria2 would TLS-handshake the proxy.
        assert_eq!(
            normalize_proxy_endpoint("https://127.0.0.1:7890".into()),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            normalize_proxy_endpoint("socks5://127.0.0.1:7891".into()),
            "socks5://127.0.0.1:7891"
        );
    }

    #[test]
    fn windows_proxy_server_all_form() {
        let resolved = parse_windows_proxy_server("proxy.example:8080", "localhost;<local>");
        assert_eq!(
            resolved.all_proxy.as_deref(),
            Some("http://proxy.example:8080")
        );
        assert!(resolved.no_proxy.contains(&"localhost".to_owned()));
        assert!(resolved.no_proxy.contains(&"127.0.0.1".to_owned()));
    }

    #[test]
    fn windows_proxy_server_per_protocol() {
        let resolved = parse_windows_proxy_server(
            "http=http-proxy:80;https=https-proxy:443;ftp=ftp-proxy:21",
            "",
        );
        assert_eq!(resolved.http_proxy.as_deref(), Some("http://http-proxy:80"));
        assert_eq!(
            resolved.https_proxy.as_deref(),
            Some("http://https-proxy:443")
        );
        assert_eq!(resolved.ftp_proxy.as_deref(), Some("http://ftp-proxy:21"));
        assert!(resolved.all_proxy.is_none());
    }

    #[test]
    fn identical_http_https_slots_coalesce_to_all_proxy() {
        let mut map = HashMap::new();
        map.insert("HTTP_PROXY".to_owned(), "http://127.0.0.1:7890".into());
        map.insert("HTTPS_PROXY".to_owned(), "https://127.0.0.1:7890".into());
        let resolved = resolve_from_env_map(|name| map.get(name).cloned());
        assert_eq!(resolved.all_proxy.as_deref(), Some("http://127.0.0.1:7890"));
        assert!(resolved.http_proxy.is_none());
        assert!(resolved.https_proxy.is_none());
    }

    #[test]
    fn http_only_slot_mirrors_to_https_proxy() {
        let mut map = HashMap::new();
        map.insert("HTTP_PROXY".to_owned(), "http://proxy.example:8080".into());
        let resolved = resolve_from_env_map(|name| map.get(name).cloned());
        // Single protocol value becomes all-proxy via coalesce, which is ideal for aria2.
        assert_eq!(
            resolved.all_proxy.as_deref(),
            Some("http://proxy.example:8080")
        );
    }

    #[test]
    fn into_runtime_settings_keeps_system_mode() {
        let settings = ResolvedSystemProxy {
            all_proxy: Some("http://proxy.example:8080".into()),
            ..ResolvedSystemProxy::default()
        }
        .into_runtime_settings();
        assert_eq!(settings.mode, DownloadProxyMode::System);
        assert_eq!(
            settings.all_proxy.as_deref(),
            Some("http://proxy.example:8080")
        );
        assert!(settings.username.is_none());
        assert!(settings.credential.is_none());
    }
}
