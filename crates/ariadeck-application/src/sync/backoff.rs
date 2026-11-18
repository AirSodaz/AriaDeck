use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::time::{Instant, Interval, MissedTickBehavior};

use super::types::{ActivityMode, ReconnectPolicy, RefreshPolicy};

pub(super) struct PollTimers {
    pub(super) global: Interval,
    pub(super) live: Interval,
    pub(super) stopped: Interval,
    pub(super) notification: Interval,
}

impl PollTimers {
    pub(super) fn new(policy: RefreshPolicy, mode: ActivityMode) -> Self {
        let intervals = policy.intervals(mode);
        Self {
            global: interval_after(intervals.global_stat),
            live: interval_after(intervals.live_tasks),
            stopped: interval_after(intervals.stopped_tasks),
            notification: interval_after(policy.notification_debounce),
        }
    }
}

fn interval_after(duration: Duration) -> Interval {
    let duration = duration.max(Duration::from_millis(1));
    let mut interval = tokio::time::interval_at(Instant::now() + duration, duration);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    interval
}

pub(super) struct ReconnectBackoff {
    policy: ReconnectPolicy,
    attempt: u32,
    random_state: u64,
}

impl ReconnectBackoff {
    pub(super) const fn new(policy: ReconnectPolicy, seed: u64) -> Self {
        Self {
            policy,
            attempt: 0,
            random_state: seed,
        }
    }

    pub(super) const fn attempt(&self) -> u32 {
        self.attempt
    }

    pub(super) fn reset(&mut self) {
        self.attempt = 0;
    }

    pub(super) fn next_delay(&mut self) -> Duration {
        let exponent = self.attempt.min(31);
        self.attempt = self.attempt.saturating_add(1);
        let multiplier = 1_u32 << exponent;
        let base = self
            .policy
            .base_delay
            .saturating_mul(multiplier)
            .min(self.policy.max_delay);
        let jitter_percent = self.policy.jitter_percent.min(100);
        if jitter_percent == 0 {
            return base;
        }

        self.random_state ^= self.random_state << 13;
        self.random_state ^= self.random_state >> 7;
        self.random_state ^= self.random_state << 17;
        let base_millis = u64::try_from(base.as_millis()).unwrap_or(u64::MAX);
        let span = base_millis.saturating_mul(u64::from(jitter_percent)) / 100;
        if span == 0 {
            return base;
        }
        let width = span.saturating_mul(2).saturating_add(1);
        let offset = self.random_state % width;
        Duration::from_millis(base_millis.saturating_sub(span).saturating_add(offset))
            .min(self.policy.max_delay)
    }
}

pub(super) fn backoff_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0x9e37_79b9_7f4a_7c15, |duration| {
            u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
        })
}

pub(super) const fn reconnect_limit_reached(policy: ReconnectPolicy, attempt: u32) -> bool {
    match policy.max_attempts {
        Some(max_attempts) => attempt >= max_attempts,
        None => false,
    }
}
