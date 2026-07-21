use std::fmt;

use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ByteCount, ByteRate, Gid, TaskProgress};

/// aria2 task lifecycle state normalized for application use.
#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Active,
    Waiting,
    Paused,
    Complete,
    Error,
    Removed,
    Verifying,
    #[default]
    Unknown,
}

impl DownloadStatus {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Complete | Self::Error | Self::Removed)
    }
}

/// How the engine-derived display name should be interpreted by consumers.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskNameState {
    /// aria2 has not exposed a reliable file or metadata name yet.
    #[default]
    Resolving,
    /// The name came from aria2 task metadata or a file path.
    Resolved,
    /// A user supplied an explicit output name.
    Custom,
}

impl TaskNameState {
    #[must_use]
    pub const fn is_resolving(self) -> bool {
        matches!(self, Self::Resolving)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskSourceKind {
    #[default]
    Unknown,
    DirectUri,
    Magnet,
    BitTorrent,
    Metalink,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskError {
    pub code: Option<u32>,
    pub message: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskMetadata {
    pub directory: Option<EnginePath>,
    pub primary_uri: Option<String>,
    pub info_hash: Option<String>,
    pub file_count: u32,
    pub followed_by: Vec<Gid>,
    pub belongs_to: Option<Gid>,
    pub source_kind: TaskSourceKind,
}

/// Path in the engine's own filesystem namespace.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EnginePath(String);

impl EnginePath {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EnginePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl From<String> for EnginePath {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for EnginePath {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskFile {
    pub index: u32,
    pub path: EnginePath,
    pub length: ByteCount,
    pub completed_length: ByteCount,
    pub selected: bool,
}

/// On-demand task projection used by the details drawer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskDetails {
    pub gid: Gid,
    pub directory: Option<EnginePath>,
    pub info_hash: Option<String>,
    pub piece_length: Option<ByteCount>,
    pub piece_count: Option<u32>,
    pub trackers: Vec<TaskTracker>,
    pub files: Vec<TaskFile>,
}

/// One BitTorrent tracker from an aria2 announce-list tier.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskTracker {
    pub tier: u32,
    pub uri: String,
}

/// How aria2 currently treats a source URI for a task (from `getUris`).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskUriStatus {
    /// aria2 is actively using this URI.
    Used,
    /// aria2 holds this URI in reserve (a mirror not yet in use).
    Waiting,
    #[default]
    Unknown,
}

/// A single source URI and its current usage, from `aria2.getUris`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskUri {
    pub uri: String,
    pub status: TaskUriStatus,
}

/// An active HTTP(S)/FTP server connection for one file, from `aria2.getServers`.
///
/// aria2 reports servers grouped per file index; `current_uri` is the URI aria2
/// resolved to after any redirect, which can differ from the originally listed
/// `uri`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskServer {
    pub file_index: u32,
    pub uri: String,
    pub current_uri: String,
    pub download_speed: ByteRate,
}

/// An active BitTorrent peer, from `aria2.getPeers`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskPeer {
    pub address: String,
    pub port: u16,
    pub download_speed: ByteRate,
    pub upload_speed: ByteRate,
    /// True when the peer reports itself as a seed.
    pub seeder: bool,
}

/// A read-only aria2 option key/value pair from `aria2.getOption`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskOptionEntry {
    pub key: String,
    pub value: String,
    /// Sensitive values are replaced inside the RPC adapter before they can
    /// enter application or UI state.
    pub redacted: bool,
}

/// On-demand connection/source projections kept outside the list refresh.
///
/// Peer and server data exist only while a task is active, so those vectors are
/// empty for non-active tasks. URIs and options are available regardless.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskConnectionDetails {
    pub gid: Gid,
    pub uris: Vec<TaskUri>,
    pub servers: Vec<TaskServer>,
    pub peers: Vec<TaskPeer>,
    pub options: Vec<TaskOptionEntry>,
}

impl TaskConnectionDetails {
    #[must_use]
    pub fn new(gid: Gid) -> Self {
        Self {
            gid,
            uris: Vec::new(),
            servers: Vec::new(),
            peers: Vec::new(),
            options: Vec::new(),
        }
    }
}

/// Adapter-produced task values without application revision metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskSnapshot {
    pub gid: Gid,
    pub status: DownloadStatus,
    pub display_name: String,
    pub name_state: TaskNameState,
    pub total_length: ByteCount,
    pub completed_length: ByteCount,
    pub upload_length: ByteCount,
    pub download_speed: ByteRate,
    pub upload_speed: ByteRate,
    pub connections: u32,
    pub error: Option<TaskError>,
    pub metadata: TaskMetadata,
}

