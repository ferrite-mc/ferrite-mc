//! `World#getEnderDragonBattle`: the plugin view of the End fight.
//!
//! The fight state lives in the game state the End region ticks, so plugins read a snapshot the
//! region publishes every tick and leave their changes in a queue the region applies on its next
//! tick. Both only move while a player is in the End, which is when the fight is ticked at all.

use ferrite_plugin::HostCall;
use std::sync::{Mutex, OnceLock};

use crate::game::end_fight_view::{Request, Snapshot};

fn snapshot_slot() -> &'static Mutex<Snapshot> {
    static SLOT: OnceLock<Mutex<Snapshot>> = OnceLock::new();
    SLOT.get_or_init(Default::default)
}

fn queue() -> &'static Mutex<Vec<Request>> {
    static QUEUE: OnceLock<Mutex<Vec<Request>>> = OnceLock::new();
    QUEUE.get_or_init(Default::default)
}

/// Publishes the fight as the End region sees it after a tick.
pub fn publish(snapshot: Snapshot) {
    *snapshot_slot().lock().unwrap() = snapshot;
}

/// The changes plugins asked for since the last tick.
pub fn take_requests() -> Vec<Request> {
    std::mem::take(&mut *queue().lock().unwrap())
}

/// The reply to `dragonBattle()`: `started,killed,gateways,dragon_alive,stage,portal_level`
/// (`stage` is `none` and `portal_level` is `-` when there is none).
fn describe(snapshot: &Snapshot) -> String {
    format!(
        "{},{},{},{},{},{}",
        snapshot.started,
        snapshot.killed,
        snapshot.gateways,
        snapshot.dragon_alive,
        snapshot.stage.map_or("none", |stage| stage.name()),
        snapshot
            .portal_level
            .map_or_else(|| "-".to_owned(), |level| level.to_string()),
    )
}

pub fn answer(call: &HostCall) -> Option<String> {
    if call.owner != "World" {
        return None;
    }
    let request = match call.method.as_str() {
        "dragonBattle()" => return Some(describe(&snapshot_slot().lock().unwrap())),
        "dragonBattleSetKilled(boolean)" => Request::SetKilled(flag(call.args.first()?)?),
        "dragonBattleGeneratePortal(boolean)" => Request::GeneratePortal(flag(call.args.first()?)?),
        "dragonBattleInitiateRespawn()" => Request::InitiateRespawn,
        "dragonBattleSpawnGateway()" => Request::SpawnGateway,
        "dragonBattleResetCrystals()" => Request::ResetCrystals,
        "dragonBattleSpawnGatewayAt(int,int,int)" => {
            let coordinate = |index: usize| call.args.get(index)?.parse::<i32>().ok();
            Request::SpawnGatewayAt([coordinate(0)?, coordinate(1)?, coordinate(2)?])
        }
        "dragonBattleSetPhase(String)" => {
            let name = call.args.first()?;
            Request::SetPhase(match name.as_str() {
                "none" | "end" => None,
                other => Some(crate::game::end_fight_view::stage(other)?),
            })
        }
        _ => return None,
    };
    queue().lock().unwrap().push(request);
    Some(String::new())
}

fn flag(text: &str) -> Option<bool> {
    match text {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(method: &str, args: &[&str]) -> HostCall {
        HostCall {
            owner: "World".into(),
            subject: "00000000-0000-0000-0000-000000000001".into(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    #[test]
    fn requests_queue_up_and_bad_ones_are_refused() {
        take_requests();
        assert_eq!(
            answer(&call("dragonBattleSetKilled(boolean)", &["true"])).as_deref(),
            Some("")
        );
        assert_eq!(
            answer(&call(
                "dragonBattleSetPhase(String)",
                &["summoning_pillars"]
            ))
            .as_deref(),
            Some("")
        );
        assert_eq!(
            answer(&call("dragonBattleSetPhase(String)", &["none"])).as_deref(),
            Some("")
        );
        assert_eq!(
            answer(&call("dragonBattleSetPhase(String)", &["nonsense"])),
            None
        );
        assert_eq!(
            answer(&call("dragonBattleSetKilled(boolean)", &["maybe"])),
            None
        );
        assert_eq!(
            answer(&call("dragonBattleResetCrystals()", &[])).as_deref(),
            Some("")
        );
        assert_eq!(
            answer(&call(
                "dragonBattleSpawnGatewayAt(int,int,int)",
                &["1", "70", "-3"]
            ))
            .as_deref(),
            Some("")
        );
        assert_eq!(
            answer(&call(
                "dragonBattleSpawnGatewayAt(int,int,int)",
                &["1", "x", "-3"]
            )),
            None
        );
        assert_eq!(take_requests().len(), 5);
        assert!(take_requests().is_empty());
    }

    #[test]
    fn the_published_fight_is_described() {
        publish(Snapshot {
            started: true,
            killed: true,
            gateways: 2,
            dragon_alive: false,
            stage: None,
            portal_level: Some(64),
        });
        assert_eq!(
            answer(&call("dragonBattle()", &[])).as_deref(),
            Some("true,true,2,false,none,64")
        );
    }
}
