use std::{collections::VecDeque, num::NonZeroUsize, time::Duration};

use ariadeck_domain::ByteRate;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpeedSample {
    pub elapsed: Duration,
    pub download: ByteRate,
    pub upload: ByteRate,
}

/// Fixed-capacity transfer history for charts and tray summaries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpeedHistory {
    samples: VecDeque<SpeedSample>,
    capacity: NonZeroUsize,
}

impl SpeedHistory {
    pub fn new(capacity: usize) -> Result<Self, SpeedHistoryError> {
        let Some(capacity) = NonZeroUsize::new(capacity) else {
            return Err(SpeedHistoryError::ZeroCapacity);
        };
        Ok(Self {
            samples: VecDeque::with_capacity(capacity.get()),
            capacity,
        })
    }

    pub fn push(&mut self, sample: SpeedSample) {
        if self.samples.len() == self.capacity.get() {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    #[must_use]
    pub fn samples(&self) -> &VecDeque<SpeedSample> {
        &self.samples
    }

    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity.get()
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum SpeedHistoryError {
    #[error("speed history capacity must be greater than zero")]
    ZeroCapacity,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_evicts_oldest_sample_at_capacity() {
        let mut history = match SpeedHistory::new(2) {
            Ok(history) => history,
            Err(error) => panic!("valid capacity rejected: {error}"),
        };
        for second in 1..=3 {
            history.push(SpeedSample {
                elapsed: Duration::from_secs(second),
                download: ByteRate::new(second),
                upload: ByteRate::new(0),
            });
        }

        assert_eq!(history.samples().len(), 2);
        assert_eq!(
            history.samples().front().map(|sample| sample.elapsed),
            Some(Duration::from_secs(2))
        );
    }
}
