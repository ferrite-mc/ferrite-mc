pub fn break_ticks(state: u16) -> Option<u32> {
    let props = ferrite_data::block_props::get(state);
    if props.hardness < 0.0 {
        return None;
    }
    if props.hardness == 0.0 {
        return Some(1);
    }
    let divisor = if props.requires_correct_tool {
        100.0
    } else {
        30.0
    };
    Some((f64::from(props.hardness) * divisor).ceil().max(1.0) as u32)
}

/// Whether a player in `gamemode` (0 survival .. 3 spectator) with command `permission` may break
/// the block `state`, before any plugin event: vanilla `ServerPlayerGameMode.destroyBlock`.
/// Air never breaks, spectators and adventure players cannot, an unbreakable block (bedrock,
/// barrier, ...) only breaks in creative, and game-master blocks need an operator.
pub fn may_break(gamemode: i32, permission: u8, state: u32) -> bool {
    let Ok(state) = u16::try_from(state) else {
        return false;
    };
    if usize::from(state) >= ferrite_data::blocks::STATE_COUNT {
        return false;
    }
    let props = ferrite_data::block_props::get(state);
    if props.is_air {
        return false;
    }
    match gamemode {
        0 => {
            if props.hardness < 0.0 {
                return false;
            }
        }
        1 => {}
        _ => return false,
    }
    let game_master = ferrite_data::blocks::state(state).is_some_and(|(name, _)| {
        super::command_block::Mode::of_block(name).is_some()
            || matches!(
                name,
                "minecraft:structure_block" | "minecraft:jigsaw" | "minecraft:test_block"
            )
    });
    !game_master || permission >= 2
}

/// Vanilla `WallTorchBlock` variant of a torch item, when a torch is placed on a horizontal
/// face (`StandingAndWallBlockItem` picks the wall block for side placement).
fn wall_torch_name(name: &str) -> Option<&'static str> {
    let short = name.strip_prefix("minecraft:").unwrap_or(name);
    let wall = match short {
        "torch" => "minecraft:wall_torch",
        "soul_torch" => "minecraft:soul_wall_torch",
        "redstone_torch" => "minecraft:redstone_wall_torch",
        "copper_torch" => "minecraft:copper_wall_torch",
        _ => return None,
    };
    ferrite_data::blocks::default_state(wall).map(|_| wall)
}

