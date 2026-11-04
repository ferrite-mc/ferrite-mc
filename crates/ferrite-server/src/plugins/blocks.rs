use ferrite_plugin::{HostCall, PlayerCommand};

pub fn block_command(call: &HostCall) -> Option<PlayerCommand> {
    if call.owner != "Player"
        || call.method != "sendBlockDamage(Location,float,int)"
        || call.args.len() != 3
    {
        return None;
    }
    let player = call.subject.parse().ok()?;
    let (world, x, y, z) = super::location_block(&call.args[0])?;
    if world != ferrite_plugin::default_world_id()
        || !(-33_554_432..=33_554_431).contains(&x)
        || !(-2048..=2047).contains(&y)
        || !(-33_554_432..=33_554_431).contains(&z)
    {
        return None;
    }
    let value: f32 = call.args[1].parse().ok()?;
    if !(0.0..=1.0).contains(&value) {
        return None;
    }
    let source: i32 = call.args[2].parse().ok()?;
    let position = ferrite_protocol::types::BlockPos::new(x as i32, y as i32, z as i32).pack();
    Some(PlayerCommand::VisualBlockDamage {
        player,
        position,
        source,
        progress: if value == 0.0 {
            -1
        } else {
            (value * 9.0) as i8
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn call() -> HostCall {
        HostCall {
            owner: "Player".into(),
            subject: "00000000-0000-0000-0000-000000000007".into(),
            method: "sendBlockDamage(Location,float,int)".into(),
            args: vec![
                "world\u{1d}1\u{1d}64\u{1d}2\u{1d}0\u{1d}0".into(),
                "0.5".into(),
                "99".into(),
            ],
        }
    }
    #[test]
    fn block_damage_requests_visual_update() {
        assert_eq!(
            block_command(&call()),
            Some(PlayerCommand::VisualBlockDamage {
                player: "00000000-0000-0000-0000-000000000007".parse().unwrap(),
                position: ferrite_protocol::types::BlockPos::new(1, 64, 2).pack(),
                source: 99,
                progress: 4
            })
        );
    }
    #[test]
    fn zero_clears_damage_and_one_uses_last_stage() {
        let mut call = call();
        call.args[1] = "0".into();
        assert!(matches!(
            block_command(&call),
            Some(PlayerCommand::VisualBlockDamage { progress: -1, .. })
        ));
        call.args[1] = "1".into();
        assert!(matches!(
            block_command(&call),
            Some(PlayerCommand::VisualBlockDamage { progress: 9, .. })
        ));
    }
    #[test]
    fn block_damage_rejects_invalid_or_foreign_coordinates() {
        for value in ["NaN", "-0.1", "1.1"] {
            let mut call = call();
            call.args[1] = value.into();
            assert_eq!(block_command(&call), None);
        }
        let mut call = call();
        call.args[0] = "other\u{1d}1\u{1d}64\u{1d}2".into();
        assert_eq!(block_command(&call), None);
    }
}
