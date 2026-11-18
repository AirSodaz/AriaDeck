use super::task::TaskCountsView;

/// Reuses [`ariadeck_domain::DownloadFilter`] directly to eliminate redundant
/// DTO mirroring across the desktop boundary.
pub type WorkspaceFilter = ariadeck_domain::DownloadFilter;

/// Reuses [`ariadeck_domain::SortKey`] directly.
pub type WorkspaceSortKey = ariadeck_domain::SortKey;

/// Reuses [`ariadeck_domain::SortDirection`] directly.
pub type WorkspaceSortDirection = ariadeck_domain::SortDirection;

pub trait WorkspaceSortKeyExt {
    #[must_use]
    fn label(self) -> &'static str;

    #[must_use]
    fn key(self) -> &'static str;
}

impl WorkspaceSortKeyExt for WorkspaceSortKey {
    fn label(self) -> &'static str {
        match self {
            Self::Queue => "Queue",
            Self::Name => "Name",
            Self::Status => "Status",
            Self::Progress => "Progress",
            Self::DownloadSpeed => "Download speed",
            Self::Size => "Size",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Name => "name",
            Self::Status => "status",
            Self::Progress => "progress",
            Self::DownloadSpeed => "download-speed",
            Self::Size => "size",
        }
    }
}

pub trait WorkspaceSortDirectionExt {
    #[must_use]
    fn label(self) -> &'static str;
}

impl WorkspaceSortDirectionExt for WorkspaceSortDirection {
    fn label(self) -> &'static str {
        match self {
            Self::Ascending => "Ascending",
            Self::Descending => "Descending",
        }
    }
}

pub trait WorkspaceFilterExt {
    #[must_use]
    fn label(self) -> &'static str;

    #[must_use]
    fn short_label(self) -> &'static str;

    #[must_use]
    fn message_key(self) -> &'static str;

    #[must_use]
    fn count(self, counts: TaskCountsView) -> usize;

    #[must_use]
    fn key(self) -> &'static str;
}

impl WorkspaceFilterExt for WorkspaceFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All tasks",
            Self::Active => "Active",
            Self::Waiting => "Waiting",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }

    fn short_label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Active => "Active",
            Self::Waiting => "Waiting",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }

    fn message_key(self) -> &'static str {
        match self {
            Self::All => "filter-all",
            Self::Active => "filter-active",
            Self::Waiting => "filter-waiting",
            Self::Paused => "filter-paused",
            Self::Completed => "filter-completed",
            Self::Failed => "filter-failed",
        }
    }

    fn count(self, counts: TaskCountsView) -> usize {
        match self {
            Self::All => counts.all,
            Self::Active => counts.active,
            Self::Waiting => counts.waiting,
            Self::Paused => counts.paused,
            Self::Completed => counts.completed,
            Self::Failed => counts.failed,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Active => "active",
            Self::Waiting => "waiting",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceQuery {
    pub filter: WorkspaceFilter,
    pub search: String,
    pub sort_key: WorkspaceSortKey,
    pub sort_direction: WorkspaceSortDirection,
    /// When set, only tasks affiliated with this category id are shown.
    pub category_id: Option<String>,
}
