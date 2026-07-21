use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Number of bytes, preserving aria2's unsigned transfer range.
#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct ByteCount(u64);

impl ByteCount {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

/// Transfer rate in bytes per second.
#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct ByteRate(u64);

impl ByteRate {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

/// Progress calculations shared by task rows and details.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskProgress {
    pub completed: ByteCount,
    pub total: ByteCount,
}

impl TaskProgress {
    #[must_use]
    pub const fn new(completed: ByteCount, total: ByteCount) -> Self {
        Self { completed, total }
    }

    #[must_use]
    pub fn fraction(self) -> Option<f64> {
        let total = self.total.get();
        (total != 0).then(|| self.completed.get().min(total) as f64 / total as f64)
    }

    #[must_use]
    pub fn basis_points(self) -> Option<u16> {
        let total = self.total.get();
        if total == 0 {
            return None;
        }
        let completed = u128::from(self.completed.get().min(total));
        Some(((completed * 10_000) / u128::from(total)) as u16)
    }

    #[must_use]
    pub fn eta(self, speed: ByteRate) -> Option<Duration> {
        let total = self.total.get();
        if total == 0 {
            return None;
        }

        let remaining = total.saturating_sub(self.completed.get());
        if remaining == 0 {
            return Some(Duration::ZERO);
        }
        if speed.is_zero() {
            return None;
        }

        Some(Duration::from_secs(remaining.div_ceil(speed.get())))
    }
}

/// Per-engine global speed limits (0 = unlimited, which is aria2's convention).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpeedLimitConfig {
    /// Maximum aggregate download speed in bytes per second. Zero means unlimited.
    pub download_limit: ByteRate,
    /// Maximum aggregate upload speed in bytes per second. Zero means unlimited.
    pub upload_limit: ByteRate,
}

impl SpeedLimitConfig {
    /// Returns true when both limits are zero (i.e., no throttling is active).
    #[must_use]
    pub const fn is_unlimited(self) -> bool {
        self.download_limit.is_zero() && self.upload_limit.is_zero()
    }
}

/// File pre-allocation strategy mapped to aria2's `--file-allocation`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileAllocationMethod {
    /// Do not pre-allocate file space.
    None,
    /// Pre-allocate with ordinary writes (portable, slower for large files).
    #[default]
    Prealloc,
    /// Truncate/fallocate-style sparse allocation where supported.
    Trunc,
    /// Fallocate-style contiguous allocation where supported.
    Falloc,
}

impl FileAllocationMethod {
    /// aria2 option value for `file-allocation`.
    #[must_use]
    pub const fn as_aria2(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Prealloc => "prealloc",
            Self::Trunc => "trunc",
            Self::Falloc => "falloc",
        }
    }

    /// Parse an aria2 `file-allocation` value (case-sensitive as documented).
    #[must_use]
    pub fn from_aria2(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "prealloc" => Some(Self::Prealloc),
            "trunc" => Some(Self::Trunc),
            "falloc" => Some(Self::Falloc),
            _ => None,
        }
    }

    /// Stable UI label for the method.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Prealloc => "Prealloc",
            Self::Trunc => "Trunc",
            Self::Falloc => "Falloc",
        }
    }
}

/// Global transfer-policy defaults applied through `aria2.changeGlobalOption`.
///
/// Scope labels follow D-016/D-023:
/// - `max_concurrent_downloads` affects the live engine queue immediately.
/// - connection/split/min-split/file-allocation/check-integrity act as engine
///   defaults for new downloads (and for later changeOption on live tasks).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TransferPolicyConfig {
    /// Maximum number of simultaneously active downloads (aria2 `-j`).
    pub max_concurrent_downloads: u32,
    /// Default connections per server for new downloads (aria2 `-x`, 1–16).
    pub max_connection_per_server: u32,
    /// Default multi-connection split count for new downloads (aria2 `-s`).
    pub split: u32,
    /// Default minimum split size in bytes (aria2 `-k`).
    pub min_split_size: u64,
    /// Default file allocation method for new downloads.
    pub file_allocation: FileAllocationMethod,
    /// Default integrity-check policy for new downloads.
    pub check_integrity: bool,
}

impl Default for TransferPolicyConfig {
    fn default() -> Self {
        Self {
            max_concurrent_downloads: 5,
            max_connection_per_server: 1,
            split: 5,
            // aria2 default is 20M.
            min_split_size: 20 * 1024 * 1024,
            file_allocation: FileAllocationMethod::Prealloc,
            check_integrity: false,
        }
    }
}

impl TransferPolicyConfig {
    /// Validate ranges that aria2 enforces or silently clamps.
    pub fn validate(self) -> Result<(), TransferPolicyError> {
        if self.max_concurrent_downloads == 0 {
            return Err(TransferPolicyError::MaxConcurrentDownloads);
        }
        if !(1..=16).contains(&self.max_connection_per_server) {
            return Err(TransferPolicyError::MaxConnectionPerServer);
        }
        if self.split == 0 {
            return Err(TransferPolicyError::Split);
        }
        if self.min_split_size == 0 {
            return Err(TransferPolicyError::MinSplitSize);
        }
        Ok(())
    }
}

