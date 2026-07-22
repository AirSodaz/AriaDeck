use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
};

use ariadeck_domain::{
    DownloadStatus, EnginePath, ProfileId, SpeedLimitConfig, TaskConnectionPolicy, TaskIdentity,
    TaskMetadata, TaskSourceKind, TransferPolicyConfig,
};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::{
    DownloadEngineGateway, EngineCapabilities, GatewayError, GatewayErrorKind, TaskRemovalTarget,
};

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
            Self::Uris(uris) => formatter.debug_tuple("Uris").field(uris).finish(),
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AddDownloadRequest {
    pub source: AddDownloadSource,
    pub destination: Option<EnginePath>,
    pub file_conflict: FileConflictPolicy,
    pub selected_file_indices: Option<Vec<u32>>,
    pub advanced: AddDownloadAdvancedOptions,
    pub options: Vec<(String, String)>,
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
        formatter
            .debug_struct("DownloadProxyConfig")
            .field("mode", &self.mode)
            .field("all_proxy", &self.all_proxy)
            .field("http_proxy", &self.http_proxy)
            .field("https_proxy", &self.https_proxy)
            .field("ftp_proxy", &self.ftp_proxy)
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
    fn validate(&self) -> Result<(), ApplicationError> {
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
    fn validate(&self) -> Result<(), ApplicationError> {
        self.advanced.validate()?;
        if !self.advanced.is_empty() {
            // Advanced HTTP source controls apply only to direct URI tasks.
            // Magnet/Torrent/Metalink keep the typed fields unavailable so users
            // do not believe a Cookie/Referer will rewrite tracker or peer auth.
            if !matches!(self.source, AddDownloadSource::Uris(_)) {
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
            if let Some(indices) = &self.selected_file_indices
                && (indices.is_empty()
                    || indices.first() == Some(&0)
                    || indices.windows(2).any(|pair| pair[0] >= pair[1]))
            {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "Selected metadata file indexes must be non-empty, 1-based, unique, and sorted.",
                    false,
                ));
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
            if indices.is_empty() {
                return Err(ApplicationError::new(
                    ApplicationErrorCode::Validation,
                    "At least one file must remain selected.",
                    false,
                ));
            }
            let mut previous = None;
            for &index in indices {
                if index == 0 {
                    return Err(ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        "File selection uses 1-based indexes.",
                        false,
                    ));
                }
                if previous.is_some_and(|value| index <= value) {
                    return Err(ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        "File selection indexes must be unique and ascending.",
                        false,
                    ));
                }
                previous = Some(index);
            }
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

fn format_selected_file_indices(indices: &[u32]) -> String {
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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskCommandContext {
    pub status: DownloadStatus,
    pub metadata: TaskMetadata,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CommandItem {
    Task(TaskIdentity),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemFailure {
    pub item: Option<CommandItem>,
    pub error: ApplicationError,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandOutcome {
    Success {
        succeeded: Vec<CommandItem>,
    },
    PartialSuccess {
        succeeded: Vec<CommandItem>,
        failed: Vec<ItemFailure>,
    },
    Failure {
        failed: Vec<ItemFailure>,
    },
}

impl CommandOutcome {
    #[must_use]
    pub fn failure(error: ApplicationError) -> Self {
        Self::Failure {
            failed: vec![ItemFailure { item: None, error }],
        }
    }

    #[must_use]
    pub fn has_successes(&self) -> bool {
        match self {
            Self::Success { .. } => true,
            Self::PartialSuccess { succeeded, .. } => !succeeded.is_empty(),
            Self::Failure { .. } => false,
        }
    }

    #[must_use]
    pub fn has_unknown_outcome(&self) -> bool {
        let failures = match self {
            Self::Success { .. } => return false,
            Self::PartialSuccess { failed, .. } | Self::Failure { failed } => failed,
        };
        failures
            .iter()
            .any(|failure| failure.error.code == ApplicationErrorCode::OutcomeUnknown)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationErrorCode {
    Validation,
    Duplicate,
    WrongProfile,
    StaleSession,
    Disconnected,
    OutcomeUnknown,
    NotObserved,
    RetryNotObserved,
    RemovalNotObserved,
    Authentication,
    Timeout,
    Rejected,
    Unsupported,
    UnsafePath,
    Filesystem,
    Internal,
}

impl ApplicationErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Validation => "validation.invalid_request",
            Self::Duplicate => "validation.duplicate_task",
            Self::WrongProfile => "command.wrong_profile",
            Self::StaleSession => "command.stale_session",
            Self::Disconnected => "rpc.disconnected",
            Self::OutcomeUnknown => "rpc.command_outcome_unknown",
            Self::NotObserved => "rpc.add_not_observed",
            Self::RetryNotObserved => "rpc.retry_not_observed",
            Self::RemovalNotObserved => "rpc.remove_not_observed",
            Self::Authentication => "rpc.authentication_failed",
            Self::Timeout => "rpc.timeout",
            Self::Rejected => "rpc.command_rejected",
            Self::Unsupported => "command.unsupported",
            Self::UnsafePath => "filesystem.unsafe_path",
            Self::Filesystem => "filesystem.operation_failed",
            Self::Internal => "application.internal",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationError {
    pub code: ApplicationErrorCode,
    pub summary: String,
    pub retryable: bool,
}

impl ApplicationError {
    #[must_use]
    pub fn new(code: ApplicationErrorCode, summary: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            summary: summary.into(),
            retryable,
        }
    }
}

impl From<GatewayError> for ApplicationError {
    fn from(error: GatewayError) -> Self {
        let code = match error.kind {
            GatewayErrorKind::Disconnected => ApplicationErrorCode::Disconnected,
            GatewayErrorKind::OutcomeUnknown => ApplicationErrorCode::OutcomeUnknown,
            GatewayErrorKind::Authentication => ApplicationErrorCode::Authentication,
            GatewayErrorKind::Timeout => ApplicationErrorCode::Timeout,
            GatewayErrorKind::Rejected => ApplicationErrorCode::Rejected,
            GatewayErrorKind::Unsupported => ApplicationErrorCode::Unsupported,
            GatewayErrorKind::UnsafePath => ApplicationErrorCode::UnsafePath,
            GatewayErrorKind::Filesystem => ApplicationErrorCode::Filesystem,
            GatewayErrorKind::Internal => ApplicationErrorCode::Internal,
        };
        Self::new(code, error.message, error.retryable)
    }
}

pub struct CommandService {
    profile_id: ProfileId,
    gateway: Arc<dyn DownloadEngineGateway>,
    capabilities: EngineCapabilities,
}

impl CommandService {
    #[must_use]
    pub fn new(
        profile_id: ProfileId,
        gateway: Arc<dyn DownloadEngineGateway>,
        capabilities: EngineCapabilities,
    ) -> Self {
        Self {
            profile_id,
            gateway,
            capabilities,
        }
    }

    /// Unit-test helper when capability gates are not under exercise.
    #[cfg(test)]
    #[must_use]
    pub fn new_unprobed(profile_id: ProfileId, gateway: Arc<dyn DownloadEngineGateway>) -> Self {
        Self::new(profile_id, gateway, EngineCapabilities::default())
    }

    fn require_method(&self, method: &str) -> Result<(), ApplicationError> {
        if self.capabilities.allows_method(method) {
            return Ok(());
        }
        Err(ApplicationError::new(
            ApplicationErrorCode::Unsupported,
            EngineCapabilities::unsupported_method_message(method),
            false,
        ))
    }

    pub async fn execute(
        &self,
        command: AppCommand,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        match command {
            AppCommand::AddDownload(request) => self.add_download(request).await,
            AppCommand::PauseAll => self.execute_global(GlobalTaskOperation::PauseAll).await,
            AppCommand::ForcePauseAll => {
                self.execute_global(GlobalTaskOperation::ForcePauseAll)
                    .await
            }
            AppCommand::ResumeAll => self.execute_global(GlobalTaskOperation::ResumeAll).await,
            AppCommand::PauseTasks(tasks) => {
                self.execute_batch(tasks, TaskOperation::Pause, task_contexts)
                    .await
            }
            AppCommand::ForcePauseTasks(tasks) => {
                self.execute_batch(tasks, TaskOperation::ForcePause, task_contexts)
                    .await
            }
            AppCommand::ResumeTasks(tasks) => {
                self.execute_batch(tasks, TaskOperation::Resume, task_contexts)
                    .await
            }
            AppCommand::MoveTaskInQueue(request) => {
                self.execute_batch(
                    vec![request.task],
                    TaskOperation::MoveInQueue(request.movement),
                    task_contexts,
                )
                .await
            }
            AppCommand::RetryTasks(tasks) => self.retry_tasks(tasks, task_contexts).await,
            AppCommand::SetTaskOutputName(request) => {
                self.set_task_output_name(request, task_contexts).await
            }
            AppCommand::RemoveTasks(request) => {
                self.execute_batch(
                    request.tasks,
                    TaskOperation::Remove(request.scope),
                    task_contexts,
                )
                .await
            }
            AppCommand::ForceRemoveTasks(request) => {
                self.execute_batch(
                    request.tasks,
                    TaskOperation::ForceRemove(request.scope),
                    task_contexts,
                )
                .await
            }
            AppCommand::SetTaskSpeedLimit(request) => {
                self.set_task_speed_limit(request, task_contexts).await
            }
            AppCommand::SetTaskConnectionPolicy(request) => {
                self.set_task_connection_policy(request, task_contexts)
                    .await
            }
            AppCommand::SetTaskOptions(request) => {
                self.set_task_options(request, task_contexts).await
            }
        }
    }

    pub async fn apply_download_proxy(
        &self,
        config: &DownloadProxyConfig,
    ) -> Result<(), ApplicationError> {
        config.validate()?;
        self.require_method("aria2.changeGlobalOption")?;
        self.gateway
            .apply_download_proxy(config)
            .await
            .map_err(Into::into)
    }

    pub async fn apply_speed_limit(
        &self,
        config: &SpeedLimitConfig,
    ) -> Result<(), ApplicationError> {
        self.require_method("aria2.changeGlobalOption")?;
        self.gateway
            .apply_speed_limit(config)
            .await
            .map_err(Into::into)
    }

    pub async fn apply_transfer_policy(
        &self,
        config: &TransferPolicyConfig,
    ) -> Result<(), ApplicationError> {
        if let Err(error) = config.validate() {
            return Err(ApplicationError::new(
                ApplicationErrorCode::Validation,
                error.message(),
                false,
            ));
        }
        self.require_method("aria2.changeGlobalOption")?;
        self.gateway
            .apply_transfer_policy(config)
            .await
            .map_err(Into::into)
    }

    async fn change_options_gated(
        &self,
        gid: ariadeck_domain::Gid,
        options: &[(String, String)],
    ) -> Result<(), GatewayError> {
        if let Err(error) = self.require_method("aria2.changeOption") {
            return Err(GatewayError::new(
                GatewayErrorKind::Unsupported,
                error.summary,
                false,
            ));
        }
        self.gateway.change_options(gid, options).await
    }

    async fn set_task_speed_limit(
        &self,
        request: SetTaskSpeedLimitRequest,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        let item = CommandItem::Task(request.task);
        if request.task.profile_id != self.profile_id {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::WrongProfile,
                        "The task belongs to a different engine profile.",
                        false,
                    ),
                }],
            };
        }
        let Some(context) = task_contexts.get(&request.task) else {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "The task is no longer present in the current engine session.",
                        false,
                    ),
                }],
            };
        };
        if context.status.is_terminal() {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "A completed or failed task cannot change its speed limit.",
                        false,
                    ),
                }],
            };
        }
        let options = [
            (
                "max-download-limit".to_owned(),
                request.download_limit.get().to_string(),
            ),
            (
                "max-upload-limit".to_owned(),
                request.upload_limit.get().to_string(),
            ),
        ];
        match self.change_options_gated(request.task.gid, &options).await {
            Ok(()) => CommandOutcome::Success {
                succeeded: vec![item],
            },
            Err(error) => CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: error.into(),
                }],
            },
        }
    }

    async fn set_task_connection_policy(
        &self,
        request: SetTaskConnectionPolicyRequest,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        let item = CommandItem::Task(request.task);
        if let Err(error) = request.policy.validate() {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        error.message(),
                        false,
                    ),
                }],
            };
        }
        if request.task.profile_id != self.profile_id {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::WrongProfile,
                        "The task belongs to a different engine profile.",
                        false,
                    ),
                }],
            };
        }
        let Some(context) = task_contexts.get(&request.task) else {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "The task is no longer present in the current engine session.",
                        false,
                    ),
                }],
            };
        };
        if context.status.is_terminal() {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "A completed or failed task cannot change its connection policy.",
                        false,
                    ),
                }],
            };
        }
        let options = [
            (
                "max-connection-per-server".to_owned(),
                request.policy.max_connection_per_server.to_string(),
            ),
            ("split".to_owned(), request.policy.split.to_string()),
            (
                "min-split-size".to_owned(),
                request.policy.min_split_size.to_string(),
            ),
        ];
        match self.change_options_gated(request.task.gid, &options).await {
            Ok(()) => CommandOutcome::Success {
                succeeded: vec![item],
            },
            Err(error) => CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: error.into(),
                }],
            },
        }
    }

    async fn add_download(&self, mut request: AddDownloadRequest) -> CommandOutcome {
        if let Err(error) = request.validate() {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure { item: None, error }],
            };
        }
        // Flatten typed advanced controls into the option bag before the RPC
        // boundary. Keep validation on the typed form so secrets never need to
        // re-enter UI state.
        let advanced_options = request.advanced.to_option_pairs();
        if !advanced_options.is_empty() {
            request.options.extend(advanced_options);
        }

        match self.gateway.add_download(&request).await {
            Ok(gids) if gids.is_empty() => CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: None,
                    error: ApplicationError::new(
                        ApplicationErrorCode::Internal,
                        "Download engine accepted the request without returning a task ID.",
                        true,
                    ),
                }],
            },
            Ok(gids) => CommandOutcome::Success {
                succeeded: gids
                    .into_iter()
                    .map(|gid| CommandItem::Task(TaskIdentity::new(self.profile_id, gid)))
                    .collect(),
            },
            Err(error) => CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: None,
                    error: error.into(),
                }],
            },
        }
    }

    async fn set_task_options(
        &self,
        request: SetTaskOptionsRequest,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        let item = CommandItem::Task(request.task);
        if let Err(error) = request.validate() {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error,
                }],
            };
        }
        if request.task.profile_id != self.profile_id {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::WrongProfile,
                        "The task belongs to a different engine profile.",
                        false,
                    ),
                }],
            };
        }
        let Some(context) = task_contexts.get(&request.task) else {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "The task is no longer present in the current engine session.",
                        false,
                    ),
                }],
            };
        };
        if context.status.is_terminal() {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "A completed or failed task cannot change its options.",
                        false,
                    ),
                }],
            };
        }
        if (request.seed_ratio.is_some() || request.seed_time_minutes.is_some())
            && !matches!(
                context.metadata.source_kind,
                TaskSourceKind::Magnet | TaskSourceKind::BitTorrent
            )
        {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Unsupported,
                        "Seed-ratio and seed-time apply only to BitTorrent tasks.",
                        false,
                    ),
                }],
            };
        }
        if request.selected_file_indices.is_some()
            && !matches!(
                context.metadata.source_kind,
                TaskSourceKind::Magnet | TaskSourceKind::BitTorrent | TaskSourceKind::Metalink
            )
        {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Unsupported,
                        "File selection applies only to Torrent and Metalink tasks.",
                        false,
                    ),
                }],
            };
        }
        let options = request.to_option_pairs();
        match self.change_options_gated(request.task.gid, &options).await {
            Ok(()) => CommandOutcome::Success {
                succeeded: vec![item],
            },
            Err(error) => CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: error.into(),
                }],
            },
        }
    }

    async fn execute_global(&self, operation: GlobalTaskOperation) -> CommandOutcome {
        let required = match operation {
            GlobalTaskOperation::PauseAll => "aria2.pauseAll",
            GlobalTaskOperation::ForcePauseAll => "aria2.forcePauseAll",
            GlobalTaskOperation::ResumeAll => "aria2.unpauseAll",
        };
        if let Err(error) = self.require_method(required) {
            return CommandOutcome::failure(error);
        }
        let result = match operation {
            GlobalTaskOperation::PauseAll => self.gateway.pause_all().await,
            GlobalTaskOperation::ForcePauseAll => self.gateway.force_pause_all().await,
            GlobalTaskOperation::ResumeAll => self.gateway.resume_all().await,
        };
        match result {
            Ok(()) => CommandOutcome::Success {
                succeeded: Vec::new(),
            },
            Err(error) => CommandOutcome::failure(error.into()),
        }
    }

    async fn set_task_output_name(
        &self,
        request: SetTaskOutputNameRequest,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        let item = CommandItem::Task(request.task);
        if request.task.profile_id != self.profile_id {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::WrongProfile,
                        "The task belongs to a different engine profile.",
                        false,
                    ),
                }],
            };
        }
        let Some(context) = task_contexts.get(&request.task) else {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "The task is no longer present in the current engine session.",
                        false,
                    ),
                }],
            };
        };
        let output_name = request.output_name.trim();
        if output_name.is_empty()
            || output_name == "."
            || output_name == ".."
            || output_name.contains(['/', '\\', '\0'])
        {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        "Output name must be a non-empty file name without path separators.",
                        false,
                    ),
                }],
            };
        }
        if context.metadata.source_kind != TaskSourceKind::DirectUri {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Unsupported,
                        "A custom output name is currently supported only for direct URI tasks.",
                        false,
                    ),
                }],
            };
        }
        if context.status.is_terminal() {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "A completed or failed task cannot change its output name.",
                        false,
                    ),
                }],
            };
        }

        let options = [("out".to_owned(), output_name.to_owned())];
        match self.change_options_gated(request.task.gid, &options).await {
            Ok(()) => CommandOutcome::Success {
                succeeded: vec![item],
            },
            Err(error) => CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: Some(item),
                    error: error.into(),
                }],
            },
        }
    }

    async fn execute_batch(
        &self,
        tasks: Vec<TaskIdentity>,
        operation: TaskOperation,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        if tasks.is_empty() {
            return CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: None,
                    error: ApplicationError::new(
                        ApplicationErrorCode::Validation,
                        "At least one task must be selected.",
                        false,
                    ),
                }],
            };
        }

        let mut seen = HashSet::new();
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();
        for identity in tasks.into_iter().filter(|identity| seen.insert(*identity)) {
            let item = CommandItem::Task(identity);
            if identity.profile_id != self.profile_id {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::WrongProfile,
                        "The task belongs to a different engine profile.",
                        false,
                    ),
                });
                continue;
            }

            let Some(context) = task_contexts.get(&identity) else {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "The task is no longer present in the current engine session.",
                        false,
                    ),
                });
                continue;
            };
            let allowed = match operation {
                TaskOperation::Pause | TaskOperation::ForcePause => matches!(
                    context.status,
                    DownloadStatus::Active
                        | DownloadStatus::Seeding
                        | DownloadStatus::Waiting
                        | DownloadStatus::Verifying
                ),
                TaskOperation::Resume => matches!(context.status, DownloadStatus::Paused),
                TaskOperation::MoveInQueue(_) => !matches!(
                    context.status,
                    DownloadStatus::Complete
                        | DownloadStatus::Error
                        | DownloadStatus::Removed
                        | DownloadStatus::Unknown
                ),
                TaskOperation::Remove(_) | TaskOperation::ForceRemove(_) => {
                    !matches!(context.status, DownloadStatus::Unknown)
                }
            };
            if !allowed {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        format!(
                            "{} is not available while the task is {:?}.",
                            task_operation_label(operation),
                            context.status
                        ),
                        false,
                    ),
                });
                continue;
            }

            let required = match operation {
                TaskOperation::Pause => "aria2.pause",
                TaskOperation::ForcePause => "aria2.forcePause",
                TaskOperation::Resume => "aria2.unpause",
                TaskOperation::MoveInQueue(_) => "aria2.changePosition",
                TaskOperation::Remove(_) => "aria2.remove",
                TaskOperation::ForceRemove(_) => "aria2.forceRemove",
            };
            if let Err(error) = self.require_method(required) {
                failed.push(ItemFailure {
                    item: Some(item),
                    error,
                });
                continue;
            }
            let result = match operation {
                TaskOperation::Pause => self.gateway.pause(identity.gid).await,
                TaskOperation::ForcePause => self.gateway.force_pause(identity.gid).await,
                TaskOperation::Resume => self.gateway.resume(identity.gid).await,
                TaskOperation::MoveInQueue(movement) => {
                    self.gateway.move_in_queue(identity.gid, movement).await
                }
                TaskOperation::Remove(TaskRemovalScope::TaskOnly) => {
                    let target = if context.status.is_terminal() {
                        TaskRemovalTarget::DownloadResult
                    } else {
                        TaskRemovalTarget::LiveTask
                    };
                    self.gateway.remove(identity.gid, target).await
                }
                TaskOperation::ForceRemove(TaskRemovalScope::TaskOnly) => {
                    let target = if context.status.is_terminal() {
                        TaskRemovalTarget::DownloadResult
                    } else {
                        TaskRemovalTarget::LiveTask
                    };
                    self.gateway.force_remove(identity.gid, target).await
                }
            };
            match result {
                Ok(()) => succeeded.push(item),
                Err(error) => failed.push(ItemFailure {
                    item: Some(item),
                    error: error.into(),
                }),
            }
        }

        finish_batch(succeeded, failed)
    }

    async fn retry_tasks(
        &self,
        tasks: Vec<TaskIdentity>,
        task_contexts: &HashMap<TaskIdentity, TaskCommandContext>,
    ) -> CommandOutcome {
        if tasks.is_empty() {
            return CommandOutcome::failure(ApplicationError::new(
                ApplicationErrorCode::Validation,
                "At least one failed task must be selected.",
                false,
            ));
        }

        let mut seen = HashSet::new();
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();
        for identity in tasks.into_iter().filter(|identity| seen.insert(*identity)) {
            let item = CommandItem::Task(identity);
            if identity.profile_id != self.profile_id {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::WrongProfile,
                        "The task belongs to a different engine profile.",
                        false,
                    ),
                });
                continue;
            }
            let Some(context) = task_contexts.get(&identity) else {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "The failed task is no longer present in the current engine session.",
                        false,
                    ),
                });
                continue;
            };
            if context.status != DownloadStatus::Error {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "Only failed tasks can be retried.",
                        false,
                    ),
                });
                continue;
            }
            let source = context.metadata.primary_uri.clone().or_else(|| {
                context
                    .metadata
                    .info_hash
                    .as_ref()
                    .map(|hash| format!("magnet:?xt=urn:btih:{hash}"))
            });
            let Some(source) = source else {
                failed.push(ItemFailure {
                    item: Some(item),
                    error: ApplicationError::new(
                        ApplicationErrorCode::Unsupported,
                        "The task has no replayable URL, magnet link, or info hash.",
                        false,
                    ),
                });
                continue;
            };
            let request = AddDownloadRequest {
                source: AddDownloadSource::Uris(vec![source]),
                destination: context.metadata.directory.clone(),
                file_conflict: FileConflictPolicy::default(),
                selected_file_indices: None,
                advanced: Default::default(),
                options: Vec::new(),
            };
            match self.gateway.retry_download(identity.gid, &request).await {
                Ok(gid) => {
                    succeeded.push(CommandItem::Task(TaskIdentity::new(self.profile_id, gid)))
                }
                Err(error) => failed.push(ItemFailure {
                    item: Some(item),
                    error: error.into(),
                }),
            }
        }

        finish_batch(succeeded, failed)
    }
}

