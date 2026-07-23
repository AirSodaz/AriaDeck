//! Local SQLite persistence for completed/failed download summaries (B6).
//!
//! Application owns the [`TaskHistoryStore`] port; this crate is the adapter.
//! Secrets and unredacted URIs must not be written here (D-032).

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use ariadeck_application::{HistoryRecord, TaskHistoryStore};
use ariadeck_domain::{
    ByteCount, DownloadStatus, EnginePath, Gid, ProfileId, TaskError, TaskSourceKind,
    redact_source_uri,
};
use rusqlite::{Connection, params};
use thiserror::Error;

const SCHEMA_VERSION: i32 = 1;
const DEFAULT_LIST_LIMIT: usize = 10_000;
const MAX_ERROR_MESSAGE_CHARS: usize = 512;

#[derive(Debug, Error)]
pub enum HistoryError {
    #[error("history database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("history database is poisoned")]
    Poisoned,
    #[error("invalid history status value {0}")]
    InvalidStatus(String),
    #[error("invalid history source kind {0}")]
    InvalidSourceKind(String),
    #[error("invalid profile id in history: {0}")]
    InvalidProfileId(String),
    #[error("invalid gid in history: {0}")]
    InvalidGid(String),
}

/// SQLite-backed task history under the application data directory.
pub struct SqliteHistoryStore {
    path: PathBuf,
    connection: Mutex<Connection>,
}

impl SqliteHistoryStore {
    /// Opens or creates `history.sqlite` at `path`, applying migrations.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, HistoryError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                HistoryError::Database(rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
            })?;
        }
        let connection = Connection::open(&path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;
            ",
        )?;
        migrate(&connection)?;
        Ok(Self {
            path,
            connection: Mutex::new(connection),
        })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn with_connection<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, HistoryError>,
    ) -> Result<T, HistoryError> {
        let connection = self.connection.lock().map_err(|_| HistoryError::Poisoned)?;
        f(&connection)
    }
}

impl TaskHistoryStore for SqliteHistoryStore {
    fn upsert(
        &self,
        record: &HistoryRecord,
    ) -> Result<(), ariadeck_application::HistoryStoreError> {
        self.with_connection(|connection| upsert_record(connection, record))
            .map_err(Into::into)
    }

    fn remove(
        &self,
        profile_id: ProfileId,
        gid: Gid,
    ) -> Result<(), ariadeck_application::HistoryStoreError> {
        self.with_connection(|connection| {
            connection.execute(
                "DELETE FROM task_history WHERE profile_id = ?1 AND gid = ?2",
                params![profile_id.to_string(), gid.to_string()],
            )?;
            Ok(())
        })
        .map_err(Into::into)
    }

    fn list(
        &self,
        profile_id: ProfileId,
        limit: usize,
    ) -> Result<Vec<HistoryRecord>, ariadeck_application::HistoryStoreError> {
        let limit = if limit == 0 {
            DEFAULT_LIST_LIMIT
        } else {
            limit.min(DEFAULT_LIST_LIMIT)
        };
        self.with_connection(|connection| list_records(connection, profile_id, limit))
            .map_err(Into::into)
    }

    fn count(
        &self,
        profile_id: ProfileId,
    ) -> Result<usize, ariadeck_application::HistoryStoreError> {
        self.with_connection(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM task_history WHERE profile_id = ?1",
                params![profile_id.to_string()],
                |row| row.get(0),
            )?;
            Ok(count.max(0) as usize)
        })
        .map_err(Into::into)
    }
}

impl From<HistoryError> for ariadeck_application::HistoryStoreError {
    fn from(error: HistoryError) -> Self {
        ariadeck_application::HistoryStoreError::new(error.to_string())
    }
}

fn migrate(connection: &Connection) -> Result<(), HistoryError> {
    let version: i32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        tracing::warn!(
            found = version,
            supported = SCHEMA_VERSION,
            "history database is newer than this build; leaving schema unchanged"
        );
        return Ok(());
    }
    if version < 1 {
        connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS task_history (
                profile_id TEXT NOT NULL,
                gid TEXT NOT NULL,
                status TEXT NOT NULL,
                display_name TEXT NOT NULL,
                directory TEXT,
                info_hash TEXT,
                source_kind TEXT NOT NULL,
                total_length INTEGER NOT NULL,
                completed_length INTEGER NOT NULL,
                error_code INTEGER,
                error_message TEXT,
                primary_uri_redacted TEXT,
                recorded_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (profile_id, gid)
            );
            CREATE INDEX IF NOT EXISTS idx_task_history_profile_recorded
                ON task_history (profile_id, recorded_at DESC);
            CREATE INDEX IF NOT EXISTS idx_task_history_profile_status
                ON task_history (profile_id, status, recorded_at DESC);
            ",
        )?;
        connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    Ok(())
}

