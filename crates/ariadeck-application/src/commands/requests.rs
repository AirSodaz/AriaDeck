use std::{collections::HashSet, fmt, sync::Arc};

use ariadeck_domain::{
    DownloadStatus, EnginePath, TaskConnectionPolicy, TaskIdentity, TaskMetadata,
};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use super::outcome::{ApplicationError, ApplicationErrorCode};

#[derive(Clone, Eq, PartialEq)]
pub enum AddDownloadSource {
    Uris(Vec<String>),
    Torrent(Arc<[u8]>),
    Metalink(Arc<[u8]>),
}

impl Default for AddDownloadSource {
    fn default() -> Self {
        Self::Uris(Vec::new())
    }
}

impl fmt::Debug for AddDownloadSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uris(uris) => {
                let redacted: Vec<String> = uris
                    .iter()
                    .map(|uri| ariadeck_domain::redact_source_uri(uri))
                    .collect();
                formatter.debug_tuple("Uris").field(&redacted).finish()
            }
            Self::Torrent(content) => formatter
                .debug_struct("Torrent")
                .field("content_bytes", &content.len())
                .finish(),
            Self::Metalink(content) => formatter
                .debug_struct("Metalink")
                .field("content_bytes", &content.len())
                .finish(),
        }
    }
}

/// Typed advanced request controls for a new download (ADD-005 / D-022).
///
/// Secrets stay as `SecretString` and are flattened into aria2 option pairs only
/// at the RPC adapter boundary. They must never enter task rows, notices, or
/// Debug output.
#[derive(Clone, Default)]
pub struct AddDownloadAdvancedOptions {
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    /// Raw header lines excluding `Cookie:` and `Authorization:` (those have
    /// dedicated fields so they can be redacted consistently).
    pub headers: Vec<String>,
    pub cookie: Option<SecretString>,
    pub http_user: Option<String>,
    pub http_passwd: Option<SecretString>,
    /// aria2 checksum form `TYPE=DIGEST`, for example `sha-256=…`.
    pub checksum: Option<String>,
}

impl fmt::Debug for AddDownloadAdvancedOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AddDownloadAdvancedOptions")
            .field("referer", &self.referer)
            .field("user_agent", &self.user_agent)
            .field("headers", &self.headers)
            .field("cookie", &self.cookie.as_ref().map(|_| "[REDACTED]"))
            .field("http_user", &self.http_user)
            .field(
                "http_passwd",
                &self.http_passwd.as_ref().map(|_| "[REDACTED]"),
            )
            .field("checksum", &self.checksum)
            .finish()
    }
}

impl PartialEq for AddDownloadAdvancedOptions {
    fn eq(&self, other: &Self) -> bool {
        self.referer == other.referer
            && self.user_agent == other.user_agent
            && self.headers == other.headers
            && self.cookie.as_ref().map(ExposeSecret::expose_secret)
                == other.cookie.as_ref().map(ExposeSecret::expose_secret)
            && self.http_user == other.http_user
            && self.http_passwd.as_ref().map(ExposeSecret::expose_secret)
                == other.http_passwd.as_ref().map(ExposeSecret::expose_secret)
            && self.checksum == other.checksum
    }
}

impl Eq for AddDownloadAdvancedOptions {}