fn task_operation_label(operation: TaskOperation) -> &'static str {
    match operation {
        TaskOperation::Pause => "Pause",
        TaskOperation::ForcePause => "Force pause",
        TaskOperation::Resume => "Resume",
        TaskOperation::MoveInQueue(_) => "Change queue priority",
        TaskOperation::Remove(_) => "Remove",
        TaskOperation::ForceRemove(_) => "Force remove",
    }
}

fn finish_batch(succeeded: Vec<CommandItem>, failed: Vec<ItemFailure>) -> CommandOutcome {
    match (succeeded.is_empty(), failed.is_empty()) {
        (false, true) => CommandOutcome::Success { succeeded },
        (false, false) => CommandOutcome::PartialSuccess { succeeded, failed },
        (true, false) => CommandOutcome::Failure { failed },
        (true, true) => CommandOutcome::failure(ApplicationError::new(
            ApplicationErrorCode::Internal,
            "The command produced no result.",
            false,
        )),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TaskOperation {
    Pause,
    ForcePause,
    Resume,
    MoveInQueue(QueueMove),
    Remove(TaskRemovalScope),
    ForceRemove(TaskRemovalScope),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::enum_variant_names)]
enum GlobalTaskOperation {
    PauseAll,
    ForcePauseAll,
    ResumeAll,
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use ariadeck_domain::Gid;
    use async_trait::async_trait;
    use futures::executor::block_on;

    use super::*;

    #[test]
    fn proxy_config_debug_output_redacts_the_password() {
        let config = DownloadProxyConfig {
            mode: DownloadProxyMode::Manual,
            all_proxy: Some("proxy.example:8080".into()),
            username: Some("proxy-user".into()),
            password: Some(SecretString::new("never-log-this".into())),
            ..DownloadProxyConfig::default()
        };

        let debug = format!("{config:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("never-log-this"));
    }

    #[test]
    fn command_outcome_detects_unknown_failures_in_full_and_partial_results() {
        let unknown = ItemFailure {
            item: None,
            error: ApplicationError::new(
                ApplicationErrorCode::OutcomeUnknown,
                "response lost",
                false,
            ),
        };
        assert!(
            CommandOutcome::Failure {
                failed: vec![unknown.clone()]
            }
            .has_unknown_outcome()
        );
        assert!(
            CommandOutcome::PartialSuccess {
                succeeded: vec![CommandItem::Task(TaskIdentity::new(
                    ProfileId::new(),
                    Gid::from_u64(1),
                ))],
                failed: vec![unknown],
            }
            .has_unknown_outcome()
        );
        assert!(
            !CommandOutcome::Failure {
                failed: vec![ItemFailure {
                    item: None,
                    error: ApplicationError::new(
                        ApplicationErrorCode::Rejected,
                        "rejected",
                        false,
                    ),
                }],
            }
            .has_unknown_outcome()
        );
    }

    type ChangedOptionCall = (Gid, Vec<(String, String)>);

    #[derive(Default)]
    struct FakeGateway {
        adds: Mutex<Vec<AddDownloadRequest>>,
        add_gids: Mutex<Option<Vec<Gid>>>,
        retries: Mutex<Vec<(Gid, AddDownloadRequest)>>,
        calls: Mutex<Vec<(TaskOperation, Gid)>>,
        removals: Mutex<Vec<(Gid, TaskRemovalTarget)>>,
        changed_options: Mutex<Vec<ChangedOptionCall>>,
        global_options: Mutex<Vec<Vec<(String, String)>>>,
        queue_moves: Mutex<Vec<(Gid, QueueMove)>>,
        fail_gid: Option<Gid>,
    }

    #[async_trait]
    impl DownloadEngineGateway for FakeGateway {
        async fn add_download(
            &self,
            request: &AddDownloadRequest,
        ) -> Result<Vec<Gid>, GatewayError> {
            self.adds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(request.clone());
            let gids = self
                .add_gids
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            Ok(gids.unwrap_or_else(|| vec![Gid::from_u64(99)]))
        }

        async fn retry_download(
            &self,
            gid: Gid,
            fallback: &AddDownloadRequest,
        ) -> Result<Gid, GatewayError> {
            self.retries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((gid, fallback.clone()));
            Ok(Gid::from_u64(99))
        }

        async fn pause(&self, gid: Gid) -> Result<(), GatewayError> {
            self.record(TaskOperation::Pause, gid)
        }

        async fn force_pause(&self, gid: Gid) -> Result<(), GatewayError> {
            self.record(TaskOperation::ForcePause, gid)
        }

        async fn resume(&self, gid: Gid) -> Result<(), GatewayError> {
            self.record(TaskOperation::Resume, gid)
        }

        async fn force_pause_all(&self) -> Result<(), GatewayError> {
            self.calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((TaskOperation::ForcePause, Gid::from_u64(0)));
            Ok(())
        }

        async fn move_in_queue(&self, gid: Gid, movement: QueueMove) -> Result<(), GatewayError> {
            self.queue_moves
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((gid, movement));
            self.record(TaskOperation::MoveInQueue(movement), gid)
        }

        async fn change_options(
            &self,
            gid: Gid,
            options: &[(String, String)],
        ) -> Result<(), GatewayError> {
            self.changed_options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((gid, options.to_vec()));
            Ok(())
        }

        async fn apply_speed_limit(&self, config: &SpeedLimitConfig) -> Result<(), GatewayError> {
            self.global_options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(vec![
                    (
                        "max-overall-download-limit".into(),
                        config.download_limit.get().to_string(),
                    ),
                    (
                        "max-overall-upload-limit".into(),
                        config.upload_limit.get().to_string(),
                    ),
                ]);
            Ok(())
        }

        async fn apply_transfer_policy(
            &self,
            config: &TransferPolicyConfig,
        ) -> Result<(), GatewayError> {
            self.global_options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(vec![
                    (
                        "max-concurrent-downloads".into(),
                        config.max_concurrent_downloads.to_string(),
                    ),
                    (
                        "max-connection-per-server".into(),
                        config.max_connection_per_server.to_string(),
                    ),
                    ("split".into(), config.split.to_string()),
                    ("min-split-size".into(), config.min_split_size.to_string()),
                    (
                        "file-allocation".into(),
                        config.file_allocation.as_aria2().to_owned(),
                    ),
                    (
                        "check-integrity".into(),
                        if config.check_integrity {
                            "true".into()
                        } else {
                            "false".into()
                        },
                    ),
                ]);
            Ok(())
        }

        async fn remove(&self, gid: Gid, target: TaskRemovalTarget) -> Result<(), GatewayError> {
            self.removals
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((gid, target));
            self.record(TaskOperation::Remove(TaskRemovalScope::TaskOnly), gid)
        }

        async fn force_remove(
            &self,
            gid: Gid,
            target: TaskRemovalTarget,
        ) -> Result<(), GatewayError> {
            self.removals
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((gid, target));
            self.record(TaskOperation::ForceRemove(TaskRemovalScope::TaskOnly), gid)
        }
    }

    impl FakeGateway {
        fn record(&self, operation: TaskOperation, gid: Gid) -> Result<(), GatewayError> {
            self.calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((operation, gid));
            if self.fail_gid == Some(gid) {
                Err(GatewayError::new(
                    GatewayErrorKind::Rejected,
                    "aria2 rejected the command",
                    false,
                ))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn batch_command_reports_partial_success_and_deduplicates() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway {
            adds: Mutex::default(),
            add_gids: Mutex::default(),
            retries: Mutex::default(),
            calls: Mutex::default(),
            removals: Mutex::default(),
            changed_options: Mutex::default(),
            global_options: Mutex::default(),
            queue_moves: Mutex::default(),
            fail_gid: Some(Gid::from_u64(2)),
        });
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let one = TaskIdentity::new(profile_id, Gid::from_u64(1));
        let two = TaskIdentity::new(profile_id, Gid::from_u64(2));
        let contexts = HashMap::from([
            (
                one,
                TaskCommandContext {
                    status: DownloadStatus::Active,
                    metadata: TaskMetadata::default(),
                },
            ),
            (
                two,
                TaskCommandContext {
                    status: DownloadStatus::Active,
                    metadata: TaskMetadata::default(),
                },
            ),
        ]);

        let outcome =
            block_on(service.execute(AppCommand::PauseTasks(vec![one, one, two]), &contexts));

        let CommandOutcome::PartialSuccess { succeeded, failed } = outcome else {
            panic!("expected partial success");
        };
        assert_eq!(succeeded, vec![CommandItem::Task(one)]);
        assert_eq!(failed.len(), 1);
        assert_eq!(
            gateway
                .calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            2
        );
    }

    #[test]
    fn batch_command_skips_ineligible_tasks_without_calling_the_gateway() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let active = TaskIdentity::new(profile_id, Gid::from_u64(1));
        let complete = TaskIdentity::new(profile_id, Gid::from_u64(2));
        let contexts = HashMap::from([
            (
                active,
                TaskCommandContext {
                    status: DownloadStatus::Active,
                    metadata: TaskMetadata::default(),
                },
            ),
            (
                complete,
                TaskCommandContext {
                    status: DownloadStatus::Complete,
                    metadata: TaskMetadata::default(),
                },
            ),
        ]);

        let outcome =
            block_on(service.execute(AppCommand::PauseTasks(vec![active, complete]), &contexts));

        let CommandOutcome::PartialSuccess { succeeded, failed } = outcome else {
            panic!("expected partial success");
        };
        assert_eq!(succeeded, vec![CommandItem::Task(active)]);
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].item, Some(CommandItem::Task(complete)));
        assert_eq!(failed[0].error.code, ApplicationErrorCode::Rejected);
        assert_eq!(
            gateway
                .calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_slice(),
            &[(TaskOperation::Pause, active.gid)]
        );
    }

    #[test]
    fn wrong_profile_is_rejected_before_gateway_call() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let foreign = TaskIdentity::new(ProfileId::new(), Gid::from_u64(1));

        let outcome = block_on(service.execute(
            AppCommand::RemoveTasks(RemoveTasksRequest {
                tasks: vec![foreign],
                scope: TaskRemovalScope::TaskOnly,
            }),
            &HashMap::new(),
        ));

        let CommandOutcome::Failure { failed } = outcome else {
            panic!("expected command failure");
        };
        assert_eq!(failed[0].error.code, ApplicationErrorCode::WrongProfile);
        assert!(
            gateway
                .calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn add_download_rejects_unsupported_or_malformed_uris() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway);

        for uri in ["not a uri", "file:///tmp/item.bin", "javascript:alert(1)"] {
            let outcome = block_on(service.execute(
                AppCommand::AddDownload(AddDownloadRequest {
                    source: AddDownloadSource::Uris(vec![uri.into()]),
                    destination: None,
                    file_conflict: FileConflictPolicy::default(),
                    selected_file_indices: None,
                    advanced: Default::default(),
                    options: Vec::new(),
                }),
                &HashMap::new(),
            ));
            let CommandOutcome::Failure { failed } = outcome else {
                panic!("expected URI validation failure for {uri}");
            };
            assert_eq!(failed[0].error.code, ApplicationErrorCode::Validation);
        }
    }

    #[test]
    fn add_download_rejects_duplicate_mirror_sources_before_gateway_call() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());

        let outcome = block_on(service.execute(
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Uris(vec![
                    "https://user:secret@example.test/archive.iso?token=private".into(),
                    "https://user:secret@example.test/archive.iso?token=private".into(),
                ]),
                destination: None,
                file_conflict: FileConflictPolicy::default(),
                selected_file_indices: None,
                advanced: Default::default(),
                options: Vec::new(),
            }),
            &HashMap::new(),
        ));

        let CommandOutcome::Failure { failed } = outcome else {
            panic!("expected duplicate mirror validation failure");
        };
        assert_eq!(failed[0].error.code, ApplicationErrorCode::Validation);
        assert!(
            !failed[0].error.summary.contains("secret"),
            "duplicate validation must redact credentials: {}",
            failed[0].error.summary
        );
        assert!(!failed[0].error.summary.contains("token=private"));
        assert!(
            gateway
                .adds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn add_download_rejects_empty_metadata_before_gateway_call() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());

        let outcome = block_on(service.execute(
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Metalink(Arc::<[u8]>::from(&b""[..])),
                destination: None,
                file_conflict: FileConflictPolicy::Reject,
                selected_file_indices: None,
                advanced: Default::default(),
                options: Vec::new(),
            }),
            &HashMap::new(),
        ));

        let CommandOutcome::Failure { failed } = outcome else {
            panic!("expected empty metadata validation failure");
        };
        assert_eq!(failed[0].error.code, ApplicationErrorCode::Validation);
        assert!(
            gateway
                .adds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn add_download_rejects_invalid_or_non_metadata_file_selection() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());

        for selected_file_indices in [
            Some(Vec::new()),
            Some(vec![0]),
            Some(vec![1, 1]),
            Some(vec![2, 1]),
        ] {
            let outcome = block_on(service.execute(
                AppCommand::AddDownload(AddDownloadRequest {
                    source: AddDownloadSource::Torrent(Arc::<[u8]>::from(&b"metadata"[..])),
                    destination: None,
                    file_conflict: FileConflictPolicy::Reject,
                    selected_file_indices,
                    advanced: Default::default(),
                    options: Vec::new(),
                }),
                &HashMap::new(),
            ));
            let CommandOutcome::Failure { failed } = outcome else {
                panic!("expected metadata selection validation failure");
            };
            assert_eq!(failed[0].error.code, ApplicationErrorCode::Validation);
        }

        let uri_outcome = block_on(service.execute(
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Uris(vec!["https://example.test/archive.bin".into()]),
                destination: None,
                file_conflict: FileConflictPolicy::Reject,
                selected_file_indices: Some(vec![1]),
                advanced: Default::default(),
                options: Vec::new(),
            }),
            &HashMap::new(),
        ));
        let CommandOutcome::Failure { failed } = uri_outcome else {
            panic!("expected URI file selection validation failure");
        };
        assert_eq!(failed[0].error.code, ApplicationErrorCode::Validation);
        assert!(
            gateway
                .adds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn add_download_preserves_multiple_gateway_gids() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        *gateway
            .add_gids
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            Some(vec![Gid::from_u64(11), Gid::from_u64(12)]);
        let service = CommandService::new_unprobed(profile_id, gateway);

        let outcome = block_on(service.execute(
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Metalink(Arc::<[u8]>::from(&b"metadata"[..])),
                destination: None,
                file_conflict: FileConflictPolicy::Reject,
                selected_file_indices: None,
                advanced: Default::default(),
                options: Vec::new(),
            }),
            &HashMap::new(),
        ));

        let CommandOutcome::Success { succeeded } = outcome else {
            panic!("expected metadata add success");
        };
        let gids = succeeded
            .into_iter()
            .map(|item| match item {
                CommandItem::Task(identity) => identity.gid,
            })
            .collect::<Vec<_>>();
        assert_eq!(gids, vec![Gid::from_u64(11), Gid::from_u64(12)]);
    }

    #[test]
    fn add_download_rejects_an_empty_gateway_result() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        *gateway
            .add_gids
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Vec::new());
        let service = CommandService::new_unprobed(profile_id, gateway);

        let outcome = block_on(service.execute(
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Metalink(Arc::<[u8]>::from(&b"metadata"[..])),
                destination: None,
                file_conflict: FileConflictPolicy::Reject,
                selected_file_indices: None,
                advanced: Default::default(),
                options: Vec::new(),
            }),
            &HashMap::new(),
        ));

        let CommandOutcome::Failure { failed } = outcome else {
            panic!("expected empty gateway result failure");
        };
        assert_eq!(failed[0].error.code, ApplicationErrorCode::Internal);
        assert!(failed[0].error.retryable);
    }

    #[test]
    fn removal_targets_live_tasks_and_terminal_results_separately() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let live = TaskIdentity::new(profile_id, Gid::from_u64(1));
        let complete = TaskIdentity::new(profile_id, Gid::from_u64(2));
        let removed = TaskIdentity::new(profile_id, Gid::from_u64(3));

        let contexts = HashMap::from([
            (
                live,
                TaskCommandContext {
                    status: DownloadStatus::Active,
                    metadata: TaskMetadata::default(),
                },
            ),
            (
                complete,
                TaskCommandContext {
                    status: DownloadStatus::Complete,
                    metadata: TaskMetadata::default(),
                },
            ),
            (
                removed,
                TaskCommandContext {
                    status: DownloadStatus::Removed,
                    metadata: TaskMetadata::default(),
                },
            ),
        ]);
        let outcome = block_on(service.execute(
            AppCommand::RemoveTasks(RemoveTasksRequest {
                tasks: vec![live, complete, removed],
                scope: TaskRemovalScope::TaskOnly,
            }),
            &contexts,
        ));

        assert!(matches!(outcome, CommandOutcome::Success { .. }));
        assert_eq!(
            *gateway
                .removals
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![
                (live.gid, TaskRemovalTarget::LiveTask),
                (complete.gid, TaskRemovalTarget::DownloadResult),
                (removed.gid, TaskRemovalTarget::DownloadResult),
            ]
        );
    }

    #[test]
    fn queue_move_dispatches_to_the_gateway_for_a_live_task() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let waiting = TaskIdentity::new(profile_id, Gid::from_u64(4));
        let contexts = HashMap::from([(
            waiting,
            TaskCommandContext {
                status: DownloadStatus::Waiting,
                metadata: TaskMetadata::default(),
            },
        )]);

        let outcome = block_on(service.execute(
            AppCommand::MoveTaskInQueue(MoveTaskInQueueRequest {
                task: waiting,
                movement: QueueMove::Top,
            }),
            &contexts,
        ));

        assert_eq!(
            outcome,
            CommandOutcome::Success {
                succeeded: vec![CommandItem::Task(waiting)],
            }
        );
        assert_eq!(
            *gateway
                .queue_moves
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![(waiting.gid, QueueMove::Top)]
        );
    }

    #[test]
    fn queue_move_is_rejected_for_terminal_tasks_before_the_gateway() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let complete = TaskIdentity::new(profile_id, Gid::from_u64(5));
        let contexts = HashMap::from([(
            complete,
            TaskCommandContext {
                status: DownloadStatus::Complete,
                metadata: TaskMetadata::default(),
            },
        )]);

        let outcome = block_on(service.execute(
            AppCommand::MoveTaskInQueue(MoveTaskInQueueRequest {
                task: complete,
                movement: QueueMove::Up,
            }),
            &contexts,
        ));

        let CommandOutcome::Failure { failed } = outcome else {
            panic!("expected queue-move rejection for a terminal task");
        };
        assert_eq!(failed[0].error.code, ApplicationErrorCode::Rejected);
        assert!(
            gateway
                .queue_moves
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn retry_creates_a_new_task_from_the_known_source_and_destination() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let failed_task = TaskIdentity::new(profile_id, Gid::from_u64(7));
        let contexts = HashMap::from([(
            failed_task,
            TaskCommandContext {
                status: DownloadStatus::Error,
                metadata: TaskMetadata {
                    directory: Some(EnginePath::new("/downloads")),
                    primary_uri: Some("https://example.test/archive.iso".into()),
                    ..TaskMetadata::default()
                },
            },
        )]);

        let outcome =
            block_on(service.execute(AppCommand::RetryTasks(vec![failed_task]), &contexts));

        assert_eq!(
            outcome,
            CommandOutcome::Success {
                succeeded: vec![CommandItem::Task(TaskIdentity::new(
                    profile_id,
                    Gid::from_u64(99),
                ))],
            }
        );
        assert_eq!(
            *gateway
                .retries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![(
                failed_task.gid,
                AddDownloadRequest {
                    source: AddDownloadSource::Uris(vec![
                        "https://example.test/archive.iso".into(),
                    ]),
                    destination: Some(EnginePath::new("/downloads")),
                    file_conflict: FileConflictPolicy::default(),
                    selected_file_indices: None,
                    advanced: Default::default(),
                    options: Vec::new(),
                }
            )]
        );
    }

    #[test]
    fn direct_uri_output_name_change_is_validated_and_forwarded() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let task = TaskIdentity::new(profile_id, Gid::from_u64(8));
        let contexts = HashMap::from([(
            task,
            TaskCommandContext {
                status: DownloadStatus::Waiting,
                metadata: TaskMetadata {
                    source_kind: TaskSourceKind::DirectUri,
                    ..TaskMetadata::default()
                },
            },
        )]);

        let outcome = block_on(service.execute(
            AppCommand::SetTaskOutputName(SetTaskOutputNameRequest {
                task,
                output_name: " renamed.iso ".into(),
            }),
            &contexts,
        ));

        assert_eq!(
            outcome,
            CommandOutcome::Success {
                succeeded: vec![CommandItem::Task(task)]
            }
        );
        assert_eq!(
            *gateway
                .changed_options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![(task.gid, vec![("out".into(), "renamed.iso".into())])]
        );
    }

    #[test]
    fn output_name_change_rejects_paths_and_non_uri_tasks() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let task = TaskIdentity::new(profile_id, Gid::from_u64(9));
        let contexts = HashMap::from([(
            task,
            TaskCommandContext {
                status: DownloadStatus::Waiting,
                metadata: TaskMetadata {
                    source_kind: TaskSourceKind::BitTorrent,
                    ..TaskMetadata::default()
                },
            },
        )]);

        for output_name in ["folder/file.iso", "archive.iso"] {
            let outcome = block_on(service.execute(
                AppCommand::SetTaskOutputName(SetTaskOutputNameRequest {
                    task,
                    output_name: output_name.into(),
                }),
                &contexts,
            ));
            assert!(matches!(outcome, CommandOutcome::Failure { .. }));
        }
        assert!(
            gateway
                .changed_options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn task_speed_limit_forwards_typed_change_options_for_a_live_task() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let task = TaskIdentity::new(profile_id, Gid::from_u64(11));
        let contexts = HashMap::from([(
            task,
            TaskCommandContext {
                status: DownloadStatus::Active,
                metadata: TaskMetadata::default(),
            },
        )]);

        let outcome = block_on(service.execute(
            AppCommand::SetTaskSpeedLimit(SetTaskSpeedLimitRequest {
                task,
                download_limit: ariadeck_domain::ByteRate::new(2 * 1024 * 1024),
                upload_limit: ariadeck_domain::ByteRate::new(0),
            }),
            &contexts,
        ));

        assert_eq!(
            outcome,
            CommandOutcome::Success {
                succeeded: vec![CommandItem::Task(task)]
            }
        );
        assert_eq!(
            *gateway
                .changed_options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![(
                task.gid,
                vec![
                    ("max-download-limit".into(), (2 * 1024 * 1024).to_string()),
                    ("max-upload-limit".into(), "0".into()),
                ]
            )]
        );
    }

    #[test]
    fn task_speed_limit_rejects_terminal_tasks_before_the_gateway() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let task = TaskIdentity::new(profile_id, Gid::from_u64(12));
        let contexts = HashMap::from([(
            task,
            TaskCommandContext {
                status: DownloadStatus::Complete,
                metadata: TaskMetadata::default(),
            },
        )]);

        let outcome = block_on(service.execute(
            AppCommand::SetTaskSpeedLimit(SetTaskSpeedLimitRequest {
                task,
                download_limit: ariadeck_domain::ByteRate::new(1024),
                upload_limit: ariadeck_domain::ByteRate::new(1024),
            }),
            &contexts,
        ));

        assert!(matches!(outcome, CommandOutcome::Failure { .. }));
        assert!(
            gateway
                .changed_options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn global_speed_limit_forwards_overall_options_to_the_gateway() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());

        let result = block_on(service.apply_speed_limit(&SpeedLimitConfig {
            download_limit: ariadeck_domain::ByteRate::new(5 * 1024 * 1024),
            upload_limit: ariadeck_domain::ByteRate::new(512 * 1024),
        }));

        assert!(result.is_ok());
        assert_eq!(
            *gateway
                .global_options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![vec![
                (
                    "max-overall-download-limit".into(),
                    (5 * 1024 * 1024).to_string()
                ),
                ("max-overall-upload-limit".into(), (512 * 1024).to_string()),
            ]]
        );
    }

    #[test]
    fn task_connection_policy_forwards_typed_change_options_for_a_live_task() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let task = TaskIdentity::new(profile_id, Gid::from_u64(21));
        let contexts = HashMap::from([(
            task,
            TaskCommandContext {
                status: DownloadStatus::Active,
                metadata: TaskMetadata::default(),
            },
        )]);

        let outcome = block_on(service.execute(
            AppCommand::SetTaskConnectionPolicy(SetTaskConnectionPolicyRequest {
                task,
                policy: TaskConnectionPolicy {
                    max_connection_per_server: 8,
                    split: 16,
                    min_split_size: 1024 * 1024,
                },
            }),
            &contexts,
        ));

        assert_eq!(
            outcome,
            CommandOutcome::Success {
                succeeded: vec![CommandItem::Task(task)]
            }
        );
        assert_eq!(
            *gateway
                .changed_options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![(
                task.gid,
                vec![
                    ("max-connection-per-server".into(), "8".into()),
                    ("split".into(), "16".into()),
                    ("min-split-size".into(), (1024 * 1024).to_string()),
                ]
            )]
        );
    }

    #[test]
    fn task_connection_policy_rejects_terminal_and_out_of_range_values() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let task = TaskIdentity::new(profile_id, Gid::from_u64(22));
        let contexts = HashMap::from([(
            task,
            TaskCommandContext {
                status: DownloadStatus::Error,
                metadata: TaskMetadata::default(),
            },
        )]);

        let invalid = block_on(service.execute(
            AppCommand::SetTaskConnectionPolicy(SetTaskConnectionPolicyRequest {
                task,
                policy: TaskConnectionPolicy {
                    max_connection_per_server: 32,
                    split: 5,
                    min_split_size: 1024,
                },
            }),
            &contexts,
        ));
        assert!(matches!(invalid, CommandOutcome::Failure { .. }));

        let terminal = block_on(service.execute(
            AppCommand::SetTaskConnectionPolicy(SetTaskConnectionPolicyRequest {
                task,
                policy: TaskConnectionPolicy {
                    max_connection_per_server: 4,
                    split: 5,
                    min_split_size: 1024,
                },
            }),
            &contexts,
        ));
        assert!(matches!(terminal, CommandOutcome::Failure { .. }));
        assert!(
            gateway
                .changed_options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn global_transfer_policy_forwards_typed_options_to_the_gateway() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());

        let result = block_on(service.apply_transfer_policy(&TransferPolicyConfig {
            max_concurrent_downloads: 3,
            max_connection_per_server: 8,
            split: 16,
            min_split_size: 1024 * 1024,
            file_allocation: ariadeck_domain::FileAllocationMethod::Falloc,
            check_integrity: true,
        }));

        assert!(result.is_ok());
        assert_eq!(
            *gateway
                .global_options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![vec![
                ("max-concurrent-downloads".into(), "3".into()),
                ("max-connection-per-server".into(), "8".into()),
                ("split".into(), "16".into()),
                ("min-split-size".into(), (1024 * 1024).to_string()),
                ("file-allocation".into(), "falloc".into()),
                ("check-integrity".into(), "true".into()),
            ]]
        );
    }

    #[test]
    fn retry_rejects_non_failed_or_unreplayable_tasks_before_the_gateway() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let active = TaskIdentity::new(profile_id, Gid::from_u64(1));
        let missing_source = TaskIdentity::new(profile_id, Gid::from_u64(2));
        let contexts = HashMap::from([
            (
                active,
                TaskCommandContext {
                    status: DownloadStatus::Active,
                    metadata: TaskMetadata::default(),
                },
            ),
            (
                missing_source,
                TaskCommandContext {
                    status: DownloadStatus::Error,
                    metadata: TaskMetadata::default(),
                },
            ),
        ]);

        let outcome = block_on(service.execute(
            AppCommand::RetryTasks(vec![active, missing_source]),
            &contexts,
        ));
        let CommandOutcome::Failure { failed } = outcome else {
            panic!("expected retry failure");
        };
        assert_eq!(failed.len(), 2);
        assert_eq!(failed[0].error.code, ApplicationErrorCode::Rejected);
        assert_eq!(failed[1].error.code, ApplicationErrorCode::Unsupported);
        assert!(
            gateway
                .retries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn force_pause_and_force_remove_forward_to_the_gateway() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let task = TaskIdentity::new(profile_id, Gid::from_u64(21));
        let contexts = HashMap::from([(
            task,
            TaskCommandContext {
                status: DownloadStatus::Active,
                metadata: TaskMetadata::default(),
            },
        )]);

        let pause = block_on(service.execute(AppCommand::ForcePauseTasks(vec![task]), &contexts));
        assert_eq!(
            pause,
            CommandOutcome::Success {
                succeeded: vec![CommandItem::Task(task)]
            }
        );

        let remove = block_on(service.execute(
            AppCommand::ForceRemoveTasks(RemoveTasksRequest {
                tasks: vec![task],
                scope: TaskRemovalScope::TaskOnly,
            }),
            &contexts,
        ));
        assert_eq!(
            remove,
            CommandOutcome::Success {
                succeeded: vec![CommandItem::Task(task)]
            }
        );

        let calls = gateway
            .calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        assert!(calls.contains(&(TaskOperation::ForcePause, task.gid)));
        assert!(calls.contains(&(
            TaskOperation::ForceRemove(TaskRemovalScope::TaskOnly),
            task.gid
        )));
    }

    #[test]
    fn set_task_options_maps_typed_seed_and_file_selection_fields() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let task = TaskIdentity::new(profile_id, Gid::from_u64(22));
        let contexts = HashMap::from([(
            task,
            TaskCommandContext {
                status: DownloadStatus::Seeding,
                metadata: TaskMetadata {
                    source_kind: TaskSourceKind::BitTorrent,
                    ..TaskMetadata::default()
                },
            },
        )]);

        let outcome = block_on(service.execute(
            AppCommand::SetTaskOptions(SetTaskOptionsRequest {
                task,
                seed_ratio: Some("1.5".into()),
                seed_time_minutes: Some(60),
                selected_file_indices: Some(vec![1, 2, 4]),
            }),
            &contexts,
        ));

        assert_eq!(
            outcome,
            CommandOutcome::Success {
                succeeded: vec![CommandItem::Task(task)]
            }
        );
        assert_eq!(
            *gateway
                .changed_options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![(
                task.gid,
                vec![
                    ("seed-ratio".into(), "1.5".into()),
                    ("seed-time".into(), "60".into()),
                    ("select-file".into(), "1-2,4".into()),
                ]
            )]
        );
    }

    #[test]
    fn set_task_options_rejects_seed_rules_for_direct_uri_tasks() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let task = TaskIdentity::new(profile_id, Gid::from_u64(23));
        let contexts = HashMap::from([(
            task,
            TaskCommandContext {
                status: DownloadStatus::Active,
                metadata: TaskMetadata {
                    source_kind: TaskSourceKind::DirectUri,
                    ..TaskMetadata::default()
                },
            },
        )]);

        let outcome = block_on(service.execute(
            AppCommand::SetTaskOptions(SetTaskOptionsRequest {
                task,
                seed_ratio: Some("1.0".into()),
                seed_time_minutes: None,
                selected_file_indices: None,
            }),
            &contexts,
        ));

        assert!(matches!(outcome, CommandOutcome::Failure { .. }));
        assert!(
            gateway
                .changed_options
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[test]
    fn task_option_request_formats_selected_file_ranges() {
        let request = SetTaskOptionsRequest {
            task: TaskIdentity::new(ProfileId::new(), Gid::from_u64(1)),
            seed_ratio: None,
            seed_time_minutes: None,
            selected_file_indices: Some(vec![1, 2, 3, 5, 7, 8]),
        };
        assert_eq!(
            request.to_option_pairs(),
            vec![("select-file".into(), "1-3,5,7-8".into())]
        );
    }

    #[test]
    fn advanced_options_flatten_headers_cookie_auth_and_checksum() {
        let advanced = AddDownloadAdvancedOptions {
            referer: Some("https://example.test/page".into()),
            user_agent: Some("AriaDeck/test".into()),
            headers: vec!["X-Token: abc".into(), "Accept: */*".into()],
            cookie: Some(SecretString::new("session=secret".into())),
            http_user: Some("alice".into()),
            http_passwd: Some(SecretString::new("s3cret".into())),
            checksum: Some(format!("sha-256={}", "ab".repeat(32))),
        };
        assert!(advanced.validate().is_ok());
        assert_eq!(
            advanced.to_option_pairs(),
            vec![
                ("referer".into(), "https://example.test/page".into()),
                ("user-agent".into(), "AriaDeck/test".into()),
                ("header".into(), "X-Token: abc".into()),
                ("header".into(), "Accept: */*".into()),
                ("header".into(), "Cookie: session=secret".into()),
                ("http-user".into(), "alice".into()),
                ("http-passwd".into(), "s3cret".into()),
                ("checksum".into(), format!("sha-256={}", "ab".repeat(32))),
            ]
        );
        let debug = format!("{advanced:?}");
        assert!(!debug.contains("s3cret"));
        assert!(!debug.contains("session=secret"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn advanced_options_reject_secret_headers_and_bad_checksums() {
        let cookie_header = AddDownloadAdvancedOptions {
            headers: vec!["Cookie: session=abc".into()],
            ..AddDownloadAdvancedOptions::default()
        };
        assert!(cookie_header.validate().is_err());

        let auth_header = AddDownloadAdvancedOptions {
            headers: vec!["Authorization: Bearer token".into()],
            ..AddDownloadAdvancedOptions::default()
        };
        assert!(auth_header.validate().is_err());

        let bad_checksum = AddDownloadAdvancedOptions {
            checksum: Some("sha-256=deadbeef".into()),
            ..AddDownloadAdvancedOptions::default()
        };
        assert!(bad_checksum.validate().is_err());

        let password_without_user = AddDownloadAdvancedOptions {
            http_passwd: Some(SecretString::new("x".into())),
            ..AddDownloadAdvancedOptions::default()
        };
        assert!(password_without_user.validate().is_err());
    }

    #[test]
    fn add_download_forwards_typed_advanced_options_only_for_direct_uris() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let service = CommandService::new_unprobed(profile_id, gateway.clone());
        let advanced = AddDownloadAdvancedOptions {
            referer: Some("https://cdn.example/ref".into()),
            headers: vec!["X-Request-Id: 1".into()],
            cookie: Some(SecretString::new("k=v".into())),
            checksum: Some(format!("sha-256={}", "cd".repeat(32))),
            ..AddDownloadAdvancedOptions::default()
        };

        let outcome = block_on(service.execute(
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Uris(vec!["https://example.test/file.bin".into()]),
                destination: None,
                file_conflict: FileConflictPolicy::default(),
                selected_file_indices: None,
                advanced: advanced.clone(),
                options: Vec::new(),
            }),
            &HashMap::new(),
        ));
        assert!(matches!(outcome, CommandOutcome::Success { .. }));
        let adds = gateway
            .adds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        assert_eq!(adds.len(), 1);
        assert_eq!(adds[0].options, advanced.to_option_pairs());

        let rejected = block_on(service.execute(
            AppCommand::AddDownload(AddDownloadRequest {
                source: AddDownloadSource::Torrent(Arc::<[u8]>::from(&b"torrent"[..])),
                destination: None,
                file_conflict: FileConflictPolicy::Reject,
                selected_file_indices: None,
                advanced,
                options: Vec::new(),
            }),
            &HashMap::new(),
        ));
        assert!(matches!(rejected, CommandOutcome::Failure { .. }));
        assert_eq!(
            gateway
                .adds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            1,
            "invalid metadata advanced options must not reach the gateway"
        );
    }

    #[test]
    fn force_pause_is_rejected_before_the_gateway_when_list_methods_omits_it() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let capabilities = EngineCapabilities {
            version: "1.37.0".into(),
            enabled_features: Vec::new(),
            methods: vec![
                "aria2.pause".into(),
                "aria2.unpause".into(),
                "aria2.remove".into(),
            ],
        };
        let service = CommandService::new(profile_id, gateway.clone(), capabilities);
        let task = TaskIdentity::new(profile_id, Gid::from_u64(99));
        let contexts = HashMap::from([(
            task,
            TaskCommandContext {
                status: DownloadStatus::Active,
                metadata: TaskMetadata::default(),
            },
        )]);

        let outcome = block_on(service.execute(AppCommand::ForcePauseTasks(vec![task]), &contexts));
        match outcome {
            CommandOutcome::Failure { failed } => {
                assert_eq!(failed.len(), 1);
                assert_eq!(failed[0].error.code, ApplicationErrorCode::Unsupported);
                assert!(
                    failed[0].error.summary.contains("force-pause")
                        || failed[0].error.summary.contains("forcePause"),
                    "{}",
                    failed[0].error.summary
                );
            }
            other => panic!("expected capability rejection, got {other:?}"),
        }
        assert!(
            gateway
                .calls
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty(),
            "unsupported force-pause must not reach the gateway"
        );
    }

    #[test]
    fn change_global_option_controls_are_gated_when_missing_from_list_methods() {
        let profile_id = ProfileId::new();
        let gateway = Arc::new(FakeGateway::default());
        let capabilities = EngineCapabilities {
            version: "1.36.0".into(),
            enabled_features: Vec::new(),
            methods: vec!["aria2.getVersion".into(), "aria2.tellActive".into()],
        };
        let service = CommandService::new(profile_id, gateway.clone(), capabilities);
        let err = block_on(service.apply_download_proxy(&DownloadProxyConfig::default()))
            .expect_err("proxy apply must be gated");
        assert_eq!(err.code, ApplicationErrorCode::Unsupported);
        assert!(
            err.summary.contains("changeGlobalOption") || err.summary.contains("global option")
        );
    }
}
