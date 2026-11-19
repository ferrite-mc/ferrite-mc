//! A* pathfinding on the block grid, with the vanilla `WalkNodeEvaluator` subset the modelled
//! mobs need: stand on a solid block (or float on water when `can_float`), walk straight or
//! diagonally, step up one block, drop up to three, pass open doors (and closed wooden doors when
//! `can_open_doors`), climb vertical walls for spiders (`can_climb`), penalize water (`water_cost`)
//! and penalize sky-exposed sunlit blocks (`avoid_sun`). Unloaded chunks are walls.
//!
//! Paths are small (at most [`MAX_PATH`] waypoints, [`MAX_NODES`] expanded nodes) because they
//! are computed on the region thread for a handful of mobs at a time, like vanilla's
//! `PathNavigation` region-limited search.

use crate::physics::BlockGetter;
use ferrite_data::block_props::{self, FluidKind};
use ferrite_data::blocks;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// Waypoints a single path keeps.
pub const MAX_PATH: usize = 16;
/// Expanded nodes before the search gives up.
pub const MAX_NODES: usize = 512;
/// Vanilla walkable node evaluator drops at most this far.
const MAX_DROP: i32 = 3;
/// Cost of a straight step, in tenths of a block.
const STRAIGHT: u32 = 10;
/// Cost of a diagonal step.
const DIAGONAL: u32 = 14;
/// Extra cost for a one-block step up or down.
const VERTICAL: u32 = 4;
/// Cost of climbing vertically one block up or down a solid wall (`WallClimberNavigation`).
const CLIMB_STEP: u32 = 18;
/// Default vanilla `PathType.WATER` malus (8.0 blocks = 80 tenths).
pub const DEFAULT_WATER_COST: u32 = 80;

/// Per-mob navigation capabilities and cost modifiers for [`find_path_with`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PathOptions {
    /// `WallClimberNavigation`: climb vertically up adjacent solid walls and fit in 1-block-tall
    /// spaces (spiders have height 0.9 -> `entityHeight = 1`).
    pub can_climb: bool,
    /// `WalkNodeEvaluator.canOpenDoors`: closed wooden doors (`#minecraft:wooden_doors`) are
    /// traversable (`WALKABLE_DOOR`), e.g. for door-breaking zombies.
    pub can_open_doors: bool,
    /// Extra per-node cost (in tenths of a block) for water nodes, or `None` when water is
    /// impassable (`PathType.WATER = -1.0`, e.g. endermen).
    pub water_cost: Option<u32>,
    /// `FloatGoal`: floats on the water surface instead of walking on the seafloor.
    pub can_float: bool,
    /// `RestrictSunGoal`: extra per-node cost on sky-visible bright blocks (`light >= 12`).
    pub avoid_sun: bool,
}

fn state(blocks: &impl BlockGetter, x: i32, y: i32, z: i32) -> Option<u16> {
    blocks
        .block_state(x, y, z)
        .filter(|state| usize::from(*state) < blocks::STATE_COUNT)
}

fn is_water(state: u16) -> bool {
    let props = block_props::get(state);
    matches!(
        props.fluid,
        Some((FluidKind::Water | FluidKind::FlowingWater, _, _))
    ) && block_props::collision_shape(state).is_empty()
}

fn is_lava(state: u16) -> bool {
    let props = block_props::get(state);
    matches!(
        props.fluid,
        Some((FluidKind::Lava | FluidKind::FlowingLava, _, _))
    )
}

fn is_door_passable(state: u16, can_open_doors: bool) -> bool {
    let Some((name, props)) = blocks::state(state) else {
        return false;
    };
    if !ferrite_data::tags::contains("block", "doors", name) {
        return false;
    }
    if props.iter().any(|(k, v)| *k == "open" && *v == "true") {
        return true;
    }
    can_open_doors && ferrite_data::tags::contains("block", "wooden_doors", name)
}

/// A block a mob can stand on: solid collision, not a fluid.
fn walkable_ground(state: u16) -> bool {
    let props = block_props::get(state);
    !props.is_air && props.fluid.is_none() && !block_props::collision_shape(state).is_empty()
}