fn upsert_record(connection: &Connection, record: &HistoryRecord) -> Result<(), HistoryError> {
    let status = status_to_sql(record.status)?;
    let source_kind = source_kind_to_sql(record.source_kind);
    let directory = record.directory.as_ref().map(EnginePath::as_str);
    let error_code = record
        .error
        .as_ref()
        .and_then(|error| error.code.map(i64::from));
    let error_message = record
        .error
        .as_ref()
        .map(|error| truncate_chars(&error.message, MAX_ERROR_MESSAGE_CHARS));
    let primary_uri = record
        .primary_uri_redacted
        .as_deref()
        .map(|uri| {
            let redacted = redact_source_uri(uri);
            truncate_chars(&redacted, MAX_ERROR_MESSAGE_CHARS)
        })
        .filter(|uri| !uri.is_empty());

    connection.execute(
        "
        INSERT INTO task_history (
            profile_id, gid, status, display_name, directory, info_hash, source_kind,
            total_length, completed_length, error_code, error_message, primary_uri_redacted,
            recorded_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            ?8, ?9, ?10, ?11, ?12,
            ?13, ?14
        )
        ON CONFLICT(profile_id, gid) DO UPDATE SET
            status = excluded.status,
            display_name = excluded.display_name,
            directory = excluded.directory,
            info_hash = excluded.info_hash,
            source_kind = excluded.source_kind,
            total_length = excluded.total_length,
            completed_length = excluded.completed_length,
            error_code = excluded.error_code,
            error_message = excluded.error_message,
            primary_uri_redacted = excluded.primary_uri_redacted,
            updated_at = excluded.updated_at,
            recorded_at = MIN(task_history.recorded_at, excluded.recorded_at)
        ",
        params![
            record.profile_id.to_string(),
            record.gid.to_string(),
            status,
            record.display_name,
            directory,
            record.info_hash,
            source_kind,
            record.total_length.get() as i64,
            record.completed_length.get() as i64,
            error_code,
            error_message,
            primary_uri,
            record.recorded_at_ms as i64,
            record.updated_at_ms as i64,
        ],
    )?;
    Ok(())
}

fn list_records(
    connection: &Connection,
    profile_id: ProfileId,
    limit: usize,
) -> Result<Vec<HistoryRecord>, HistoryError> {
    let mut statement = connection.prepare(
        "
        SELECT profile_id, gid, status, display_name, directory, info_hash, source_kind,
               total_length, completed_length, error_code, error_message, primary_uri_redacted,
               recorded_at, updated_at
        FROM task_history
        WHERE profile_id = ?1
        ORDER BY recorded_at DESC, gid DESC
        LIMIT ?2
        ",
    )?;
    let rows = statement.query_map(params![profile_id.to_string(), limit as i64], |row| {
        Ok(RawHistoryRow {
            profile_id: row.get(0)?,
            gid: row.get(1)?,
            status: row.get(2)?,
            display_name: row.get(3)?,
            directory: row.get(4)?,
            info_hash: row.get(5)?,
            source_kind: row.get(6)?,
            total_length: row.get(7)?,
            completed_length: row.get(8)?,
            error_code: row.get(9)?,
            error_message: row.get(10)?,
            primary_uri_redacted: row.get(11)?,
            recorded_at: row.get(12)?,
            updated_at: row.get(13)?,
        })
    })?;

    let mut records = Vec::new();
    for row in rows {
        let raw = row?;
        records.push(raw.into_record()?);
    }
    Ok(records)
}

struct RawHistoryRow {
    profile_id: String,
    gid: String,
    status: String,
    display_name: String,
    directory: Option<String>,
    info_hash: Option<String>,
    source_kind: String,
    total_length: i64,
    completed_length: i64,
    error_code: Option<i64>,
    error_message: Option<String>,
    primary_uri_redacted: Option<String>,
    recorded_at: i64,
    updated_at: i64,
}

