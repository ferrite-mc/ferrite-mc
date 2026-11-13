//! `Player#getShoulderEntity*`, `setShoulderEntity*` and `release*ShoulderEntity`.
//!
//! The shoulder table is the game's per-player state (`game::ride`), keyed by player uuid, so reading it from the
//! plugin thread touches no region. Writes are commands the player's own region applies.

use ferrite_plugin::{HostCall, PlayerCommand, ShoulderOp};
use uuid::Uuid;

fn side(text: &str) -> Option<bool> {
    match text {
        "LEFT" => Some(true),
        "RIGHT" => Some(false),
        _ => None,
    }
}

/// `shoulder(String,String)`: the side (`LEFT`/`RIGHT`) and `SEAT:<uuid>`, `CLEAR` or `RELEASE`.
pub fn command(call: &HostCall) -> Option<PlayerCommand> {
    if call.owner != "Player" || call.method != "shoulder(String,String)" {
        return None;
    }
    let op = call.args.get(1)?.as_str();
    let op = match op {
        "CLEAR" => ShoulderOp::Clear,
        "RELEASE" => ShoulderOp::Release,
        _ => ShoulderOp::Seat(Uuid::parse_str(op.strip_prefix("SEAT:")?).ok()?),
    };
    Some(PlayerCommand::Shoulder {
        player: Uuid::parse_str(&call.subject).ok()?,
        left: side(call.args.first()?)?,
        op,
    })
}

/// `getShoulder(String)`: `uuid,variant` of the parrot on that side, or empty.
pub fn answer(call: &HostCall) -> Option<String> {
    if call.owner != "Player" || call.method != "getShoulder(String)" {
        return None;
    }
    let player = Uuid::parse_str(&call.subject).ok()?;
    let side = if side(call.args.first()?)? {
        ferrite_entity::shoulder::ShoulderSide::Left
    } else {
        ferrite_entity::shoulder::ShoulderSide::Right
    };
    Some(
        crate::game::ride::shoulders_of(player)
            .get(side)
            .map(|parrot| format!("{},{}", parrot.uuid, parrot.variant))
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(method: &str, args: &[&str]) -> HostCall {
        HostCall {
            owner: "Player".into(),
            subject: "00000000-0000-0000-0000-000000000051".into(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    #[test]
    fn shoulder_calls_become_commands_and_bad_ones_are_dropped() {
        let parrot = "00000000-0000-0000-0000-0000000000aa";
        let seat = format!("SEAT:{parrot}");
        assert_eq!(
            command(&call("shoulder(String,String)", &["LEFT", &seat])),
            Some(PlayerCommand::Shoulder {
                player: Uuid::from_u128(0x51),
                left: true,
                op: ShoulderOp::Seat(Uuid::parse_str(parrot).unwrap())
            })
        );
        assert!(matches!(
            command(&call("shoulder(String,String)", &["RIGHT", "CLEAR"])),
            Some(PlayerCommand::Shoulder {
                left: false,
                op: ShoulderOp::Clear,
                ..
            })
        ));
        assert!(matches!(
            command(&call("shoulder(String,String)", &["RIGHT", "RELEASE"])),
            Some(PlayerCommand::Shoulder {
                op: ShoulderOp::Release,
                ..
            })
        ));
        for bad in [
            call("shoulder(String,String)", &["UP", "CLEAR"]),
            call("shoulder(String,String)", &["LEFT", "SEAT:nope"]),
            call("shoulder(String,String)", &["LEFT", "EAT"]),
            call("shoulder(String,String)", &["LEFT"]),
            call("other()", &[]),
        ] {
            assert_eq!(command(&bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_answer_reads_the_game_table() {
        let player = Uuid::from_u128(0x52);
        let mut bad = call("getShoulder(String)", &["LEFT"]);
        bad.subject = player.to_string();
        assert_eq!(answer(&bad).as_deref(), Some(""), "an empty shoulder");
        assert_eq!(answer(&call("getShoulder(String)", &["SIDE"])), None);
    }
}