impl TaskSnapshot {
    #[must_use]
    pub fn new(gid: Gid, status: DownloadStatus, display_name: impl Into<String>) -> Self {
        Self {
            gid,
            status,
            display_name: display_name.into(),
            name_state: TaskNameState::Resolved,
            total_length: ByteCount::default(),
            completed_length: ByteCount::default(),
            upload_length: ByteCount::default(),
            download_speed: ByteRate::default(),
            upload_speed: ByteRate::default(),
            connections: 0,
            error: None,
            metadata: TaskMetadata::default(),
        }
    }
}

bitflags! {
    /// Allocation-free description of fields changed by a task refresh.
    #[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
    pub struct TaskFields: u16 {
        const STATUS = 1 << 0;
        const DISPLAY_NAME = 1 << 1;
        const TOTAL_LENGTH = 1 << 2;
        const COMPLETED_LENGTH = 1 << 3;
        const UPLOAD_LENGTH = 1 << 4;
        const DOWNLOAD_SPEED = 1 << 5;
        const UPLOAD_SPEED = 1 << 6;
        const CONNECTIONS = 1 << 7;
        const ERROR = 1 << 8;
        const METADATA = 1 << 9;
        const NAME_STATE = 1 << 10;
    }
}

/// Application-owned task state with a semantic revision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadTask {
    pub gid: Gid,
    pub status: DownloadStatus,
    pub display_name: String,
    pub name_state: TaskNameState,
    pub total_length: ByteCount,
    pub completed_length: ByteCount,
    pub upload_length: ByteCount,
    pub download_speed: ByteRate,
    pub upload_speed: ByteRate,
    pub connections: u32,
    pub error: Option<TaskError>,
    pub metadata: TaskMetadata,
    pub revision: u64,
}

impl DownloadTask {
    #[must_use]
    pub fn from_snapshot(snapshot: TaskSnapshot) -> Self {
        Self {
            gid: snapshot.gid,
            status: snapshot.status,
            display_name: snapshot.display_name,
            name_state: snapshot.name_state,
            total_length: snapshot.total_length,
            completed_length: snapshot.completed_length,
            upload_length: snapshot.upload_length,
            download_speed: snapshot.download_speed,
            upload_speed: snapshot.upload_speed,
            connections: snapshot.connections,
            error: snapshot.error,
            metadata: snapshot.metadata,
            revision: 1,
        }
    }

    pub fn apply_snapshot(
        &mut self,
        snapshot: TaskSnapshot,
    ) -> Result<TaskFields, TaskUpdateError> {
        if self.gid != snapshot.gid {
            return Err(TaskUpdateError::GidMismatch {
                expected: self.gid,
                received: snapshot.gid,
            });
        }

        let mut changed = TaskFields::empty();
        update_field(
            &mut self.status,
            snapshot.status,
            TaskFields::STATUS,
            &mut changed,
        );
        if self.name_state != TaskNameState::Custom {
            update_field(
                &mut self.display_name,
                snapshot.display_name,
                TaskFields::DISPLAY_NAME,
                &mut changed,
            );
            update_field(
                &mut self.name_state,
                snapshot.name_state,
                TaskFields::NAME_STATE,
                &mut changed,
            );
        }
        update_field(
            &mut self.total_length,
            snapshot.total_length,
            TaskFields::TOTAL_LENGTH,
            &mut changed,
        );
        update_field(
            &mut self.completed_length,
            snapshot.completed_length,
            TaskFields::COMPLETED_LENGTH,
            &mut changed,
        );
        update_field(
            &mut self.upload_length,
            snapshot.upload_length,
            TaskFields::UPLOAD_LENGTH,
            &mut changed,
        );
        update_field(
            &mut self.download_speed,
            snapshot.download_speed,
            TaskFields::DOWNLOAD_SPEED,
            &mut changed,
        );
        update_field(
            &mut self.upload_speed,
            snapshot.upload_speed,
            TaskFields::UPLOAD_SPEED,
            &mut changed,
        );
        update_field(
            &mut self.connections,
            snapshot.connections,
            TaskFields::CONNECTIONS,
            &mut changed,
        );
        update_field(
            &mut self.error,
            snapshot.error,
            TaskFields::ERROR,
            &mut changed,
        );
        update_field(
            &mut self.metadata,
            snapshot.metadata,
            TaskFields::METADATA,
            &mut changed,
        );

        if !changed.is_empty() {
            self.revision = self.revision.saturating_add(1);
        }
        Ok(changed)
    }

    pub fn set_custom_output_name(&mut self, output_name: impl Into<String>) -> TaskFields {
        let mut changed = TaskFields::empty();
        update_field(
            &mut self.display_name,
            output_name.into(),
            TaskFields::DISPLAY_NAME,
            &mut changed,
        );
        update_field(
            &mut self.name_state,
            TaskNameState::Custom,
            TaskFields::NAME_STATE,
            &mut changed,
        );
        if !changed.is_empty() {
            self.revision = self.revision.saturating_add(1);
        }
        changed
    }