impl RawHistoryRow {
    fn into_record(self) -> Result<HistoryRecord, HistoryError> {
        let profile_id = self
            .profile_id
            .parse()
            .map_err(|_| HistoryError::InvalidProfileId(self.profile_id.clone()))?;
        let gid = self
            .gid
            .parse()
            .map_err(|_| HistoryError::InvalidGid(self.gid.clone()))?;
        Ok(HistoryRecord {
            profile_id,
            gid,
            status: status_from_sql(&self.status)?,
            display_name: self.display_name,
            directory: self.directory.map(EnginePath::new),
            info_hash: self.info_hash,
            source_kind: source_kind_from_sql(&self.source_kind)?,
            total_length: ByteCount::new(self.total_length.max(0) as u64),
            completed_length: ByteCount::new(self.completed_length.max(0) as u64),
            error: self.error_message.map(|message| TaskError {
                code: self.error_code.and_then(|code| u32::try_from(code).ok()),
                message,
            }),
            primary_uri_redacted: self.primary_uri_redacted,
            recorded_at_ms: self.recorded_at.max(0) as u64,
            updated_at_ms: self.updated_at.max(0) as u64,
        })
    }
}

fn status_to_sql(status: DownloadStatus) -> Result<&'static str, HistoryError> {
    match status {
        DownloadStatus::Complete => Ok("complete"),
        DownloadStatus::Error => Ok("error"),
        other => Err(HistoryError::InvalidStatus(format!("{other:?}"))),
    }
}

fn status_from_sql(value: &str) -> Result<DownloadStatus, HistoryError> {
    match value {
        "complete" => Ok(DownloadStatus::Complete),
        "error" => Ok(DownloadStatus::Error),
        other => Err(HistoryError::InvalidStatus(other.to_owned())),
    }
}

fn source_kind_to_sql(kind: TaskSourceKind) -> &'static str {
    match kind {
        TaskSourceKind::Unknown => "unknown",
        TaskSourceKind::DirectUri => "direct_uri",
        TaskSourceKind::Magnet => "magnet",
        TaskSourceKind::BitTorrent => "bittorrent",
        TaskSourceKind::Metalink => "metalink",
    }
}

