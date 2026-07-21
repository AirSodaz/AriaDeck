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
}