impl AddDownloadAdvancedOptions {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.referer.is_none()
            && self.user_agent.is_none()
            && self.headers.is_empty()
            && self.cookie.is_none()
            && self.http_user.is_none()
            && self.http_passwd.is_none()
            && self.checksum.is_none()
    }

    pub fn validate(&self) -> Result<(), ApplicationError> {
        if let Some(referer) = &self.referer {
            validate_non_empty_line("Referer", referer)?;
            if referer.contains(['\r', '\n']) {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "Referer must be a single line.",
                    false,
                ));
            }
        }
        if let Some(user_agent) = &self.user_agent {
            validate_non_empty_line("User-Agent", user_agent)?;
            if user_agent.contains(['\r', '\n']) {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "User-Agent must be a single line.",
                    false,
                ));
            }
        }
        for header in &self.headers {
            validate_header_line(header)?;
        }
        if let Some(cookie) = &self.cookie {
            let cookie = cookie.expose_secret().trim();
            if cookie.is_empty() {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "Cookie must not be empty when provided.",
                    false,
                ));
            }
            if cookie.contains(['\r', '\n']) {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "Cookie must be a single line.",
                    false,
                ));
            }
        }
        if self.http_passwd.is_some() && self.http_user.as_deref().is_none_or(str::is_empty) {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "HTTP authentication requires a non-empty username.",
                false,
            ));
        }
        if let Some(user) = &self.http_user {
            validate_non_empty_line("HTTP username", user)?;
            if user.contains(['\r', '\n', ':']) {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "HTTP username must be a single line without ':'.",
                    false,
                ));
            }
        }
        if let Some(password) = &self.http_passwd {
            let password = password.expose_secret();
            if password.is_empty() {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "HTTP password must not be empty when provided.",
                    false,
                ));
            }
            if password.contains(['\r', '\n']) {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "HTTP password must be a single line.",
                    false,
                ));
            }
        }
        if let Some(checksum) = &self.checksum {
            validate_checksum(checksum)?;
        }
        Ok(())
    }

    /// Flatten into aria2 option pairs. Multi-value `header` is emitted once
    /// per line so the RPC adapter can rebuild the array form.
    #[must_use]
    pub fn to_option_pairs(&self) -> Vec<(String, String)> {
        let mut options = Vec::new();
        if let Some(referer) = &self.referer {
            options.push(("referer".into(), referer.trim().to_owned()));
        }
        if let Some(user_agent) = &self.user_agent {
            options.push(("user-agent".into(), user_agent.trim().to_owned()));
        }
        for header in &self.headers {
            options.push(("header".into(), header.trim().to_owned()));
        }
        if let Some(cookie) = &self.cookie {
            options.push((
                "header".into(),
                format!("Cookie: {}", cookie.expose_secret().trim()),
            ));
        }
        if let Some(user) = &self.http_user {
            options.push(("http-user".into(), user.trim().to_owned()));
        }
        if let Some(password) = &self.http_passwd {
            options.push(("http-passwd".into(), password.expose_secret().to_owned()));
        }
        if let Some(checksum) = &self.checksum {
            options.push(("checksum".into(), checksum.trim().to_owned()));
        }
        options
    }
}

fn validate_non_empty_line(label: &str, value: &str) -> Result<(), ApplicationError> {
    if value.trim().is_empty() {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            format!("{label} must not be empty when provided."),
            false,
        ));
    }
    Ok(())
}

fn validate_header_line(header: &str) -> Result<(), ApplicationError> {
    let header = header.trim();
    if header.is_empty() {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            "Custom headers must not contain blank lines.",
            false,
        ));
    }
    if header.contains(['\r', '\n']) {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            "Each custom header must be a single line.",
            false,
        ));
    }
    let Some((name, value)) = header.split_once(':') else {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            "Custom headers must use `Name: value` form.",
            false,
        ));
    };
    if name.trim().is_empty() || value.trim().is_empty() {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            "Custom headers must include both a name and a value.",
            false,
        ));
    }
    let name_lower = name.trim().to_ascii_lowercase();
    if name_lower == "cookie" || name_lower == "authorization" {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            "Use the dedicated Cookie or HTTP authentication fields for secrets.",
            false,
        ));
    }
    Ok(())
}

