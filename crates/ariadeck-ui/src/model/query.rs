use super::task::TaskCountsView;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum WorkspaceFilter {
    #[default]
    All,
    Active,
    Waiting,
    Paused,
    Completed,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum WorkspaceSortKey {
    #[default]
    Queue,
    Name,
    Status,
    Progress,
    DownloadSpeed,
    Size,
}

impl WorkspaceSortKey {
    pub const ALL: [Self; 6] = [
        Self::Queue,
        Self::Name,
        Self::Status,
        Self::Progress,
        Self::DownloadSpeed,
        Self::Size,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Queue => "Queue",
            Self::Name => "Name",
            Self::Status => "Status",
            Self::Progress => "Progress",
            Self::DownloadSpeed => "Download speed",
            Self::Size => "Size",
        }
    }

    #[must_use]
    pub const fn key(self) -> &'static str {
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

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum WorkspaceSortDirection {
    #[default]
    Ascending,
    Descending,
}

impl WorkspaceSortDirection {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ascending => "Ascending",
            Self::Descending => "Descending",
        }
    }

    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

impl WorkspaceFilter {
    pub const ALL: [Self; 6] = [
        Self::All,
        Self::Active,
        Self::Waiting,
        Self::Paused,
        Self::Completed,
        Self::Failed,
    ];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All tasks",
            Self::Active => "Active",
            Self::Waiting => "Waiting",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }

    #[must_use]
    pub const fn short_label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Active => "Active",
            Self::Waiting => "Waiting",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }

    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::All => "filter-all",
            Self::Active => "filter-active",
            Self::Waiting => "filter-waiting",
            Self::Paused => "filter-paused",
            Self::Completed => "filter-completed",
            Self::Failed => "filter-failed",
        }
    }

    #[must_use]
    pub const fn count(self, counts: TaskCountsView) -> usize {
        match self {
            Self::All => counts.all,
            Self::Active => counts.active,
            Self::Waiting => counts.waiting,
            Self::Paused => counts.paused,
            Self::Completed => counts.completed,
            Self::Failed => counts.failed,
        }
    }

    #[must_use]
    pub const fn key(self) -> &'static str {
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
