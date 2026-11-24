//! A* pathfinding on the block grid, with the vanilla `WalkNodeEvaluator` subset the modelled
//! mobs need: stand on a solid block (or float on water when `can_float`), walk straight or
//! diagonally, step up one block, drop up to three, pass open doors (and closed hand-openable doors
//! `#minecraft:mob_interactable_doors` when `can_open_doors`), climb vertical walls and traverse
//! wall faces around overhangs or down drops for spiders (`can_climb`), penalize water
//! (`water_cost`), require 3-block headroom when `tall`, and penalize sky-exposed sunlit blocks
//! (`avoid_sun`). Unloaded chunks are walls.
//!
//! ## Bounded tick budget
//!
//! To handle long detours (up to [`MAX_PATH`] = 128 waypoints across vanilla's 48-block villager
//! `requiredPathLength` and large U-shaped walls) while keeping region-thread tick cost bounded:
//! - [`find_path_with`] uses weighted A* (`f = g + 1.5 * h`, matching vanilla `PathFinder.FUDGING`),
//!   a single per-node state map, and per-search `eval_cell` / sky-visibility caches so each
//!   candidate block is evaluated at most once per search.
//! - Expanded nodes are capped by [`node_budget`] (scaling with `max_range` up to [`MAX_NODES`] =
//!   4096), and candidate cells stay within `max_range + 8` horizontally of `start`.
//! - Callers throttle failed/unreachable searches with `REPATH_TICKS` so worst-case capped
//!   searches do not run every tick.

use crate::physics::BlockGetter;
use ferrite_data::block_props::{self, FluidKind};
use ferrite_data::blocks;
use ferrite_math::Vec3;
use std::collections::{BinaryHeap, HashMap};
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Default)]
struct PathFxHasher(u64);

impl Hasher for PathFxHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut h = self.0;
        for &b in bytes {
            h = (h.rotate_left(5) ^ u64::from(b)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
        self.0 = h;
    }
    #[inline]
    fn write_i64(&mut self, i: i64) {
        self.0 = (self.0.rotate_left(5) ^ (i as u64)).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<PathFxHasher>>;

#[inline(always)]
fn pack_pos(p: [i32; 3]) -> i64 {
    ((i64::from(p[0]) & 0x3FF_FFFF) << 38)
        | ((i64::from(p[2]) & 0x3FF_FFFF) << 12)
        | (i64::from(p[1]) & 0xFFF)
}

/// Waypoints a single path keeps.
pub const MAX_PATH: usize = 128;
/// Maximum expanded nodes before [`find_path_with`] stops searching.
pub const MAX_NODES: usize = 4096;
/// Vanilla walkable node evaluator drops at most this far.
const MAX_DROP: i32 = 3;
/// Cost of a straight step, in tenths of a block.
const STRAIGHT: u32 = 10;
/// Cost of a diagonal step.
const DIAGONAL: u32 = 14;
/// Extra cost for a one-block step up or down.
const VERTICAL: u32 = 4;
/// Cost of climbing vertically or sidestepping mid-air along a solid wall face (`WallClimberNavigation`).
const CLIMB_STEP: u32 = 18;
/// Default vanilla `PathType.WATER` malus (8.0 blocks = 80 tenths).
pub const DEFAULT_WATER_COST: u32 = 80;
/// Vanilla `GroundPathNavigation.getSurfaceY` upward water scan limit.
const MAX_SURFACE_SCAN: i32 = 16;

/// Expansion budget for a search with `max_range` blocks, bounded to keep region ticks predictable.
pub fn node_budget(max_range: i32) -> usize {
    (max_range.max(1) as usize * 96).clamp(1024, MAX_NODES)
}

/// Per-mob navigation capabilities and cost modifiers for [`find_path_with`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PathOptions {
    /// `WallClimberNavigation`: climb vertically up/down and sidestep along adjacent solid walls,
    /// and fit in 1-block-tall spaces (spiders have height 0.9 -> `entityHeight = 1`).
    pub can_climb: bool,
    /// `WalkNodeEvaluator.canOpenDoors`: closed hand-openable doors
    /// (`#minecraft:mob_interactable_doors`: wooden and copper doors, never `iron_door`) are
    /// traversable (`WALKABLE_DOOR`).
    pub can_open_doors: bool,
    /// Extra per-node cost (in tenths of a block) for water nodes, or `None` when water is
    /// impassable (`PathType.WATER = -1.0`, e.g. endermen, blazes).
    pub water_cost: Option<u32>,
    /// `FloatGoal`: floats on the water surface instead of walking on the seafloor.
    pub can_float: bool,
    /// `WaterBoundPathNavigation`: swim through the body of water in three dimensions instead
    /// of only walking the seafloor or floating on the surface.
    pub can_swim: bool,
    /// `RestrictSunGoal`: extra per-node cost on sky-visible bright blocks (`light >= 12`).
    pub avoid_sun: bool,
    /// Mobs taller than 2.0 blocks (`y + 2` must also be passable, e.g. endermen, iron golems).
    pub tall: bool,
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

const DOOR_FLAG_OPEN: u8 = 1;
const DOOR_FLAG_OPENABLE: u8 = 2;
const DOOR_FLAG_CLOSED_OPENABLE: u8 = 4;

fn door_flags_table() -> &'static [u8] {
    static TABLE: std::sync::OnceLock<Box<[u8]>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = vec![0u8; blocks::STATE_COUNT];
        for (id, slot) in table.iter_mut().enumerate() {
            let Some((name, props)) = blocks::state(id as u16) else {
                continue;
            };
            let is_door = ferrite_data::tags::contains("block", "doors", name);
            let is_openable = ferrite_data::tags::contains("block", "mob_interactable_doors", name);
            if !is_door && !is_openable {
                continue;
            }
            let open = props.iter().any(|(k, v)| *k == "open" && *v == "true");
            let closed = props.iter().any(|(k, v)| *k == "open" && *v == "false");
            let mut flags = 0u8;
            if is_door && open {
                flags |= DOOR_FLAG_OPEN;
            }
            if is_openable {
                flags |= DOOR_FLAG_OPENABLE;
                if closed {
                    flags |= DOOR_FLAG_CLOSED_OPENABLE;
                }
            }
            *slot = flags;
        }
        table.into_boxed_slice()
    })
}

