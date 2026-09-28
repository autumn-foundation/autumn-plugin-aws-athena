//! The delay between two status polls.

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
        Self { initial, max, multiplier }
    }

    /// Gives the delay before poll `attempt`. Attempt `0` is the first poll.
    pub(crate) fn delay(&self, attempt: u32) -> Duration {
        let _ = attempt;
        todo!()
    }
}

#[cfg(test)]
mod tests;
