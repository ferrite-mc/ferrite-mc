//! Facts about an online player that only the game loop knows, answered to plugin queries:
//! the network entity id, the protocol version, when the session began and how long it has run.
//!
//! The game loop records a player when they join and forgets them when they leave; plugin
//! threads only read. A query about a player who is not recorded gets no answer, so the plugin
//! sees the usual "not implemented" error instead of an invented value.

use ferrite_plugin::HostCall;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerFacts {
    pub entity_id: i32,
    pub joined: SystemTime,
}

fn registry() -> &'static Mutex<HashMap<Uuid, PlayerFacts>> {
    static REGISTRY: OnceLock<Mutex<HashMap<Uuid, PlayerFacts>>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

pub fn record(player: Uuid, facts: PlayerFacts) {
    registry().lock().unwrap().insert(player, facts);
}

/// The network entity id of an online player.
pub fn entity_id(player: Uuid) -> Option<i32> {
    registry().lock().unwrap().get(&player).map(|f| f.entity_id)
}

pub fn forget(player: Uuid) {
    registry().lock().unwrap().remove(&player);
}

fn millis(time: SystemTime) -> u128 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
}

/// The answer to a read-only query, given the facts and the current time.
fn answer_at(facts: &PlayerFacts, method: &str, now: SystemTime) -> Option<String> {
    Some(match method {
        "getEntityId()" => facts.entity_id.to_string(),
        "getProtocolVersion()" => ferrite_protocol::PROTOCOL_VERSION.to_string(),
        "getTicksLived()" => {
            let elapsed = now.duration_since(facts.joined).unwrap_or(Duration::ZERO);
            (elapsed.as_millis() / 50).min(i32::MAX as u128).to_string()
        }
        "getLastLogin()" => millis(facts.joined).to_string(),
        // An online player is being seen and is playing right now.
        "getLastSeen()" | "getLastPlayed()" => millis(now).to_string(),
        "isInWorld()" | "isTicking()" => "true".to_owned(),
        _ => return None,
    })
}

/// The answer to `call`, or `None` when it is not one of these queries or the player is unknown.
pub fn answer(call: &HostCall) -> Option<String> {
    if call.owner != "Player" || !call.args.is_empty() {
        return None;
    }
    let player = Uuid::parse_str(&call.subject).ok()?;
    let facts = *registry().lock().unwrap().get(&player)?;
    answer_at(&facts, &call.method, SystemTime::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(player: Uuid, method: &str) -> HostCall {
        HostCall {
            owner: "Player".into(),
            subject: player.to_string(),
            method: method.into(),
            args: Vec::new(),
        }
    }

    #[test]
    fn recorded_players_answer_and_forgotten_ones_do_not() {
        let player = Uuid::from_u128(0xfac7);
        assert_eq!(answer(&call(player, "getEntityId()")), None);
        record(
            player,
            PlayerFacts {
                entity_id: 77,
                joined: SystemTime::now(),
            },
        );
        assert_eq!(
            answer(&call(player, "getEntityId()")).as_deref(),
            Some("77")
        );
        assert_eq!(
            answer(&call(player, "getProtocolVersion()")).as_deref(),
            Some(ferrite_protocol::PROTOCOL_VERSION.to_string().as_str())
        );
        forget(player);
        assert_eq!(answer(&call(player, "getEntityId()")), None);
    }

    #[test]
    fn ticks_lived_counts_fifty_milliseconds_each() {
        let joined = UNIX_EPOCH + Duration::from_secs(1_000);
        let facts = PlayerFacts {
            entity_id: 1,
            joined,
        };
        let later = joined + Duration::from_millis(2_025);
        assert_eq!(
            answer_at(&facts, "getTicksLived()", later).as_deref(),
            Some("40")
        );
        assert_eq!(
            answer_at(&facts, "getLastLogin()", later).as_deref(),
            Some("1000000")
        );
        assert_eq!(
            answer_at(&facts, "getLastSeen()", later).as_deref(),
            Some("1002025")
        );
        assert_eq!(answer_at(&facts, "getPing()", later), None);
    }

    #[test]
    fn queries_with_arguments_are_not_facts() {
        let player = Uuid::from_u128(0xfac8);
        record(
            player,
            PlayerFacts {
                entity_id: 5,
                joined: SystemTime::now(),
            },
        );
        let mut with_arg = call(player, "getEntityId()");
        with_arg.args.push("x".into());
        assert_eq!(answer(&with_arg), None);
        forget(player);
    }
}