/// Whether `state` is an already-open door (`#minecraft:doors`, `open=true`).
#[inline]
pub fn is_open_door(state: u16) -> bool {
    door_flags_table()
        .get(usize::from(state))
        .is_some_and(|&f| (f & DOOR_FLAG_OPEN) != 0)
}

/// Whether `state` is a hand-openable door (`#minecraft:mob_interactable_doors`: wooden and
/// copper doors, matching vanilla `DoorBlockType.canOpenByHand()`). Never matches `iron_door`.
#[inline]
pub fn is_openable_door(state: u16) -> bool {
    door_flags_table()
        .get(usize::from(state))
        .is_some_and(|&f| (f & DOOR_FLAG_OPENABLE) != 0)
}

/// Whether `state` is a closed hand-openable door (`#minecraft:mob_interactable_doors`,
/// `open=false`).
#[inline]
pub fn is_closed_openable_door(state: u16) -> bool {
    door_flags_table()
        .get(usize::from(state))
        .is_some_and(|&f| (f & DOOR_FLAG_CLOSED_OPENABLE) != 0)
}

fn lower_openable_door(
    blocks: &impl BlockGetter,
    x: i32,
    y: i32,
    z: i32,
    want_open: bool,
) -> Option<[i32; 3]> {
    let raw = state(blocks, x, y, z)?;
    let (name, props) = blocks::state(raw)?;
    if !ferrite_data::tags::contains("block", "mob_interactable_doors", name) {
        return None;
    }
    let is_open = props.iter().any(|(k, v)| *k == "open" && *v == "true");
    if is_open != want_open {
        return None;
    }
    let lower = props.iter().any(|(k, v)| *k == "half" && *v == "lower");
    Some([x, if lower { y } else { y - 1 }, z])
}

/// Closed hand-openable door along the mob's upcoming path within 2.5 blocks, returned as its
/// lower half (`InteractWithDoor` / `OpenDoorGoal` entry check).
pub fn door_to_open_on_path(
    blocks: &impl BlockGetter,
    pos: Vec3,
    path: &[[i32; 3]],
    path_index: usize,
    path_len: usize,
) -> Option<[i32; 3]> {
    let end = path_len.min(path.len()).min(path_index.saturating_add(3));
    for waypoint in path.get(path_index..end).into_iter().flatten() {
        let dx = f64::from(waypoint[0]) + 0.5 - pos.x;
        let dy = f64::from(waypoint[1]) + 0.5 - pos.y;
        let dz = f64::from(waypoint[2]) + 0.5 - pos.z;
        if dx * dx + dz * dz <= 6.25 && dy.abs() <= 1.5 {
            for door_y in [waypoint[1], waypoint[1] + 1] {
                if let Some(door) =
                    lower_openable_door(blocks, waypoint[0], door_y, waypoint[2], false)
                {
                    return Some(door);
                }
            }
        }
    }
    None
}

