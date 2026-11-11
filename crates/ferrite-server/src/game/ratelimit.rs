//! Vanilla chat and command spam throttling (`ServerGamePacketListenerImpl.chatSpamTickCount`
//! and `commandSpamTickCount`).
//!
//! Every message adds 20 to its counter and a server tick removes one. Reaching more than 200
//! disconnects with `disconnect.spam`, so a burst of ten fits and then one message per second is
//! sustained. Operators are exempt, like vanilla.
//!
//! The counters are process-wide because the game loop, the global region and the command
//! dispatcher all see the same players. They are tiny bounded maps (online players only) and
//! every access is a short mutex hold, not a world-state lock.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};
use uuid::Uuid;

/// Added by one chat message or command.
const INCREMENT: u32 = 20;
/// Above this many points a non-operator is disconnected.
const LIMIT: u32 = 200;

#[derive(Default)]
struct Counters {
    chat: u32,
    command: u32,
}

fn counters() -> MutexGuard<'static, HashMap<Uuid, Counters>> {
    static COUNTERS: OnceLock<Mutex<HashMap<Uuid, Counters>>> = OnceLock::new();
    COUNTERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

/// Advances both counters by one tick.
pub fn tick() {
    for counters in counters().values_mut() {
        counters.chat = counters.chat.saturating_sub(1);
        counters.command = counters.command.saturating_sub(1);
    }
}

/// Drops a player's counters on disconnect.
pub fn forget(player: Uuid) {
    counters().remove(&player);
}

/// Charges one chat message; `false` means the player must be disconnected.
pub fn charge_chat(player: Uuid, operator: bool) -> bool {
    charge(player, operator, |counters| &mut counters.chat)
}

/// Charges one command; `false` means the player must be disconnected.
pub fn charge_command(player: Uuid, operator: bool) -> bool {
    charge(player, operator, |counters| &mut counters.command)
}

fn charge(player: Uuid, operator: bool, select: impl FnOnce(&mut Counters) -> &mut u32) -> bool {
    let mut all = counters();
    let counter = select(all.entry(player).or_default());
    *counter = counter.saturating_add(INCREMENT);
    operator || *counter <= LIMIT
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `tick` advances every player's counters, so tests that tick must not interleave.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock() -> MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner())
    }

    #[test]
    fn ten_messages_fit_then_the_bucket_drains_one_point_per_tick() {
        let _guard = lock();
        let player = Uuid::from_u128(0x5a0);
        forget(player);
        for message in 0..10 {
            assert!(charge_chat(player, false), "message {message}");
        }
        assert!(
            !charge_chat(player, false),
            "the eleventh message exceeds the vanilla limit"
        );
        // 220 points, one removed per tick: four seconds have to pass before the next message.
        for _ in 0..39 {
            tick();
        }
        assert!(!charge_chat(player, false), "still over the limit");
        for _ in 0..21 {
            tick();
        }
        assert!(
            charge_chat(player, false),
            "a message per second is sustained"
        );
        forget(player);
        assert!(charge_chat(player, false), "forget resets the counters");
    }

    #[test]
    fn commands_have_their_own_counter_and_operators_are_exempt() {
        let _guard = lock();
        let player = Uuid::from_u128(0x5b0);
        forget(player);
        for _ in 0..10 {
            assert!(charge_command(player, false));
        }
        assert!(!charge_command(player, false));
        assert!(
            charge_chat(player, false),
            "chat has not been charged by the command bucket"
        );
        assert!(
            charge_command(player, true),
            "operators bypass the command throttle"
        );
        assert!(
            charge_chat(player, true),
            "operators bypass the chat throttle"
        );
        forget(player);
    }
}
