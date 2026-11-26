//! Client-wide domain header presets and URL mirror rewrite settings.

use ariadeck_domain::{
    DomainHeaderPreset, MirrorRewriteRule, validate_header_presets, validate_mirror_rules,
};
use serde::{Deserialize, Serialize};

use crate::SettingsError;

const fn default_true() -> bool {
    true
}

/// Client-wide URL mirror rewrite configuration.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MirrorRewriteSettings {
    /// When true, rewritten downloads also include the original URL as a fallback mirror.
    #[serde(default = "default_true")]
    pub keep_original_as_mirror: bool,
    #[serde(default)]
    pub rules: Vec<MirrorRewriteRule>,
}

impl Default for MirrorRewriteSettings {
    fn default() -> Self {
        Self {
            keep_original_as_mirror: true,
            rules: Vec::new(),
        }
    }
}

impl MirrorRewriteSettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        validate_mirror_rules(&self.rules).map_err(|error| SettingsError::InvalidMirrorRule {
            reason: error.to_string(),
        })
    }
}

pub(crate) fn validate_header_preset_list(
    presets: &[DomainHeaderPreset],
) -> Result<(), SettingsError> {
    validate_header_presets(presets).map_err(|error| SettingsError::InvalidHeaderPreset {
        reason: error.to_string(),
    })
}
