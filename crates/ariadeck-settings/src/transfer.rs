use ariadeck_domain::{FileAllocationMethod, TransferPolicyConfig, TransferPolicyError};
use serde::{Deserialize, Serialize};

use crate::SettingsError;

/// Persisted global speed limits. Zero means unlimited (aria2 convention).
/// Values are stored in bytes per second and applied to aria2 on each connection.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpeedLimitSettings {
    /// Max aggregate download speed in bytes/s (0 = unlimited).
    pub download_limit: u64,
    /// Max aggregate upload speed in bytes/s (0 = unlimited).
    pub upload_limit: u64,
}

/// Persisted aria2 file-allocation method. Values match aria2's option strings.
pub type FileAllocationSetting = FileAllocationMethod;

/// Persisted transfer-policy defaults for connection, split, allocation, and
/// integrity checks. Applied through `aria2.changeGlobalOption` on save and
/// reconnect. Defaults match aria2 1.37.0 documented values.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransferPolicySettings {
    /// Maximum simultaneously active downloads (`max-concurrent-downloads`).
    pub max_concurrent_downloads: u32,
    /// Default max connections per server for new downloads (1–16).
    pub max_connection_per_server: u32,
    /// Default multi-connection split count for new downloads.
    pub split: u32,
    /// Default minimum split size in bytes.
    pub min_split_size: u64,
    /// Default file allocation method for new downloads.
    pub file_allocation: FileAllocationSetting,
    /// Default integrity-check policy for new downloads.
    pub check_integrity: bool,
}

impl Default for TransferPolicySettings {
    fn default() -> Self {
        let domain = TransferPolicyConfig::default();
        Self {
            max_concurrent_downloads: domain.max_concurrent_downloads,
            max_connection_per_server: domain.max_connection_per_server,
            split: domain.split,
            min_split_size: domain.min_split_size,
            file_allocation: domain.file_allocation,
            check_integrity: domain.check_integrity,
        }
    }
}

impl From<TransferPolicySettings> for TransferPolicyConfig {
    fn from(settings: TransferPolicySettings) -> Self {
        Self {
            max_concurrent_downloads: settings.max_concurrent_downloads,
            max_connection_per_server: settings.max_connection_per_server,
            split: settings.split,
            min_split_size: settings.min_split_size,
            file_allocation: settings.file_allocation,
            check_integrity: settings.check_integrity,
        }
    }
}

impl TransferPolicySettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        TransferPolicyConfig::from(*self)
            .validate()
            .map_err(|error| match error {
                TransferPolicyError::MaxConcurrentDownloads => {
                    SettingsError::InvalidTransferPolicy {
                        field: "max_concurrent_downloads",
                        reason: "must be at least 1".into(),
                    }
                }
                TransferPolicyError::MaxConnectionPerServer => {
                    SettingsError::InvalidTransferPolicy {
                        field: "max_connection_per_server",
                        reason: "must be between 1 and 16".into(),
                    }
                }
                TransferPolicyError::Split => SettingsError::InvalidTransferPolicy {
                    field: "split",
                    reason: "must be at least 1".into(),
                },
                TransferPolicyError::MinSplitSize => SettingsError::InvalidTransferPolicy {
                    field: "min_split_size",
                    reason: "must be greater than 0".into(),
                },
            })
    }
}