fn validate_checksum(checksum: &str) -> Result<(), ApplicationError> {
    let checksum = checksum.trim();
    let Some((kind, digest)) = checksum.split_once('=') else {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            "Checksum must use `type=digest` form, for example `sha-256=…`.",
            false,
        ));
    };
    let kind = kind.trim().to_ascii_lowercase();
    let digest = digest.trim();
    if !matches!(
        kind.as_str(),
        "sha-1" | "sha-224" | "sha-256" | "sha-384" | "sha-512" | "md5" | "adler32"
    ) {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            "Unsupported checksum type. Use sha-1/224/256/384/512, md5, or adler32.",
            false,
        ));
    }
    if digest.is_empty()
        || !digest.chars().all(|ch| ch.is_ascii_hexdigit())
        || digest.len() % 2 == 1
    {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            "Checksum digest must be a non-empty even-length hex string.",
            false,
        ));
    }
    let expected_len = match kind.as_str() {
        "md5" => Some(32),
        "sha-1" => Some(40),
        "sha-224" => Some(56),
        "sha-256" => Some(64),
        "sha-384" => Some(96),
        "sha-512" => Some(128),
        "adler32" => Some(8),
        _ => None,
    };
    if let Some(expected) = expected_len
        && digest.len() != expected
    {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            format!("Checksum digest for {kind} must be {expected} hex characters."),
            false,
        ));
    }
    Ok(())
}

fn validate_selected_file_indices(indices: &[u32]) -> Result<(), ApplicationError> {
    if indices.is_empty() {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            "At least one file must remain selected.",
            false,
        ));
    }
    if indices.first() == Some(&0) {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            "File selection uses 1-based indexes.",
            false,
        ));
    }
    if indices.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(ApplicationError::new(
            ApplicationErrorCode::Validation,
            "File selection indexes must be unique and ascending.",
            false,
        ));
    }
    Ok(())
}

#[derive(Clone, Default, Eq, PartialEq)]
pub struct AddDownloadRequest {
    pub source: AddDownloadSource,
    pub destination: Option<EnginePath>,
    pub file_conflict: FileConflictPolicy,
    pub selected_file_indices: Option<Vec<u32>>,
    pub advanced: AddDownloadAdvancedOptions,
    pub options: Vec<(String, String)>,
}

impl fmt::Debug for AddDownloadRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let redacted_options: Vec<(&str, &str)> = self
            .options
            .iter()
            .map(|(key, value)| {
                let display = if ariadeck_domain::task_option_key_is_sensitive(key)
                    || key.eq_ignore_ascii_case("header")
                {
                    "[REDACTED]"
                } else {
                    value.as_str()
                };
                (key.as_str(), display)
            })
            .collect();
        formatter
            .debug_struct("AddDownloadRequest")
            .field("source", &self.source)
            .field("destination", &self.destination)
            .field("file_conflict", &self.file_conflict)
            .field("selected_file_indices", &self.selected_file_indices)
            .field("advanced", &self.advanced)
            .field("options", &redacted_options)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FileConflictPolicy {
    #[default]
    AutoRename,
    Reject,
    Overwrite,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DownloadProxyMode {
    #[default]
    Disabled,
    /// Endpoints were resolved from the OS / environment by the desktop layer.
    /// Treated like Manual when serializing to aria2 options.
    System,
    Manual,
}

#[derive(Clone)]
pub struct DownloadProxyConfig {
    pub mode: DownloadProxyMode,
    pub all_proxy: Option<String>,
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub ftp_proxy: Option<String>,
    pub no_proxy: Vec<String>,
    pub username: Option<String>,
    pub password: Option<SecretString>,
    /// Maps to aria2 `check-certificate`. Independent of proxy mode; default true.
    pub check_certificate: bool,
}

impl Default for DownloadProxyConfig {
    fn default() -> Self {
        Self {
            mode: DownloadProxyMode::Disabled,
            all_proxy: None,
            http_proxy: None,
            https_proxy: None,
            ftp_proxy: None,
            no_proxy: Vec::new(),
            username: None,
            password: None,
            check_certificate: true,
        }
    }
}

impl std::fmt::Debug for DownloadProxyConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redact_opt =
            |value: &Option<String>| value.as_deref().map(ariadeck_domain::redact_source_uri);
        formatter
            .debug_struct("DownloadProxyConfig")
            .field("mode", &self.mode)
            .field("all_proxy", &redact_opt(&self.all_proxy))
            .field("http_proxy", &redact_opt(&self.http_proxy))
            .field("https_proxy", &redact_opt(&self.https_proxy))
            .field("ftp_proxy", &redact_opt(&self.ftp_proxy))
            .field("no_proxy", &self.no_proxy)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .field("check_certificate", &self.check_certificate)
            .finish()
    }
}