/// A block a mob can occupy: air, collisionless non-fluid, open door, closed wooden door (when
/// `can_open_doors`), or water (when `water_cost.is_some()`).
fn passable_with(state: u16, options: &PathOptions) -> bool {
    if is_lava(state) {
        return false;
    }
    if is_door_passable(state, options.can_open_doors) {
        return true;
    }
    if is_water(state) {
        return options.water_cost.is_some();
    }
    let props = block_props::get(state);
    props.is_air || (props.fluid.is_none() && block_props::collision_shape(state).is_empty())
}

/// A solid wall face a spider can cling to while climbing vertically.
fn solid_wall(state: u16) -> bool {
    let props = block_props::get(state);
    !props.is_air
        && props.fluid.is_none()
        && !block_props::collision_shape(state).is_empty()
        && !is_door_passable(state, false)
}

fn has_adjacent_wall(blocks: &impl BlockGetter, x: i32, y: i32, z: i32) -> bool {
    [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .into_iter()
        .any(|(wx, wz)| state(blocks, x + wx, y, z + wz).is_some_and(solid_wall))
}

/// Evaluates whether a mob with `options` can occupy `(x, y, z)`, returning any traversal cost
/// penalty (e.g. water malus).
fn eval_node(
    blocks: &impl BlockGetter,
    x: i32,
    y: i32,
    z: i32,
    options: &PathOptions,
    allow_wall_cling: bool,
) -> Option<u32> {
    let below = state(blocks, x, y - 1, z)?;
    let feet = state(blocks, x, y, z)?;
    let head = state(blocks, x, y + 1, z)?;
    if !passable_with(feet, options) {
        return None;
    }
    let feet_water = is_water(feet);
    if feet_water && options.can_float {
        // Floaters stay on the water surface (`head` must not be water or solid).
        if is_water(head) || !passable_with(head, options) {
            return None;
        }
        return options.water_cost;
    }
    if !options.can_climb && !passable_with(head, options) {
        return None;
    }
    if walkable_ground(below) {
        let in_water = feet_water || (!options.can_climb && is_water(head));
        if in_water {
            return options.water_cost;
        }
        return Some(0);
    }
    if allow_wall_cling && options.can_climb && !feet_water && has_adjacent_wall(blocks, x, y, z) {
        return Some(0);
    }
    None
}

fn sunlight_penalty(
    blocks: &impl BlockGetter,
    x: i32,
    y: i32,
    z: i32,
    sky_cache: &mut HashMap<[i32; 3], bool>,
) -> u32 {
    let Some(level) = blocks.brightness(x, y + 1, z) else {
        return 0;
    };
    let magic = crate::sun::light_magic_value(level);
    if magic <= 0.5 {
        return 0;
    }
    let pos = [x, y + 1, z];
    let sees_sky = *sky_cache
        .entry(pos)
        .or_insert_with(|| crate::sun::sees_sky(blocks, pos[0], pos[1], pos[2]));
    if sees_sky {
        ((magic - 0.5) * 60.0).round().max(1.0) as u32
    } else {
        0
    }
}

/// The node a search starts from: the mob's own block, or the closest walkable level around it.
pub fn start_node(blocks: &impl BlockGetter, at: [i32; 3]) -> Option<[i32; 3]> {
    start_node_with(blocks, at, &PathOptions::default())
}

/// [`start_node`] respecting `options` (e.g. water surface, climb nodes, doors).
pub fn start_node_with(
    blocks: &impl BlockGetter,
    at: [i32; 3],
    options: &PathOptions,
) -> Option<[i32; 3]> {
    for dy in 0..=2 {
        let candidate = [at[0], at[1] + dy, at[2]];
        if eval_node(
            blocks,
            candidate[0],
            candidate[1],
            candidate[2],
            options,
            true,
        )
        .is_some()
        {
            return Some(candidate);
        }
    }
    for dy in 1..=MAX_DROP {
        let candidate = [at[0], at[1] - dy, at[2]];
        if eval_node(
            blocks,
            candidate[0],
            candidate[1],
            candidate[2],
            options,
            true,
        )
        .is_some()
        {
            return Some(candidate);
        }
    }
    None
}

/// A* from the block at `start` to the block at `goal`, as waypoint blocks (without the start
/// node), at most [`MAX_PATH`] long. An empty path means "no path found" or "already there".
pub fn find_path(
    blocks: &impl BlockGetter,
    start: [i32; 3],
    goal: [i32; 3],
    max_range: i32,
) -> Vec<[i32; 3]> {
    find_path_with(blocks, start, goal, max_range, &PathOptions::default())
}

/// [`find_path`] with per-mob [`PathOptions`] (wall climbing, doors, water, sunlight avoidance).
pub fn find_path_with(
    blocks: &impl BlockGetter,
    start: [i32; 3],
    goal: [i32; 3],
    max_range: i32,
    options: &PathOptions,
) -> Vec<[i32; 3]> {
    let Some(start) = start_node_with(blocks, start, options) else {
        return Vec::new();
    };
    let dx = goal[0] - start[0];
    let dz = goal[2] - start[2];
    if dx * dx + dz * dz > max_range * max_range {
        return Vec::new();
    }
    let heuristic = |p: [i32; 3]| -> u32 {
        let dx = (p[0] - goal[0]).unsigned_abs();
        let dz = (p[2] - goal[2]).unsigned_abs();
        let dy = (p[1] - goal[1]).unsigned_abs();
        dx.min(dz) * DIAGONAL + dx.abs_diff(dz) * STRAIGHT + dy * VERTICAL
    };
    let at_goal = |p: [i32; 3]| {
        (p[0] - goal[0]).abs() <= 1 && (p[2] - goal[2]).abs() <= 1 && (p[1] - goal[1]).abs() <= 1
    };
    let mut open: BinaryHeap<(std::cmp::Reverse<u32>, [i32; 3])> = BinaryHeap::new();
    let mut cost: HashMap<[i32; 3], u32> = HashMap::new();
    let mut parent: HashMap<[i32; 3], [i32; 3]> = HashMap::new();
    let mut closed: HashSet<[i32; 3]> = HashSet::new();
    let mut sky_cache: HashMap<[i32; 3], bool> = HashMap::new();
    cost.insert(start, 0);
    open.push((std::cmp::Reverse(heuristic(start)), start));
    let mut goal_node = None;
    let mut expanded = 0;
    while let Some((_, node)) = open.pop() {
        if at_goal(node) {
            goal_node = Some(node);
            break;
        }
        if !closed.insert(node) {
            continue;
        }
        expanded += 1;
        if expanded > MAX_NODES {
            break;
        }
        let base = cost[&node];
        let mut try_relax = |next: [i32; 3], step_cost: u32| {
            if closed.contains(&next) {
                return;
            }
            let sun = if options.avoid_sun {
                sunlight_penalty(blocks, next[0], next[1], next[2], &mut sky_cache)
            } else {
                0
            };
            let tentative = base.saturating_add(step_cost).saturating_add(sun);
            if tentative < *cost.get(&next).unwrap_or(&u32::MAX) {
                cost.insert(next, tentative);
                parent.insert(next, node);
                open.push((std::cmp::Reverse(tentative + heuristic(next)), next));
            }
        };

        for (dx, dz) in [
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ] {
            // A diagonal needs both of its straight neighbours open on ground/water, so mobs do
            // not cut corners through walls or float mid-air diagonally.
            if dx != 0
                && dz != 0
                && (eval_node(blocks, node[0] + dx, node[1], node[2], options, false).is_none()
                    || eval_node(blocks, node[0], node[1], node[2] + dz, options, false).is_none())
            {
                continue;
            }
            let x = node[0] + dx;
            let z = node[2] + dz;
            // Level ground first, then a step up, then drops up to three blocks.
            let mut next = None;
            for dy in [0, 1, -1, -2, -3] {
                if dy < -MAX_DROP {
                    continue;
                }
                let y = node[1] + dy;
                if let Some(extra) = eval_node(blocks, x, y, z, options, false) {
                    // Stepping up needs open space above the current position.
                    if dy > 0 {
                        let clearance_y = if options.can_climb {
                            node[1] + 1
                        } else {
                            node[1] + 2
                        };
                        if !state(blocks, node[0], clearance_y, node[2])
                            .is_some_and(|s| passable_with(s, options))
                        {
                            continue;
                        }
                    }
                    next = Some(([x, y, z], extra));
                    break;
                }
            }
            let Some((next, extra)) = next else {
                continue;
            };
            let step = if dx != 0 && dz != 0 {
                DIAGONAL
            } else {
                STRAIGHT
            };
            let step =
                step + u32::try_from((next[1] - node[1]).abs()).unwrap_or(0) * VERTICAL + extra;
            try_relax(next, step);
        }

        if options.can_climb {
            // Vertical climb up along an adjacent solid wall face.
            if has_adjacent_wall(blocks, node[0], node[1], node[2])
                && let Some(extra) = eval_node(blocks, node[0], node[1] + 1, node[2], options, true)
            {
                try_relax([node[0], node[1] + 1, node[2]], CLIMB_STEP + extra);
            }
            // Vertical climb down a wall face (beyond normal MAX_DROP).
            if let Some(extra) = eval_node(blocks, node[0], node[1] - 1, node[2], options, true) {
                try_relax([node[0], node[1] - 1, node[2]], CLIMB_STEP + extra);
            }
        }
    }
    let Some(mut node) = goal_node else {
        return Vec::new();
    };
    let mut path = Vec::new();
    while node != start {
        path.push(node);
        let Some(&previous) = parent.get(&node) else {
            return Vec::new();
        };
        node = previous;
    }
    path.reverse();
    path.truncate(MAX_PATH);
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A flat floor with optional walls.
    struct World {
        floor: i32,
        blocks: HashMap<[i32; 3], &'static str>,
    }

    impl World {
        fn flat(floor: i32) -> Self {
            Self {
                floor,
                blocks: HashMap::new(),
            }
        }

        fn wall(&mut self, x: i32, z: i32, from: i32, height: i32) {
            for y in from..from + height {
                self.blocks.insert([x, y, z], "stone");
            }
        }
    }

    impl BlockGetter for World {
        fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            if let Some(name) = self.blocks.get(&[x, y, z]) {
                return blocks::default_state(name);
            }
            blocks::default_state(if y < self.floor { "stone" } else { "air" })
        }
    }

    #[test]
    fn a_flat_path_is_straight_and_bounded() {
        let world = World::flat(64);
        let path = find_path(&world, [0, 64, 0], [5, 64, 0], 32);
        assert!(!path.is_empty());
        let last = *path.last().unwrap();
        // The search stops at the first node within one block of the goal.
        assert!((last[0] - 5).abs() <= 1 && last[2].abs() <= 1, "{last:?}");
        assert!(path.len() <= MAX_PATH);
        assert!(find_path(&world, [0, 64, 0], [5, 64, 0], 2).is_empty());
        assert!(find_path(&world, [0, 64, 0], [0, 64, 0], 8).is_empty());
    }

    #[test]
    fn a_wall_forces_a_detour() {
        let mut world = World::flat(64);
        for z in -6..=6 {
            world.wall(3, z, 64, 3);
        }
        let path = find_path(&world, [0, 64, 0], [6, 64, 0], 32);
        assert!(!path.is_empty(), "a detour exists around the wall");
        // No waypoint is inside the wall (which covers z -6..=6).
        assert!(path.iter().all(|p| p[0] != 3 || p[2].abs() > 6));
        let last = *path.last().unwrap();
        assert!((last[0] - 6).abs() <= 1, "{last:?}");
    }

    #[test]
    fn a_step_up_and_a_drop_are_walkable_but_a_wall_is_not() {
        let mut world = World::flat(64);
        for z in -2..=2 {
            for x in 2..=4 {
                world.blocks.insert([x, 64, z], "stone");
            }
        }
        let path = find_path(&world, [0, 64, 0], [6, 64, 0], 32);
        assert!(!path.is_empty(), "a one-block step is climbable");
        assert!(path.iter().any(|p| p[1] == 65));
        // A five-block wall has no waypoint inside it; the only route goes around the ends.
        let mut tall = World::flat(64);
        for z in -8..=8 {
            tall.wall(3, z, 64, 5);
        }
        let detour = find_path(&tall, [0, 64, 0], [6, 64, 0], 32);
        // The wall covers z -8..=8; x = 3 only appears beyond its ends.
        assert!(detour.iter().all(|p| p[0] != 3 || p[2].abs() > 8));
    }

    #[test]
    fn unloaded_columns_are_walls() {
        struct Void;
        impl BlockGetter for Void {
            fn block_state(&self, _x: i32, _y: i32, _z: i32) -> Option<u16> {
                None
            }
        }
        assert!(find_path(&Void, [0, 64, 0], [2, 64, 0], 32).is_empty());
    }

    #[test]
    fn spider_climbs_vertically_up_a_wall_and_respects_overhangs() {
        let mut world = World::flat(64);
        // Build a 4-block high platform with no ground stairs up to [4, 68, 0].
        for x in 3..=5 {
            for z in -1..=1 {
                world.wall(x, z, 64, 4);
            }
        }
        // Ground A* cannot reach the elevated platform.
        assert!(find_path(&world, [0, 64, 0], [4, 68, 0], 32).is_empty());
        // Spider A* climbs vertically up x=2 onto the platform at y=68.
        let climb = PathOptions {
            can_climb: true,
            ..Default::default()
        };
        let path = find_path_with(&world, [0, 64, 0], [4, 68, 0], 32, &climb);
        assert!(!path.is_empty(), "spider A* must climb onto the platform");
        assert!(
            path.iter().any(|p| p[1] >= 67),
            "path must include vertical climb waypoints: {path:?}"
        );
        // Add an overhang ring around the platform at y=67 so every face is blocked.
        for z in -2..=2 {
            world.blocks.insert([2, 67, z], "stone");
            world.blocks.insert([6, 67, z], "stone");
        }
        for x in 2..=6 {
            world.blocks.insert([x, 67, -2], "stone");
            world.blocks.insert([x, 67, 2], "stone");
        }
        assert!(
            find_path_with(&world, [0, 64, 0], [4, 68, 0], 32, &climb).is_empty(),
            "an overhang blocks vertical climbing"
        );
    }

    #[test]
    fn avoid_sun_takes_a_shaded_detour_in_bright_daylight() {
        struct Shaded {
            light: u8,
            roof_z_min: i32,
        }
        impl BlockGetter for Shaded {
            fn block_state(&self, _x: i32, y: i32, z: i32) -> Option<u16> {
                if y < 64 || (y == 68 && z >= self.roof_z_min) {
                    return blocks::default_state("stone");
                }
                blocks::default_state("air")
            }
            fn brightness(&self, _x: i32, _y: i32, _z: i32) -> Option<u8> {
                Some(self.light)
            }
        }
        let day = Shaded {
            light: 15,
            roof_z_min: 2,
        };
        let avoid = PathOptions {
            avoid_sun: true,
            ..Default::default()
        };
        // Direct route at z=0 is sunlit; z>=2 is shaded under the roof.
        let path = find_path_with(&day, [0, 64, 0], [10, 64, 0], 32, &avoid);
        assert!(!path.is_empty());
        assert!(
            path.iter().any(|p| p[2] >= 2),
            "avoid_sun detours under the roof: {path:?}"
        );
        // At night (light 4), there is no sunlight penalty so it walks straight.
        let night = Shaded {
            light: 4,
            roof_z_min: 2,
        };
        let night_path = find_path_with(&night, [0, 64, 0], [10, 64, 0], 32, &avoid);
        assert!(
            night_path.iter().all(|p| p[2] == 0),
            "no detour at night: {night_path:?}"
        );
    }

    #[test]
    fn open_doors_are_walkable_and_closed_wooden_doors_require_can_open_doors() {
        struct DoorWorld {
            states: HashMap<[i32; 3], u16>,
        }
        impl BlockGetter for DoorWorld {
            fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
                if let Some(&s) = self.states.get(&[x, y, z]) {
                    return Some(s);
                }
                blocks::default_state(if y < 64 { "stone" } else { "air" })
            }
        }
        let mut world = DoorWorld {
            states: HashMap::new(),
        };
        let stone = blocks::default_state("stone").unwrap();
        // Solid wall across x=3 except doorway at z=0.
        for z in -12..=12 {
            if z != 0 {
                for y in 64..67 {
                    world.states.insert([3, y, z], stone);
                }
            }
        }
        let door = |name: &str, open: bool| {
            let open_s = if open { "true" } else { "false" };
            (
                blocks::state_id(
                    name,
                    &[
                        ("facing", "east"),
                        ("half", "lower"),
                        ("hinge", "left"),
                        ("open", open_s),
                        ("powered", "false"),
                    ],
                )
                .unwrap(),
                blocks::state_id(
                    name,
                    &[
                        ("facing", "east"),
                        ("half", "upper"),
                        ("hinge", "left"),
                        ("open", open_s),
                        ("powered", "false"),
                    ],
                )
                .unwrap(),
            )
        };
        // Open oak door is passable even with default options.
        let (lo, hi) = door("minecraft:oak_door", true);
        world.states.insert([3, 64, 0], lo);
        world.states.insert([3, 65, 0], hi);
        let open_path = find_path(&world, [0, 64, 0], [5, 64, 0], 32);
        assert!(
            open_path.contains(&[3, 64, 0]),
            "open door is walkable: {open_path:?}"
        );

        // Closed oak door blocks normal mobs, but is walkable when can_open_doors is true.
        let (lo, hi) = door("minecraft:oak_door", false);
        world.states.insert([3, 64, 0], lo);
        world.states.insert([3, 65, 0], hi);
        let normal = find_path(&world, [0, 64, 0], [5, 64, 0], 32);
        assert!(
            normal.iter().all(|p| *p != [3, 64, 0]),
            "closed door blocks normal mobs: {normal:?}"
        );
        let breaker = PathOptions {
            can_open_doors: true,
            ..Default::default()
        };
        let breaker_path = find_path_with(&world, [0, 64, 0], [5, 64, 0], 32, &breaker);
        assert!(
            breaker_path.contains(&[3, 64, 0]),
            "door-breaking zombie paths through closed wooden door: {breaker_path:?}"
        );

        // Closed iron door is never walkable, even with can_open_doors.
        let (lo, hi) = door("minecraft:iron_door", false);
        world.states.insert([3, 64, 0], lo);
        world.states.insert([3, 65, 0], hi);
        let iron_path = find_path_with(&world, [0, 64, 0], [5, 64, 0], 32, &breaker);
        assert!(
            iron_path.iter().all(|p| *p != [3, 64, 0]),
            "iron door never opens: {iron_path:?}"
        );
    }

    #[test]
    fn water_handling_detours_swims_and_blocks_endermen() {
        let mut world = World::flat(64);
        // A deep moat at x=3..=4 from z=-2..=2, with dry ground at z=3 (short detour).
        for x in 3..=4 {
            for z in -2..=2 {
                world.blocks.insert([x, 62, z], "water");
                world.blocks.insert([x, 63, z], "water");
            }
        }
        let standard = PathOptions {
            water_cost: Some(DEFAULT_WATER_COST),
            can_float: true,
            ..Default::default()
        };
        let detour = find_path_with(&world, [0, 64, 0], [7, 64, 0], 32, &standard);
        assert!(!detour.is_empty());
        assert!(
            detour
                .iter()
                .all(|p| p[0] < 3 || p[0] > 4 || p[2].abs() > 2),
            "prefers dry detour around small pond: {detour:?}"
        );

        // Expand moat across z=-40..=40 so swimming is the only way across.
        for x in 3..=4 {
            for z in -40..=40 {
                world.blocks.insert([x, 62, z], "water");
                world.blocks.insert([x, 63, z], "water");
            }
        }
        let swim = find_path_with(&world, [0, 64, 0], [7, 64, 0], 32, &standard);
        assert!(
            swim.iter().any(|p| (3..=4).contains(&p[0]) && p[1] == 63),
            "floater swims across water surface when no dry detour exists: {swim:?}"
        );

        // Enderman (water_cost = None) never paths into water.
        let enderman = PathOptions {
            water_cost: None,
            can_float: true,
            ..Default::default()
        };
        assert!(find_path_with(&world, [0, 64, 0], [7, 64, 0], 32, &enderman).is_empty());

        // Zombie/Skeleton (!can_float) walks along the seabed in 1-block-deep water instead of
        // floating on deep water.
        let mut shallow = World::flat(64);
        for x in 3..=4 {
            for z in -40..=40 {
                shallow.blocks.insert([x, 63, z], "water");
            }
        }
        let sinker = PathOptions {
            water_cost: Some(DEFAULT_WATER_COST),
            can_float: false,
            ..Default::default()
        };
        let walk_bottom = find_path_with(&shallow, [0, 64, 0], [7, 64, 0], 32, &sinker);
        assert!(
            walk_bottom
                .iter()
                .any(|p| (3..=4).contains(&p[0]) && p[1] == 63),
            "non-floater paths along water bottom: {walk_bottom:?}"
        );
        // A sinker cannot step out of a 2-block-deep pit, whereas a floater swims across.
        assert!(find_path_with(&world, [0, 64, 0], [7, 64, 0], 32, &sinker).is_empty());
    }
}
