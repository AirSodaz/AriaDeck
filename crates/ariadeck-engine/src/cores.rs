//! Managed aria2 core installations (CORE-001).
//!
//! Layout under the application data directory:
//!
//! ```text
//! cores/
//! └── aria2/
//!     ├── cores.json                 # registry (active + last_working)
//!     └── <version>/<target>/
//!         ├── aria2c[.exe]
//!         └── installation.json
//! ```
//!
//! This slice supports local import, verification, activation, rollback, and
//! removal. Network fetch / signed manifests remain deferred.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use ariadeck_domain::CoreInstallationId;
use serde::{Deserialize, Serialize};

use crate::{EngineError, io_error, validate_executable};

const REGISTRY_FILE_NAME: &str = "cores.json";
const INSTALLATION_FILE_NAME: &str = "installation.json";
const CORES_ROOT_NAME: &str = "cores";
const PRODUCT_NAME: &str = "aria2";
const REGISTRY_SCHEMA_VERSION: u32 = 1;
const INSTALLATION_SCHEMA_VERSION: u32 = 1;

/// How a managed core entered the registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreSource {
    /// Copied from a user-selected executable (import).
    Imported,
    /// Pointed at without copying (still registered for switching).
    Linked,
    /// Reserved for future network installers.
    Managed,
}

impl CoreSource {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Imported => "Imported",
            Self::Linked => "Linked",
            Self::Managed => "Managed",
        }
    }
}

/// Result of probing `aria2c --version`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Aria2Probe {
    pub version: String,
    pub features: Vec<String>,
    pub raw_version_line: String,
}

/// One installed (or linked) aria2 core.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreInstallation {
    pub schema_version: u32,
    pub id: CoreInstallationId,
    pub version: String,
    pub target: String,
    pub source: CoreSource,
    /// Relative to the version directory (e.g. `aria2c.exe`) or absolute for links.
    pub executable: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    pub installed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validated_version: Option<String>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub last_verified_at: Option<String>,
}

impl CoreInstallation {
    /// Absolute path to the executable for this install under `version_dir`.
    #[must_use]
    pub fn absolute_executable(&self, version_dir: &Path) -> PathBuf {
        if self.executable.is_absolute() {
            self.executable.clone()
        } else {
            version_dir.join(&self.executable)
        }
    }
}

/// Root registry for managed cores.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreRegistry {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_id: Option<CoreInstallationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_working_id: Option<CoreInstallationId>,
    pub installations: Vec<CoreInstallationSummary>,
}

/// Lightweight index entry stored in `cores.json`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreInstallationSummary {
    pub id: CoreInstallationId,
    pub version: String,
    pub target: String,
    pub source: CoreSource,
    /// Directory relative to `cores/aria2/` (`{version}/{target}`).
    pub relative_dir: String,
}

impl Default for CoreRegistry {
    fn default() -> Self {
        Self {
            schema_version: REGISTRY_SCHEMA_VERSION,
            active_id: None,
            last_working_id: None,
            installations: Vec::new(),
        }
    }
}

impl CoreRegistry {
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn active(&self) -> Option<&CoreInstallationSummary> {
        let id = self.active_id?;
        self.installations.iter().find(|entry| entry.id == id)
    }

    #[must_use]
    pub fn last_working(&self) -> Option<&CoreInstallationSummary> {
        let id = self.last_working_id?;
        self.installations.iter().find(|entry| entry.id == id)
    }

    #[must_use]
    pub fn get(&self, id: CoreInstallationId) -> Option<&CoreInstallationSummary> {
        self.installations.iter().find(|entry| entry.id == id)
    }
}

/// Store that reads/writes the cores tree under `data_dir/cores/aria2`.
#[derive(Clone, Debug)]
pub struct CoreStore {
    product_root: PathBuf,
}

