//! Atomic file persistence helper shared across profile and core stores.

use std::{
    fs::{self, File},
    io::Write,
    path::Path,
};

use tempfile::Builder;

use crate::{EngineError, io_error};

/// Atomically write `payload` (optionally followed by `\n`) to `path` via a
/// unique temporary file in the same directory, fsynced before replacement.
///
/// Uses `NamedTempFile::persist`, which invokes `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`
/// on Windows without deleting `path` first.
pub(crate) fn write_atomic_bytes(
    path: &Path,
    payload: &[u8],
    trailing_newline: bool,
    dir_op: &'static str,
    write_op: &'static str,
    replace_op: &'static str,
) -> Result<(), EngineError> {
    let Some(file_name) = path.file_name() else {
        return Err(EngineError::InvalidStorePath {
            path: path.to_path_buf(),
        });
    };
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| io_error(dir_op, parent, source))?;

    let prefix = format!(".{}.", file_name.to_string_lossy());
    let mut temp_file = Builder::new()
        .prefix(&prefix)
        .suffix(".tmp")
        .tempfile_in(parent)
        .map_err(|source| io_error(write_op, parent, source))?;
    let temp_path = temp_file.path().to_path_buf();

    temp_file
        .as_file_mut()
        .write_all(payload)
        .and_then(|()| {
            if trailing_newline {
                temp_file.as_file_mut().write_all(b"\n")
            } else {
                Ok(())
            }
        })
        .and_then(|()| temp_file.as_file_mut().flush())
        .and_then(|()| temp_file.as_file_mut().sync_all())
        .map_err(|source| io_error(write_op, &temp_path, source))?;

    temp_file
        .persist(path)
        .map(|_| ())
        .map_err(|error| io_error(replace_op, path, error.error))?;

    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}
