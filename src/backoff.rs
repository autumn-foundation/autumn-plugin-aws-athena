//! The delay between two status polls.
//!
//! # Contract
//!
//! - `delay(0)` is the initial delay.
//! - Each delay is the last delay times the multiplier, to the cap.
//! - The delay never decreases and never exceeds the cap.
//! - The caller makes sure that `initial <= max` and `multiplier >= 1`.

use std::time::Duration;

/// A capped exponential backoff.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Backoff {
    initial: Duration,
    max: Duration,
    multiplier: f64,
}

impl Backoff {
    /// Makes a backoff. The caller validates the values.
    pub(crate) const fn new(initial: Duration, max: Duration, multiplier: f64) -> Self {
        Self {
            initial,
            max,
            multiplier,
        }
    }

    /// Gives the delay before poll `attempt`. Attempt `0` is the first poll.
    pub(crate) fn delay(&self, attempt: u32) -> Duration {
        let factor = self
            .multiplier
            .powi(i32::try_from(attempt).unwrap_or(i32::MAX));
        let delay = self.initial.as_secs_f64() * factor;
        Duration::try_from_secs_f64(delay)
            .map_or(self.max, |delay| delay.clamp(self.initial, self.max))
    }
}

#[cfg(test)]
mod tests;