/// Typed validation failure for transfer-policy values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferPolicyError {
    MaxConcurrentDownloads,
    MaxConnectionPerServer,
    Split,
    MinSplitSize,
}

impl TransferPolicyError {
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::MaxConcurrentDownloads => "Maximum concurrent downloads must be at least 1.",
            Self::MaxConnectionPerServer => {
                "Maximum connections per server must be between 1 and 16."
            }
            Self::Split => "Split count must be at least 1.",
            Self::MinSplitSize => "Minimum split size must be greater than 0.",
        }
    }
}

/// Per-task connection policy applied through `aria2.changeOption`.
///
/// Affects only the targeted download. `max_connection_per_server` is bounded
/// to aria2's documented 1–16 range; `split` and `min_split_size` must be ≥ 1.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TaskConnectionPolicy {
    pub max_connection_per_server: u32,
    pub split: u32,
    pub min_split_size: u64,
}

impl TaskConnectionPolicy {
    pub fn validate(self) -> Result<(), TransferPolicyError> {
        if !(1..=16).contains(&self.max_connection_per_server) {
            return Err(TransferPolicyError::MaxConnectionPerServer);
        }
        if self.split == 0 {
            return Err(TransferPolicyError::Split);
        }
        if self.min_split_size == 0 {
            return Err(TransferPolicyError::MinSplitSize);
        }
        Ok(())
    }
}

/// Lightweight global statistics returned by aria2.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct GlobalStat {
    pub download_speed: ByteRate,
    pub upload_speed: ByteRate,
    pub active_tasks: u32,
    pub waiting_tasks: u32,
    pub stopped_tasks: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_clamps_overreported_completed_bytes() {
        let progress = TaskProgress::new(ByteCount::new(120), ByteCount::new(100));

        assert_eq!(progress.basis_points(), Some(10_000));
        assert_eq!(progress.eta(ByteRate::new(50)), Some(Duration::ZERO));
    }

    #[test]
    fn unknown_total_or_zero_speed_has_no_eta() {
        let unknown = TaskProgress::new(ByteCount::new(0), ByteCount::new(0));
        let stalled = TaskProgress::new(ByteCount::new(50), ByteCount::new(100));

        assert_eq!(unknown.fraction(), None);
        assert_eq!(stalled.eta(ByteRate::new(0)), None);
        assert_eq!(stalled.eta(ByteRate::new(20)), Some(Duration::from_secs(3)));
    }

    #[test]
    fn transfer_policy_defaults_match_aria2_documented_values() {
        let config = TransferPolicyConfig::default();
        assert_eq!(config.max_concurrent_downloads, 5);
        assert_eq!(config.max_connection_per_server, 1);
        assert_eq!(config.split, 5);
        assert_eq!(config.min_split_size, 20 * 1024 * 1024);
        assert_eq!(config.file_allocation, FileAllocationMethod::Prealloc);
        assert!(!config.check_integrity);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn transfer_policy_rejects_out_of_range_connection_counts() {
        assert_eq!(
            TransferPolicyConfig {
                max_connection_per_server: 0,
                ..TransferPolicyConfig::default()
            }
            .validate(),
            Err(TransferPolicyError::MaxConnectionPerServer)
        );
        assert_eq!(
            TransferPolicyConfig {
                max_connection_per_server: 17,
                ..TransferPolicyConfig::default()
            }
            .validate(),
            Err(TransferPolicyError::MaxConnectionPerServer)
        );
        assert_eq!(
            TransferPolicyConfig {
                max_concurrent_downloads: 0,
                max_connection_per_server: 16,
                ..TransferPolicyConfig::default()
            }
            .validate(),
            Err(TransferPolicyError::MaxConcurrentDownloads)
        );
        assert_eq!(
            TransferPolicyConfig {
                split: 0,
                max_connection_per_server: 16,
                ..TransferPolicyConfig::default()
            }
            .validate(),
            Err(TransferPolicyError::Split)
        );
        assert_eq!(
            TransferPolicyConfig {
                min_split_size: 0,
                max_connection_per_server: 16,
                ..TransferPolicyConfig::default()
            }
            .validate(),
            Err(TransferPolicyError::MinSplitSize)
        );
    }

    #[test]
    fn file_allocation_round_trips_through_aria2_values() {
        for method in [
            FileAllocationMethod::None,
            FileAllocationMethod::Prealloc,
            FileAllocationMethod::Trunc,
            FileAllocationMethod::Falloc,
        ] {
            assert_eq!(
                FileAllocationMethod::from_aria2(method.as_aria2()),
                Some(method)
            );
        }
        assert_eq!(FileAllocationMethod::from_aria2("sparse"), None);
    }
}
