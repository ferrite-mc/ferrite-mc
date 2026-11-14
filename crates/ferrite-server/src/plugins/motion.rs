//! Plugin-driven player motion: `Player#setVelocity` and `Player#knockback`.
//!
//! The client simulates its own movement, so the server can only push it: both calls become one
//! `set_entity_motion` packet. Every number comes from plugin code, so non-finite values and
//! speeds the client could not have reached are rejected.

use ferrite_plugin::{HostCall, PlayerCommand};
use uuid::Uuid;

const PART: char = '\u{1d}';
/// Blocks per tick; vanilla caps a player's own motion far below this.
const MAX_SPEED: f64 = 10.0;

fn number(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// The motion command behind `call`, or `None` when it is some other call or its values are bad.
pub fn motion_command(call: &HostCall) -> Option<PlayerCommand> {
    if call.owner != "Player" {
        return None;
    }
    let player = Uuid::parse_str(&call.subject).ok()?;
    if call.method == "castBobber()" {
        return Some(PlayerCommand::CastBobber { player });
    }
    if let "saveData()" | "loadData()" = call.method.as_str() {
        return Some(PlayerCommand::PlayerData {
            player,
            load: call.method == "loadData()",
        });
    }
    if call.method == "fireworkBoost(int,int)" {
        let flight: u8 = call
            .args
            .first()?
            .parse()
            .ok()
            .filter(|f| (0..=127).contains(f))?;
        let explosions: u8 = call.args.get(1)?.parse().ok().filter(|n| *n <= 64)?;
        return Some(PlayerCommand::FireworkBoost {
            player,
            flight,
            explosions,
        });
    }
    let velocity = match call.method.as_str() {
        "setVelocity(Vector)" => {
            let mut parts = call.args.first()?.split(PART).map(number);
            let v = [parts.next()??, parts.next()??, parts.next()??];
            parts.next().is_none().then_some(v)?
        }
        // Vanilla `LivingEntity.knockback`: away from the direction, plus a hop when grounded.
        "knockback(double,double,double)" => {
            let strength = number(call.args.first()?)?;
            let (dx, dz) = (number(call.args.get(1)?)?, number(call.args.get(2)?)?);
            let length = (dx * dx + dz * dz).sqrt();
            if strength <= 0.0 || length < 1.0e-5 {
                return None;
            }
            [
                -dx / length * strength,
                strength.min(0.4),
                -dz / length * strength,
            ]
        }
        _ => return None,
    };
    velocity
        .iter()
        .all(|v| v.abs() <= MAX_SPEED)
        .then_some(PlayerCommand::Velocity { player, velocity })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(method: &str, args: &[&str]) -> HostCall {
        HostCall {
            owner: "Player".into(),
            subject: "00000000-0000-0000-0000-000000000009".into(),
            method: method.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    fn velocity(call: HostCall) -> Option<[f64; 3]> {
        match motion_command(&call)? {
            PlayerCommand::Velocity { velocity, .. } => Some(velocity),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn set_velocity_pushes_the_player() {
        assert_eq!(
            velocity(call("setVelocity(Vector)", &["0.5\u{1d}1.25\u{1d}-2"])),
            Some([0.5, 1.25, -2.0])
        );
    }

    #[test]
    fn knockback_points_away_from_the_direction() {
        let v = velocity(call("knockback(double,double,double)", &["0.4", "1", "0"])).unwrap();
        assert!((v[0] + 0.4).abs() < 1e-9 && v[2].abs() < 1e-9 && (v[1] - 0.4).abs() < 1e-9);
    }

    #[test]
    fn rejects_untrusted_values() {
        for bad in [
            call("setVelocity(Vector)", &["NaN\u{1d}0\u{1d}0"]),
            call("setVelocity(Vector)", &["inf\u{1d}0\u{1d}0"]),
            call("setVelocity(Vector)", &["99\u{1d}0\u{1d}0"]),
            call("setVelocity(Vector)", &["1\u{1d}2"]),
            call("setVelocity(Vector)", &["1\u{1d}2\u{1d}3\u{1d}4"]),
            call("knockback(double,double,double)", &["0.4", "0", "0"]),
            call("knockback(double,double,double)", &["-1", "1", "0"]),
        ] {
            assert!(motion_command(&bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn a_firework_boost_carries_flight_and_star_counts_and_rejects_bad_ones() {
        let ok = motion_command(&call("fireworkBoost(int,int)", &["2", "3"]));
        assert!(matches!(
            ok,
            Some(PlayerCommand::FireworkBoost {
                flight: 2,
                explosions: 3,
                ..
            })
        ));
        assert!(motion_command(&call("fireworkBoost(int,int)", &["-1", "0"])).is_none());
        assert!(motion_command(&call("fireworkBoost(int,int)", &["1", "999"])).is_none());
    }

    #[test]
    fn save_and_load_data_become_player_data_commands() {
        for (method, load) in [("saveData()", false), ("loadData()", true)] {
            assert!(matches!(
                motion_command(&call(method, &[])),
                Some(PlayerCommand::PlayerData { load: l, .. }) if l == load
            ));
        }
    }

    #[test]
    fn a_bobber_cast_becomes_a_command() {
        assert!(matches!(
            motion_command(&call("castBobber()", &[])),
            Some(PlayerCommand::CastBobber { .. })
        ));
    }
}
