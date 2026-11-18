use super::{commands::OperationErrorView, task::RequestId};

/// How a managed aria2 core entered the local registry (CORE-001).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CoreSourceView {
    #[default]
    Imported,
    Linked,
    Managed,
}

impl CoreSourceView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Imported => "Imported",
            Self::Linked => "Linked",
            Self::Managed => "Managed",
        }
    }

    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::Imported => "settings-core-source-imported",
            Self::Linked => "settings-core-source-linked",
            Self::Managed => "settings-core-source-managed",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CoreInstallStatusView {
    #[default]
    Ready,
    MissingExecutable,
    MissingManifest,
}

impl CoreInstallStatusView {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::MissingExecutable => "Missing executable",
            Self::MissingManifest => "Missing manifest",
        }
    }

    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::Ready => "settings-core-status-ready",
            Self::MissingExecutable => "settings-core-status-missing-executable",
            Self::MissingManifest => "settings-core-status-missing-manifest",
        }
    }
}

/// One managed aria2 installation for Settings → Engine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreInstallationView {
    pub id: String,
    pub version: String,
    pub target: String,
    pub source: CoreSourceView,
    pub executable: String,
    pub features: Vec<String>,
    pub is_active: bool,
    pub is_last_working: bool,
    pub validated_version: Option<String>,
    pub status: CoreInstallStatusView,
}

/// Snapshot of the managed core registry.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CoreRegistryView {
    pub installations: Vec<CoreInstallationView>,
    pub active_id: Option<String>,
    pub last_working_id: Option<String>,
}

impl CoreRegistryView {
    #[must_use]
    pub fn active(&self) -> Option<&CoreInstallationView> {
        self.installations.iter().find(|core| core.is_active)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreCommandView {
    /// Copy a local aria2c into the managed cores tree and register it.
    Import {
        path: String,
    },
    /// Register a path without copying.
    Link {
        path: String,
    },
    Verify {
        core_id: String,
    },
    Activate {
        core_id: String,
    },
    Rollback,
    Remove {
        core_id: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreCommandRequestView {
    pub request_id: RequestId,
    pub command: CoreCommandView,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreCommandOutcomeView {
    Success,
    Failure(OperationErrorView),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreCommandResultView {
    pub request_id: RequestId,
    pub command: CoreCommandView,
    pub registry: CoreRegistryView,
    pub outcome: CoreCommandOutcomeView,
}

/// Where a discovered `aria2c` was found (B4).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CoreDiscoveryOriginView {
    Environment,
    #[default]
    SearchPath,
    Scoop,
    WinGet,
    Chocolatey,
    Homebrew,
    SystemInstall,
    Portable,
}

impl CoreDiscoveryOriginView {
    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::Environment => "core-origin-environment",
            Self::SearchPath => "core-origin-path",
            Self::Scoop => "core-origin-scoop",
            Self::WinGet => "core-origin-winget",
            Self::Chocolatey => "core-origin-chocolatey",
            Self::Homebrew => "core-origin-homebrew",
            Self::SystemInstall => "core-origin-system",
            Self::Portable => "core-origin-portable",
        }
    }
}

/// One `aria2c` found on this machine, offered for import or link (B4).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiscoveredCoreView {
    pub path: String,
    pub version: String,
    pub origin: CoreDiscoveryOriginView,
    pub features: Vec<String>,
    /// Already present in the managed core registry, so Import would duplicate it.
    pub already_registered: bool,
}

/// The pinned, checksum-verified build offered for this platform (B4).
///
/// `available == false` means upstream publishes nothing for this target; the UI
/// says so rather than offering an unverified download.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CoreDownloadOfferView {
    pub available: bool,
    pub version: String,
    pub target: String,
    pub url: String,
    /// The pinned build is not native to this target and runs under OS emulation.
    pub emulated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreDiscoveryResultView {
    pub request_id: RequestId,
    pub discovered: Vec<DiscoveredCoreView>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreDownloadResultView {
    pub request_id: RequestId,
    pub registry: CoreRegistryView,
    /// Version that landed in the registry, for the success message.
    pub installed_version: Option<String>,
    pub outcome: CoreCommandOutcomeView,
}