/// Action for a previously opened door tracked on a mob (`opened_door`), matching vanilla's
/// `InteractWithDoor`: close once the mob is >2.0 blocks past it, unless it is still ahead on
/// this mob's path or another local mob in the same region is near/using it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorCloseCheck {
    KeepOpen,
    Close,
    Cleared,
}

pub fn check_door_to_close<'a>(
    blocks: &impl BlockGetter,
    pos: Vec3,
    path: &[[i32; 3]],
    path_index: usize,
    path_len: usize,
    door_lower: [i32; 3],
    other_mobs: impl IntoIterator<Item = (Vec3, &'a [[i32; 3]])>,
) -> DoorCloseCheck {
    if lower_openable_door(blocks, door_lower[0], door_lower[1], door_lower[2], true).is_none() {
        return DoorCloseCheck::Cleared;
    }
    let matches_door = |wp: &[i32; 3]| {
        wp[0] == door_lower[0]
            && wp[2] == door_lower[2]
            && (wp[1] == door_lower[1] || wp[1] == door_lower[1] + 1)
    };
    let end = path_len.min(path.len()).min(path_index.saturating_add(3));
    if path
        .get(path_index..end)
        .into_iter()
        .flatten()
        .any(matches_door)
    {
        return DoorCloseCheck::KeepOpen;
    }
    let dx = f64::from(door_lower[0]) + 0.5 - pos.x;
    let dz = f64::from(door_lower[2]) + 0.5 - pos.z;
    let dy = f64::from(door_lower[1]) - pos.y;
    if dx * dx + dz * dz <= 4.0 && dy.abs() <= 2.0 {
        return DoorCloseCheck::KeepOpen;
    }
    for (other_pos, other_path) in other_mobs {
        let odx = f64::from(door_lower[0]) + 0.5 - other_pos.x;
        let odz = f64::from(door_lower[2]) + 0.5 - other_pos.z;
        let ody = f64::from(door_lower[1]) - other_pos.y;
        if (odx * odx + odz * odz <= 4.0 && ody.abs() <= 2.0)
            || other_path.iter().take(4).any(matches_door)
        {
            return DoorCloseCheck::KeepOpen;
        }
    }
    DoorCloseCheck::Close
}

fn is_door_passable(state: u16, can_open_doors: bool) -> bool {
    if is_open_door(state) {
        return true;
    }
    can_open_doors && is_closed_openable_door(state)
}

/// A block a mob can stand on: solid collision, not a fluid.
fn walkable_ground(state: u16) -> bool {
    let props = block_props::get(state);
    !props.is_air && props.fluid.is_none() && !block_props::collision_shape(state).is_empty()
}

/// A block a mob can occupy: air, collisionless non-fluid, open door, closed hand-openable door
/// (when `can_open_doors`), or water (when `water_cost.is_some()`).
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

#[derive(Debug, Clone, Copy)]
struct NodeEval {
    extra_cost: u32,
    grounded: bool,
    closed_door: bool,
    adjacent_wall: bool,
    swimming: bool,
}

