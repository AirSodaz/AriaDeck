use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::SettingsError;

/// Soft cap so settings UI and export stay manageable.
pub const MAX_DOWNLOAD_CATEGORIES: usize = 32;
/// Soft cap for extension tokens per category.
pub const MAX_CATEGORY_EXTENSIONS: usize = 128;
/// Soft cap for a single extension token length.
pub const MAX_CATEGORY_EXTENSION_LEN: usize = 32;

/// Named output folder used as a download category (C1 / D-040 / D-042).
///
/// Routing is extension-based (IDM-style). Exactly one category must be
/// `is_fallback` for unmatched names. `download_directory` mirrors the
/// fallback category path for engine/profile defaults.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadCategory {
    pub id: Uuid,
    pub name: String,
    pub directory: PathBuf,
    /// Lowercase extensions without a leading dot (e.g. `mp4`). Empty = no auto-match.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Unmatched / no-filename downloads land here. Exactly one category is fallback.
    #[serde(default)]
    pub is_fallback: bool,
}

impl DownloadCategory {
    #[must_use]
    pub fn new(name: impl Into<String>, directory: impl Into<PathBuf>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            directory: directory.into(),
            extensions: Vec::new(),
            is_fallback: false,
        }
    }

    #[must_use]
    pub fn with_extensions(
        mut self,
        extensions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.extensions = normalize_category_extensions(extensions.into_iter().map(Into::into));
        self
    }

    #[must_use]
    pub fn fallback(mut self) -> Self {
        self.is_fallback = true;
        self
    }
}

/// Normalize extension tokens for storage: trim, strip dots, lowercase, drop empties/dupes.
#[must_use]
pub fn normalize_category_extensions(
    raw: impl IntoIterator<Item = impl AsRef<str>>,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for item in raw {
        let Some(norm) = ariadeck_domain::normalize_extension(item.as_ref()) else {
            continue;
        };
        if norm.len() > MAX_CATEGORY_EXTENSION_LEN {
            continue;
        }
        if seen.insert(norm.clone()) {
            out.push(norm);
        }
    }
    out
}

/// Parse a user-edited extension list (`mp4, mkv; zip` or whitespace).
#[must_use]
pub fn parse_category_extensions_text(text: &str) -> Vec<String> {
    normalize_category_extensions(
        text.split(|c: char| c == ',' || c == ';' || c == '|' || c.is_whitespace()),
    )
}

/// Format extensions for a settings text field.
#[must_use]
pub fn format_category_extensions(extensions: &[String]) -> String {
    extensions.join(", ")
}

/// Default categories under `root` (General is the fixed fallback).
///
/// Presets share the same directory by default; users set paths manually.
#[must_use]
pub fn default_download_categories(root: impl AsRef<Path>) -> Vec<DownloadCategory> {
    let root = root.as_ref().to_path_buf();
    vec![
        DownloadCategory::new("General", root.clone()).fallback(),
        DownloadCategory::new("Compressed", root.clone())
            .with_extensions(["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "tgz"]),
        DownloadCategory::new("Documents", root.clone()).with_extensions([
            "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "epub", "mobi", "rtf", "odt",
        ]),
        DownloadCategory::new("Music", root.clone())
            .with_extensions(["mp3", "flac", "aac", "wav", "ogg", "m4a", "wma", "opus"]),
        DownloadCategory::new("Video", root.clone()).with_extensions([
            "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "ts",
        ]),
        DownloadCategory::new("Programs", root.clone())
            .with_extensions(["exe", "msi", "dmg", "pkg", "deb", "rpm", "apk", "appimage"]),
        DownloadCategory::new("Images", root).with_extensions([
            "jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "heic", "raw",
        ]),
    ]
}

/// Resolve category for a filename hint using extension rules (D-042).
#[must_use]
pub fn resolve_download_category<'a>(
    categories: &'a [DownloadCategory],
    filename_hint: Option<&str>,
) -> Option<&'a DownloadCategory> {
    let keys: Vec<String> = (0..categories.len()).map(|i| i.to_string()).collect();
    let resolved = ariadeck_domain::resolve_category_id(
        categories.iter().zip(&keys).map(|(category, key)| {
            (
                key.as_str(),
                category.is_fallback,
                category.extensions.as_slice(),
            )
        }),
        filename_hint,
    )?;
    resolved
        .parse::<usize>()
        .ok()
        .and_then(|idx| categories.get(idx))
}

pub fn validate_categories(categories: &[DownloadCategory]) -> Result<(), SettingsError> {
    if categories.is_empty() {
        return Err(SettingsError::EmptyCategories);
    }
    if categories.len() > MAX_DOWNLOAD_CATEGORIES {
        return Err(SettingsError::TooManyCategories);
    }
    let mut seen_names = HashSet::new();
    let mut seen_ids = HashSet::new();
    let mut fallbacks = 0usize;
    for category in categories {
        let name = category.name.trim();
        if name.is_empty() {
            return Err(SettingsError::EmptyCategoryName);
        }
        if category.directory.as_os_str().is_empty() {
            return Err(SettingsError::EmptyCategoryDirectory);
        }
        if category.extensions.len() > MAX_CATEGORY_EXTENSIONS {
            return Err(SettingsError::TooManyCategoryExtensions);
        }
        for ext in &category.extensions {
            let Some(norm) = ariadeck_domain::normalize_extension(ext) else {
                return Err(SettingsError::InvalidCategoryExtension {
                    extension: ext.clone(),
                });
            };
            if norm.len() > MAX_CATEGORY_EXTENSION_LEN || norm != *ext {
                return Err(SettingsError::InvalidCategoryExtension {
                    extension: ext.clone(),
                });
            }
        }
        let key = name.to_ascii_lowercase();
        if !seen_names.insert(key) {
            return Err(SettingsError::DuplicateCategoryName);
        }
        if !seen_ids.insert(category.id) {
            return Err(SettingsError::DuplicateCategoryId { id: category.id });
        }
        if category.is_fallback {
            fallbacks += 1;
        }
    }
    match fallbacks {
        0 => Err(SettingsError::MissingFallbackCategory),
        1 => Ok(()),
        _ => Err(SettingsError::MultipleFallbackCategories),
    }
}