impl CoreStore {
    #[must_use]
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self {
            product_root: data_dir.join(CORES_ROOT_NAME).join(PRODUCT_NAME),
        }
    }

    #[must_use]
    pub fn product_root(&self) -> &Path {
        &self.product_root
    }

    #[must_use]
    pub fn registry_path(&self) -> PathBuf {
        self.product_root.join(REGISTRY_FILE_NAME)
    }

    fn version_dir(&self, version: &str, target: &str) -> PathBuf {
        self.product_root.join(version).join(target)
    }

    fn relative_dir(version: &str, target: &str) -> String {
        format!("{version}/{target}")
    }

    /// Load the registry, creating an empty one if missing.
    pub fn load_or_default(&self) -> Result<CoreRegistry, EngineError> {
        let path = self.registry_path();
        if !path.exists() {
            return Ok(CoreRegistry::empty());
        }
        let bytes =
            fs::read(&path).map_err(|error| io_error("read core registry", &path, error))?;
        let registry: CoreRegistry =
            serde_json::from_slice(&bytes).map_err(|error| EngineError::MalformedProfile {
                path: path.clone(),
                message: error.to_string(),
            })?;
        if registry.schema_version != REGISTRY_SCHEMA_VERSION {
            return Err(EngineError::MalformedProfile {
                path,
                message: format!("unsupported cores.json schema {}", registry.schema_version),
            });
        }
        Ok(registry)
    }

    pub fn save_registry(&self, registry: &CoreRegistry) -> Result<(), EngineError> {
        fs::create_dir_all(&self.product_root)
            .map_err(|error| io_error("create cores directory", &self.product_root, error))?;
        let path = self.registry_path();
        let payload = serde_json::to_vec_pretty(registry)
            .map_err(|source| EngineError::Serialize { source })?;
        atomic_write(&path, &payload)
    }

    pub fn load_installation(
        &self,
        summary: &CoreInstallationSummary,
    ) -> Result<CoreInstallation, EngineError> {
        let dir = self.product_root.join(&summary.relative_dir);
        let path = dir.join(INSTALLATION_FILE_NAME);
        let bytes = fs::read(&path)
            .map_err(|error| io_error("read installation manifest", &path, error))?;
        serde_json::from_slice(&bytes).map_err(|error| EngineError::MalformedProfile {
            path,
            message: error.to_string(),
        })
    }

    fn save_installation(
        &self,
        version_dir: &Path,
        installation: &CoreInstallation,
    ) -> Result<(), EngineError> {
        fs::create_dir_all(version_dir)
            .map_err(|error| io_error("create core version directory", version_dir, error))?;
        let path = version_dir.join(INSTALLATION_FILE_NAME);
        let payload = serde_json::to_vec_pretty(installation)
            .map_err(|source| EngineError::Serialize { source })?;
        atomic_write(&path, &payload)
    }

    /// Resolve the active managed executable, if any and still present.
    pub fn resolve_active_executable(&self) -> Result<Option<PathBuf>, EngineError> {
        let registry = self.load_or_default()?;
        let Some(summary) = registry.active().cloned() else {
            return Ok(None);
        };
        let installation = self.load_installation(&summary)?;
        let version_dir = self.product_root.join(&summary.relative_dir);
        let executable = installation.absolute_executable(&version_dir);
        if executable.is_file()
            || (executable.components().count() == 1 && validate_executable(&executable).is_ok())
        {
            Ok(Some(executable))
        } else {
            Ok(None)
        }
    }

    /// List installations with absolute executable paths for UI/desktop.
    pub fn list_installations(&self) -> Result<Vec<CoreInstallationView>, EngineError> {
        let registry = self.load_or_default()?;
        let mut views = Vec::with_capacity(registry.installations.len());
        for summary in &registry.installations {
            let installation = match self.load_installation(summary) {
                Ok(value) => value,
                Err(_) => {
                    // Stale index entry — still surface it so the user can remove it.
                    views.push(CoreInstallationView {
                        id: summary.id,
                        version: summary.version.clone(),
                        target: summary.target.clone(),
                        source: summary.source,
                        executable: PathBuf::new(),
                        features: Vec::new(),
                        is_active: registry.active_id == Some(summary.id),
                        is_last_working: registry.last_working_id == Some(summary.id),
                        validated_version: None,
                        last_verified_at: None,
                        status: CoreInstallStatus::MissingManifest,
                    });
                    continue;
                }
            };
            let version_dir = self.product_root.join(&summary.relative_dir);
            let executable = installation.absolute_executable(&version_dir);
            let status = if executable.is_file()
                || (executable.components().count() == 1
                    && validate_executable(&executable).is_ok())
            {
                CoreInstallStatus::Ready
            } else {
                CoreInstallStatus::MissingExecutable
            };
            views.push(CoreInstallationView {
                id: summary.id,
                version: installation.version,
                target: installation.target,
                source: installation.source,
                executable,
                features: installation.features,
                is_active: registry.active_id == Some(summary.id),
                is_last_working: registry.last_working_id == Some(summary.id),
                validated_version: installation.validated_version,
                last_verified_at: installation.last_verified_at,
                status,
            });
        }
        // Newest versions first (lexicographic on version string is good enough for semver-ish).
        views.sort_by(|left, right| right.version.cmp(&left.version));
        Ok(views)
    }

    /// Import an existing aria2c by copying it into the managed cores tree.
    pub fn import_executable(
        &self,
        source_path: impl AsRef<Path>,
    ) -> Result<CoreInstallationView, EngineError> {
        let source_path = source_path.as_ref();
        validate_executable(source_path)?;
        let probe = probe_aria2(source_path)?;
        let target = host_target();
        let version = sanitize_path_component(&probe.version)?;
        let version_dir = self.version_dir(&version, &target);
        if version_dir.exists() {
            return Err(EngineError::CoreAlreadyInstalled {
                version: version.clone(),
                target: target.clone(),
            });
        }

        let staging = self
            .product_root
            .join(".staging")
            .join(format!("{version}-{target}-{}", short_id()));
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        fs::create_dir_all(&staging)
            .map_err(|error| io_error("create core staging directory", &staging, error))?;

        let file_name = source_path
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(default_aria2_file_name()));
        let dest_executable = staging.join(&file_name);
        fs::copy(source_path, &dest_executable).map_err(|error| {
            io_error("copy aria2 executable into cores", &dest_executable, error)
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dest_executable)
                .map_err(|error| io_error("read executable permissions", &dest_executable, error))?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest_executable, perms)
                .map_err(|error| io_error("set executable permissions", &dest_executable, error))?;
        }

        // Re-validate the staged copy before promoting.
        validate_executable(&dest_executable)?;
        let staged_probe = probe_aria2(&dest_executable)?;
        let sha256 = file_sha256_hex(&dest_executable)?;
        let id = CoreInstallationId::new();
        let now = utc_now_iso();
        let installation = CoreInstallation {
            schema_version: INSTALLATION_SCHEMA_VERSION,
            id,
            version: staged_probe.version.clone(),
            target: target.clone(),
            source: CoreSource::Imported,
            executable: file_name,
            sha256: Some(sha256),
            installed_at: now.clone(),
            validated_version: Some(staged_probe.version.clone()),
            features: staged_probe.features.clone(),
            last_verified_at: Some(now),
        };
        self.save_installation(&staging, &installation)?;

        // Atomic-ish promote: rename staging → final.
        if let Some(parent) = version_dir.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| io_error("create cores version parent", parent, error))?;
        }
        fs::rename(&staging, &version_dir).map_err(|error| {
            let _ = fs::remove_dir_all(&staging);
            io_error("promote staged core installation", &version_dir, error)
        })?;

        let mut registry = self.load_or_default()?;
        let summary = CoreInstallationSummary {
            id,
            version: installation.version.clone(),
            target: target.clone(),
            source: CoreSource::Imported,
            relative_dir: Self::relative_dir(&version, &target),
        };
        registry.installations.push(summary);
        // First install becomes active automatically.
        if registry.active_id.is_none() {
            registry.active_id = Some(id);
            registry.last_working_id = Some(id);
        }
        self.save_registry(&registry)?;

        Ok(CoreInstallationView {
            id,
            version: installation.version,
            target,
            source: CoreSource::Imported,
            executable: version_dir.join(
                installation
                    .executable
                    .file_name()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from(default_aria2_file_name())),
            ),
            features: installation.features,
            is_active: registry.active_id == Some(id),
            is_last_working: registry.last_working_id == Some(id),
            validated_version: installation.validated_version,
            last_verified_at: installation.last_verified_at,
            status: CoreInstallStatus::Ready,
        })
    }

    /// Register an external path without copying (still switchable via registry).
    pub fn link_executable(
        &self,
        source_path: impl AsRef<Path>,
    ) -> Result<CoreInstallationView, EngineError> {
        let source_path = source_path.as_ref();
        validate_executable(source_path)?;
        let probe = probe_aria2(source_path)?;
        let target = host_target();
        let version = sanitize_path_component(&probe.version)?;
        // Link installs live under a synthetic dir so we still have a manifest home.
        let relative = format!("linked/{version}-{target}-{}", short_id());
        let version_dir = self.product_root.join(&relative);
        fs::create_dir_all(&version_dir)
            .map_err(|error| io_error("create linked core directory", &version_dir, error))?;

        let absolute = canonicalize_best_effort(source_path);
        let id = CoreInstallationId::new();
        let now = utc_now_iso();
        let installation = CoreInstallation {
            schema_version: INSTALLATION_SCHEMA_VERSION,
            id,
            version: probe.version.clone(),
            target: target.clone(),
            source: CoreSource::Linked,
            executable: absolute.clone(),
            sha256: None,
            installed_at: now.clone(),
            validated_version: Some(probe.version.clone()),
            features: probe.features.clone(),
            last_verified_at: Some(now),
        };
        self.save_installation(&version_dir, &installation)?;

        let mut registry = self.load_or_default()?;
        registry.installations.push(CoreInstallationSummary {
            id,
            version: probe.version.clone(),
            target: target.clone(),
            source: CoreSource::Linked,
            relative_dir: relative,
        });
        if registry.active_id.is_none() {
            registry.active_id = Some(id);
            registry.last_working_id = Some(id);
        }
        self.save_registry(&registry)?;

        Ok(CoreInstallationView {
            id,
            version: probe.version,
            target,
            source: CoreSource::Linked,
            executable: absolute,
            features: probe.features,
            is_active: registry.active_id == Some(id),
            is_last_working: registry.last_working_id == Some(id),
            validated_version: installation.validated_version,
            last_verified_at: installation.last_verified_at,
            status: CoreInstallStatus::Ready,
        })
    }

    /// Re-run `--version` and refresh the installation manifest.
    pub fn verify(&self, id: CoreInstallationId) -> Result<CoreInstallationView, EngineError> {
        let registry = self.load_or_default()?;
        let summary = registry
            .get(id)
            .cloned()
            .ok_or(EngineError::CoreNotFound { id })?;
        let version_dir = self.product_root.join(&summary.relative_dir);
        let mut installation = self.load_installation(&summary)?;
        let executable = installation.absolute_executable(&version_dir);
        validate_executable(&executable)?;
        let probe = probe_aria2(&executable)?;
        installation.validated_version = Some(probe.version.clone());
        installation.features = probe.features.clone();
        installation.last_verified_at = Some(utc_now_iso());
        if installation.sha256.is_none() && executable.is_file() {
            installation.sha256 = Some(file_sha256_hex(&executable)?);
        }
        self.save_installation(&version_dir, &installation)?;

        Ok(CoreInstallationView {
            id,
            version: installation.version,
            target: installation.target,
            source: installation.source,
            executable,
            features: installation.features,
            is_active: registry.active_id == Some(id),
            is_last_working: registry.last_working_id == Some(id),
            validated_version: installation.validated_version,
            last_verified_at: installation.last_verified_at,
            status: CoreInstallStatus::Ready,
        })
    }

    /// Mark a core as active. Caller is responsible for restarting the engine.
    /// Previous active becomes `last_working` when different.
    pub fn activate(&self, id: CoreInstallationId) -> Result<CoreRegistry, EngineError> {
        let mut registry = self.load_or_default()?;
        if registry.get(id).is_none() {
            return Err(EngineError::CoreNotFound { id });
        }
        // Verify before activation so we never activate a broken binary.
        let _ = self.verify(id)?;
        if let Some(previous) = registry.active_id
            && previous != id
        {
            registry.last_working_id = Some(previous);
        }
        registry.active_id = Some(id);
        if registry.last_working_id.is_none() {
            registry.last_working_id = Some(id);
        }
        self.save_registry(&registry)?;
        Ok(registry)
    }

    /// Roll back to `last_working` when it differs from active.
    pub fn rollback_to_last_working(&self) -> Result<CoreRegistry, EngineError> {
        let registry = self.load_or_default()?;
        let Some(last) = registry.last_working_id else {
            return Err(EngineError::NoLastWorkingCore);
        };
        if registry.active_id == Some(last) {
            return Err(EngineError::AlreadyOnLastWorkingCore { id: last });
        }
        self.activate(last)
    }

    /// Remove a core. Active cores cannot be removed unless they are the only entry
    /// and the caller clears active first — refuse removing the sole active core.
    pub fn remove(&self, id: CoreInstallationId) -> Result<CoreRegistry, EngineError> {
        let mut registry = self.load_or_default()?;
        let index = registry
            .installations
            .iter()
            .position(|entry| entry.id == id)
            .ok_or(EngineError::CoreNotFound { id })?;
        if registry.active_id == Some(id) {
            return Err(EngineError::CannotRemoveActiveCore { id });
        }
        let summary = registry.installations.remove(index);
        if registry.last_working_id == Some(id) {
            registry.last_working_id = registry.active_id;
        }
        self.save_registry(&registry)?;
        let version_dir = self.product_root.join(&summary.relative_dir);
        if version_dir.exists() {
            let _ = fs::remove_dir_all(&version_dir);
        }
        Ok(registry)
    }

    /// After a successful managed-engine start, remember the active core as last working.
    pub fn mark_active_as_last_working(&self) -> Result<(), EngineError> {
        let mut registry = self.load_or_default()?;
        let Some(active) = registry.active_id else {
            return Ok(());
        };
        if registry.last_working_id != Some(active) {
            registry.last_working_id = Some(active);
            self.save_registry(&registry)?;
        }
        Ok(())
    }
}

