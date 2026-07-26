//! Local aria2 discovery (B4).
//!
//! Scans the conventional places an `aria2c` binary ends up on each platform and
//! probes every hit with `--version`. Nothing here touches the network: the
//! result is a list of executables the user already has, ready to be imported or
//! linked into the managed core registry (`cores.rs`).
//!
//! Probing spawns a child process per candidate, so callers must run this off
//! the render path (the desktop layer uses `spawn_blocking`).

use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
};

use crate::cores::{Aria2Probe, probe_aria2};

/// Where a discovered `aria2c` came from, so the UI can explain the row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreDiscoveryOrigin {
    /// `ARIADECK_ARIA2C_PATH` is set for this process.
    EnvironmentOverride,
    /// Found by walking `PATH`.
    SearchPath,
    /// Scoop app directory under the user profile.
    Scoop,
    /// WinGet package links directory.
    WinGet,
    /// Chocolatey shim directory.
    Chocolatey,
    /// Homebrew prefix (macOS).
    Homebrew,
    /// A conventional install root (`Program Files`, `/usr/bin`, ...).
    SystemInstall,
    /// Sitting next to (or under) the AriaDeck executable — portable layout.
    Portable,
}

impl CoreDiscoveryOrigin {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::EnvironmentOverride => "ARIADECK_ARIA2C_PATH",
            Self::SearchPath => "PATH",
            Self::Scoop => "Scoop",
            Self::WinGet => "WinGet",
            Self::Chocolatey => "Chocolatey",
            Self::Homebrew => "Homebrew",
            Self::SystemInstall => "System install",
            Self::Portable => "Portable",
        }
    }

    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::EnvironmentOverride => "core-origin-environment",
            Self::SearchPath => "core-origin-path",
            Self::Scoop => "core-origin-scoop",
            Self::WinGet => "core-origin-winget",
            Self::Chocolatey => "core-origin-chocolatey",
            Self::Homebrew => "core-origin-homebrew",
            Self::SystemInstall => "core-origin-system",
            Self::Portable => "core-origin-portable",
        }
    }
}

/// One `aria2c` that exists on this machine and answered `--version`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredCore {
    pub path: PathBuf,
    pub origin: CoreDiscoveryOrigin,
    pub version: String,
    pub features: Vec<String>,
}

/// Executable name to look for on this platform.
#[must_use]
pub const fn aria2_executable_name() -> &'static str {
    if cfg!(windows) {
        "aria2c.exe"
    } else {
        "aria2c"
    }
}

/// Ordered candidate paths, most specific source first.
///
/// Pure over its inputs so the ordering and platform rules stay testable without
/// touching the real filesystem or environment.
#[must_use]
pub fn candidate_paths(
    mut env_var: impl FnMut(&str) -> Option<OsString>,
    executable_dir: Option<&Path>,
) -> Vec<(PathBuf, CoreDiscoveryOrigin)> {
    let name = aria2_executable_name();
    let mut candidates: Vec<(PathBuf, CoreDiscoveryOrigin)> = Vec::new();

    if let Some(path) = env_var("ARIADECK_ARIA2C_PATH") {
        candidates.push((
            PathBuf::from(path),
            CoreDiscoveryOrigin::EnvironmentOverride,
        ));
    }

    // Portable layout: shipped beside the app, or in an `aria2` subfolder.
    if let Some(dir) = executable_dir {
        candidates.push((dir.join(name), CoreDiscoveryOrigin::Portable));
        candidates.push((dir.join("aria2").join(name), CoreDiscoveryOrigin::Portable));
    }

    // Package managers keep stable, well-known layouts; check them before PATH so
    // the UI can name the manager instead of showing a bare shim.
    let home = env_var("USERPROFILE").or_else(|| env_var("HOME"));
    if let Some(home) = home.as_ref().map(PathBuf::from) {
        candidates.push((
            home.join("scoop")
                .join("apps")
                .join("aria2")
                .join("current")
                .join(name),
            CoreDiscoveryOrigin::Scoop,
        ));
    }
    if let Some(local_app_data) = env_var("LOCALAPPDATA").map(PathBuf::from) {
        candidates.push((
            local_app_data
                .join("Microsoft")
                .join("WinGet")
                .join("Links")
                .join(name),
            CoreDiscoveryOrigin::WinGet,
        ));
    }
    if let Some(choco) = env_var("ChocolateyInstall").map(PathBuf::from) {
        candidates.push((
            choco.join("bin").join(name),
            CoreDiscoveryOrigin::Chocolatey,
        ));
    }

    if cfg!(windows) {
        for key in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(root) = env_var(key).map(PathBuf::from) {
                candidates.push((
                    root.join("aria2").join(name),
                    CoreDiscoveryOrigin::SystemInstall,
                ));
            }
        }
        candidates.push((
            PathBuf::from(r"C:\ProgramData\chocolatey\bin").join(name),
            CoreDiscoveryOrigin::Chocolatey,
        ));
    } else {
        if cfg!(target_os = "macos") {
            for root in ["/opt/homebrew/bin", "/usr/local/bin"] {
                candidates.push((
                    PathBuf::from(root).join(name),
                    CoreDiscoveryOrigin::Homebrew,
                ));
            }
        }
        for root in ["/usr/bin", "/usr/local/bin", "/bin", "/snap/bin"] {
            candidates.push((
                PathBuf::from(root).join(name),
                CoreDiscoveryOrigin::SystemInstall,
            ));
        }
    }

    // PATH last: anything it resolves to that we already named above is dropped by
    // the dedup in `discover_cores`, so managers keep their friendlier label.
    if let Some(path_var) = env_var("PATH") {
        for entry in std::env::split_paths(&path_var) {
            if entry.as_os_str().is_empty() {
                continue;
            }
            candidates.push((entry.join(name), CoreDiscoveryOrigin::SearchPath));
        }
    }

    candidates
}

