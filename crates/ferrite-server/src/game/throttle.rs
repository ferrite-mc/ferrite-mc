//! Vanilla `net.minecraft.util.TickThrottler`, used for the chat and command spam limits.
//!
//! Vanilla derives a threshold in seconds from `server.properties` and constructs the throttler
//! as `TickThrottler(20, 20 * seconds)`. Each message adds 20 and every game tick removes one, so a
//! default 10-second budget allows nine back-to-back messages and kicks on the tenth
//! (`isUnderThreshold` is false at `20 * 10`). A threshold of zero or less disables the limit,
//! exactly like `TickThrottler.isUnderThreshold`.

/// Vanilla `TickThrottler.incrementStep` for chat and commands.
const SPAM_INCREMENT_STEP: i32 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickThrottler {
    increment_step: i32,
    threshold: i32,
    count: i32,
}

impl TickThrottler {
    /// Vanilla `new TickThrottler(20, 20 * thresholdSeconds)`.
    pub fn from_seconds(threshold_seconds: i32) -> Self {
        Self::new(
            SPAM_INCREMENT_STEP,
            threshold_seconds.saturating_mul(SPAM_INCREMENT_STEP),
        )
    }

    pub fn new(increment_step: i32, threshold: i32) -> Self {
        Self {
            increment_step,
            threshold,
            count: 0,
        }
    }

    /// Vanilla `TickThrottler.increment`.
    pub fn increment(&mut self) {
        // Vanilla wraps at `i32::MAX`; saturating keeps an absurd burst from becoming a pass.
        self.count = self.count.saturating_add(self.increment_step);
    }

    /// Vanilla `TickThrottler.tick`: one point per game tick, never below zero.
    pub fn tick(&mut self) {
        if self.count > 0 {
            self.count -= 1;
        }
    }

    /// Vanilla `TickThrottler.isUnderThreshold`: non-positive thresholds disable the limit.
    pub fn is_under_threshold(&self) -> bool {
        self.threshold <= 0 || self.count < self.threshold
    }
}

#[cfg(test)]
mod tests {
    use super::TickThrottler;

    #[test]
    fn default_ten_seconds_allows_nine_instant_messages() {
        let mut throttler = TickThrottler::from_seconds(10);
        for _ in 0..9 {
            throttler.increment();
            assert!(throttler.is_under_threshold());
        }
        throttler.increment();
        assert!(!throttler.is_under_threshold());
    }

    #[test]
    fn one_tick_decays_one_point() {
        let mut throttler = TickThrottler::from_seconds(10);
        for _ in 0..10 {
            throttler.increment();
        }
        assert!(!throttler.is_under_threshold());
        for _ in 0..20 {
            throttler.tick();
        }
        assert!(throttler.is_under_threshold());
    }

    #[test]
    fn count_never_ticks_below_zero() {
        let mut throttler = TickThrottler::from_seconds(10);
        throttler.increment();
        for _ in 0..100 {
            throttler.tick();
        }
        assert_eq!(throttler.count, 0);
    }

    #[test]
    fn zero_or_negative_seconds_disable_the_limit() {
        for seconds in [0, -1, i32::MIN] {
            let mut throttler = TickThrottler::from_seconds(seconds);
            for _ in 0..1_000 {
                throttler.increment();
            }
            assert!(
                throttler.is_under_threshold(),
                "{seconds}s must disable the limit"
            );
        }
    }
}