impl PartialEq for DownloadProxyConfig {
    fn eq(&self, other: &Self) -> bool {
        self.mode == other.mode
            && self.all_proxy == other.all_proxy
            && self.http_proxy == other.http_proxy
            && self.https_proxy == other.https_proxy
            && self.ftp_proxy == other.ftp_proxy
            && self.no_proxy == other.no_proxy
            && self.username == other.username
            && self.password.as_ref().map(ExposeSecret::expose_secret)
                == other.password.as_ref().map(ExposeSecret::expose_secret)
            && self.check_certificate == other.check_certificate
    }
}

impl Eq for DownloadProxyConfig {}

impl DownloadProxyConfig {
    pub(super) fn validate(&self) -> Result<(), ApplicationError> {
        for endpoint in [
            self.all_proxy.as_deref(),
            self.http_proxy.as_deref(),
            self.https_proxy.as_deref(),
            self.ftp_proxy.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if endpoint.trim().is_empty() || endpoint.chars().any(char::is_control) {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "Proxy endpoint must be non-empty and free of control characters.",
                    false,
                ));
            }
        }
        // Manual requires at least one endpoint. System may resolve to “direct”
        // (no proxy), which applies as cleared global proxy options.
        if self.mode == DownloadProxyMode::Manual
            && self.all_proxy.is_none()
            && self.http_proxy.is_none()
            && self.https_proxy.is_none()
            && self.ftp_proxy.is_none()
        {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "Manual download proxy requires at least one proxy endpoint.",
                false,
            ));
        }
        if self.password.is_some() && self.username.as_deref().is_none_or(str::is_empty) {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "A proxy password requires a non-empty username.",
                false,
            ));
        }
        Ok(())
    }
}

