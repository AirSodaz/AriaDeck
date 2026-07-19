//! Typed, versioned application settings and their persistence boundary.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub const CURRENT_SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorScheme {
    Light,
    #[default]
    Dark,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppSettings {
    pub color_scheme: ColorScheme,
    pub download_directory: PathBuf,
}

impl AppSettings {
    #[must_use]
    pub fn new(download_directory: impl Into<PathBuf>) -> Self {
        Self {
            color_scheme: ColorScheme::default(),
            download_directory: download_directory.into(),
        }
    }

    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.download_directory.as_os_str().is_empty() {
            return Err(SettingsError::EmptyDownloadDirectory);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsRecovery {
    pub backup_path: PathBuf,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedSettings {
    pub settings: AppSettings,
    pub recovery: Option<SettingsRecovery>,
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("settings store path has no file name: {path}")]
    InvalidStorePath { path: PathBuf },
    #[error("download directory must not be empty")]
    EmptyDownloadDirectory,
    #[error("unsupported settings schema version {found}; this build supports {supported}")]
    UnsupportedSchemaVersion { found: u32, supported: u32 },
    #[error("malformed settings document at {path}: {message}")]
    MalformedDocument { path: PathBuf, message: String },
    #[error("failed to serialize settings: {0}")]
    Serialize(#[source] serde_json::Error),
    #[error("I/O error while {operation} at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SettingsDocument {
    schema_version: u32,
    color_scheme: ColorScheme,
    download_directory: PathBuf,
}

impl From<&AppSettings> for SettingsDocument {
    fn from(settings: &AppSettings) -> Self {
        Self {
            schema_version: CURRENT_SETTINGS_SCHEMA_VERSION,
            color_scheme: settings.color_scheme,
            download_directory: settings.download_directory.clone(),
        }
    }
}

impl TryFrom<SettingsDocument> for AppSettings {
    type Error = SettingsError;

    fn try_from(document: SettingsDocument) -> Result<Self, Self::Error> {
        if document.schema_version != CURRENT_SETTINGS_SCHEMA_VERSION {
            return Err(SettingsError::UnsupportedSchemaVersion {
                found: document.schema_version,
                supported: CURRENT_SETTINGS_SCHEMA_VERSION,
            });
        }
        let settings = Self {
            color_scheme: document.color_scheme,
            download_directory: document.download_directory,
        };
        settings.validate()?;
        Ok(settings)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonSettingsStore {
    path: PathBuf,
}

impl JsonSettingsStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<AppSettings, SettingsError> {
        let bytes = fs::read(&self.path)
            .map_err(|source| io_error("read the settings document", &self.path, source))?;
        let document: SettingsDocument =
            serde_json::from_slice(&bytes).map_err(|error| SettingsError::MalformedDocument {
                path: self.path.clone(),
                message: error.to_string(),
            })?;
        AppSettings::try_from(document)
    }

    pub fn load_or_initialize(
        &self,
        defaults: &AppSettings,
    ) -> Result<LoadedSettings, SettingsError> {
        defaults.validate()?;
        if !self.path.exists() {
            self.save(defaults)?;
            return Ok(LoadedSettings {
                settings: defaults.clone(),
                recovery: None,
            });
        }

        match self.load() {
            Ok(settings) => Ok(LoadedSettings {
                settings,
                recovery: None,
            }),
            Err(error @ SettingsError::MalformedDocument { .. })
            | Err(error @ SettingsError::EmptyDownloadDirectory) => {
                let reason = error.to_string();
                let backup_path = self.preserve_corrupt_document()?;
                self.save(defaults)?;
                Ok(LoadedSettings {
                    settings: defaults.clone(),
                    recovery: Some(SettingsRecovery {
                        backup_path,
                        reason,
                    }),
                })
            }
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, settings: &AppSettings) -> Result<(), SettingsError> {
        settings.validate()?;
        let (parent, file_name) = self.parent_and_file_name()?;
        fs::create_dir_all(parent)
            .map_err(|source| io_error("create the settings directory", parent, source))?;
        let payload = serde_json::to_vec_pretty(&SettingsDocument::from(settings))
            .map_err(SettingsError::Serialize)?;
        let temp_path = parent.join(format!(
            ".{}.{}.tmp",
            file_name.to_string_lossy(),
            Uuid::new_v4()
        ));

        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp_path)
                .map_err(|source| {
                    io_error("create the temporary settings file", &temp_path, source)
                })?;
            file.write_all(&payload)
                .map_err(|source| io_error("write the settings document", &temp_path, source))?;
            file.write_all(b"\n")
                .map_err(|source| io_error("finish the settings document", &temp_path, source))?;
            file.flush()
                .map_err(|source| io_error("flush the settings document", &temp_path, source))?;
            file.sync_all()
                .map_err(|source| io_error("sync the settings document", &temp_path, source))?;
            replace_file(&temp_path, &self.path)
                .map_err(|source| io_error("replace the settings document", &self.path, source))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result
    }

    fn preserve_corrupt_document(&self) -> Result<PathBuf, SettingsError> {
        let (parent, file_name) = self.parent_and_file_name()?;
        for suffix in 0_u32.. {
            let marker = if suffix == 0 {
                String::new()
            } else {
                format!(".{suffix}")
            };
            let candidate = parent.join(format!("{}.corrupt{marker}", file_name.to_string_lossy()));
            if !candidate.exists() {
                fs::rename(&self.path, &candidate).map_err(|source| {
                    io_error("preserve the corrupt settings document", &candidate, source)
                })?;
                return Ok(candidate);
            }
        }
        unreachable!("the corruption backup suffix space is finite but unreachable in practice")
    }

    fn parent_and_file_name(&self) -> Result<(&Path, &std::ffi::OsStr), SettingsError> {
        let Some(file_name) = self.path.file_name() else {
            return Err(SettingsError::InvalidStorePath {
                path: self.path.clone(),
            });
        };
        let parent = self
            .path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        Ok((parent, file_name))
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(root: &Path) -> AppSettings {
        AppSettings {
            color_scheme: ColorScheme::Light,
            download_directory: root.join("downloads"),
        }
    }

    #[test]
    fn initializes_and_round_trips_a_versioned_document() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        let expected = settings(root.path());

        let loaded = store
            .load_or_initialize(&expected)
            .expect("initialize settings");
        assert_eq!(loaded.settings, expected);
        assert!(loaded.recovery.is_none());
        assert_eq!(store.load().expect("load settings"), expected);

        let document = fs::read_to_string(store.path()).expect("read settings JSON");
        assert!(document.contains("\"schema_version\": 1"));
        assert!(document.ends_with('\n'));
    }

    #[test]
    fn malformed_document_is_preserved_before_defaults_are_restored() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        fs::write(store.path(), b"{not-json").expect("seed malformed settings");
        let defaults = settings(root.path());

        let loaded = store
            .load_or_initialize(&defaults)
            .expect("recover settings");
        let recovery = loaded.recovery.expect("recovery metadata");
        assert_eq!(
            fs::read(recovery.backup_path).expect("read backup"),
            b"{not-json"
        );
        assert_eq!(store.load().expect("load restored defaults"), defaults);
    }

    #[test]
    fn future_schema_is_rejected_without_replacing_the_document() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store = JsonSettingsStore::new(root.path().join("settings.json"));
        let future = format!(
            "{{\"schema_version\":{},\"color_scheme\":\"dark\",\"download_directory\":\"downloads\"}}",
            CURRENT_SETTINGS_SCHEMA_VERSION + 1
        );
        fs::write(store.path(), &future).expect("seed future settings");

        assert!(matches!(
            store.load_or_initialize(&settings(root.path())),
            Err(SettingsError::UnsupportedSchemaVersion { .. })
        ));
        assert_eq!(
            fs::read_to_string(store.path()).expect("read future JSON"),
            future
        );
    }

    #[test]
    fn empty_download_directory_is_rejected() {
        let store = JsonSettingsStore::new("settings.json");
        let settings = AppSettings::new(PathBuf::new());
        assert!(matches!(
            store.save(&settings),
            Err(SettingsError::EmptyDownloadDirectory)
        ));
    }
}