    #[must_use]
    pub const fn progress(&self) -> TaskProgress {
        TaskProgress::new(self.completed_length, self.total_length)
    }
}

fn update_field<T: PartialEq>(
    current: &mut T,
    incoming: T,
    field: TaskFields,
    changed: &mut TaskFields,
) {
    if *current != incoming {
        *current = incoming;
        changed.insert(field);
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum TaskUpdateError {
    #[error("task update GID mismatch: expected {expected}, received {received}")]
    GidMismatch { expected: Gid, received: Gid },
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadFilter {
    #[default]
    All,
    Active,
    Waiting,
    Paused,
    Completed,
    Failed,
}

impl DownloadFilter {
    #[must_use]
    pub const fn matches(self, status: DownloadStatus) -> bool {
        match self {
            Self::All => true,
            Self::Active => matches!(status, DownloadStatus::Active | DownloadStatus::Verifying),
            Self::Waiting => matches!(status, DownloadStatus::Waiting),
            Self::Paused => matches!(status, DownloadStatus::Paused),
            Self::Completed => matches!(status, DownloadStatus::Complete),
            Self::Failed => matches!(status, DownloadStatus::Error),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    #[default]
    Queue,
    Name,
    Status,
    Progress,
    DownloadSpeed,
    Size,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct DownloadSort {
    pub key: SortKey,
    pub direction: SortDirection,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_snapshot_does_not_increment_revision() {
        let snapshot = TaskSnapshot::new(Gid::from_u64(1), DownloadStatus::Active, "archive.iso");
        let mut task = DownloadTask::from_snapshot(snapshot.clone());

        let changed = match task.apply_snapshot(snapshot) {
            Ok(changed) => changed,
            Err(error) => panic!("matching snapshot rejected: {error}"),
        };

        assert!(changed.is_empty());
        assert_eq!(task.revision, 1);
    }

    #[test]
    fn snapshot_reports_only_semantically_changed_fields() {
        let mut initial = TaskSnapshot::new(Gid::from_u64(2), DownloadStatus::Waiting, "video.mkv");
        initial.total_length = ByteCount::new(100);
        let mut task = DownloadTask::from_snapshot(initial.clone());

        initial.status = DownloadStatus::Active;
        initial.completed_length = ByteCount::new(25);
        let changed = match task.apply_snapshot(initial) {
            Ok(changed) => changed,
            Err(error) => panic!("matching snapshot rejected: {error}"),
        };

        assert_eq!(changed, TaskFields::STATUS | TaskFields::COMPLETED_LENGTH);
        assert_eq!(task.revision, 2);
    }

    #[test]
    fn resolved_name_state_is_a_semantic_task_change() {
        let mut snapshot = TaskSnapshot::new(
            Gid::from_u64(3),
            DownloadStatus::Waiting,
            "0000000000000003",
        );
        snapshot.name_state = TaskNameState::Resolving;
        let mut task = DownloadTask::from_snapshot(snapshot.clone());

        snapshot.display_name = "archive.iso".into();
        snapshot.name_state = TaskNameState::Resolved;
        let changed = task
            .apply_snapshot(snapshot)
            .expect("matching resolved snapshot");

        assert_eq!(changed, TaskFields::DISPLAY_NAME | TaskFields::NAME_STATE);
        assert_eq!(task.name_state, TaskNameState::Resolved);
        assert_eq!(task.revision, 2);
    }

    #[test]
    fn connection_details_start_empty_for_a_gid() {
        let details = TaskConnectionDetails::new(Gid::from_u64(7));
        assert_eq!(details.gid, Gid::from_u64(7));
        assert!(details.uris.is_empty());
        assert!(details.servers.is_empty());
        assert!(details.peers.is_empty());
        assert!(details.options.is_empty());
    }

    #[test]
    fn custom_output_name_survives_later_engine_snapshots() {
        let mut task = DownloadTask::from_snapshot(TaskSnapshot::new(
            Gid::from_u64(4),
            DownloadStatus::Waiting,
            "original.bin",
        ));
        let changed = task.set_custom_output_name("renamed.bin");
        assert_eq!(changed, TaskFields::DISPLAY_NAME | TaskFields::NAME_STATE);
        assert_eq!(task.display_name, "renamed.bin");
        assert_eq!(task.name_state, TaskNameState::Custom);

        let mut snapshot =
            TaskSnapshot::new(Gid::from_u64(4), DownloadStatus::Active, "engine.bin");
        snapshot.name_state = TaskNameState::Resolved;
        let changed = task
            .apply_snapshot(snapshot)
            .expect("matching engine snapshot");
        assert_eq!(changed, TaskFields::STATUS);
        assert_eq!(task.display_name, "renamed.bin");
        assert_eq!(task.name_state, TaskNameState::Custom);
    }
}
