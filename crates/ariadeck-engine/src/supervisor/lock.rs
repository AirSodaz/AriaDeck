use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use ariadeck_domain::ProfileId;
use serde::{Deserialize, Serialize};

use crate::{EngineError, OWNERSHIP_LOCK_FILE_NAME, error::io_error};

/// Record written into `.ariadeck-engine.lock` while a managed local engine is live.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct OwnershipLockRecord {
    profile_id: ProfileId,
    owner_pid: u32,
    acquired_unix_secs: u64,
}

/// Held exclusive ownership of one managed profile directory.
///
/// The lock file prevents two AriaDeck instances from sharing the same
/// `--input-file` / `--save-session` path. Stale locks from crashed processes
/// are reclaimed; live owners fail closed with [`EngineError::ProfileAlreadyOwned`].
#[derive(Debug)]
pub struct ProfileOwnershipLock {
    path: PathBuf,
    profile_id: ProfileId,
    owner_pid: u32,
    /// Keeps the exclusive open handle alive on platforms that honor share modes.
    file: Option<File>,
}

impl ProfileOwnershipLock {
    /// Acquire exclusive ownership for `profile_dir`, reclaiming a dead owner.
    pub fn acquire(profile_id: ProfileId, profile_dir: &Path) -> Result<Self, EngineError> {
        fs::create_dir_all(profile_dir)
            .map_err(|error| io_error("create the profile directory", profile_dir, error))?;
        let path = profile_dir.join(OWNERSHIP_LOCK_FILE_NAME);
        match try_create_lock_file(&path) {
            Ok(file) => write_lock_record(file, profile_id, &path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                reclaim_or_reject_existing_lock(profile_id, &path)
            }
            Err(error) => Err(io_error("create the profile ownership lock", &path, error)),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn owner_pid(&self) -> u32 {
        self.owner_pid
    }

    #[must_use]
    pub fn profile_id(&self) -> ProfileId {
        self.profile_id
    }
}

impl Drop for ProfileOwnershipLock {
    fn drop(&mut self) {
        // Close the exclusive handle first so Windows allows unlinking.
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

fn try_create_lock_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        // Allow peers to read `owner_pid` (`FILE_SHARE_READ = 1`) while denying
        // concurrent writers and deletion while this AriaDeck instance is alive.
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1);
    }
    options.open(path)
}

fn write_lock_record(
    mut file: File,
    profile_id: ProfileId,
    path: &Path,
) -> Result<ProfileOwnershipLock, EngineError> {
    let owner_pid = std::process::id();
    let record = OwnershipLockRecord {
        profile_id,
        owner_pid,
        acquired_unix_secs: unix_now_secs(),
    };
    let payload =
        serde_json::to_vec_pretty(&record).map_err(|source| EngineError::Serialize { source })?;
    file.set_len(0)
        .map_err(|error| io_error("truncate the profile ownership lock", path, error))?;
    file.write_all(&payload)
        .map_err(|error| io_error("write the profile ownership lock", path, error))?;
    file.write_all(b"\n")
        .map_err(|error| io_error("finish the profile ownership lock", path, error))?;
    file.flush()
        .map_err(|error| io_error("flush the profile ownership lock", path, error))?;
    Ok(ProfileOwnershipLock {
        path: path.to_path_buf(),
        profile_id,
        owner_pid,
        file: Some(file),
    })
}

fn reclaim_or_reject_existing_lock(
    profile_id: ProfileId,
    path: &Path,
) -> Result<ProfileOwnershipLock, EngineError> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let owner_pid = serde_json::from_str::<OwnershipLockRecord>(&existing)
        .ok()
        .map(|record| record.owner_pid)
        .or_else(|| parse_legacy_lock_pid(&existing));
    let this_pid = std::process::id();

    // Live foreign owner, or a second acquire in this same process (re-entry),
    // must fail closed. Same-pid happens when Windows exclusive share mode
    // still holds the first lock handle open.
    if let Some(owner_pid) = owner_pid
        && (owner_pid == this_pid || process_appears_alive(owner_pid))
    {
        return Err(EngineError::ProfileAlreadyOwned {
            profile_id,
            owner_pid,
            lock_path: path.to_path_buf(),
        });
    }

    // Stale or unreadable lock: remove and recreate.
    // If another process still holds the lock handle, remove/create fails closed.
    if let Err(_error) = fs::remove_file(path) {
        return Err(EngineError::ProfileAlreadyOwned {
            profile_id,
            owner_pid: owner_pid.unwrap_or(0),
            lock_path: path.to_path_buf(),
        });
    }
    let file = try_create_lock_file(path).map_err(|_error| EngineError::ProfileAlreadyOwned {
        profile_id,
        owner_pid: owner_pid.unwrap_or(0),
        lock_path: path.to_path_buf(),
    })?;
    write_lock_record(file, profile_id, path)
}

fn parse_legacy_lock_pid(contents: &str) -> Option<u32> {
    contents
        .lines()
        .find_map(|line| line.trim().parse::<u32>().ok())
}

pub(crate) fn process_appears_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // sysinfo is already a dependency for disk checks. Refresh only the target
    // pid so startup stays cheap.
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let Ok(mut system) = System::new() else {
        return false;
    };
    system.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
    system.process(Pid::from_u32(pid)).is_some()
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