impl AddDownloadRequest {
    pub(super) fn validate(&self) -> Result<(), ApplicationError> {
        self.advanced.validate()?;
        if !self.advanced.is_empty() {
            // Advanced HTTP source controls apply only to direct URI tasks.
            // Magnet/Torrent/Metalink keep the typed fields unavailable so users
            // do not believe a Cookie/Referer will rewrite tracker or peer auth.
            let has_non_direct_source = match &self.source {
                AddDownloadSource::Uris(uris) => uris
                    .iter()
                    .any(|uri| uri.trim().to_ascii_lowercase().starts_with("magnet:")),
                AddDownloadSource::Torrent(_) | AddDownloadSource::Metalink(_) => true,
            };
            if has_non_direct_source {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "Referer, headers, cookies, authentication, and checksum apply only to direct URL downloads.",
                    false,
                ));
            }
        }
        let AddDownloadSource::Uris(uris) = &self.source else {
            let content = match &self.source {
                AddDownloadSource::Torrent(content) | AddDownloadSource::Metalink(content) => {
                    content
                }
                AddDownloadSource::Uris(_) => unreachable!(),
            };
            if content.is_empty() {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "Torrent or Metalink metadata must not be empty.",
                    false,
                ));
            }
            if let Some(indices) = &self.selected_file_indices {
                validate_selected_file_indices(indices)?;
            }
            return Ok(());
        };
        if self.selected_file_indices.is_some() {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "File selection is supported only for Torrent or Metalink metadata.",
                false,
            ));
        }
        if uris.is_empty() || uris.iter().any(|uri| uri.trim().is_empty()) {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "At least one non-empty URL or magnet link is required.",
                false,
            ));
        }
        let mut unique = HashSet::new();
        for uri in uris {
            let uri = uri.trim();
            if !unique.insert(uri) {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    format!(
                        "Duplicate download URI: {}",
                        ariadeck_domain::redact_source_uri(uri)
                    ),
                    false,
                ));
            }
            let parsed = Url::parse(uri).map_err(|error| {
                ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    format!("Invalid download URI: {error}"),
                    false,
                )
            })?;
            if !matches!(
                parsed.scheme(),
                "http" | "https" | "ftp" | "sftp" | "magnet"
            ) {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    format!("Unsupported download URI scheme: {}", parsed.scheme()),
                    false,
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoveTasksRequest {
    pub tasks: Vec<TaskIdentity>,
    pub scope: TaskRemovalScope,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetTaskOutputNameRequest {
    pub task: TaskIdentity,
    pub output_name: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum QueueMove {
    Top,
    Up,
    Down,
    Bottom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MoveTaskInQueueRequest {
    pub task: TaskIdentity,
    pub movement: QueueMove,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskRemovalScope {
    TaskOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetTaskSpeedLimitRequest {
    pub task: TaskIdentity,
    pub download_limit: ariadeck_domain::ByteRate,
    pub upload_limit: ariadeck_domain::ByteRate,
}

/// Per-task connection policy applied through `aria2.changeOption` (RATE-002).
///
/// Affects only the targeted live download. Values are validated against aria2's
/// documented ranges before the gateway call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetTaskConnectionPolicyRequest {
    pub task: TaskIdentity,
    pub policy: TaskConnectionPolicy,
}

/// Typed subset of dynamically changeable aria2 task options (RPC-001).
///
/// Free-form option bags are intentionally not exposed: each field maps to a
/// documented changeOption key and is validated on the desktop before mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetTaskOptionsRequest {
    pub task: TaskIdentity,
    /// BitTorrent share ratio limit as an aria2 decimal string; `0`/`0.0`
    /// disables the ratio condition. Stored as text so the request stays Eq.
    pub seed_ratio: Option<String>,
    /// BitTorrent seed time in minutes; aria2 stops at the first satisfied
    /// seed-ratio / seed-time condition.
    pub seed_time_minutes: Option<u64>,
    /// 1-based file indexes for BitTorrent/Metalink selection. `None` leaves
    /// the current selection unchanged; `Some([])` is rejected.
    pub selected_file_indices: Option<Vec<u32>>,
}

impl SetTaskOptionsRequest {
    pub fn validate(&self) -> Result<(), ApplicationError> {
        if self.seed_ratio.is_none()
            && self.seed_time_minutes.is_none()
            && self.selected_file_indices.is_none()
        {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "At least one task option must be provided.",
                false,
            ));
        }
        if let Some(ratio) = &self.seed_ratio {
            let parsed = ratio.parse::<f64>().ok().filter(|value| value.is_finite());
            if parsed.is_none_or(|value| value < 0.0) {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "Seed ratio must be a finite number greater than or equal to 0.",
                    false,
                ));
            }
        }
        if let Some(indices) = &self.selected_file_indices {
            validate_selected_file_indices(indices)?;
        }
        Ok(())
    }

    #[must_use]
    pub fn to_option_pairs(&self) -> Vec<(String, String)> {
        let mut options = Vec::new();
        if let Some(ratio) = &self.seed_ratio {
            options.push(("seed-ratio".into(), normalize_seed_ratio(ratio)));
        }
        if let Some(minutes) = self.seed_time_minutes {
            options.push(("seed-time".into(), minutes.to_string()));
        }
        if let Some(indices) = &self.selected_file_indices {
            options.push(("select-file".into(), format_selected_file_indices(indices)));
        }
        options
    }
}

