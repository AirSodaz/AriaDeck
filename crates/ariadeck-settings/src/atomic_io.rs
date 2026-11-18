//! Shared atomic JSON persistence helper for settings, window geometry, and
//! per-profile engine environments.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
};

use uuid::Uuid;

use crate::SettingsError;

/// Atomically write `payload` (plus trailing newline) to `path` using a unique
/// temporary file in the same directory, fsyncing and closing the handle before
/// replacing the target.
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

    let temp_path = parent.join(format!(
        ".{}.{}.tmp",
        file_name.to_string_lossy(),
        Uuid::new_v4()
    ));

    let result = (|| -> Result<(), SettingsError> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .map_err(|source| io_error(temp_op, &temp_path, source))?;
        file.write_all(payload)
            .and_then(|()| file.write_all(b"\n"))
            .map_err(|source| io_error(write_op, &temp_path, source))?;
        file.flush()
            .and_then(|()| file.sync_all())
            .map_err(|source| io_error(sync_op, &temp_path, source))?;
        // Ensure the temporary file handle is closed before rename on Windows.
        drop(file);
        replace_file(&temp_path, path).map_err(|source| io_error(replace_op, path, source))
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

#[cfg(windows)]
fn replace_file(temp_path: &Path, target_path: &Path) -> io::Result<()> {
    match fs::rename(temp_path, target_path) {
        Ok(()) => Ok(()),
        Err(first_error) if target_path.exists() => {
            fs::remove_file(target_path)?;
            fs::rename(temp_path, target_path).or(Err(first_error))
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(windows))]
fn replace_file(temp_path: &Path, target_path: &Path) -> io::Result<()> {
    fs::rename(temp_path, target_path)
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> SettingsError {
    SettingsError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}
