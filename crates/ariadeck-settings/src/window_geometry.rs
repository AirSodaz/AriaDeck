use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{SettingsError, atomic_io};

/// Schema version for the session-local window geometry document.
pub const WINDOW_GEOMETRY_SCHEMA_VERSION: u32 = 1;

/// Minimum restoreable content size in logical pixels (matches desktop min).
pub const WINDOW_MIN_WIDTH: f32 = 960.0;
pub const WINDOW_MIN_HEIGHT: f32 = 620.0;
pub const WINDOW_DEFAULT_WIDTH: f32 = 1180.0;
pub const WINDOW_DEFAULT_HEIGHT: f32 = 760.0;

/// Persisted main-window placement (UI-001 / D-031). Stored separately from
/// settings so resize storms never rewrite the full settings document.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WindowGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
}

impl Default for WindowGeometry {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: WINDOW_DEFAULT_WIDTH,
            height: WINDOW_DEFAULT_HEIGHT,
            maximized: false,
        }
    }
}

impl WindowGeometry {
    /// Reject NaN/non-finite sizes and clamp below the desktop minimum.
    #[must_use]
    pub fn sanitized(self) -> Self {
        let width = if self.width.is_finite() {
            self.width.max(WINDOW_MIN_WIDTH)
        } else {
            WINDOW_DEFAULT_WIDTH
        };
        let height = if self.height.is_finite() {
            self.height.max(WINDOW_MIN_HEIGHT)
        } else {
            WINDOW_DEFAULT_HEIGHT
        };
        let x = if self.x.is_finite() { self.x } else { 0.0 };
        let y = if self.y.is_finite() { self.y } else { 0.0 };
        Self {
            x,
            y,
            width,
            height,
            maximized: self.maximized,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WindowGeometryDocument {
    schema_version: u32,
    geometry: WindowGeometry,
}

/// Atomic JSON store for main-window geometry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonWindowGeometryStore {
    path: PathBuf,
}

impl JsonWindowGeometryStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load a previously saved geometry, or `None` when missing/corrupt.
    pub fn load(&self) -> Option<WindowGeometry> {
        let bytes = fs::read(&self.path).ok()?;
        let document: WindowGeometryDocument = serde_json::from_slice(&bytes).ok()?;
        if document.schema_version != WINDOW_GEOMETRY_SCHEMA_VERSION {
            return None;
        }
        Some(document.geometry.sanitized())
    }

    pub fn save(&self, geometry: WindowGeometry) -> Result<(), SettingsError> {
        let geometry = geometry.sanitized();
        let payload = serde_json::to_vec_pretty(&WindowGeometryDocument {
            schema_version: WINDOW_GEOMETRY_SCHEMA_VERSION,
            geometry,
        })
        .map_err(SettingsError::Serialize)?;
        atomic_io::write_atomic_json(
            &self.path,
            &payload,
            "create the window-geometry directory",
            "write the window-geometry temp file",
            "write the window-geometry temp file",
            "sync the window-geometry temp file",
            "replace the window-geometry document",
        )
    }
}
