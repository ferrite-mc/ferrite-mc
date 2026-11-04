//! Item cooldowns set by plugins (`Player#setCooldown`, `resetCooldown`, `hasCooldown`,
//! `getCooldown`).
//!
//! The client greys out the item and refuses to use it while a `cooldown` packet is active; the
//! host keeps the expiry so the plugin can read it back. The server does not yet refuse the use
//! itself, so a modified client could ignore it.

use ferrite_plugin::{HostCall, PlayerCommand};
use std::{collections::HashMap, time::Instant};
use uuid::Uuid;

const PART: char = '\u{1d}';
const TICK_MILLIS: u128 = 50;
/// A day of ticks; anything longer is almost certainly a mistake.
const MAX_TICKS: i32 = 1_728_000;

/// When each item cooldown of each player ends.
#[derive(Default)]
pub struct Cooldowns {
    ends: HashMap<(Uuid, String), Instant>,
}

/// `MATERIAL`, `MATERIAL<US>amount` or `namespace:key` to the cooldown group key.
fn group(arg: &str) -> Option<String> {
    let name = arg.split(PART).next()?;
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '.' | '-' | '/'));
    if !valid || matches!(name, "AIR" | "CAVE_AIR" | "VOID_AIR") {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    let key = if lower.contains(':') {
        lower
    } else {
        format!("minecraft:{lower}")
    };
    ferrite_item::item_id(&key).map(|_| key)
}

fn remaining_ticks(end: Instant, now: Instant) -> i32 {
    end.checked_duration_since(now)
        .map_or(0, |left| left.as_millis().div_ceil(TICK_MILLIS) as i32)
}

impl Cooldowns {
    /// Handles a cooldown call: the command to send (if any) and the answer for the plugin.
    pub fn handle(
        &mut self,
        call: &HostCall,
        now: Instant,
    ) -> Option<(Option<PlayerCommand>, String)> {
        if call.owner != "Player" {
            return None;
        }
        let player = Uuid::parse_str(&call.subject).ok()?;
        let item = group(call.args.first()?)?;
        let key = (player, item.clone());
        match call.method.as_str() {
            "setCooldown(Material,int)" | "setCooldown(ItemStack,int)" | "setCooldown(Key,int)" => {
                let ticks = call.args.get(1)?.parse::<i32>().ok()?;
                if !(0..=MAX_TICKS).contains(&ticks) {
                    return None;
                }
                if ticks == 0 {
                    self.ends.remove(&key);
                } else {
                    self.ends.insert(
                        key,
                        now + std::time::Duration::from_millis(ticks as u64 * 50),
                    );
                }
                Some((
                    Some(PlayerCommand::Cooldown {
                        player,
                        group: item,
                        ticks,
                    }),
                    String::new(),
                ))
            }
            "resetCooldown(Material)" | "resetCooldown(ItemStack)" | "resetCooldown(Key)" => {
                self.ends.remove(&key);
                Some((
                    Some(PlayerCommand::Cooldown {
                        player,
                        group: item,
                        ticks: 0,
                    }),
                    String::new(),
                ))
            }
            "hasCooldown(Material)" | "hasCooldown(ItemStack)" | "hasCooldown(Key)" => {
                let left = self
                    .ends
                    .get(&key)
                    .map_or(0, |end| remaining_ticks(*end, now));
                Some((None, (left > 0).to_string()))
            }
            "getCooldown(Material)" | "getCooldown(ItemStack)" | "getCooldown(Key)" => {
                let left = self
                    .ends
                    .get(&key)
                    .map_or(0, |end| remaining_ticks(*end, now));
                Some((None, left.to_string()))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn call(method: &str, args: &[&str]) -> HostCall {
        HostCall {
            owner: "Player".into(),
            subject: "00000000-0000-0000-0000-00000000000b".into(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    #[test]
    fn a_cooldown_is_sent_and_counts_down() {
        let mut cooldowns = Cooldowns::default();
        let start = Instant::now();
        let (command, reply) = cooldowns
            .handle(
                &call("setCooldown(Material,int)", &["ENDER_PEARL", "40"]),
                start,
            )
            .unwrap();
        assert_eq!(reply, "");
        assert!(matches!(
            command,
            Some(PlayerCommand::Cooldown { ref group, ticks: 40, .. }) if group == "minecraft:ender_pearl"
        ));
        let ask = |method: &str, at: Instant, cooldowns: &mut Cooldowns| {
            cooldowns
                .handle(&call(method, &["ENDER_PEARL"]), at)
                .unwrap()
                .1
        };
        assert_eq!(ask("hasCooldown(Material)", start, &mut cooldowns), "true");
        assert_eq!(
            ask(
                "getCooldown(Material)",
                start + Duration::from_millis(1000),
                &mut cooldowns
            ),
            "20"
        );
        assert_eq!(
            ask(
                "hasCooldown(Material)",
                start + Duration::from_millis(2100),
                &mut cooldowns
            ),
            "false"
        );
    }

    #[test]
    fn reset_and_zero_clear_the_cooldown() {
        let mut cooldowns = Cooldowns::default();
        let now = Instant::now();
        cooldowns.handle(
            &call("setCooldown(Material,int)", &["SNOWBALL", "100"]),
            now,
        );
        let (command, _) = cooldowns
            .handle(&call("resetCooldown(Material)", &["SNOWBALL"]), now)
            .unwrap();
        assert!(matches!(
            command,
            Some(PlayerCommand::Cooldown { ticks: 0, .. })
        ));
        let (_, has) = cooldowns
            .handle(&call("hasCooldown(Material)", &["SNOWBALL"]), now)
            .unwrap();
        assert_eq!(has, "false");
    }

    #[test]
    fn rejects_air_unknown_items_and_bad_ticks() {
        let mut cooldowns = Cooldowns::default();
        let now = Instant::now();
        for bad in [
            call("setCooldown(Material,int)", &["AIR", "10"]),
            call("setCooldown(Material,int)", &["NOT_AN_ITEM", "10"]),
            call("setCooldown(Material,int)", &["SNOWBALL", "-1"]),
            call("setCooldown(Material,int)", &["SNOWBALL", "99999999"]),
            call("setCooldown(Material,int)", &["SNOWBALL", "x"]),
            call("setCooldown(Material,int)", &["SNOW BALL", "5"]),
        ] {
            assert!(cooldowns.handle(&bad, now).is_none(), "{bad:?}");
        }
    }
}
