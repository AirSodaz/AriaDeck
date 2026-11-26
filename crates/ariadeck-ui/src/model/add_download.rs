use super::{
    commands::{CommandOutcomeView, OperationErrorView},
    settings::SecretStringView,
    task::{EngineSessionView, RequestId, TaskIdentity},
};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AddDownloadInputModeView {
    #[default]
    Links,
    MetadataFiles,
}

impl AddDownloadInputModeView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Links => "Links",
            Self::MetadataFiles => "Torrent / Metalink",
        }
    }

    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::Links => "dialog-add-input-links",
            Self::MetadataFiles => "dialog-add-input-metadata",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddDownloadMetadataKindView {
    Torrent,
    Metalink,
}

impl AddDownloadMetadataKindView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Torrent => "Torrent",
            Self::Metalink => "Metalink",
        }
    }

    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::Torrent => "dialog-add-kind-torrent",
            Self::Metalink => "dialog-add-kind-metalink",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AddDownloadModeView {
    #[default]
    SeparateTasks,
    Mirrors,
}

impl AddDownloadModeView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::SeparateTasks => "Separate tasks",
            Self::Mirrors => "Mirrors (one task)",
        }
    }

    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::SeparateTasks => "dialog-add-mode-separate",
            Self::Mirrors => "dialog-add-mode-mirrors",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FileConflictPolicyView {
    #[default]
    AutoRename,
    Reject,
    Overwrite,
}

impl FileConflictPolicyView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::AutoRename => "Keep both",
            Self::Reject => "Reject",
            Self::Overwrite => "Overwrite",
        }
    }

    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::AutoRename => "dialog-add-conflict-keep-both",
            Self::Reject => "dialog-add-conflict-reject",
            Self::Overwrite => "dialog-add-conflict-overwrite",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AddDownloadSourceView {
    Uri {
        line: usize,
        uri: String,
    },
    MetadataFile {
        path: PathBuf,
        kind: AddDownloadMetadataKindView,
        content_sha256: String,
        info_hash: Option<String>,
        selected_file_indices: Vec<u32>,
    },
}

impl AddDownloadSourceView {
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Uri { line, uri } => {
                format!("Line {line} - {}", ariadeck_domain::redact_source_uri(uri))
            }
            Self::MetadataFile { path, kind, .. } => {
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into(),
                );
                format!("{} - {name}", kind.label())
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadMetadataFileView {
    pub index: u32,
    pub path: String,
    pub length: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadMetadataPreviewView {
    pub path: PathBuf,
    pub kind: AddDownloadMetadataKindView,
    pub content_sha256: String,
    pub info_hash: Option<String>,
    pub files: Vec<AddDownloadMetadataFileView>,
    pub selected_file_indices: Vec<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadMetadataPreviewRequestView {
    pub request_id: RequestId,
    pub paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AddDownloadMetadataPreviewOutcomeView {
    Ready(AddDownloadMetadataPreviewView),
    Failed(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadMetadataPreviewItemView {
    pub path: PathBuf,
    pub outcome: AddDownloadMetadataPreviewOutcomeView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadMetadataPreviewResultView {
    pub request_id: RequestId,
    pub items: Vec<AddDownloadMetadataPreviewItemView>,
}

/// Advanced source controls for a new direct-URI download (ADD-005).
///
/// Secrets use `SecretStringView` so Debug and notices never echo them. Cookie
/// and HTTP password are separate from free-form headers so the application
/// layer can keep redaction and validation consistent.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AddDownloadAdvancedOptionsView {
    pub referer: String,
    pub user_agent: String,
    /// Multi-line `Name: value` headers. Cookie/Authorization belong in the
    /// dedicated secret fields below.
    pub headers: String,
    pub cookie: Option<SecretStringView>,
    pub http_user: String,
    pub http_passwd: Option<SecretStringView>,
    /// aria2 `type=digest` form, for example `sha-256=…`.
    pub checksum: String,
}

impl AddDownloadAdvancedOptionsView {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.referer.trim().is_empty()
            && self.user_agent.trim().is_empty()
            && self.headers.trim().is_empty()
            && self.cookie.is_none()
            && self.http_user.trim().is_empty()
            && self.http_passwd.is_none()
            && self.checksum.trim().is_empty()
    }
}

/// A download handed over by the browser bridge, awaiting confirmation (D-045).
///
/// The add dialog carries one shared advanced-options block for every link in a
/// request, so the shell only fills a run of downloads that agree on referer,
/// user agent, and cookie; the rest stay queued for the next dialog. `filename`
/// and `file_size` are display hints and never become an output path (D-001).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BridgeDownloadView {
    pub url: String,
    pub referer: String,
    pub user_agent: String,
    pub cookie: Option<SecretStringView>,
    pub filename: Option<String>,
    pub file_size: Option<u64>,
}

impl BridgeDownloadView {
    /// Whether two downloads can share a single add dialog.
    #[must_use]
    pub fn shares_headers_with(&self, other: &Self) -> bool {
        self.referer == other.referer
            && self.user_agent == other.user_agent
            && self.cookie == other.cookie
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadRequestView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub sources: Vec<AddDownloadSourceView>,
    pub mode: AddDownloadModeView,
    pub destination: Option<String>,
    /// Optional category id from settings; desktop persists affiliation after add.
    pub category_id: Option<String>,
    pub required_bytes: Option<u64>,
    pub file_conflict: FileConflictPolicyView,
    pub advanced: AddDownloadAdvancedOptionsView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadItemResultView {
    pub sources: Vec<AddDownloadSourceView>,
    pub existing_task: Option<TaskIdentity>,
    pub outcome: CommandOutcomeView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddDownloadResultView {
    pub request_id: RequestId,
    pub session: EngineSessionView,
    pub items: Vec<AddDownloadItemResultView>,
}

/// Request to run a lightweight HTTP HEAD/Range probe before adding a URI.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbeDownloadUrlRequestView {
    pub request_id: RequestId,
    pub url: String,
    pub referer: String,
    pub user_agent: String,
    pub headers: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbedDownloadMetadataView {
    pub final_url: String,
    pub resolved_filename: Option<String>,
    pub total_bytes: Option<u64>,
    pub etag: Option<String>,
    pub accept_ranges: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbeDownloadUrlResultView {
    pub request_id: RequestId,
    pub url: String,
    pub outcome: Result<ProbedDownloadMetadataView, OperationErrorView>,
}
