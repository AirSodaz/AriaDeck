use std::{
    fs,
    path::{Path, PathBuf},
};

use uuid::Uuid;

use crate::{EngineError, error::io_error, profiles::create_runtime_file};

/// Outcome of preparing the aria2 session file before spawn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionFilePreparation {
    pub path: PathBuf,
    pub recovered_from_corruption: bool,
    pub backup_path: Option<PathBuf>,
}

/// Ensure `session_path` exists and is a plausible aria2 session text file.
///
/// Corrupt contents are moved aside and replaced with an empty session so the
/// managed engine can start. The original bytes are preserved for diagnosis.
pub fn prepare_session_file(session_path: &Path) -> Result<SessionFilePreparation, EngineError> {
    if !session_path.exists() {
        create_runtime_file(session_path)?;
        return Ok(SessionFilePreparation {
            path: session_path.to_path_buf(),
            recovered_from_corruption: false,
            backup_path: None,
        });
    }

    let bytes = fs::read(session_path)
        .map_err(|error| io_error("read the aria2 session file", session_path, error))?;
    if session_bytes_are_plausible(&bytes) {
        return Ok(SessionFilePreparation {
            path: session_path.to_path_buf(),
            recovered_from_corruption: false,
            backup_path: None,
        });
    }

    let backup_path = backup_corrupt_file(session_path, "session")?;
    // Replace with a fresh empty session aria2 can rewrite.
    fs::write(session_path, b"")
        .map_err(|error| io_error("reset the aria2 session file", session_path, error))?;
    Ok(SessionFilePreparation {
        path: session_path.to_path_buf(),
        recovered_from_corruption: true,
        backup_path: Some(backup_path),
    })
}

fn session_bytes_are_plausible(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    // aria2 session files are line-oriented text. Reject NULs and invalid UTF-8.
    if bytes.contains(&0) {
        return false;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    // Reject control characters other than tab/CR/LF.
    !text.chars().any(|ch| {
        let code = ch as u32;
        code < 0x20 && ch != '\t' && ch != '\n' && ch != '\r'
    })
}

pub(crate) fn backup_corrupt_file(path: &Path, kind: &str) -> Result<PathBuf, EngineError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(kind);
    let backup_path = parent.join(format!("{file_name}.corrupt-{}.bak", Uuid::new_v4()));
    fs::rename(path, &backup_path)
        .map_err(|error| io_error("preserve the corrupt runtime file", &backup_path, error))?;
    Ok(backup_path)
}
