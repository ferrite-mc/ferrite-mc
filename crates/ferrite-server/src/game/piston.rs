//! Piston movement planning.

use ferrite_data::{block_props, blocks, piston_props};
use ferrite_math::Direction;
use std::collections::HashSet;

/// Read-only view of blocks owned by the current region. None means unavailable.
pub trait PistonWorld {
    fn block(&self, pos: [i32; 3]) -> Option<u16>;
    fn has_block_entity(&self, pos: [i32; 3]) -> bool;
}

impl PistonWorld for super::fluid::RegionFluidWorld<'_> {
    fn block(&self, pos: [i32; 3]) -> Option<u16> {
        super::redstone::RedstoneWorld::block(self, pos)
    }
    fn has_block_entity(&self, pos: [i32; 3]) -> bool {
        self.source.block_entity(pos[0], pos[1], pos[2]).is_some()
    }
}

/// A complete plan; no world mutation has happened when resolution returns.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub to_move: Vec<[i32; 3]>,
    pub to_destroy: Vec<[i32; 3]>,
}

fn offset([x, y, z]: [i32; 3], direction: Direction) -> [i32; 3] {
    let (dx, dy, dz) = direction.step();
    [x + dx, y + dy, z + dz]
}

pub(super) fn within_border(border: &super::commands::WorldBorderState, pos: [i32; 3]) -> bool {
    const LIMIT: f64 = 29_999_984.0;
    let half = border.diameter / 2.0;
    let x = f64::from(pos[0]);
    let z = f64::from(pos[2]);
    x >= (border.center_x - half).clamp(-LIMIT, LIMIT)
        && x < (border.center_x + half).clamp(-LIMIT, LIMIT)
        && z >= (border.center_z - half).clamp(-LIMIT, LIMIT)
        && z < (border.center_z + half).clamp(-LIMIT, LIMIT)
}

/// Check direct and quasi-connected power using the redstone engine's directional signal.
pub fn powered(
    pos: [i32; 3],
    facing: Direction,
    mut signal: impl FnMut([i32; 3], Direction) -> bool,
) -> bool {
    Direction::ALL
        .into_iter()
        .any(|side| side != facing && signal(pos, side))
        || Direction::ALL
            .into_iter()
            .any(|side| side != Direction::Down && signal(offset(pos, Direction::Up), side))
}

#[derive(PartialEq, Eq)]
enum Reaction {
    Air,
    Move,
    Block,
    Destroy,
    PushOnly,
}

fn reaction(world: &impl PistonWorld, pos: [i32; 3], state: u16) -> Reaction {
    let Some((name, props)) = blocks::state(state) else {
        return Reaction::Block;
    };
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    let physical = block_props::get(state);
    if physical.is_air {
        return Reaction::Air;
    }
    let piston = piston_props::get(state);
    if world.has_block_entity(pos) || piston.has_block_entity || physical.hardness < 0.0 {
        return Reaction::Block;
    }
    if matches!(name, "piston" | "sticky_piston") {
        return if props.iter().any(|&(k, v)| k == "extended" && v == "true") {
            Reaction::Block
        } else {
            Reaction::Move
        };
    }
    match piston.reaction {
        piston_props::PushReaction::PushPull | piston_props::PushReaction::IgnoreEntity => {
            Reaction::Move
        }
        piston_props::PushReaction::PushOnly => Reaction::PushOnly,
        piston_props::PushReaction::Destroy => Reaction::Destroy,
        piston_props::PushReaction::Immovable => Reaction::Block,
    }
}

fn sticky(state: u16) -> Option<&'static str> {
    let (name, _) = blocks::state(state)?;
    match name.strip_prefix("minecraft:").unwrap_or(name) {
        "slime_block" => Some("slime"),
        "honey_block" => Some("honey"),
        _ => None,
    }
}

