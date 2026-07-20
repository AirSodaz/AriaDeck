use ariadeck_domain::{
    ByteCount, ByteRate, DownloadStatus, EnginePath, Gid, GlobalStat, TaskDetails, TaskError,
    TaskFile, TaskMetadata, TaskNameState, TaskSnapshot, TaskSourceKind,
};
use serde::Deserialize;
use url::Url;

use crate::RpcError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VersionInfo {
    pub version: String,
    pub enabled_features: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TaskKey {
    Gid,
    Status,
    TotalLength,
    CompletedLength,
    UploadLength,
    DownloadSpeed,
    UploadSpeed,
    Connections,
    ErrorCode,
    ErrorMessage,
    VerifiedLength,
    VerifyIntegrityPending,
    Files,
    BitTorrent,
    Directory,
    InfoHash,
    FollowedBy,
    BelongsTo,
    PieceLength,
    NumPieces,
}

impl TaskKey {
    pub const LIST_PROJECTION: &'static [Self] = &[
        Self::Gid,
        Self::Status,
        Self::TotalLength,
        Self::CompletedLength,
        Self::UploadLength,
        Self::DownloadSpeed,
        Self::UploadSpeed,
        Self::Connections,
        Self::ErrorCode,
        Self::ErrorMessage,
        Self::VerifyIntegrityPending,
    ];

    /// Used only when a task first enters the adapter cache or is explicitly refreshed.
    pub const DISCOVERY_PROJECTION: &'static [Self] = &[
        Self::Gid,
        Self::Status,
        Self::TotalLength,
        Self::CompletedLength,
        Self::UploadLength,
        Self::DownloadSpeed,
        Self::UploadSpeed,
        Self::Connections,
        Self::ErrorCode,
        Self::ErrorMessage,
        Self::VerifyIntegrityPending,
        Self::Files,
        Self::BitTorrent,
        Self::Directory,
        Self::InfoHash,
        Self::FollowedBy,
        Self::BelongsTo,
    ];

    pub const DETAILS_PROJECTION: &'static [Self] = &[
        Self::Gid,
        Self::Files,
        Self::Directory,
        Self::InfoHash,
        Self::PieceLength,
        Self::NumPieces,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gid => "gid",
            Self::Status => "status",
            Self::TotalLength => "totalLength",
            Self::CompletedLength => "completedLength",
            Self::UploadLength => "uploadLength",
            Self::DownloadSpeed => "downloadSpeed",
            Self::UploadSpeed => "uploadSpeed",
            Self::Connections => "connections",
            Self::ErrorCode => "errorCode",
            Self::ErrorMessage => "errorMessage",
            Self::VerifiedLength => "verifiedLength",
            Self::VerifyIntegrityPending => "verifyIntegrityPending",
            Self::Files => "files",
            Self::BitTorrent => "bittorrent",
            Self::Directory => "dir",
            Self::InfoHash => "infoHash",
            Self::FollowedBy => "followedBy",
            Self::BelongsTo => "belongsTo",
            Self::PieceLength => "pieceLength",
            Self::NumPieces => "numPieces",
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VersionWire {
    pub version: String,
    #[serde(default)]
    pub enabled_features: Vec<String>,
}

