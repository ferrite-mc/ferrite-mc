//! Process-wide limits derived from the vanilla game rules.
//!
//! A server has one rule set, and commands run on several region threads, so the live
//! values are plain atomics rather than a lock. The defaults are the vanilla 26.3
//! defaults; [`set_from_rules`] is called whenever the rule set is loaded or changed.

use crate::gamerule::GameRules;
use std::sync::atomic::{AtomicU64, Ordering};

/// Vanilla `maxCommandForks` default.
pub const DEFAULT_MAX_FORKS: u64 = 65_536;
/// Vanilla `maxCommandSequenceLength` default.
pub const DEFAULT_MAX_SEQUENCE: u64 = 65_536;
/// Vanilla `maxBlockModifications` default.
pub const DEFAULT_MAX_BLOCK_MODIFICATIONS: u64 = 32_768;

static MAX_FORKS: AtomicU64 = AtomicU64::new(DEFAULT_MAX_FORKS);
static MAX_SEQUENCE: AtomicU64 = AtomicU64::new(DEFAULT_MAX_SEQUENCE);
static MAX_BLOCK_MODIFICATIONS: AtomicU64 = AtomicU64::new(DEFAULT_MAX_BLOCK_MODIFICATIONS);

/// Maximum branches one command may fork into (`max_command_forks`).
pub fn max_forks() -> u64 {
    MAX_FORKS.load(Ordering::Relaxed)
}

/// Maximum commands one top-level command may execute (`max_command_sequence_length`).
pub fn max_sequence() -> u64 {
    MAX_SEQUENCE.load(Ordering::Relaxed)
}

/// Maximum blocks one `/fill` or `/clone` may modify (`max_block_modifications`).
pub fn max_block_modifications() -> u64 {
    MAX_BLOCK_MODIFICATIONS.load(Ordering::Relaxed)
}

/// Applies the three limits from a rule set. Missing or invalid values keep the defaults.
pub fn set_from_rules(rules: &GameRules) {
    MAX_FORKS.store(
        rules
            .get_integer("max_command_forks")
            .map(|value| value.max(0) as u64)
            .unwrap_or(DEFAULT_MAX_FORKS),
        Ordering::Relaxed,
    );
    MAX_SEQUENCE.store(
        rules
            .get_integer("max_command_sequence_length")
            .map(|value| value.max(0) as u64)
            .unwrap_or(DEFAULT_MAX_SEQUENCE),
        Ordering::Relaxed,
    );
    MAX_BLOCK_MODIFICATIONS.store(
        rules
            .get_integer("max_block_modifications")
            .map(|value| value.max(1) as u64)
            .unwrap_or(DEFAULT_MAX_BLOCK_MODIFICATIONS),
        Ordering::Relaxed,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The limit tests mutate process-wide atomics, so they must not overlap each other.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Restores vanilla defaults however the test ends.
    struct ResetLimits;

    impl Drop for ResetLimits {
        fn drop(&mut self) {
            set_from_rules(&GameRules::default());
        }
    }

    #[test]
    fn rules_update_all_three_limits() {
        let _guard = TEST_LOCK.lock().unwrap();
        let _reset = ResetLimits;
        let mut rules = GameRules::default();
        rules.set("max_command_forks", "12").unwrap();
        rules.set("max_command_sequence_length", "34").unwrap();
        rules.set("max_block_modifications", "56").unwrap();
        set_from_rules(&rules);
        assert_eq!(max_forks(), 12);
        assert_eq!(max_sequence(), 34);
        assert_eq!(max_block_modifications(), 56);
        // Restore the defaults so other tests in this process are not affected.
        set_from_rules(&GameRules::default());
        assert_eq!(max_forks(), DEFAULT_MAX_FORKS);
        assert_eq!(max_sequence(), DEFAULT_MAX_SEQUENCE);
        assert_eq!(max_block_modifications(), DEFAULT_MAX_BLOCK_MODIFICATIONS);
    }
}