fn normalize_seed_ratio(ratio: &str) -> String {
    ratio
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .map(|value| {
            let text = format!("{value:.4}");
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        })
        .unwrap_or_else(|| ratio.trim().to_owned())
}

#[must_use]
pub fn format_selected_file_indices(indices: &[u32]) -> String {
    let mut ranges = Vec::new();
    let Some(&first) = indices.first() else {
        return String::new();
    };
    let mut start = first;
    let mut end = first;
    for &index in &indices[1..] {
        if index == end.saturating_add(1) {
            end = index;
        } else {
            if start == end {
                ranges.push(start.to_string());
            } else {
                ranges.push(format!("{start}-{end}"));
            }
            start = index;
            end = index;
        }
    }
    if start == end {
        ranges.push(start.to_string());
    } else {
        ranges.push(format!("{start}-{end}"));
    }
    ranges.join(",")
}

/// Typed subset of dynamic task options editable via `EditTask` (`aria2.changeOption`
/// or `Error` task relink resume). Empty strings on optional text fields clear the
/// option in aria2; `None` leaves the existing option untouched.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EditTaskDynamicOptions {
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub headers: Option<Vec<String>>,
    pub proxy: Option<String>,
    pub max_download_limit: Option<ariadeck_domain::ByteRate>,
    pub max_connection_per_server: Option<u32>,
    pub split: Option<u32>,
    pub out: Option<String>,
}

impl EditTaskDynamicOptions {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.referer.is_none()
            && self.user_agent.is_none()
            && self.headers.is_none()
            && self.proxy.is_none()
            && self.max_download_limit.is_none()
            && self.max_connection_per_server.is_none()
            && self.split.is_none()
            && self.out.is_none()
    }

    pub fn validate(&self) -> Result<(), ApplicationError> {
        if let Some(referer) = &self.referer
            && (referer.contains(['\r', '\n']) || referer.chars().any(char::is_control))
        {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "Referer must be a single line without control characters.",
                false,
            ));
        }
        if let Some(ua) = &self.user_agent
            && (ua.contains(['\r', '\n']) || ua.chars().any(char::is_control))
        {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "User-Agent must be a single line without control characters.",
                false,
            ));
        }
        if let Some(headers) = &self.headers {
            for header in headers {
                validate_header_line(header)?;
            }
        }
        if let Some(proxy) = &self.proxy {
            let trimmed = proxy.trim();
            if !trimmed.is_empty() {
                if trimmed.contains(['\r', '\n']) || trimmed.chars().any(char::is_whitespace) {
                    return Err(ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        "Proxy URL must not contain whitespace or line breaks.",
                        false,
                    ));
                }
                let parsed = Url::parse(trimmed).map_err(|error| {
                    ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        format!("Invalid task proxy URL: {error}"),
                        false,
                    )
                })?;
                if !matches!(parsed.scheme(), "http" | "https" | "socks5" | "socks5h")
                    || parsed.host_str().is_none()
                {
                    return Err(ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        "Task proxy URL must use http, https, or socks5 with a host.",
                        false,
                    ));
                }
                if !parsed.username().is_empty() || parsed.password().is_some() {
                    return Err(ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        "Task proxy override must not embed credentials.",
                        false,
                    ));
                }
            }
        }
        if let Some(connections) = self.max_connection_per_server
            && !(1..=16).contains(&connections)
        {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "Max connections per server must be between 1 and 16.",
                false,
            ));
        }
        if let Some(split) = self.split
            && !(1..=64).contains(&split)
        {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "Split count must be between 1 and 64.",
                false,
            ));
        }
        if let Some(out) = &self.out {
            let trimmed = out.trim();
            if trimmed.is_empty()
                || trimmed == "."
                || trimmed == ".."
                || trimmed.contains(['/', '\\', '\0', '\r', '\n'])
            {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "Output name must be a non-empty file name without path separators.",
                    false,
                ));
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn to_option_pairs(&self) -> Vec<(String, String)> {
        let mut options = Vec::new();
        if let Some(referer) = &self.referer {
            options.push(("referer".into(), referer.trim().to_owned()));
        }
        if let Some(ua) = &self.user_agent {
            options.push(("user-agent".into(), ua.trim().to_owned()));
        }
        if let Some(headers) = &self.headers {
            if headers.is_empty() {
                options.push(("header".into(), String::new()));
            } else {
                for header in headers {
                    options.push(("header".into(), header.trim().to_owned()));
                }
            }
        }
        if let Some(proxy) = &self.proxy {
            options.push(("all-proxy".into(), proxy.trim().to_owned()));
        }
        if let Some(limit) = self.max_download_limit {
            options.push(("max-download-limit".into(), limit.get().to_string()));
        }
        if let Some(connections) = self.max_connection_per_server {
            options.push(("max-connection-per-server".into(), connections.to_string()));
        }
        if let Some(split) = self.split {
            options.push(("split".into(), split.to_string()));
        }
        if let Some(out) = &self.out {
            options.push(("out".into(), out.trim().to_owned()));
        }
        options
    }
}

