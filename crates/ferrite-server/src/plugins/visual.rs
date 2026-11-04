use ferrite_plugin::{HostCall, PlayerCommand};

pub fn visual_command(call: &HostCall) -> Option<PlayerCommand> {
    if call.owner == "Player"
        && call.method == "sendExperienceChange(float)"
        && call.args.len() == 1
    {
        let player = call.subject.parse().ok()?;
        let progress: f32 = call.args[0].parse().ok()?;
        if !(0.0..=1.0).contains(&progress) {
            return None;
        }
        return Some(PlayerCommand::VisualExperienceCurrentLevel { player, progress });
    }
    if call.owner == "Player"
        && call.method == "sendExperienceChange(float,int)"
        && call.args.len() == 2
    {
        let player = call.subject.parse().ok()?;
        let progress: f32 = call.args[0].parse().ok()?;
        let level: i32 = call.args[1].parse().ok()?;
        if !(0.0..=1.0).contains(&progress) || level < 0 {
            return None;
        }
        return Some(PlayerCommand::VisualExperience {
            player,
            progress,
            level,
        });
    }
    if call.owner != "Player"
        || call.method != "sendHealthUpdate(double,int,float)"
        || call.args.len() != 3
    {
        return None;
    }
    let player = call.subject.parse().ok()?;
    let health: f32 = call.args[0].parse().ok()?;
    let food: i32 = call.args[1].parse().ok()?;
    let saturation: f32 = call.args[2].parse().ok()?;
    if !health.is_finite()
        || health < 0.0
        || !(0..=20).contains(&food)
        || !saturation.is_finite()
        || saturation < 0.0
    {
        return None;
    }
    Some(PlayerCommand::VisualHealth {
        player,
        health,
        food,
        saturation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn call(args: &[&str]) -> HostCall {
        HostCall {
            owner: "Player".into(),
            subject: "00000000-0000-0000-0000-000000000007".into(),
            method: "sendHealthUpdate(double,int,float)".into(),
            args: args.iter().map(|s| (*s).into()).collect(),
        }
    }
    #[test]
    fn health_update_produces_a_client_only_packet_command() {
        assert_eq!(
            visual_command(&call(&["12.5", "9", "2.5"])),
            Some(PlayerCommand::VisualHealth {
                player: "00000000-0000-0000-0000-000000000007".parse().unwrap(),
                health: 12.5,
                food: 9,
                saturation: 2.5
            })
        );
    }
    #[test]
    fn experience_update_produces_a_client_only_packet_command() {
        let mut call = call(&["0.75", "12"]);
        call.method = "sendExperienceChange(float,int)".into();
        assert_eq!(
            visual_command(&call),
            Some(PlayerCommand::VisualExperience {
                player: "00000000-0000-0000-0000-000000000007".parse().unwrap(),
                progress: 0.75,
                level: 12
            })
        );
        call.args[0] = "NaN".into();
        assert_eq!(visual_command(&call), None);
        call.args = vec!["0.5".into(), "-1".into()];
        assert_eq!(visual_command(&call), None);
    }
    #[test]
    fn experience_update_without_level_is_accepted() {
        let mut call = call(&["0.25"]);
        call.method = "sendExperienceChange(float)".into();
        assert_eq!(
            visual_command(&call),
            Some(PlayerCommand::VisualExperienceCurrentLevel {
                player: "00000000-0000-0000-0000-000000000007".parse().unwrap(),
                progress: 0.25
            })
        );
    }
    #[test]
    fn health_update_rejects_invalid_values() {
        for args in [
            ["NaN", "9", "2"],
            ["1", "21", "2"],
            ["1", "9", "inf"],
            ["-1", "9", "2"],
        ] {
            assert_eq!(visual_command(&call(&args)), None);
        }
    }
}
