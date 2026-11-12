//! Per-connection chat and command rate limits (token buckets).
//!
//! Vanilla kicks a player with `disconnect.spam` once its spam counter passes 200, where every
//! chat message or command adds 20 and the counter drains 1 per tick (20 per second). That is a
//! bucket of 10 messages refilling at 1 message per second, which is the default here. Chat and
//! commands get separate buckets, so a connection can send up to the two allowances combined
//! (vanilla shares one counter). The 30 s read timeout is unrelated and unchanged.

use std::time::Instant;

use crate::game::ServerboundPlay;

/// Shown to a player kicked for exceeding a limit (vanilla's `disconnect.spam`).
pub const SPAM_KICK_REASON: &str = "Kicked for spamming";

/// One token bucket: `burst` messages may arrive at once, refilling at `per_second`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BucketConfig {
    pub burst: u32,
    pub per_second: f64,
}
impl BucketConfig {
    /// Vanilla's spam threshold: 10 messages, one more per second.
    pub const VANILLA: Self = Self {
        burst: 10,
        per_second: 1.0,
    };
}

/// What to do with a message that finds its bucket empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LimitAction {
    /// Disconnect the player with [`SPAM_KICK_REASON`], like vanilla.
    #[default]
    Kick,
    /// Silently discard the message and keep the connection.
    Drop,
}

/// Chat and command limits. `None` disables that bucket.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateLimits {
    pub chat: Option<BucketConfig>,
    pub command: Option<BucketConfig>,
    pub action: LimitAction,
}
impl RateLimits {
    pub const fn disabled() -> Self {
        Self {
            chat: None,
            command: None,
            action: LimitAction::Kick,
        }
    }
}
impl Default for RateLimits {
    fn default() -> Self {
        Self {
            chat: Some(BucketConfig::VANILLA),
            command: Some(BucketConfig::VANILLA),
            action: LimitAction::Kick,
        }
    }
}