/// Evaluates whether a mob with `options` can occupy `(x, y, z)` and records its traversal
/// properties for caching during a search.
fn eval_cell(
    blocks: &impl BlockGetter,
    x: i32,
    y: i32,
    z: i32,
    options: &PathOptions,
) -> Option<NodeEval> {
    let below = state(blocks, x, y - 1, z)?;
    let feet = state(blocks, x, y, z)?;
    let head = state(blocks, x, y + 1, z)?;
    if !passable_with(feet, options) {
        return None;
    }
    if options.tall && !state(blocks, x, y + 2, z).is_some_and(|s| passable_with(s, options)) {
        return None;
    }
    let closed_door = is_closed_openable_door(feet) || is_closed_openable_door(head);
    let feet_water = is_water(feet);
    if feet_water && options.can_swim {
        // `WaterBoundPathNavigation`: any water cell with a passable head space is a node, and
        // the search may move vertically through the water column.
        if !passable_with(head, options) {
            return None;
        }
        let extra_cost = options.water_cost?;
        return Some(NodeEval {
            extra_cost,
            grounded: false,
            closed_door,
            adjacent_wall: false,
            swimming: true,
        });
    }
    if feet_water && options.can_float {
        // Floaters stay on the water surface (`head` must not be water or solid).
        if is_water(head) || !passable_with(head, options) {
            return None;
        }
        let extra_cost = options.water_cost?;
        return Some(NodeEval {
            extra_cost,
            grounded: true,
            closed_door,
            adjacent_wall: false,
            swimming: false,
        });
    }
    if !options.can_climb && !passable_with(head, options) {
        return None;
    }
    if walkable_ground(below) {
        let in_water = feet_water || (!options.can_climb && is_water(head));
        let extra_cost = if in_water { options.water_cost? } else { 0 };
        let adjacent_wall = options.can_climb && !feet_water && has_adjacent_wall(blocks, x, y, z);
        return Some(NodeEval {
            extra_cost,
            grounded: true,
            closed_door,
            adjacent_wall,
            swimming: false,
        });
    }
    if options.can_climb && !feet_water && has_adjacent_wall(blocks, x, y, z) {
        return Some(NodeEval {
            extra_cost: 0,
            grounded: false,
            closed_door,
            adjacent_wall: true,
            swimming: false,
        });
    }
    None
}

fn eval_node(
    blocks: &impl BlockGetter,
    x: i32,
    y: i32,
    z: i32,
    options: &PathOptions,
    allow_wall_cling: bool,
) -> Option<u32> {
    let eval = eval_cell(blocks, x, y, z, options)?;
    if eval.grounded || eval.swimming || allow_wall_cling {
        Some(eval.extra_cost)
    } else {
        None
    }
}

