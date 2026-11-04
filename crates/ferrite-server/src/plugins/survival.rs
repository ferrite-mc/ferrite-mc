//! Survival numbers plugins set directly: saturation, exhaustion, absorption, fall distance and
//! air. Each one is real game state (fall distance decides fall damage, air decides drowning),
//! not just something the client shows.

use ferrite_plugin::{HostCall, PlayerCommand, SurvivalStat};
use uuid::Uuid;

/// The command behind a `set...` call, or `None` for other calls and out-of-range values.
pub fn survival_command(call: &HostCall) -> Option<PlayerCommand> {
    if call.owner != "Player" {
        return None;
    }
    let player = Uuid::parse_str(&call.subject).ok()?;
    let value: f64 = call
        .args
        .first()?
        .parse()
        .ok()
        .filter(|v: &f64| v.is_finite())?;
    let (stat, range) = match call.method.as_str() {
        "setSaturation(float)" => (SurvivalStat::Saturation, 0.0..=20.0),
        "setExhaustion(float)" => (SurvivalStat::Exhaustion, 0.0..=40.0),
        "setAbsorptionAmount(double)" => (SurvivalStat::Absorption, 0.0..=1024.0),
        "setFallDistance(float)" => (SurvivalStat::FallDistance, 0.0..=1.0e6),
        "setRemainingAir(int)" => (SurvivalStat::Air, -20.0..=3600.0),
        _ => return None,
    };
    range.contains(&value).then_some(PlayerCommand::Survival {
        player,
        stat,
        value: value as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(method: &str, arg: &str) -> HostCall {
        HostCall {
            owner: "Player".into(),
            subject: "00000000-0000-0000-0000-00000000000c".into(),
            method: method.into(),
            args: vec![arg.into()],
        }
    }

    #[test]
    fn sets_each_stat() {
        for (method, arg, expected) in [
            ("setSaturation(float)", "3.5", SurvivalStat::Saturation),
            ("setExhaustion(float)", "2", SurvivalStat::Exhaustion),
            ("setAbsorptionAmount(double)", "4", SurvivalStat::Absorption),
            ("setFallDistance(float)", "12.5", SurvivalStat::FallDistance),
            ("setRemainingAir(int)", "150", SurvivalStat::Air),
        ] {
            assert!(
                matches!(
                    survival_command(&call(method, arg)),
                    Some(PlayerCommand::Survival { stat, .. }) if stat == expected
                ),
                "{method}"
            );
        }
    }

    #[test]
    fn rejects_values_the_game_cannot_hold() {
        for bad in [
            call("setSaturation(float)", "NaN"),
            call("setSaturation(float)", "-1"),
            call("setSaturation(float)", "21"),
            call("setExhaustion(float)", "inf"),
            call("setAbsorptionAmount(double)", "-0.5"),
            call("setFallDistance(float)", "-3"),
            call("setRemainingAir(int)", "100000"),
            call("setRemainingAir(int)", "abc"),
        ] {
            assert!(survival_command(&bad).is_none(), "{bad:?}");
        }
    }
}