/// Request to relink a task's URIs and/or update dynamic options.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditTaskRequest {
    pub task: TaskIdentity,
    /// Replacement HTTP(S)/FTP/SFTP URIs for single-file direct tasks.
    pub new_uris: Option<Vec<String>>,
    pub options: EditTaskDynamicOptions,
    /// When true and the target task is `Error` (non-BT), create a new task at
    /// the same `dir` + `out` to resume from any `.aria2` control file.
    pub allow_error_relink_resume: bool,
}

impl EditTaskRequest {
    pub fn validate(&self) -> Result<(), ApplicationError> {
        if self.new_uris.is_none() && self.options.is_empty() {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "Provide a new URL or at least one changed option.",
                false,
            ));
        }
        if let Some(uris) = &self.new_uris {
            if uris.is_empty() {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "At least one download URL must be provided when relinking.",
                    false,
                ));
            }
            let mut seen = HashSet::with_capacity(uris.len());
            for raw in uris {
                let uri = raw.trim();
                if uri.is_empty() {
                    return Err(ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        "Download URL must not be empty.",
                        false,
                    ));
                }
                if !seen.insert(uri.to_owned()) {
                    return Err(ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        format!(
                            "Duplicate download URI: {}",
                            ariadeck_domain::redact_source_uri(uri)
                        ),
                        false,
                    ));
                }
                let parsed = Url::parse(uri).map_err(|error| {
                    ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        format!("Invalid download URI: {error}"),
                        false,
                    )
                })?;
                if !matches!(parsed.scheme(), "http" | "https" | "ftp" | "sftp") {
                    return Err(ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        format!(
                            "Relink supports http/https/ftp/sftp URIs, not {}.",
                            parsed.scheme()
                        ),
                        false,
                    ));
                }
            }
        }
        self.options.validate()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppCommand {
    AddDownload(AddDownloadRequest),
    PauseAll,
    ForcePauseAll,
    ResumeAll,
    PauseTasks(Vec<TaskIdentity>),
    ForcePauseTasks(Vec<TaskIdentity>),
    ResumeTasks(Vec<TaskIdentity>),
    MoveTaskInQueue(MoveTaskInQueueRequest),
    RetryTasks(Vec<TaskIdentity>),
    SetTaskOutputName(SetTaskOutputNameRequest),
    RemoveTasks(RemoveTasksRequest),
    ForceRemoveTasks(RemoveTasksRequest),
    SetTaskSpeedLimit(SetTaskSpeedLimitRequest),
    SetTaskConnectionPolicy(SetTaskConnectionPolicyRequest),
    SetTaskOptions(SetTaskOptionsRequest),
    EditTask(EditTaskRequest),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskCommandContext {
    pub status: DownloadStatus,
    pub metadata: TaskMetadata,
    pub is_history_only: bool,
}
