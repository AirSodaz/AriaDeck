//! Shared atomic JSON persistence helper for settings, window geometry, and
//! per-profile engine environments.

use std::{fs, io::Write, path::Path};

use tempfile::Builder;

use crate::{SettingsError, error::io_error};

/// Atomically write `payload` (plus trailing newline) to `path` using a unique
/// temporary file in the same directory, fsyncing before atomically replacing
/// the target via `NamedTempFile::persist` (which uses `MoveFileExW` with
/// `MOVEFILE_REPLACE_EXISTING` on Windows without pre-deleting the target).
pub(crate) fn write_atomic_json(
    path: &Path,
    payload: &[u8],
    create_dir_op: &'static str,
    temp_op: &'static str,
    write_op: &'static str,
    sync_op: &'static str,
    replace_op: &'static str,
) -> Result<(), SettingsError> {
    let Some(file_name) = path.file_name() else {
        return Err(SettingsError::InvalidStorePath {
            path: path.to_path_buf(),
        });
    };
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| io_error(create_dir_op, parent, source))?;

    let prefix = format!(".{}.", file_name.to_string_lossy());
    let mut temp_file = Builder::new()
        .prefix(&prefix)
        .suffix(".tmp")
        .tempfile_in(parent)
        .map_err(|source| io_error(temp_op, parent, source))?;
    let temp_path = temp_file.path().to_path_buf();

    temp_file
        .as_file_mut()
        .write_all(payload)
        .and_then(|()| temp_file.as_file_mut().write_all(b"\n"))
        .map_err(|source| io_error(write_op, &temp_path, source))?;
    temp_file
        .as_file_mut()
        .flush()
        .and_then(|()| temp_file.as_file_mut().sync_all())
        .map_err(|source| io_error(sync_op, &temp_path, source))?;

    temp_file
        .persist(path)
        .map(|_| ())
        .map_err(|error| io_error(replace_op, path, error.error))?;
    if let Ok(dir) = fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}