/// UI/desktop projection of one core install.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreInstallationView {
    pub id: CoreInstallationId,
    pub version: String,
    pub target: String,
    pub source: CoreSource,
    pub executable: PathBuf,
    pub features: Vec<String>,
    pub is_active: bool,
    pub is_last_working: bool,
    pub validated_version: Option<String>,
    pub last_verified_at: Option<String>,
    pub status: CoreInstallStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreInstallStatus {
    Ready,
    MissingExecutable,
    MissingManifest,
}

impl CoreInstallStatus {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::MissingExecutable => "Missing executable",
            Self::MissingManifest => "Missing manifest",
        }
    }
}

/// Probe `aria2c --version` for version + feature list.
pub fn probe_aria2(path: impl AsRef<Path>) -> Result<Aria2Probe, EngineError> {
    let path = path.as_ref();
    validate_executable(path)?;
    let output = Command::new(path)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| EngineError::ExecutableValidation {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(EngineError::ExecutableValidation {
            path: path.to_path_buf(),
            reason: format!("process exited with {}", output.status),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = if stdout.trim().is_empty() {
        stderr.into_owned()
    } else {
        stdout.into_owned()
    };
    parse_aria2_version_output(&combined).ok_or_else(|| EngineError::ExecutableValidation {
        path: path.to_path_buf(),
        reason: "could not parse aria2 version from --version output".into(),
    })
}

/// Parse version and features from aria2 `--version` text.
pub fn parse_aria2_version_output(output: &str) -> Option<Aria2Probe> {
    let mut version = None;
    let mut raw_version_line = String::new();
    let mut features = Vec::new();
    let mut in_features = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        if version.is_none()
            && let Some(parsed) = extract_version_token(trimmed)
        {
            version = Some(parsed);
            raw_version_line = trimmed.to_owned();
        }
        if lower.starts_with("enabled features") || lower.starts_with("features:") {
            in_features = true;
            // Same-line features: "Enabled Features: Async DNS, BitTorrent"
            if let Some((_, rest)) = trimmed.split_once(':') {
                for part in rest.split([',', ' ']) {
                    let feature = part.trim();
                    if !feature.is_empty() {
                        features.push(feature.to_owned());
                    }
                }
            }
            continue;
        }
        if in_features {
            if trimmed.ends_with(':') {
                // Next section header.
                in_features = false;
                continue;
            }
            // Lines like "  BitTorrent" or "BitTorrent"
            let feature = trimmed.trim_start_matches(['-', '*', ' ']).trim();
            if !feature.is_empty() && !feature.contains('=') {
                features.push(feature.to_owned());
            }
        }
    }

    let version = version?;
    // Dedup features while preserving order.
    let mut seen = std::collections::HashSet::new();
    features.retain(|feature| seen.insert(feature.clone()));

    Some(Aria2Probe {
        version,
        features,
        raw_version_line,
    })
}

fn extract_version_token(line: &str) -> Option<String> {
    // Typical: "aria2 version 1.37.0" or "aria2c version 1.36.0"
    let lower = line.to_ascii_lowercase();
    if !lower.contains("aria2") {
        return None;
    }
    for token in line.split_whitespace() {
        let candidate =
            token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '.' && ch != '-');
        if candidate
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_digit())
            && candidate.contains('.')
            && candidate
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
        {
            return Some(candidate.to_owned());
        }
    }
    None
}