#[derive(Debug, Clone)]
struct TokenBucket {
    capacity: f64,
    per_second: f64,
    tokens: f64,
    last: Instant,
}
impl TokenBucket {
    fn new(config: BucketConfig, now: Instant) -> Self {
        // A zero burst could never admit anything; treat it as one message.
        let capacity = f64::from(config.burst.max(1));
        let per_second = if config.per_second.is_finite() {
            config.per_second.max(0.0)
        } else {
            0.0
        };
        Self {
            capacity,
            per_second,
            tokens: capacity,
            last: now,
        }
    }
    fn try_take(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = self.last.max(now);
        self.tokens = (self.tokens + elapsed * self.per_second).min(self.capacity);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// The verdict for one inbound packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Drop,
    Kick,
}

/// One connection's buckets.
#[derive(Debug, Clone)]
pub struct PlayerLimiter {
    chat: Option<TokenBucket>,
    command: Option<TokenBucket>,
    action: LimitAction,
}
impl PlayerLimiter {
    pub fn new(limits: &RateLimits, now: Instant) -> Self {
        Self {
            chat: limits.chat.map(|c| TokenBucket::new(c, now)),
            command: limits.command.map(|c| TokenBucket::new(c, now)),
            action: limits.action,
        }
    }
    pub fn check(&mut self, packet: &ServerboundPlay, now: Instant) -> Verdict {
        let bucket = match packet {
            ServerboundPlay::Chat(_) => self.chat.as_mut(),
            ServerboundPlay::ChatCommand(_) | ServerboundPlay::ChatCommandSigned(_) => {
                self.command.as_mut()
            }
            _ => None,
        };
        let admitted = bucket.is_none_or(|bucket| bucket.try_take(now));
        match (admitted, self.action) {
            (true, _) => Verdict::Allow,
            (false, LimitAction::Kick) => Verdict::Kick,
            (false, LimitAction::Drop) => Verdict::Drop,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_protocol::packets::play::{
        LastSeenUpdate, MovePosition, ServerboundChat, ServerboundChatCommand,
    };
    use std::time::Duration;

    fn chat() -> ServerboundPlay {
        ServerboundPlay::Chat(Box::new(ServerboundChat {
            message: "hi".into(),
            timestamp: 0,
            salt: 0,
            signature: None,
            last_seen: LastSeenUpdate::new(0, &[], 0),
        }))
    }
    fn command() -> ServerboundPlay {
        ServerboundPlay::ChatCommand(ServerboundChatCommand {
            command: "list".into(),
        })
    }
    fn limits(burst: u32, per_second: f64, action: LimitAction) -> RateLimits {
        let bucket = Some(BucketConfig { burst, per_second });
        RateLimits {
            chat: bucket,
            command: bucket,
            action,
        }
    }

    #[test]
    fn burst_passes_then_the_next_message_is_refused() {
        let t0 = Instant::now();
        let mut l = PlayerLimiter::new(&limits(3, 1.0, LimitAction::Kick), t0);
        for _ in 0..3 {
            assert_eq!(l.check(&chat(), t0), Verdict::Allow);
        }
        assert_eq!(l.check(&chat(), t0), Verdict::Kick);
    }

    #[test]
    fn tokens_refill_with_time_and_cap_at_the_burst() {
        let t0 = Instant::now();
        let mut l = PlayerLimiter::new(&limits(2, 1.0, LimitAction::Drop), t0);
        assert_eq!(l.check(&chat(), t0), Verdict::Allow);
        assert_eq!(l.check(&chat(), t0), Verdict::Allow);
        assert_eq!(l.check(&chat(), t0), Verdict::Drop);
        // Half a second is not enough for a whole token.
        let t1 = t0 + Duration::from_millis(500);
        assert_eq!(l.check(&chat(), t1), Verdict::Drop);
        // 1 s since start: exactly one token has accumulated.
        let t2 = t0 + Duration::from_secs(1);
        assert_eq!(l.check(&chat(), t2), Verdict::Allow);
        assert_eq!(l.check(&chat(), t2), Verdict::Drop);
        // A long idle period refills only up to the burst, not beyond it.
        let t3 = t2 + Duration::from_secs(3600);
        assert_eq!(l.check(&chat(), t3), Verdict::Allow);
        assert_eq!(l.check(&chat(), t3), Verdict::Allow);
        assert_eq!(l.check(&chat(), t3), Verdict::Drop);
    }

    #[test]
    fn chat_and_command_buckets_are_independent() {
        let t0 = Instant::now();
        let mut l = PlayerLimiter::new(&limits(1, 0.0, LimitAction::Kick), t0);
        assert_eq!(l.check(&chat(), t0), Verdict::Allow);
        assert_eq!(l.check(&chat(), t0), Verdict::Kick);
        assert_eq!(l.check(&command(), t0), Verdict::Allow);
        assert_eq!(l.check(&command(), t0), Verdict::Kick);
    }

    #[test]
    fn other_packets_are_never_limited() {
        let t0 = Instant::now();
        let mut l = PlayerLimiter::new(&limits(1, 0.0, LimitAction::Kick), t0);
        let position = ServerboundPlay::Position(MovePosition {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            flags: 0,
        });
        for _ in 0..100 {
            assert_eq!(l.check(&position, t0), Verdict::Allow);
        }
    }

    #[test]
    fn disabled_limits_allow_everything() {
        let t0 = Instant::now();
        let mut l = PlayerLimiter::new(&RateLimits::disabled(), t0);
        for _ in 0..1000 {
            assert_eq!(l.check(&chat(), t0), Verdict::Allow);
            assert_eq!(l.check(&command(), t0), Verdict::Allow);
        }
    }

    #[test]
    fn default_matches_vanillas_spam_threshold() {
        let t0 = Instant::now();
        let mut l = PlayerLimiter::new(&RateLimits::default(), t0);
        for _ in 0..10 {
            assert_eq!(l.check(&chat(), t0), Verdict::Allow);
        }
        assert_eq!(l.check(&chat(), t0), Verdict::Kick);
    }

    #[test]
    fn degenerate_configs_do_not_panic_or_lock_out() {
        let t0 = Instant::now();
        // Zero burst is treated as one; NaN, negative and infinite rates never refill.
        for per_second in [f64::NAN, -5.0, f64::INFINITY] {
            let mut l = PlayerLimiter::new(&limits(0, per_second, LimitAction::Drop), t0);
            assert_eq!(l.check(&chat(), t0), Verdict::Allow);
            assert_eq!(
                l.check(&chat(), t0 + Duration::from_secs(60)),
                Verdict::Drop
            );
        }
    }

    #[test]
    fn a_clock_that_goes_backwards_does_not_grant_tokens() {
        let t0 = Instant::now() + Duration::from_secs(10);
        let mut l = PlayerLimiter::new(&limits(1, 1.0, LimitAction::Drop), t0);
        assert_eq!(l.check(&chat(), t0), Verdict::Allow);
        assert_eq!(l.check(&chat(), t0 - Duration::from_secs(5)), Verdict::Drop);
    }
}