impl From<VersionWire> for VersionInfo {
    fn from(value: VersionWire) -> Self {
        Self {
            version: value.version,
            enabled_features: value.enabled_features,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GlobalStatWire {
    #[serde(default)]
    download_speed: String,
    #[serde(default)]
    upload_speed: String,
    #[serde(default)]
    num_active: String,
    #[serde(default)]
    num_waiting: String,
    #[serde(default)]
    num_stopped_total: String,
}

impl GlobalStatWire {
    pub(crate) fn into_domain(self, method: &str) -> Result<GlobalStat, RpcError> {
        Ok(GlobalStat {
            download_speed: ByteRate::new(parse_u64(
                method,
                "downloadSpeed",
                &self.download_speed,
            )?),
            upload_speed: ByteRate::new(parse_u64(method, "uploadSpeed", &self.upload_speed)?),
            active_tasks: parse_u32(method, "numActive", &self.num_active)?,
            waiting_tasks: parse_u32(method, "numWaiting", &self.num_waiting)?,
            stopped_tasks: parse_u64(method, "numStoppedTotal", &self.num_stopped_total)?,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskWire {
    gid: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    total_length: String,
    #[serde(default)]
    completed_length: String,
    #[serde(default)]
    upload_length: String,
    #[serde(default)]
    download_speed: String,
    #[serde(default)]
    upload_speed: String,
    #[serde(default)]
    connections: String,
    #[serde(default)]
    error_code: String,
    #[serde(default)]
    error_message: String,
    #[serde(default)]
    verify_integrity_pending: String,
    #[serde(default)]
    files: Vec<FileWire>,
    #[serde(default)]
    bittorrent: Option<BitTorrentWire>,
    #[serde(default)]
    dir: String,
    #[serde(default)]
    info_hash: String,
    #[serde(default)]
    followed_by: Vec<String>,
    #[serde(default)]
    belongs_to: String,
    #[serde(default)]
    piece_length: String,
    #[serde(default)]
    num_pieces: String,
}

impl TaskWire {
    pub(crate) fn into_domain(self, method: &str) -> Result<TaskSnapshot, RpcError> {
        let gid = self
            .gid
            .parse::<Gid>()
            .map_err(|error| RpcError::InvalidData {
                method: method.into(),
                field: "gid".into(),
                message: error.to_string(),
            })?;
        let mut status = parse_status(&self.status);
        if status == DownloadStatus::Active && self.verify_integrity_pending == "true" {
            status = DownloadStatus::Verifying;
        }

        let resolved_name = self
            .bittorrent
            .as_ref()
            .and_then(|torrent| torrent.info.as_ref())
            .and_then(|info| non_empty(&info.name))
            .map(str::to_owned)
            .or_else(|| {
                self.files
                    .first()
                    .and_then(|file| basename(&file.path))
                    .map(str::to_owned)
            });
        let (display_name, name_state) = resolved_name.map_or_else(
            || (gid.to_string(), TaskNameState::Resolving),
            |name| (name, TaskNameState::Resolved),
        );
        let primary_uri = self
            .files
            .first()
            .and_then(|file| file.uris.first())
            .and_then(|uri| non_empty(&uri.uri))
            .map(str::to_owned);
        let followed_by = self
            .followed_by
            .iter()
            .map(|value| parse_gid(method, "followedBy", value))
            .collect::<Result<Vec<_>, _>>()?;
        let belongs_to = parse_optional_gid(method, "belongsTo", &self.belongs_to)?;
        let source_kind = task_source_kind(
            self.bittorrent.is_some(),
            self.files.len(),
            primary_uri.as_deref(),
            !self.info_hash.is_empty(),
        );
        let error_code = parse_optional_u32(method, "errorCode", &self.error_code)?;
        let error = (error_code.is_some() || !self.error_message.is_empty()).then_some(TaskError {
            code: error_code,
            message: self.error_message,
        });

        Ok(TaskSnapshot {
            gid,
            status,
            display_name,
            name_state,
            total_length: ByteCount::new(parse_u64(method, "totalLength", &self.total_length)?),
            completed_length: ByteCount::new(parse_u64(
                method,
                "completedLength",
                &self.completed_length,
            )?),
            upload_length: ByteCount::new(parse_u64(method, "uploadLength", &self.upload_length)?),
            download_speed: ByteRate::new(parse_u64(
                method,
                "downloadSpeed",
                &self.download_speed,
            )?),
            upload_speed: ByteRate::new(parse_u64(method, "uploadSpeed", &self.upload_speed)?),
            connections: parse_u32(method, "connections", &self.connections)?,
            error,
            metadata: TaskMetadata {
                directory: non_empty(&self.dir).map(EnginePath::new),
                primary_uri,
                info_hash: non_empty(&self.info_hash).map(str::to_owned),
                file_count: u32::try_from(self.files.len()).unwrap_or(u32::MAX),
                followed_by,
                belongs_to,
                source_kind,
            },
        })
    }

    pub(crate) fn into_details(self, method: &str) -> Result<TaskDetails, RpcError> {
        let files = self
            .files
            .iter()
            .map(|file| file.to_domain(method))
            .collect::<Result<Vec<_>, _>>()?;
        let gid = self
            .gid
            .parse::<Gid>()
            .map_err(|error| RpcError::InvalidData {
                method: method.into(),
                field: "gid".into(),
                message: error.to_string(),
            })?;
        Ok(TaskDetails {
            gid,
            directory: non_empty(&self.dir).map(EnginePath::new),
            info_hash: non_empty(&self.info_hash).map(str::to_owned),
            piece_length: parse_optional_u64(method, "pieceLength", &self.piece_length)?
                .map(ByteCount::new),
            piece_count: parse_optional_u32(method, "numPieces", &self.num_pieces)?,
            files,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileWire {
    #[serde(default)]
    index: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    length: String,
    #[serde(default)]
    completed_length: String,
    #[serde(default)]
    selected: String,
    #[serde(default)]
    uris: Vec<UriWire>,
}

impl FileWire {
    fn to_domain(&self, method: &str) -> Result<TaskFile, RpcError> {
        Ok(TaskFile {
            index: parse_u32(method, "files.index", &self.index)?,
            path: EnginePath::new(&self.path),
            length: ByteCount::new(parse_u64(method, "files.length", &self.length)?),
            completed_length: ByteCount::new(parse_u64(
                method,
                "files.completedLength",
                &self.completed_length,
            )?),
            selected: parse_bool(method, "files.selected", &self.selected)?,
        })
    }
}

#[derive(Deserialize)]
struct UriWire {
    #[serde(default)]
    uri: String,
}

#[derive(Deserialize)]
struct BitTorrentWire {
    #[serde(default)]
    info: Option<BitTorrentInfoWire>,
}

#[derive(Deserialize)]
struct BitTorrentInfoWire {
    #[serde(default)]
    name: String,
}

fn parse_status(value: &str) -> DownloadStatus {
    match value {
        "active" => DownloadStatus::Active,
        "waiting" => DownloadStatus::Waiting,
        "paused" => DownloadStatus::Paused,
        "complete" => DownloadStatus::Complete,
        "error" => DownloadStatus::Error,
        "removed" => DownloadStatus::Removed,
        _ => DownloadStatus::Unknown,
    }
}

fn task_source_kind(
    has_bittorrent_metadata: bool,
    file_count: usize,
    primary_uri: Option<&str>,
    has_info_hash: bool,
) -> TaskSourceKind {
    if has_bittorrent_metadata {
        return TaskSourceKind::BitTorrent;
    }
    let parsed_uri = primary_uri.and_then(|uri| Url::parse(uri).ok());
    if parsed_uri
        .as_ref()
        .is_some_and(|uri| uri.scheme() == "magnet")
    {
        return TaskSourceKind::Magnet;
    }
    if has_info_hash {
        return TaskSourceKind::BitTorrent;
    }
    if file_count > 1 {
        return TaskSourceKind::Metalink;
    }
    let Some(uri) = parsed_uri else {
        return TaskSourceKind::Unknown;
    };
    let path = uri.path().to_ascii_lowercase();
    if path.ends_with(".torrent") {
        return TaskSourceKind::BitTorrent;
    }
    if path.ends_with(".metalink") || path.ends_with(".meta4") {
        return TaskSourceKind::Metalink;
    }
    if matches!(uri.scheme(), "http" | "https" | "ftp" | "sftp") {
        TaskSourceKind::DirectUri
    } else {
        TaskSourceKind::Unknown
    }
}

fn parse_u64(method: &str, field: &str, value: &str) -> Result<u64, RpcError> {
    if value.is_empty() {
        return Ok(0);
    }
    value.parse::<u64>().map_err(|error| RpcError::InvalidData {
        method: method.into(),
        field: field.into(),
        message: error.to_string(),
    })
}

fn parse_u32(method: &str, field: &str, value: &str) -> Result<u32, RpcError> {
    if value.is_empty() {
        return Ok(0);
    }
    value.parse::<u32>().map_err(|error| RpcError::InvalidData {
        method: method.into(),
        field: field.into(),
        message: error.to_string(),
    })
}

fn parse_optional_u32(method: &str, field: &str, value: &str) -> Result<Option<u32>, RpcError> {
    let parsed = parse_u32(method, field, value)?;
    Ok((parsed != 0).then_some(parsed))
}

fn parse_optional_u64(method: &str, field: &str, value: &str) -> Result<Option<u64>, RpcError> {
    let parsed = parse_u64(method, field, value)?;
    Ok((parsed != 0).then_some(parsed))
}

fn parse_gid(method: &str, field: &str, value: &str) -> Result<Gid, RpcError> {
    value.parse::<Gid>().map_err(|error| RpcError::InvalidData {
        method: method.into(),
        field: field.into(),
        message: error.to_string(),
    })
}

fn parse_optional_gid(method: &str, field: &str, value: &str) -> Result<Option<Gid>, RpcError> {
    if value.is_empty() {
        Ok(None)
    } else {
        parse_gid(method, field, value).map(Some)
    }
}

fn parse_bool(method: &str, field: &str, value: &str) -> Result<bool, RpcError> {
    match value {
        "" | "false" => Ok(false),
        "true" => Ok(true),
        _ => Err(RpcError::InvalidData {
            method: method.into(),
            field: field.into(),
            message: format!("expected true or false, got {value}"),
        }),
    }
}

fn non_empty(value: &str) -> Option<&str> {
    (!value.is_empty()).then_some(value)
}

fn basename(path: &str) -> Option<&str> {
    path.rsplit(['/', '\\']).find(|segment| !segment.is_empty())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn task_conversion_accepts_unknown_and_missing_fields() {
        let wire = match serde_json::from_value::<TaskWire>(json!({
            "gid": "0000000000000001",
            "status": "active",
            "totalLength": "100",
            "completedLength": "25",
            "downloadSpeed": "10",
            "verifyIntegrityPending": "true",
            "files": [{
                "path": "/downloads/archive.iso",
                "uris": [{"uri": "https://example.test/archive.iso"}]
            }],
            "futureAria2Field": "ignored"
        })) {
            Ok(wire) => wire,
            Err(error) => panic!("valid task fixture rejected: {error}"),
        };
        let task = match wire.into_domain("aria2.tellActive") {
            Ok(task) => task,
            Err(error) => panic!("valid task conversion failed: {error}"),
        };

        assert_eq!(task.status, DownloadStatus::Verifying);
        assert_eq!(task.display_name, "archive.iso");
        assert_eq!(task.name_state, TaskNameState::Resolved);
        assert_eq!(task.metadata.source_kind, TaskSourceKind::DirectUri);
        assert_eq!(task.total_length, ByteCount::new(100));
        assert_eq!(
            task.metadata.primary_uri.as_deref(),
            Some("https://example.test/archive.iso")
        );
    }

    #[test]
    fn task_conversion_preserves_disk_space_error_details() {
        let wire = serde_json::from_value::<TaskWire>(json!({
            "gid": "0000000000000009",
            "status": "error",
            "errorCode": "9",
            "errorMessage": "File allocation failed",
            "files": [{"path": "/downloads/large.iso"}]
        }))
        .expect("valid failed task fixture");

        let task = wire
            .into_domain("aria2.tellStopped")
            .expect("valid failed task conversion");

        assert_eq!(task.status, DownloadStatus::Error);
        assert_eq!(
            task.error,
            Some(TaskError {
                code: Some(9),
                message: "File allocation failed".into(),
            })
        );
    }

    #[test]
    fn bittorrent_name_precedes_file_name() {
        let wire = match serde_json::from_value::<TaskWire>(json!({
            "gid": "0000000000000002",
            "status": "waiting",
            "bittorrent": {"info": {"name": "Linux Images"}},
            "files": [{"path": "C:\\downloads\\first.iso"}]
        })) {
            Ok(wire) => wire,
            Err(error) => panic!("valid torrent fixture rejected: {error}"),
        };
        let task = match wire.into_domain("aria2.tellWaiting") {
            Ok(task) => task,
            Err(error) => panic!("valid torrent conversion failed: {error}"),
        };

        assert_eq!(task.display_name, "Linux Images");
        assert_eq!(task.name_state, TaskNameState::Resolved);
        assert_eq!(task.metadata.source_kind, TaskSourceKind::BitTorrent);
    }

    #[test]
    fn task_without_metadata_keeps_gid_as_fallback_while_name_resolves() {
        let wire = serde_json::from_value::<TaskWire>(json!({
            "gid": "0000000000000004",
            "status": "waiting"
        }))
        .expect("valid unresolved task fixture");
        let task = wire
            .into_domain("aria2.tellWaiting")
            .expect("valid unresolved task conversion");

        assert_eq!(task.display_name, "0000000000000004");
        assert_eq!(task.name_state, TaskNameState::Resolving);
        assert_eq!(task.metadata.source_kind, TaskSourceKind::Unknown);
    }

    #[test]
    fn source_kind_distinguishes_magnet_and_metalink_inputs() {
        assert_eq!(
            task_source_kind(false, 0, Some("magnet:?xt=urn:btih:abcd"), false),
            TaskSourceKind::Magnet
        );
        assert_eq!(
            task_source_kind(
                false,
                1,
                Some("https://example.test/download.meta4?token=one"),
                false,
            ),
            TaskSourceKind::Metalink
        );
    }

    #[test]
    fn task_metadata_preserves_magnet_task_relationships() {
        let wire = serde_json::from_value::<TaskWire>(json!({
            "gid": "0000000000000005",
            "status": "active",
            "followedBy": ["0000000000000006"],
            "belongsTo": "0000000000000004"
        }))
        .expect("valid relationship fixture");
        let task = wire
            .into_domain("aria2.tellActive")
            .expect("valid relationship conversion");

        assert_eq!(task.metadata.followed_by, vec![Gid::from_u64(6)]);
        assert_eq!(task.metadata.belongs_to, Some(Gid::from_u64(4)));
    }

    #[test]
    fn malformed_decimal_string_is_rejected() {
        let wire = match serde_json::from_value::<TaskWire>(json!({
            "gid": "0000000000000003",
            "status": "active",
            "totalLength": "not-a-number"
        })) {
            Ok(wire) => wire,
            Err(error) => panic!("wire fixture failed before conversion: {error}"),
        };

        assert!(matches!(
            wire.into_domain("aria2.tellActive"),
            Err(RpcError::InvalidData { field, .. }) if field == "totalLength"
        ));
    }
}