fn source_kind_from_sql(value: &str) -> Result<TaskSourceKind, HistoryError> {
    match value {
        "unknown" => Ok(TaskSourceKind::Unknown),
        "direct_uri" => Ok(TaskSourceKind::DirectUri),
        "magnet" => Ok(TaskSourceKind::Magnet),
        "bittorrent" => Ok(TaskSourceKind::BitTorrent),
        "metalink" => Ok(TaskSourceKind::Metalink),
        other => Err(HistoryError::InvalidSourceKind(other.to_owned())),
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    value.chars().take(max_chars).collect()
}

/// Builds a durable history record from a live/stopped domain task.
#[must_use]
pub fn history_record_from_task(
    profile_id: ProfileId,
    task: &ariadeck_domain::DownloadTask,
    now_ms: u64,
) -> Option<HistoryRecord> {
    if !matches!(
        task.status,
        DownloadStatus::Complete | DownloadStatus::Error
    ) {
        return None;
    }
    Some(HistoryRecord {
        profile_id,
        gid: task.gid,
        status: task.status,
        display_name: task.display_name.clone(),
        directory: task.metadata.directory.clone(),
        info_hash: task.metadata.info_hash.clone(),
        source_kind: task.metadata.source_kind,
        total_length: task.total_length,
        completed_length: task.completed_length,
        error: task.error.clone(),
        primary_uri_redacted: task
            .metadata
            .primary_uri
            .as_deref()
            .map(redact_source_uri)
            .filter(|uri| !uri.is_empty()),
        recorded_at_ms: now_ms,
        updated_at_ms: now_ms,
    })
}

/// Same as [`history_record_from_task`] for adapter snapshots.
#[must_use]
pub fn history_record_from_snapshot(
    profile_id: ProfileId,
    snapshot: &ariadeck_domain::TaskSnapshot,
    now_ms: u64,
) -> Option<HistoryRecord> {
    if !matches!(
        snapshot.status,
        DownloadStatus::Complete | DownloadStatus::Error
    ) {
        return None;
    }
    Some(HistoryRecord {
        profile_id,
        gid: snapshot.gid,
        status: snapshot.status,
        display_name: snapshot.display_name.clone(),
        directory: snapshot.metadata.directory.clone(),
        info_hash: snapshot.metadata.info_hash.clone(),
        source_kind: snapshot.metadata.source_kind,
        total_length: snapshot.total_length,
        completed_length: snapshot.completed_length,
        error: snapshot.error.clone(),
        primary_uri_redacted: snapshot
            .metadata
            .primary_uri
            .as_deref()
            .map(redact_source_uri)
            .filter(|uri| !uri.is_empty()),
        recorded_at_ms: now_ms,
        updated_at_ms: now_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariadeck_domain::{DownloadTask, TaskSnapshot};
    use tempfile::TempDir;

    fn sample_task(gid: u64, status: DownloadStatus, name: &str) -> DownloadTask {
        let mut task =
            DownloadTask::from_snapshot(TaskSnapshot::new(Gid::from_u64(gid), status, name));
        task.metadata.directory = Some(EnginePath::new("C:/Downloads"));
        task.metadata.primary_uri =
            Some("https://user:secret@cdn.example/file.bin?token=abc#frag".into());
        task.metadata.info_hash = Some("0123456789abcdef0123456789abcdef01234567".into());
        task.total_length = ByteCount::new(1024);
        task.completed_length = ByteCount::new(1024);
        if status == DownloadStatus::Error {
            task.error = Some(TaskError {
                code: Some(1),
                message: "download failed".into(),
            });
        }
        task
    }

    #[test]
    fn upsert_list_and_remove_are_profile_scoped() {
        let dir = TempDir::new().expect("tempdir");
        let store = SqliteHistoryStore::open(dir.path().join("history.sqlite")).expect("open");
        let profile_a = ProfileId::new();
        let profile_b = ProfileId::new();
        let task = sample_task(1, DownloadStatus::Complete, "one.bin");
        let record = history_record_from_task(profile_a, &task, 100).expect("record");
        store.upsert(&record).expect("upsert");
        store
            .upsert(
                &history_record_from_task(
                    profile_b,
                    &sample_task(2, DownloadStatus::Error, "two.bin"),
                    200,
                )
                .expect("record"),
            )
            .expect("upsert b");

        let listed = store.list(profile_a, 10).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].gid, Gid::from_u64(1));
        assert_eq!(
            listed[0].primary_uri_redacted.as_deref(),
            Some("https://cdn.example/file.bin")
        );
        assert!(
            !listed[0]
                .primary_uri_redacted
                .as_deref()
                .unwrap_or_default()
                .contains("secret")
        );
        assert_eq!(store.count(profile_a).expect("count"), 1);

        store.remove(profile_a, Gid::from_u64(1)).expect("remove");
        assert!(store.list(profile_a, 10).expect("list").is_empty());
        assert_eq!(store.count(profile_b).expect("count b"), 1);
    }

    #[test]
    fn upsert_is_idempotent_and_keeps_earliest_recorded_at() {
        let dir = TempDir::new().expect("tempdir");
        let store = SqliteHistoryStore::open(dir.path().join("history.sqlite")).expect("open");
        let profile = ProfileId::new();
        let mut first = history_record_from_task(
            profile,
            &sample_task(9, DownloadStatus::Complete, "file.bin"),
            1000,
        )
        .expect("record");
        store.upsert(&first).expect("upsert");
        first.display_name = "file-renamed.bin".into();
        first.updated_at_ms = 2000;
        first.recorded_at_ms = 1500;
        store.upsert(&first).expect("upsert again");
        let listed = store.list(profile, 10).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].display_name, "file-renamed.bin");
        assert_eq!(listed[0].recorded_at_ms, 1000);
        assert_eq!(listed[0].updated_at_ms, 2000);
    }

    #[test]
    fn active_tasks_are_not_recorded() {
        let task = sample_task(3, DownloadStatus::Active, "live.bin");
        assert!(history_record_from_task(ProfileId::new(), &task, 1).is_none());
    }

    #[test]
    fn open_recovers_missing_parent_directories() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("nested").join("history.sqlite");
        let store = SqliteHistoryStore::open(&path).expect("open");
        assert_eq!(store.path(), path.as_path());
        assert_eq!(store.count(ProfileId::new()).expect("count"), 0);
    }
}