/// Resolve an extension or a sticky retraction without mutating the world.
pub fn resolve(
    world: &impl PistonWorld,
    base: [i32; 3],
    facing: Direction,
    extending: bool,
) -> Option<Plan> {
    let direction = if extending { facing } else { facing.opposite() };
    let first = if extending {
        offset(base, facing)
    } else {
        offset(offset(base, facing), facing)
    };
    let mut plan = Plan::default();
    let mut visited = HashSet::new();
    let mut pending = vec![(first, true)];
    while let Some((pos, required)) = pending.pop() {
        if pos == base || visited.contains(&pos) {
            continue;
        }
        // Retraction removes the head before the pulled block enters its cell.
        if !extending && pos == offset(base, facing) {
            continue;
        }
        let state = world.block(pos)?;
        match reaction(world, pos, state) {
            Reaction::Air => continue,
            Reaction::Block | Reaction::PushOnly if !required => continue,
            Reaction::Block => return None,
            Reaction::PushOnly if !extending => return None,
            Reaction::Destroy if required && extending => {
                plan.to_destroy.push(pos);
                continue;
            }
            Reaction::Destroy => continue,
            _ => {}
        }
        if plan.to_move.len() == 12 {
            return None;
        }
        visited.insert(pos);
        plan.to_move.push(pos);
        pending.push((offset(pos, direction), true));
        if let Some(kind) = sticky(state) {
            for side in Direction::ALL {
                if side == direction {
                    continue;
                }
                let adjacent = offset(pos, side);
                if adjacent == base {
                    continue;
                }
                let other = world.block(adjacent)?;
                if sticky(other).is_some_and(|other_kind| other_kind != kind) {
                    continue;
                }
                pending.push((adjacent, false));
            }
        }
    }
    plan.to_move.sort_by_key(|pos| {
        let (x, y, z) = direction.step();
        -(i64::from(pos[0]) * i64::from(x)
            + i64::from(pos[1]) * i64::from(y)
            + i64::from(pos[2]) * i64::from(z))
    });
    Some(plan)
}

/// Convert a movement plan into expected-state changes for an atomic block batch.
pub fn changes(
    world: &impl PistonWorld,
    base: [i32; 3],
    facing: Direction,
    extending: bool,
    plan: &Plan,
) -> Option<Vec<([i32; 3], u32, u32)>> {
    let base_state = world.block(base)?;
    let (base_name, props) = blocks::state(base_state)?;
    let name = base_name.strip_prefix("minecraft:").unwrap_or(base_name);
    if !matches!(name, "piston" | "sticky_piston") {
        return None;
    }
    let facing_name = match facing {
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
    };
    if !props.contains(&("facing", facing_name))
        || !props.contains(&("extended", if extending { "false" } else { "true" }))
    {
        return None;
    }
    let next_base = blocks::state_id(
        base_name,
        &[
            ("facing", facing_name),
            ("extended", if extending { "true" } else { "false" }),
        ],
    )?;
    let air = blocks::default_state("air")?;
    let head = offset(base, facing);
    if !extending {
        let (head_name, head_props) = blocks::state(world.block(head)?)?;
        if head_name.strip_prefix("minecraft:").unwrap_or(head_name) != "piston_head"
            || !head_props.contains(&("facing", facing_name))
        {
            return None;
        }
    }
    let mut desired = std::collections::BTreeMap::new();
    desired.insert(base, next_base);
    desired.insert(
        head,
        if extending {
            blocks::state_id(
                "piston_head",
                &[
                    ("facing", facing_name),
                    (
                        "type",
                        if name == "sticky_piston" {
                            "sticky"
                        } else {
                            "normal"
                        },
                    ),
                    ("short", "false"),
                ],
            )?
        } else {
            air
        },
    );
    for &pos in plan.to_destroy.iter().chain(plan.to_move.iter()) {
        desired.insert(pos, air);
    }
    let direction = if extending { facing } else { facing.opposite() };
    for &pos in &plan.to_move {
        desired.insert(offset(pos, direction), world.block(pos)?);
    }
    // The head takes precedence over a vacated source during extension.
    if extending {
        desired.insert(
            head,
            blocks::state_id(
                "piston_head",
                &[
                    ("facing", facing_name),
                    (
                        "type",
                        if name == "sticky_piston" {
                            "sticky"
                        } else {
                            "normal"
                        },
                    ),
                    ("short", "false"),
                ],
            )?,
        );
    }
    desired
        .into_iter()
        .map(|(pos, next)| Some((pos, u32::from(world.block(pos)?), u32::from(next))))
        .collect()
}