pub fn placement_state(
    item_id: i32,
    face: i32,
    cursor_y: f32,
    yaw: f32,
    replaced: u16,
) -> Option<u16> {
    let name = ferrite_data::registries::entries("minecraft:item")?
        .iter()
        .find(|name| ferrite_data::registries::id("minecraft:item", name) == Some(item_id))?;
    // Vanilla `BlockItem.getPlacementState`: a torch on a side face becomes its wall variant.
    let name = if matches!(face, 2..=5) {
        wall_torch_name(name).unwrap_or(name)
    } else {
        name
    };
    let default = ferrite_data::blocks::default_state(name)?;
    let (_, defaults) = ferrite_data::blocks::state(default)?;
    let mut properties = defaults.to_vec();
    let water = ferrite_data::block_props::get(replaced)
        .fluid
        .is_some_and(|(kind, _, _)| {
            matches!(
                kind,
                ferrite_data::block_props::FluidKind::Water
                    | ferrite_data::block_props::FluidKind::FlowingWater
            )
        });
    let axis = match face {
        0 | 1 => "y",
        2 | 3 => "z",
        4 | 5 => "x",
        _ => return None,
    };
    let facing = match ((yaw / 90.0).round() as i32).rem_euclid(4) {
        0 => "north",
        1 => "east",
        2 => "south",
        _ => "west",
    };
    // Vanilla `HorizontalDirectionalBlock.getStateForPlacement`: the clicked face's direction.
    let clicked_facing = match face {
        2 => "north",
        3 => "south",
        4 => "west",
        5 => "east",
        _ => facing,
    };
    let upper = face == 0 || (face != 1 && cursor_y > 0.5);
    for (key, value) in &mut properties {
        *value = match *key {
            "axis" => axis,
            // `ButtonBlock` attach face: floor, ceiling or wall.
            "face" if name.ends_with("_button") => match face {
                0 => "ceiling",
                1 => "floor",
                _ => "wall",
            },
            // Wall torches and wall buttons face the clicked block.
            "facing"
                if (name.ends_with("wall_torch") || name.ends_with("_button"))
                    && ["north", "east", "south", "west"].contains(value) =>
            {
                clicked_facing
            }
            // `DoorBlock`, `FenceGateBlock` and `StairBlock` face the way the player looks
            // (`getHorizontalDirection`), unlike repeaters, chests and most others, which
            // face back at the player.
            "facing"
                if (name.ends_with("_door")
                    || name.ends_with("_fence_gate")
                    || name.ends_with("_stairs"))
                    && ["north", "east", "south", "west"].contains(value) =>
            {
                match facing {
                    "north" => "south",
                    "south" => "north",
                    "east" => "west",
                    _ => "east",
                }
            }
            "facing" if ["north", "east", "south", "west"].contains(value) => facing,
            "half" if name.contains("stairs") => {
                if upper {
                    "top"
                } else {
                    "bottom"
                }
            }
            "half" if name.contains("door") => "lower",
            "type" if name.contains("slab") => {
                if upper {
                    "top"
                } else {
                    "bottom"
                }
            }
            "waterlogged" => {
                if water {
                    "true"
                } else {
                    "false"
                }
            }
            _ => *value,
        };
    }
    ferrite_data::blocks::state_id(name, &properties).or(Some(default))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(name: &str) -> u32 {
        u32::from(ferrite_data::blocks::default_state(name).unwrap())
    }
    #[test]
    fn break_rules_follow_hardness_mode_and_permission() {
        let (air, stone, bedrock) = (id("air"), id("stone"), id("bedrock"));
        // Air has nothing to break, in any mode.
        for mode in 0..=3 {
            assert!(!may_break(mode, 4, air), "air in mode {mode}");
        }
        assert!(may_break(0, 0, stone));
        assert!(may_break(1, 0, stone));
        assert!(!may_break(2, 4, stone), "adventure players cannot break");
        assert!(!may_break(3, 4, stone), "spectators cannot break");
        // Bedrock only gives way to creative.
        assert!(!may_break(0, 4, bedrock));
        assert!(may_break(1, 0, bedrock));
        // Game-master blocks need an operator even in creative.
        let command = id("command_block");
        assert!(!may_break(1, 0, command));
        assert!(may_break(1, 2, command));
        assert!(!may_break(0, 4, id("barrier")));
        // A state number outside the table is never a break.
        assert!(!may_break(1, 4, u32::MAX));
    }

    #[test]
    fn tool_less_stone_and_log_axis() {
        let stone = ferrite_data::blocks::default_state("stone").unwrap();
        assert_eq!(break_ticks(stone), Some(150));
        let item = ferrite_data::registries::id("minecraft:item", "minecraft:oak_log").unwrap();
        let state = placement_state(item, 5, 0.5, 0.0, 0).unwrap();
        assert_eq!(
            ferrite_data::blocks::state(state)
                .unwrap()
                .1
                .iter()
                .find(|(k, _)| *k == "axis")
                .unwrap()
                .1,
            "x"
        );
    }
    #[test]
    fn stairs_half_facing_and_waterlogging() {
        let item = ferrite_data::registries::id("minecraft:item", "minecraft:oak_stairs").unwrap();
        let water = ferrite_data::blocks::default_state("minecraft:water").unwrap();
        let state = placement_state(item, 2, 0.8, 0.0, water).unwrap();
        let (_, props) = ferrite_data::blocks::state(state).unwrap();
        // Yaw 0 looks south: stairs, doors and gates face the way the player looks.
        assert!(props.contains(&("facing", "south")));
        assert!(props.contains(&("half", "top")));
        assert!(props.contains(&("waterlogged", "true")));
    }
    #[test]
    fn doors_and_gates_face_the_look_while_diodes_face_back_at_the_player() {
        let facing_of = |item: &str, yaw: f32| {
            let item = ferrite_data::registries::id("minecraft:item", &format!("minecraft:{item}"))
                .unwrap();
            let state = placement_state(item, 1, 0.5, yaw, 0).unwrap();
            let (_, props) = ferrite_data::blocks::state(state).unwrap();
            props.iter().find(|(k, _)| *k == "facing").unwrap().1
        };
        // Yaw 0 looks south, yaw 90 looks west.
        assert_eq!(facing_of("oak_door", 0.0), "south");
        assert_eq!(facing_of("oak_fence_gate", 90.0), "west");
        assert_eq!(
            facing_of("repeater", 0.0),
            "north",
            "`DiodeBlock` faces the player"
        );
        assert_eq!(facing_of("comparator", 90.0), "east");
    }
    #[test]
    fn torch_on_a_side_face_becomes_the_matching_wall_torch() {
        let item = ferrite_data::registries::id("minecraft:item", "minecraft:soul_torch").unwrap();
        let state = placement_state(item, 2, 0.5, 0.0, 0).unwrap();
        assert_eq!(
            ferrite_data::blocks::state(state).unwrap().0,
            "minecraft:soul_wall_torch"
        );
        assert!(
            ferrite_data::blocks::state(state)
                .unwrap()
                .1
                .contains(&("facing", "north"))
        );
        let item = ferrite_data::registries::id("minecraft:item", "minecraft:torch").unwrap();
        let state = placement_state(item, 1, 0.5, 0.0, 0).unwrap();
        assert_eq!(
            ferrite_data::blocks::state(state).unwrap().0,
            "minecraft:torch"
        );
    }
    #[test]
    fn wall_button_gets_the_clicked_face_and_facing() {
        let item = ferrite_data::registries::id("minecraft:item", "minecraft:oak_button").unwrap();
        let state = placement_state(item, 5, 0.5, 0.0, 0).unwrap();
        let (_, props) = ferrite_data::blocks::state(state).unwrap();
        assert!(props.contains(&("face", "wall")));
        assert!(props.contains(&("facing", "east")));
    }
}