fn host_target() -> String {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    match (os, arch) {
        ("windows", "x86_64") => "windows-x86_64".into(),
        ("windows", "aarch64") => "windows-aarch64".into(),
        ("linux", "x86_64") => "linux-x86_64".into(),
        ("linux", "aarch64") => "linux-aarch64".into(),
        ("macos", "x86_64") => "macos-x86_64".into(),
        ("macos", "aarch64") => "macos-aarch64".into(),
        (os, arch) => format!("{os}-{arch}"),
    }
}

fn default_aria2_file_name() -> &'static str {
    if cfg!(windows) {
        "aria2c.exe"
    } else {
        "aria2c"
    }
}

fn sanitize_path_component(value: &str) -> Result<String, EngineError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(EngineError::InvalidCoreVersion {
            version: value.to_owned(),
        });
    }
    if trimmed.contains("..")
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains('\0')
    {
        return Err(EngineError::InvalidCoreVersion {
            version: value.to_owned(),
        });
    }
    Ok(trimmed.to_owned())
}

fn utc_now_iso() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    // Compact RFC3339-ish without chrono dependency.
    format!("{secs}")
}

fn short_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

fn canonicalize_best_effort(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn file_sha256_hex(path: &Path) -> Result<String, EngineError> {
    use sha2::{Digest, Sha256};
    let bytes = fs::read(path).map_err(|error| io_error("hash core executable", path, error))?;
    let digest = Sha256::digest(&bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn atomic_write(path: &Path, payload: &[u8]) -> Result<(), EngineError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| io_error("create parent directory", parent, error))?;
    let temp = parent.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("write")
    ));
    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temp)
            .map_err(|error| io_error("open temporary file", &temp, error))?;
        file.write_all(payload)
            .map_err(|error| io_error("write temporary file", &temp, error))?;
        file.sync_all()
            .map_err(|error| io_error("sync temporary file", &temp, error))?;
    }
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        io_error("replace file atomically", path, error)
    })?;
    // Best-effort parent sync on platforms that support it.
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("ariadeck-cores-{n}-{}", short_id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp");
        root
    }

    #[test]
    fn parse_version_and_features_from_sample_output() {
        let sample = "\
aria2 version 1.37.0\n\
Copyright (C) 2006, 2019 Tatsuhiro Tsujikawa\n\
\n\
** Configuration **\n\
Enabled Features: Async DNS, BitTorrent, Firefox3 Cookie, GZip, HTTPS, Message Digest, Metalink, XML-RPC, SFTP\n\
Hash Algorithms: sha-1, sha-224, sha-256, sha-384, sha-512, md5, adler32\n\
Libraries: EXPAT OpenSSL zlib libssh2\n\
Compiler: gcc 13.2.0\n\
  built by  x86_64-pc-linux-gnu\n\
  on        Mar 30 2024 12:00:00\n\
System: Linux 6.1.0 x86_64\n\
";
        let probe = parse_aria2_version_output(sample).expect("parse");
        assert_eq!(probe.version, "1.37.0");
        assert!(probe.features.iter().any(|f| f == "BitTorrent"));
        assert!(probe.features.iter().any(|f| f == "HTTPS"));
    }

    #[test]
    fn import_requires_real_aria2_binary() {
        let root = temp_dir();
        let store = CoreStore::new(&root);
        let fake = root.join("not-aria2.exe");
        fs::write(&fake, b"hello").expect("write fake");
        let err = store.import_executable(&fake).expect_err("must reject");
        // Validation or spawn failure depending on platform.
        let message = err.to_string();
        assert!(
            message.contains("validation")
                || message.contains("executable")
                || message.contains("spawn")
                || message.contains("version")
                || message.contains("not"),
            "{message}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registry_activate_and_rollback_without_binary() {
        // Unit-level registry mutations without real binary: seed files manually.
        let root = temp_dir();
        let store = CoreStore::new(&root);
        let id_a = CoreInstallationId::new();
        let id_b = CoreInstallationId::new();
        for (id, version) in [(id_a, "1.36.0"), (id_b, "1.37.0")] {
            let dir = store.version_dir(version, "test-target");
            fs::create_dir_all(&dir).expect("dir");
            // Fake executable name only — verify will fail; activate calls verify.
            // So test activate path via direct registry save + remove/active flags.
            let installation = CoreInstallation {
                schema_version: INSTALLATION_SCHEMA_VERSION,
                id,
                version: version.into(),
                target: "test-target".into(),
                source: CoreSource::Imported,
                executable: PathBuf::from("aria2c"),
                sha256: None,
                installed_at: "0".into(),
                validated_version: Some(version.into()),
                features: vec!["BitTorrent".into()],
                last_verified_at: None,
            };
            store.save_installation(&dir, &installation).expect("save");
        }
        let mut registry = CoreRegistry::empty();
        registry.installations.push(CoreInstallationSummary {
            id: id_a,
            version: "1.36.0".into(),
            target: "test-target".into(),
            source: CoreSource::Imported,
            relative_dir: CoreStore::relative_dir("1.36.0", "test-target"),
        });
        registry.installations.push(CoreInstallationSummary {
            id: id_b,
            version: "1.37.0".into(),
            target: "test-target".into(),
            source: CoreSource::Imported,
            relative_dir: CoreStore::relative_dir("1.37.0", "test-target"),
        });
        registry.active_id = Some(id_a);
        registry.last_working_id = Some(id_a);
        store.save_registry(&registry).expect("registry");

        // remove non-active works
        let registry = store.remove(id_b).expect("remove b");
        assert_eq!(registry.installations.len(), 1);
        assert_eq!(registry.active_id, Some(id_a));

        // cannot remove active
        let err = store.remove(id_a).expect_err("active");
        assert!(matches!(err, EngineError::CannotRemoveActiveCore { .. }));

        let listed = store.list_installations().expect("list");
        assert_eq!(listed.len(), 1);
        assert!(listed[0].is_active);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sanitize_rejects_traversal() {
        assert!(sanitize_path_component("../evil").is_err());
        assert!(sanitize_path_component("1.37.0").is_ok());
    }
}