/// Discover every usable `aria2c` on this machine.
///
/// Each surviving candidate answered `--version`; entries that fail to spawn or
/// parse are dropped rather than surfaced as broken rows, because the user did
/// not ask for them by name.
#[must_use]
pub fn discover_cores() -> Vec<DiscoveredCore> {
    let executable_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let candidates = candidate_paths(|key| std::env::var_os(key), executable_dir.as_deref());
    probe_candidates(candidates)
}

/// Probe an explicit candidate list. Split out so tests can drive it directly.
#[must_use]
pub fn probe_candidates(candidates: Vec<(PathBuf, CoreDiscoveryOrigin)>) -> Vec<DiscoveredCore> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut discovered = Vec::new();
    for (path, origin) in candidates {
        if !path.is_file() {
            continue;
        }
        // Canonicalize so a scoop shim reached through PATH does not appear twice.
        let identity = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if !seen.insert(identity) {
            continue;
        }
        let Ok(Aria2Probe {
            version, features, ..
        }) = probe_aria2(&path)
        else {
            continue;
        };
        discovered.push(DiscoveredCore {
            path,
            origin,
            version,
            features,
        });
    }
    discovered
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_from(pairs: &[(&str, &str)]) -> impl FnMut(&str) -> Option<OsString> + use<> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), OsString::from(*value)))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    #[test]
    fn environment_override_is_offered_first() {
        let candidates = candidate_paths(
            env_from(&[("ARIADECK_ARIA2C_PATH", "/custom/aria2c")]),
            None,
        );
        assert_eq!(candidates[0].0, PathBuf::from("/custom/aria2c"));
        assert_eq!(candidates[0].1, CoreDiscoveryOrigin::EnvironmentOverride);
    }

    #[test]
    fn portable_layout_precedes_package_managers() {
        let executable_dir = PathBuf::from("/apps/ariadeck");
        let candidates = candidate_paths(
            env_from(&[("HOME", "/home/user"), ("USERPROFILE", "/home/user")]),
            Some(&executable_dir),
        );
        let portable = candidates
            .iter()
            .position(|(_, origin)| *origin == CoreDiscoveryOrigin::Portable)
            .expect("portable candidate");
        let scoop = candidates
            .iter()
            .position(|(_, origin)| *origin == CoreDiscoveryOrigin::Scoop)
            .expect("scoop candidate");
        assert!(portable < scoop);
        assert_eq!(
            candidates[portable].0,
            executable_dir.join(aria2_executable_name())
        );
    }

    #[test]
    fn path_entries_are_expanded_and_come_last() {
        let joined =
            std::env::join_paths([PathBuf::from("/opt/tools"), PathBuf::from("/opt/more")])
                .expect("join paths");
        let joined = joined.to_string_lossy().into_owned();
        let candidates = candidate_paths(env_from(&[("PATH", joined.as_str())]), None);
        let path_entries: Vec<_> = candidates
            .iter()
            .filter(|(_, origin)| *origin == CoreDiscoveryOrigin::SearchPath)
            .map(|(path, _)| path.clone())
            .collect();
        assert_eq!(
            path_entries,
            vec![
                PathBuf::from("/opt/tools").join(aria2_executable_name()),
                PathBuf::from("/opt/more").join(aria2_executable_name()),
            ]
        );
        let first_path_entry = candidates
            .iter()
            .position(|(_, origin)| *origin == CoreDiscoveryOrigin::SearchPath)
            .expect("path candidate");
        assert!(
            candidates[first_path_entry..]
                .iter()
                .all(|(_, origin)| *origin == CoreDiscoveryOrigin::SearchPath),
            "PATH candidates must be last so package managers keep their labels"
        );
    }

    #[test]
    fn missing_candidates_are_skipped_without_probing() {
        let discovered = probe_candidates(vec![(
            PathBuf::from("/definitely/not/here/aria2c"),
            CoreDiscoveryOrigin::SearchPath,
        )]);
        assert!(discovered.is_empty());
    }

    #[test]
    fn non_aria2_files_are_dropped_rather_than_surfaced() {
        let dir = std::env::temp_dir().join(format!("ariadeck-discovery-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let fake = dir.join("not-aria2.txt");
        std::fs::write(&fake, b"hello").expect("write fake");
        let discovered = probe_candidates(vec![(fake, CoreDiscoveryOrigin::SearchPath)]);
        assert!(discovered.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
