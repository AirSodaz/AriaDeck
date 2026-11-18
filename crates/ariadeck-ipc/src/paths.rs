use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use data_encoding::HEXLOWER;
use sha2::{Digest, Sha256};

/// Marker file placed next to the executable to enable portable mode (RELEASE-001).
pub const PORTABLE_MARKER_FILE: &str = "ariadeck.portable";

pub(crate) fn socket_label(data_dir: &Path) -> String {
    let digest = Sha256::digest(data_dir.as_os_str().as_encoded_bytes());
    format!("ariadeck-{}", HEXLOWER.encode(&digest[..16]))
}

/// Application data directory used for settings, profiles, cores, and window
/// geometry — and, since the socket label is derived from it, the address of the
/// primary instance.
///
/// This lives here rather than in the desktop crate so the native messaging host
/// resolves the exact same directory; a divergence would silently address a
/// different socket and every forward would report "not running".
///
/// Resolution order:
/// 1. `ARIADECK_DATA_DIR` when set
/// 2. `<exe_dir>/data` when `<exe_dir>/ariadeck.portable` exists
/// 3. `%LOCALAPPDATA%/AriaDeck` (Windows only)
/// 4. `$XDG_DATA_HOME/ariadeck` or `~/.local/share/ariadeck`
/// 5. `./.ariadeck` fallback
#[must_use]
pub fn default_data_dir() -> PathBuf {
    resolve_data_dir(|key| std::env::var_os(key), current_exe_dir().as_deref())
}

fn current_exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

/// Testable data-dir resolver (RELEASE-001).
#[must_use]
pub fn resolve_data_dir(
    mut env_var: impl FnMut(&str) -> Option<OsString>,
    exe_dir: Option<&Path>,
) -> PathBuf {
    if let Some(path) = env_var("ARIADECK_DATA_DIR") {
        return PathBuf::from(path);
    }
    if let Some(exe_dir) = exe_dir {
        let marker = exe_dir.join(PORTABLE_MARKER_FILE);
        if marker.is_file() {
            return exe_dir.join("data");
        }
    }
    #[cfg(windows)]
    if let Some(path) = env_var("LOCALAPPDATA") {
        return PathBuf::from(path).join("AriaDeck");
    }
    if let Some(path) = env_var("XDG_DATA_HOME") {
        return PathBuf::from(path).join("ariadeck");
    }
    if let Some(path) = env_var("HOME") {
        return PathBuf::from(path).join(".local/share/ariadeck");
    }
    PathBuf::from(".ariadeck")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_data_dir_prefers_explicit_env_over_portable_and_os() {
        let root = tempfile::tempdir().expect("temp");
        let explicit = root.path().join("custom-data");
        let exe_dir = root.path().join("app");
        std::fs::create_dir_all(&exe_dir).expect("exe dir");
        std::fs::write(exe_dir.join(PORTABLE_MARKER_FILE), b"").expect("marker");
        let local_app = root.path().join("LocalAppData");

        let resolved = resolve_data_dir(
            |key| match key {
                "ARIADECK_DATA_DIR" => Some(explicit.as_os_str().to_owned()),
                "LOCALAPPDATA" => Some(local_app.as_os_str().to_owned()),
                _ => None,
            },
            Some(exe_dir.as_path()),
        );
        assert_eq!(resolved, explicit);
    }

    #[test]
    fn resolve_data_dir_uses_portable_data_when_marker_present() {
        let root = tempfile::tempdir().expect("temp");
        let exe_dir = root.path().join("portable-app");
        std::fs::create_dir_all(&exe_dir).expect("exe dir");
        std::fs::write(exe_dir.join(PORTABLE_MARKER_FILE), b"").expect("marker");
        let local_app = root.path().join("LocalAppData");

        let resolved = resolve_data_dir(
            |key| match key {
                "LOCALAPPDATA" => Some(local_app.as_os_str().to_owned()),
                _ => None,
            },
            Some(exe_dir.as_path()),
        );
        assert_eq!(resolved, exe_dir.join("data"));
    }

    #[test]
    fn resolve_data_dir_uses_platform_default_without_marker() {
        let root = tempfile::tempdir().expect("temp");
        let exe_dir = root.path().join("installed-app");
        std::fs::create_dir_all(&exe_dir).expect("exe dir");
        let local_app = root.path().join("LocalAppData");
        let xdg_data = root.path().join("XdgData");

        let resolved = resolve_data_dir(
            |key| match key {
                "LOCALAPPDATA" => Some(local_app.as_os_str().to_owned()),
                "XDG_DATA_HOME" => Some(xdg_data.as_os_str().to_owned()),
                _ => None,
            },
            Some(exe_dir.as_path()),
        );
        #[cfg(windows)]
        assert_eq!(resolved, local_app.join("AriaDeck"));
        #[cfg(not(windows))]
        assert_eq!(resolved, xdg_data.join("ariadeck"));
    }

    /// The host and the desktop must land on the same socket for the same data
    /// directory; a divergence would look like "AriaDeck is not running".
    #[test]
    fn socket_label_is_stable_and_data_dir_scoped() {
        let first = Path::new("/tmp/ariadeck-a");
        let second = Path::new("/tmp/ariadeck-b");
        assert_eq!(socket_label(first), socket_label(first));
        assert_ne!(socket_label(first), socket_label(second));
        assert_eq!(socket_label(first).len(), "ariadeck-".len() + 32);
    }
}