/// Expected block changes and blocks whose drops must be spawned after commit.
pub struct Operation {
    pub edits: Vec<([i32; 3], u32, u32)>,
    pub destroyed: Vec<([i32; 3], u16)>,
}

/// Prepare a complete operation without modifying the world.
pub fn operation(
    world: &impl PistonWorld,
    base: [i32; 3],
    facing: Direction,
    extending: bool,
) -> Option<Operation> {
    let (name, _) = blocks::state(world.block(base)?)?;
    let sticky = name.strip_prefix("minecraft:").unwrap_or(name) == "sticky_piston";
    let plan = if extending {
        resolve(world, base, facing, true)?
    } else if sticky {
        resolve(world, base, facing, false).unwrap_or_default()
    } else {
        Plan::default()
    };
    let destroyed = plan
        .to_destroy
        .iter()
        .map(|&pos| Some((pos, world.block(pos)?)))
        .collect::<Option<Vec<_>>>()?;
    Some(Operation {
        edits: changes(world, base, facing, extending, &plan)?,
        destroyed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Grid(HashMap<[i32; 3], u16>);
    impl PistonWorld for Grid {
        fn block(&self, pos: [i32; 3]) -> Option<u16> {
            Some(*self.0.get(&pos).unwrap_or(&0))
        }
        fn has_block_entity(&self, _: [i32; 3]) -> bool {
            false
        }
    }
    fn state(name: &str) -> u16 {
        ferrite_data::blocks::default_state(name).unwrap()
    }

    #[test]
    fn twelve_blocks_move_but_thirteen_do_not() {
        let mut grid = Grid::default();
        for x in 1..=12 {
            grid.0.insert([x, 64, 0], state("stone"));
        }
        assert_eq!(
            resolve(&grid, [0, 64, 0], Direction::East, true)
                .unwrap()
                .to_move
                .len(),
            12
        );
        grid.0.insert([13, 64, 0], state("stone"));
        assert!(resolve(&grid, [0, 64, 0], Direction::East, true).is_none());
    }

    #[test]
    fn immovable_blocks_stop_and_breakable_blocks_are_destroyed() {
        let mut grid = Grid::default();
        grid.0.insert([1, 64, 0], state("obsidian"));
        assert!(resolve(&grid, [0, 64, 0], Direction::East, true).is_none());
        grid.0.insert([1, 64, 0], state("redstone_wire"));
        let plan = resolve(&grid, [0, 64, 0], Direction::East, true).unwrap();
        assert_eq!(plan.to_destroy, vec![[1, 64, 0]]);
    }

    #[test]
    fn state_metadata_prevents_moving_an_uninitialized_chest_and_pops_water() {
        let mut grid = Grid::default();
        grid.0.insert([1, 64, 0], state("chest"));
        assert!(resolve(&grid, [0, 64, 0], Direction::East, true).is_none());
        grid.0.insert([1, 64, 0], state("water"));
        assert_eq!(
            resolve(&grid, [0, 64, 0], Direction::East, true)
                .unwrap()
                .to_destroy,
            vec![[1, 64, 0]]
        );
    }

    #[test]
    fn an_unextended_piston_can_move_but_an_extended_one_cannot() {
        let mut grid = Grid::default();
        let target = [1, 64, 0];
        grid.0.insert(
            target,
            blocks::state_id("piston", &[("facing", "east"), ("extended", "false")]).unwrap(),
        );
        assert_eq!(
            resolve(&grid, [0, 64, 0], Direction::East, true)
                .unwrap()
                .to_move,
            vec![target]
        );
        grid.0.insert(
            target,
            blocks::state_id("piston", &[("facing", "east"), ("extended", "true")]).unwrap(),
        );
        assert!(resolve(&grid, [0, 64, 0], Direction::East, true).is_none());
    }

    #[test]
    fn slime_moves_a_side_block_but_not_honey() {
        let mut grid = Grid::default();
        grid.0.insert([1, 64, 0], state("slime_block"));
        grid.0.insert([1, 65, 0], state("stone"));
        grid.0.insert([1, 63, 0], state("honey_block"));
        let plan = resolve(&grid, [0, 64, 0], Direction::East, true).unwrap();
        assert!(plan.to_move.contains(&[1, 65, 0]));
        assert!(!plan.to_move.contains(&[1, 63, 0]));
    }

    #[test]
    fn slime_does_not_drag_glazed_terracotta_or_immovable_side_blocks() {
        let mut grid = Grid::default();
        grid.0.insert([1, 64, 0], state("slime_block"));
        grid.0.insert([1, 65, 0], state("white_glazed_terracotta"));
        grid.0.insert([1, 63, 0], state("obsidian"));
        let plan = resolve(&grid, [0, 64, 0], Direction::East, true).unwrap();
        assert_eq!(plan.to_move, vec![[1, 64, 0]]);
    }

    #[test]
    fn sticky_branches_share_the_twelve_block_limit() {
        let mut grid = Grid::default();
        grid.0.insert([1, 64, 0], state("slime_block"));
        for x in 2..=12 {
            grid.0.insert([x, 64, 0], state("stone"));
        }
        assert_eq!(
            resolve(&grid, [0, 64, 0], Direction::East, true)
                .unwrap()
                .to_move
                .len(),
            12
        );
        grid.0.insert([1, 65, 0], state("stone"));
        assert!(resolve(&grid, [0, 64, 0], Direction::East, true).is_none());
    }

    #[test]
    fn sticky_retraction_moves_the_block_into_the_head_cell() {
        let mut grid = Grid::default();
        grid.0.insert([1, 64, 0], state("piston_head"));
        grid.0.insert([2, 64, 0], state("stone"));
        let plan = resolve(&grid, [0, 64, 0], Direction::East, false).unwrap();
        assert_eq!(plan.to_move, vec![[2, 64, 0]]);
    }

    #[test]
    fn unloaded_destination_rejects_the_entire_plan() {
        struct Boundary;
        impl PistonWorld for Boundary {
            fn block(&self, pos: [i32; 3]) -> Option<u16> {
                match pos[0] {
                    1 => Some(state("stone")),
                    _ => None,
                }
            }
            fn has_block_entity(&self, _: [i32; 3]) -> bool {
                false
            }
        }
        assert!(resolve(&Boundary, [0, 64, 0], Direction::East, true).is_none());
    }

    #[tokio::test]
    async fn region_refuses_a_push_into_an_unowned_loaded_chunk() {
        use std::sync::{Arc, Mutex};
        let source = super::super::anvil::AnvilChunkSource::new_memory();
        for x in [0, 1] {
            loop {
                if let Some(result) = source.get(x, 0, 0) {
                    result.unwrap();
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
        let piston =
            blocks::state_id("piston", &[("facing", "east"), ("extended", "false")]).unwrap();
        let stone = state("stone");
        source.set_block(14, 70, 0, u32::from(piston)).unwrap();
        source.set_block(15, 70, 0, u32::from(stone)).unwrap();
        let held = HashMap::from([(ferrite_region::ChunkPos(0, 0), 1)]);
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let world = super::super::fluid::RegionFluidWorld::new(&source, &held, &tracking);
        assert!(operation(&world, [14, 70, 0], Direction::East, true).is_none());
        assert_eq!(source.block(14, 70, 0), Some(u32::from(piston)));
        assert_eq!(source.block(15, 70, 0), Some(u32::from(stone)));
    }

    #[test]
    fn world_border_includes_minimum_and_excludes_maximum() {
        let border = super::super::commands::WorldBorderState {
            center_x: 0.0,
            center_z: 0.0,
            diameter: 10.0,
            ..Default::default()
        };
        assert!(within_border(&border, [-5, 64, 0]));
        assert!(within_border(&border, [4, 64, 0]));
        assert!(!within_border(&border, [5, 64, 0]));
        assert!(!within_border(&border, [-6, 64, 0]));
    }

    #[test]
    fn extension_changes_move_a_line_and_create_a_head() {
        let mut grid = Grid::default();
        let base = [0, 64, 0];
        let piston =
            blocks::state_id("piston", &[("facing", "east"), ("extended", "false")]).unwrap();
        let stone = state("stone");
        grid.0.insert(base, piston);
        grid.0.insert([1, 64, 0], stone);
        let plan = resolve(&grid, base, Direction::East, true).unwrap();
        for (pos, _, next) in changes(&grid, base, Direction::East, true, &plan).unwrap() {
            grid.0.insert(pos, u16::try_from(next).unwrap());
        }
        assert_eq!(grid.block([2, 64, 0]), Some(stone));
        assert_eq!(
            grid.block([1, 64, 0]),
            blocks::state_id(
                "piston_head",
                &[("facing", "east"), ("type", "normal"), ("short", "false")]
            )
        );
        assert_eq!(
            grid.block(base),
            blocks::state_id("piston", &[("facing", "east"), ("extended", "true")])
        );
    }

    #[test]
    fn retraction_removes_head_and_pulls_a_block() {
        let mut grid = Grid::default();
        let base = [0, 64, 0];
        let piston =
            blocks::state_id("sticky_piston", &[("facing", "east"), ("extended", "true")]).unwrap();
        let head =
            blocks::state_id("piston_head", &[("facing", "east"), ("type", "sticky")]).unwrap();
        let stone = state("stone");
        grid.0.insert(base, piston);
        grid.0.insert([1, 64, 0], head);
        grid.0.insert([2, 64, 0], stone);
        let plan = resolve(&grid, base, Direction::East, false).unwrap();
        for (pos, _, next) in changes(&grid, base, Direction::East, false, &plan).unwrap() {
            grid.0.insert(pos, u16::try_from(next).unwrap());
        }
        assert_eq!(grid.block([1, 64, 0]), Some(stone));
        assert_eq!(grid.block([2, 64, 0]), Some(state("air")));
        assert_eq!(
            grid.block(base),
            blocks::state_id(
                "sticky_piston",
                &[("facing", "east"), ("extended", "false")]
            )
        );
    }

    #[test]
    fn sticky_retracts_even_when_its_target_cannot_be_pulled() {
        let mut grid = Grid::default();
        let base = [0, 64, 0];
        grid.0.insert(
            base,
            blocks::state_id("sticky_piston", &[("facing", "east"), ("extended", "true")]).unwrap(),
        );
        grid.0.insert(
            [1, 64, 0],
            blocks::state_id("piston_head", &[("facing", "east"), ("type", "sticky")]).unwrap(),
        );
        grid.0.insert([2, 64, 0], state("obsidian"));
        let operation = operation(&grid, base, Direction::East, false).unwrap();
        for (pos, _, next) in operation.edits {
            grid.0.insert(pos, u16::try_from(next).unwrap());
        }
        assert_eq!(grid.block([1, 64, 0]), Some(state("air")));
        assert_eq!(grid.block([2, 64, 0]), Some(state("obsidian")));
    }
}