fn sunlight_penalty(
    blocks: &impl BlockGetter,
    x: i32,
    y: i32,
    z: i32,
    sky_cache: &mut FastMap<i64, bool>,
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
        .entry(pack_pos(pos))
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

/// [`start_node`] respecting `options` (e.g. deep water surface recovery, climb nodes, doors).
pub fn start_node_with(
    blocks: &impl BlockGetter,
    at: [i32; 3],
    options: &PathOptions,
) -> Option<[i32; 3]> {
    if options.can_swim && state(blocks, at[0], at[1], at[2]).is_some_and(is_water) {
        // A swimmer starts in the water cell it occupies; it does not have to surface first.
        if eval_node(blocks, at[0], at[1], at[2], options, true).is_some() {
            return Some(at);
        }
    }
    if options.can_float && state(blocks, at[0], at[1], at[2]).is_some_and(is_water) {
        // Mirror vanilla `GroundPathNavigation.getSurfaceY()`: floaters submerged underwater
        // scan upward to the water surface so deep-water recovery still finds paths.
        for dy in 0..=MAX_SURFACE_SCAN {
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
            if !state(blocks, candidate[0], candidate[1], candidate[2]).is_some_and(is_water) {
                break;
            }
        }
    }
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

#[derive(Debug, Clone, Copy)]
struct SearchNode {
    g: u32,
    parent: [i32; 3],
    closed: bool,
}

#[derive(Default)]
struct PathScratch {
    open: BinaryHeap<(std::cmp::Reverse<u32>, [i32; 3])>,
    nodes: FastMap<i64, SearchNode>,
    eval_cache: FastMap<i64, Option<NodeEval>>,
    sky_cache: FastMap<i64, bool>,
}

thread_local! {
    static PATH_SCRATCH: std::cell::RefCell<PathScratch> =
        std::cell::RefCell::new(PathScratch::default());
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
    let budget = node_budget(max_range);
    let search_limit = max_range.max(1) + 8;
    let heuristic = |p: [i32; 3]| -> u32 {
        let dx = (p[0] - goal[0]).unsigned_abs();
        let dz = (p[2] - goal[2]).unsigned_abs();
        let dy = (p[1] - goal[1]).unsigned_abs();
        dx.min(dz) * DIAGONAL + dx.abs_diff(dz) * STRAIGHT + dy * VERTICAL
    };
    let weighted_f = |g: u32, p: [i32; 3]| -> u32 {
        // Weighted A* with 1.5x heuristic matching vanilla `PathFinder.FUDGING`.
        g.saturating_add(heuristic(p).saturating_mul(3) / 2)
    };
    let at_goal = |p: [i32; 3]| {
        (p[0] - goal[0]).abs() <= 1 && (p[2] - goal[2]).abs() <= 1 && (p[1] - goal[1]).abs() <= 1
    };

    PATH_SCRATCH.with(|cell| {
        let mut scratch = cell.borrow_mut();
        let PathScratch {
            open,
            nodes,
            eval_cache,
            sky_cache,
        } = &mut *scratch;
        open.clear();
        nodes.clear();
        eval_cache.clear();
        sky_cache.clear();

        let mut eval_cached = |pos: [i32; 3]| -> Option<NodeEval> {
            *eval_cache
                .entry(pack_pos(pos))
                .or_insert_with(|| eval_cell(blocks, pos[0], pos[1], pos[2], options))
        };

        nodes.insert(
            pack_pos(start),
            SearchNode {
                g: 0,
                parent: start,
                closed: false,
            },
        );
        open.push((std::cmp::Reverse(weighted_f(0, start)), start));
        let mut goal_node = None;
        let mut expanded = 0;

        while let Some((_, node)) = open.pop() {
            if at_goal(node) {
                goal_node = Some(node);
                break;
            }
            let node_key = pack_pos(node);
            let Some(entry) = nodes.get_mut(&node_key) else {
                continue;
            };
            if entry.closed {
                continue;
            }
            entry.closed = true;
            let base = entry.g;

            expanded += 1;
            if expanded > budget {
                break;
            }

            let Some(node_eval) = eval_cached(node) else {
                continue;
            };
            let mut try_relax = |next: [i32; 3], step_cost: u32| {
                let next_key = pack_pos(next);
                if nodes.get(&next_key).is_some_and(|n| n.closed) {
                    return;
                }
                let sun = if options.avoid_sun {
                    sunlight_penalty(blocks, next[0], next[1], next[2], sky_cache)
                } else {
                    0
                };
                let tentative = base.saturating_add(step_cost).saturating_add(sun);
                let current_g = nodes.get(&next_key).map_or(u32::MAX, |n| n.g);
                if tentative < current_g {
                    nodes.insert(
                        next_key,
                        SearchNode {
                            g: tentative,
                            parent: node,
                            closed: false,
                        },
                    );
                    open.push((std::cmp::Reverse(weighted_f(tentative, next)), next));
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
                let x = node[0] + dx;
                let z = node[2] + dz;
                if (x - start[0]).abs() > search_limit || (z - start[2]).abs() > search_limit {
                    continue;
                }
                let diagonal = dx != 0 && dz != 0;
                // A diagonal needs both of its straight neighbours grounded and free of closed doors
                // (`WalkNodeEvaluator.isDiagonalValid`), so mobs do not cut corners through walls or
                // closed doorways or float mid-air diagonally.
                if diagonal {
                    if node_eval.closed_door {
                        continue;
                    }
                    let n1 = eval_cached([node[0] + dx, node[1], node[2]]);
                    let n2 = eval_cached([node[0], node[1], node[2] + dz]);
                    if n1.is_some_and(|n| n.closed_door) || n2.is_some_and(|n| n.closed_door) {
                        continue;
                    }
                    if options.can_climb {
                        // `WallClimberNavigation` wraps a corner when the current cell or one
                        // straight neighbour is a wall face the spider can cling to.
                        if !node_eval.adjacent_wall && n1.is_none() && n2.is_none() {
                            continue;
                        }
                    } else if !n1.is_some_and(|n| n.grounded) || !n2.is_some_and(|n| n.grounded) {
                        continue;
                    }
                }
                let allow_cling = options.can_climb;
                // Level ground first, then a step up, then drops up to three blocks.
                let mut next = None;
                for dy in [0, 1, -1, -2, -3] {
                    if dy < -MAX_DROP {
                        continue;
                    }
                    let y = node[1] + dy;
                    let Some(cand) = eval_cached([x, y, z]) else {
                        continue;
                    };
                    if diagonal && cand.closed_door {
                        continue;
                    }
                    // Wall-clinging only applies to level wall-sidesteps and step-offs (`dy <= 0`);
                    // stepping upward (`dy > 0`) must land on grounded blocks (e.g. a wall top).
                    if !cand.grounded && !(allow_cling && dy <= 0) {
                        continue;
                    }
                    if dy > 0 {
                        let clearance_y = if options.can_climb {
                            node[1] + 1
                        } else if options.tall {
                            node[1] + 3
                        } else {
                            node[1] + 2
                        };
                        if !state(blocks, node[0], clearance_y, node[2])
                            .is_some_and(|s| passable_with(s, options))
                        {
                            continue;
                        }
                        if !options.can_climb
                            && options.tall
                            && !state(blocks, node[0], node[1] + 2, node[2])
                                .is_some_and(|s| passable_with(s, options))
                        {
                            continue;
                        }
                    }
                    next = Some(([x, y, z], cand));
                    break;
                }
                let Some((next, cand)) = next else {
                    continue;
                };
                let base_step = if diagonal { DIAGONAL } else { STRAIGHT };
                let climb_cost = if cand.grounded { 0 } else { CLIMB_STEP };
                let step = base_step
                    + u32::try_from((next[1] - node[1]).abs()).unwrap_or(0) * VERTICAL
                    + cand.extra_cost
                    + climb_cost;
                try_relax(next, step);
            }

            if options.can_climb {
                // Vertical climb up along an adjacent solid wall face.
                if node_eval.adjacent_wall
                    && let Some(cand) = eval_cached([node[0], node[1] + 1, node[2]])
                {
                    try_relax(
                        [node[0], node[1] + 1, node[2]],
                        CLIMB_STEP + cand.extra_cost,
                    );
                }
                // Vertical climb down a wall face (beyond normal MAX_DROP).
                if let Some(cand) = eval_cached([node[0], node[1] - 1, node[2]]) {
                    try_relax(
                        [node[0], node[1] - 1, node[2]],
                        CLIMB_STEP + cand.extra_cost,
                    );
                }
            }

            if options.can_swim && node_eval.swimming {
                // Vertical and horizontal swimming through connected water cells.
                for (dx, dy, dz) in [
                    (0, 1, 0),
                    (0, -1, 0),
                    (1, 0, 0),
                    (-1, 0, 0),
                    (0, 0, 1),
                    (0, 0, -1),
                ] {
                    let next = [node[0] + dx, node[1] + dy, node[2] + dz];
                    if (next[0] - start[0]).abs() > search_limit
                        || (next[2] - start[2]).abs() > search_limit
                    {
                        continue;
                    }
                    let Some(cand) = eval_cached(next) else {
                        continue;
                    };
                    if !cand.swimming {
                        continue;
                    }
                    try_relax(next, STRAIGHT + cand.extra_cost);
                }
            }
        }

        let Some(mut node) = goal_node else {
            return Vec::new();
        };
        let mut path = Vec::new();
        while node != start {
            path.push(node);
            let Some(entry) = nodes.get(&pack_pos(node)) else {
                return Vec::new();
            };
            node = entry.parent;
        }
        path.reverse();
        path.truncate(MAX_PATH);
        path
    })
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

    #[test]
    fn a_water_bound_mob_swims_vertically_through_water() {
        let mut world = World::flat(60);
        for y in 61..=64 {
            world.blocks.insert([1, y, 0], "water");
        }
        let swim = PathOptions {
            water_cost: Some(0),
            can_swim: true,
            ..Default::default()
        };
        let path = find_path_with(&world, [1, 61, 0], [1, 64, 0], 8, &swim);
        assert!(
            path.iter().any(|p| p[1] >= 62),
            "swimmer moves vertically through the water column: {path:?}"
        );
        // A walker that neither floats nor swims cannot rise through the column.
        let walker = PathOptions {
            water_cost: Some(0),
            can_float: false,
            ..Default::default()
        };
        assert!(find_path_with(&world, [1, 61, 0], [1, 64, 0], 8, &walker).is_empty());
    }

    #[test]
    fn spider_wraps_a_diagonal_wall_corner() {
        struct Ledge;
        impl BlockGetter for Ledge {
            fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
                let solid = matches!((x, y, z), (1, 63, 0) | (2, 64, 0) | (2, 65, 0) | (3, 63, 1));
                blocks::default_state(if solid { "stone" } else { "air" })
            }
        }
        let climb = PathOptions {
            can_climb: true,
            ..Default::default()
        };
        let path = find_path_with(&Ledge, [1, 64, 0], [3, 64, 1], 8, &climb);
        assert!(
            !path.is_empty(),
            "spider wraps the diagonal corner through the adjacent wall face: {path:?}"
        );
        assert!(
            find_path(&Ledge, [1, 64, 0], [3, 64, 1], 8).is_empty(),
            "a non-climber cannot use the unsupported diagonal"
        );
    }

    #[test]
    fn long_detour_exceeds_old_32_node_cap_without_failing() {
        let mut world = World::flat(64);
        // Deep U-trap requiring >50 waypoints to escape and loop around to [20, 64, 0].
        for z in -22..=22 {
            world.wall(12, z, 64, 5);
        }
        for x in 5..=12 {
            world.wall(x, -22, 64, 5);
            world.wall(x, 22, 64, 5);
        }
        let detour = find_path(&world, [8, 64, 0], [20, 64, 0], 48);
        assert!(
            detour.len() > 32,
            "detour must exceed the old 32-node cap, got len={}: {detour:?}",
            detour.len()
        );
        assert!(detour.len() <= MAX_PATH);
        let last = *detour.last().unwrap();
        assert!(
            (last[0] - 20).abs() <= 1 && last[2].abs() <= 1,
            "detour must reach target vicinity, ended at {last:?}"
        );
        // Straight 45-block path at villager range keeps full length (> 16).
        let flat = World::flat(64);
        let straight = find_path(&flat, [0, 64, 0], [45, 64, 0], 48);
        assert!(
            straight.len() >= 43 && straight.len() <= MAX_PATH,
            "45-block straight path must not truncate at 16: len={}",
            straight.len()
        );
    }

    #[test]
    fn closed_copper_door_is_openable_and_blocks_diagonal_corners() {
        let mut world = World::flat(64);
        for z in -5..=5 {
            if z != 0 {
                world.wall(3, z, 64, 4);
            }
        }
        let door = |name: &str, open: bool| {
            let state_str = if open { "true" } else { "false" };
            let lower = blocks::state_id(name, &[("half", "lower"), ("open", state_str)])
                .unwrap_or_else(|| blocks::default_state(name).unwrap());
            let upper = blocks::state_id(name, &[("half", "upper"), ("open", state_str)])
                .unwrap_or_else(|| blocks::default_state(name).unwrap());
            (lower, upper)
        };
        let (copper_lo, copper_hi) = door("minecraft:copper_door", false);
        assert!(is_openable_door(copper_lo));
        assert!(is_closed_openable_door(copper_lo));
        let (oak_lo, _) = door("minecraft:oak_door", false);
        assert!(is_openable_door(oak_lo));
        let (iron_lo, _) = door("minecraft:iron_door", false);
        assert!(!is_openable_door(iron_lo));

        world.blocks.insert([3, 64, 0], "copper_door");
        // Override with explicit state ids for closed copper door halves.
        struct CopperDoorWorld {
            lo: u16,
            hi: u16,
        }
        impl BlockGetter for CopperDoorWorld {
            fn block_state(&self, x: i32, y: i32, z: i32) -> Option<u16> {
                if y < 64 {
                    return blocks::default_state("stone");
                }
                if x == 3 && z == 0 {
                    if y == 64 {
                        return Some(self.lo);
                    }
                    if y == 65 {
                        return Some(self.hi);
                    }
                }
                // Wall blocking [2, 64, 1] and [4, 64, -1] so only orthogonal door entry works.
                if (x == 2 && z == 1 && y < 66) || (x == 3 && z == -1 && y < 66) {
                    return blocks::default_state("stone");
                }
                blocks::default_state("air")
            }
        }
        let opts = PathOptions {
            can_open_doors: true,
            ..Default::default()
        };
        let copper_world = CopperDoorWorld {
            lo: copper_lo,
            hi: copper_hi,
        };
        // Walking past adjacent closed door [3, 64, 0] to [3, 64, 1] must not cut diagonally
        // directly from [2, 64, 0] to [3, 64, 1]; it steps through [3, 64, 0] orthogonally.
        let path = find_path_with(&copper_world, [2, 64, 0], [3, 64, 2], 16, &opts);
        assert!(!path.is_empty());
        let mut prev = [2, 64, 0];
        for &wp in &path {
            if wp == [3, 64, 1] {
                assert_ne!(
                    prev,
                    [2, 64, 0],
                    "must not diagonally bypass adjacent closed door frame: {path:?}"
                );
            }
            prev = wp;
        }
    }

    #[test]
    fn submerged_floater_recovers_surface_node_and_swims_deep_water() {
        struct OceanWorld;
        impl BlockGetter for OceanWorld {
            fn block_state(&self, x: i32, y: i32, _z: i32) -> Option<u16> {
                if y < 56 {
                    return blocks::default_state("stone");
                }
                if x >= 6 && y < 64 {
                    return blocks::default_state("stone");
                }
                if x < 6 && y <= 63 {
                    return blocks::default_state("water");
                }
                blocks::default_state("air")
            }
        }
        let floater = PathOptions {
            water_cost: Some(DEFAULT_WATER_COST),
            can_float: true,
            ..Default::default()
        };
        assert_eq!(
            start_node_with(&OceanWorld, [0, 57, 0], &floater),
            Some([0, 63, 0]),
            "deeply submerged floater must scan up to the water surface"
        );
        let path = find_path_with(&OceanWorld, [0, 57, 0], [8, 64, 0], 32, &floater);
        assert!(!path.is_empty(), "submerged floater must swim to shore");
        assert!(
            path.iter().all(|p| p[0] >= 6 || p[1] == 63),
            "water portion stays at surface y=63: {path:?}"
        );
    }

    #[test]
    fn spider_traverses_wall_around_partial_overhang_and_climbs_down_tall_wall() {
        let mut world = World::flat(64);
        // 7-block tall platform at x=3..=6, z=-3..=3 (top walkable at y=71).
        for x in 3..=6 {
            for z in -3..=3 {
                world.wall(x, z, 64, 7);
            }
        }
        // Lava floor outside z=-1..=1 so spider cannot walk around on the ground at y=64.
        for x in -2_i32..=2 {
            for z in -4_i32..=4 {
                if z.abs() >= 2 {
                    world.blocks.insert([x, 63, z], "lava");
                }
            }
        }
        // Partial overhang at [2, 69, -1..=1]: forces spider to sidestep along x=2 to z=+-2.
        for z in -1..=1 {
            world.blocks.insert([2, 69, z], "stone");
        }
        let climb = PathOptions {
            can_climb: true,
            ..Default::default()
        };
        let up = find_path_with(&world, [1, 64, 0], [4, 71, 0], 32, &climb);
        assert!(
            !up.is_empty(),
            "spider must sidestep mid-wall past partial overhang"
        );
        assert!(
            up.iter()
                .any(|p| p[0] == 2 && p[2].abs() >= 2 && p[1] >= 65),
            "path must sidestep along the wall around z=-1..=1: {up:?}"
        );

        // Climbing DOWN a 7-block sheer wall (exceeds MAX_DROP = 3): ground fails, spider climbs down.
        let clean_wall = World::flat(64);
        let mut sheer = clean_wall;
        for x in 3..=6 {
            for z in -3..=3 {
                sheer.wall(x, z, 64, 7);
            }
        }
        assert!(
            find_path(&sheer, [3, 71, 0], [0, 64, 0], 32).is_empty(),
            "7-block drop exceeds MAX_DROP=3 for non-climbing mobs"
        );
        let down = find_path_with(&sheer, [3, 71, 0], [0, 64, 0], 32, &climb);
        assert!(
            !down.is_empty(),
            "spider must climb down a 7-block sheer wall"
        );
        assert!(
            down.iter().any(|p| p[0] == 2 && (65..=69).contains(&p[1])),
            "descent must step along the wall face: {down:?}"
        );
    }

    #[test]
    fn tall_mob_rejects_two_block_headroom() {
        let mut world = World::flat(64);
        for z in -32..=32 {
            if z != 0 {
                world.wall(3, z, 64, 5);
            } else {
                // 2-block-high doorway/tunnel at [3, 64..=65, 0] with ceiling at y=66.
                world.blocks.insert([3, 66, 0], "stone");
            }
        }
        let normal = PathOptions::default();
        let tall = PathOptions {
            tall: true,
            ..Default::default()
        };
        assert!(!find_path_with(&world, [0, 64, 0], [6, 64, 0], 16, &normal).is_empty());
        assert!(
            find_path_with(&world, [0, 64, 0], [6, 64, 0], 16, &tall).is_empty(),
            "tall mob (>2 blocks) must not squeeze through a 2-high tunnel"
        );
    }
}
