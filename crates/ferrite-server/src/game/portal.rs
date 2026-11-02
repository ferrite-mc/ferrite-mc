//! Nether portals: obsidian frame ignition, `NetherPortalBlock` states, 8:1 coordinate
//! scaling, and searching/creating the destination portal.
//!
//! Frame geometry follows vanilla `PortalShape`: an interior of air (or existing portal
//! blocks) between 2x3 and 21x21, completely surrounded by obsidian. Ignition only reports
//! the blocks to place; the caller applies them through the normal block-change path so
//! every tracking client receives the update.

use super::{Dimension, anvil::AnvilChunkSource};

pub const MIN_WIDTH: i32 = 2;
pub const MIN_HEIGHT: i32 = 3;
pub const MAX_WIDTH: i32 = 21;
pub const MAX_HEIGHT: i32 = 21;
/// Vanilla `PortalForcer.FIND_PORTAL_RADIUS`.
pub const SEARCH_RADIUS: i32 = 128;
/// Vanilla `Entity.getDimensionChangingDelay`: 4 seconds.
pub const WAIT_TICKS: u32 = 80;
/// Vanilla `ServerPlayer.portalCooldown`: 10 seconds after a trip.
pub const COOLDOWN_TICKS: u32 = 200;

const PORTAL: &str = "minecraft:nether_portal";
const OBSIDIAN: &str = "minecraft:obsidian";

/// Block access for portal geometry. `set_block` reports whether the write landed (a chunk
/// that is not loaded cannot be edited).
pub trait PortalWorld {
    fn block(&self, x: i32, y: i32, z: i32) -> Option<u16>;
    fn set_block(&self, x: i32, y: i32, z: i32, state: u16) -> bool;
}

impl PortalWorld for AnvilChunkSource {
    fn block(&self, x: i32, y: i32, z: i32) -> Option<u16> {
        AnvilChunkSource::block(self, x, y, z).and_then(|state| u16::try_from(state).ok())
    }
    fn set_block(&self, x: i32, y: i32, z: i32, state: u16) -> bool {
        AnvilChunkSource::set_block(self, x, y, z, u32::from(state)).is_ok()
    }
}

/// Portal plane orientation: `axis=x` fills a plane extending along X.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Z,
}

impl Axis {
    fn into_axis(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Z => "z",
        }
    }
}

pub fn portal_state(axis: Axis) -> Option<u16> {
    ferrite_data::blocks::state_id(PORTAL, &[("axis", axis.into_axis())])
}

pub fn is_portal(state: u16) -> bool {
    ferrite_data::blocks::state(state).is_some_and(|(name, _)| name == PORTAL)
}

fn is_obsidian(state: u16) -> bool {
    ferrite_data::blocks::state(state).is_some_and(|(name, _)| name == OBSIDIAN)
}

fn is_air(state: u16) -> bool {
    ferrite_data::block_props::get(state).is_air
}

/// `PortalShape.getDistanceUntilEdgeAboveFrame` in one direction.
fn edge(world: &impl PortalWorld, start: [i32; 3], step: [i32; 3], max: i32) -> Option<i32> {
    let mut distance = 0;
    let mut pos = start;
    while distance < max {
        pos = [pos[0] + step[0], pos[1] + step[1], pos[2] + step[2]];
        match world.block(pos[0], pos[1], pos[2]) {
            Some(state) if is_obsidian(state) => return Some(distance),
            Some(state) if is_air(state) || is_portal(state) => distance += 1,
            _ => return None,
        }
    }
    None
}

/// Finds the valid frame around `air` on `axis` and returns the interior bounds
/// `(min, size)` plus the axis.
fn frame(
    world: &impl PortalWorld,
    air: [i32; 3],
    axis: Axis,
) -> Option<([i32; 3], [i32; 3], Axis)> {
    let (dx, dz) = match axis {
        Axis::X => (1, 0),
        Axis::Z => (0, 1),
    };
    // Descend to the bottom of the interior column.
    let mut bottom = air;
    for _ in 0..MAX_HEIGHT {
        let below = [bottom[0], bottom[1] - 1, bottom[2]];
        match world.block(below[0], below[1], below[2]) {
            Some(state) if is_obsidian(state) => break,
            Some(state) if is_air(state) || is_portal(state) => bottom = below,
            _ => return None,
        }
    }
    let below = [bottom[0], bottom[1] - 1, bottom[2]];
    if !world
        .block(below[0], below[1], below[2])
        .is_some_and(is_obsidian)
    {
        return None;
    }
    let left = edge(world, bottom, [-dx, 0, -dz], MAX_WIDTH)?;
    let right = edge(world, bottom, [dx, 0, dz], MAX_WIDTH)?;
    let width = left + right + 1;
    if !(MIN_WIDTH..=MAX_WIDTH).contains(&width) {
        return None;
    }
    let low = [bottom[0] - dx * left, bottom[1], bottom[2] - dz * left];
    let high = edge(world, low, [0, 1, 0], MAX_HEIGHT)?;
    let height = high + 1;
    if !(MIN_HEIGHT..=MAX_HEIGHT).contains(&height) {
        return None;
    }
    let min = [low[0], low[1], low[2]];
    let size = [width, height, 1];
    // Every border block must be obsidian. Border positions follow the portal plane.
    let border = |along: i32, y: i32| {
        if axis == Axis::X {
            (min[0] + dx * along, y, min[2])
        } else {
            (min[0], y, min[2] + dz * along)
        }
    };
    for along in -1..=width {
        for y in [min[1] - 1, min[1] + height] {
            let (x, y, z) = border(along, y);
            if !world.block(x, y, z).is_some_and(is_obsidian) {
                return None;
            }
        }
    }
    for step in 0..height {
        for along in [-1, width] {
            let (x, y, z) = border(along, min[1] + step);
            if !world.block(x, y, z).is_some_and(is_obsidian) {
                return None;
            }
        }
    }
    Some((min, size, axis))
}

/// Vanilla `PortalShape.findEmptyPortalShape`: recognises the frame next to `air` and
/// returns every interior block that should become a portal block.
pub fn frame_blocks(world: &impl PortalWorld, air: [i32; 3]) -> Option<Vec<([i32; 3], u16)>> {
    for axis in [Axis::X, Axis::Z] {
        let Some((min, size, axis)) = frame(world, air, axis) else {
            continue;
        };
        let state = portal_state(axis)?;
        let (dx, dz) = match axis {
            Axis::X => (1, 0),
            Axis::Z => (0, 1),
        };
        let mut blocks = Vec::new();
        for x in 0..size[0] {
            for y in 0..size[1] {
                blocks.push(([min[0] + dx * x, min[1] + y, min[2] + dz * x], state));
            }
        }
        return Some(blocks);
    }
    None
}

/// Nether coordinate scaling: an overworld point divides by 8, a nether point multiplies.
pub fn scale(from: Dimension, to: Dimension, pos: [f64; 3]) -> [f64; 3] {
    let factor = if from == Dimension::Overworld && to == Dimension::Nether {
        0.125
    } else if from == Dimension::Nether && to == Dimension::Overworld {
        8.0
    } else {
        1.0
    };
    [pos[0] * factor, pos[1], pos[2] * factor]
}

/// The other side of a nether trip, if the dimension has one.
pub fn counterpart(dimension: Dimension) -> Option<Dimension> {
    match dimension {
        Dimension::Overworld => Some(Dimension::Nether),
        Dimension::Nether => Some(Dimension::Overworld),
        Dimension::End => None,
    }
}

/// Searches loaded chunks for a portal near `center` (vanilla `PortalForcer.findPortalAround`,
/// 128 blocks in the target dimension). Returns the position of the lowest portal block.
pub fn find_portal(source: &AnvilChunkSource, center: [f64; 3], radius: i32) -> Option<[f64; 3]> {
    let dimension = source.dimension();
    let cx = (center[0].floor() as i32).div_euclid(16);
    let cz = (center[2].floor() as i32).div_euclid(16);
    let reach = radius.div_euclid(16) + 1;
    let mut best: Option<[i32; 3]> = None;
    for x in (cx - reach)..=(cx + reach) {
        for z in (cz - reach)..=(cz + reach) {
            let chunk_min_y = dimension.min_y();
            let chunk_max_y = dimension.min_y() + dimension.height() - 1;
            if let Some(Some(found)) = source.with_loaded_chunk(x, z, |chunk| {
                let origin_x = x * 16;
                let origin_z = z * 16;
                let mut found = None;
                for local_x in 0..16 {
                    for local_z in 0..16 {
                        let world_x = origin_x + local_x;
                        let world_z = origin_z + local_z;
                        if (world_x - center[0].floor() as i32).abs() > radius
                            || (world_z - center[2].floor() as i32).abs() > radius
                        {
                            continue;
                        }
                        for y in chunk_min_y..=chunk_max_y {
                            let Some(state) = chunk.block(local_x as u8, y, local_z as u8) else {
                                continue;
                            };
                            let Some(state) = u16::try_from(state).ok() else {
                                continue;
                            };
                            if is_portal(state) {
                                found = Some([world_x, y, world_z]);
                                break;
                            }
                        }
                    }
                }
                found
            }) && best.is_none_or(|best| {
                let d = |p: [i32; 3]| {
                    let dx = f64::from(p[0]) - center[0];
                    let dy = f64::from(p[1]) - center[1];
                    let dz = f64::from(p[2]) - center[2];
                    dx * dx + dy * dy + dz * dz
                };
                d(found) < d(best)
            }) {
                best = Some(found);
            }
        }
    }
    best.map(|[x, y, z]| [f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5])
}

/// Builds a minimal 2x3 portal with an obsidian frame when no existing portal was found.
/// Returns the interior standing position, or `None` when the chunk is not loaded.
pub fn create_portal(source: &AnvilChunkSource, pos: [f64; 3]) -> Option<[f64; 3]> {
    let dimension = source.dimension();
    let x = pos[0].floor() as i32;
    let z = pos[2].floor() as i32;
    let y = (pos[1].floor() as i32).clamp(
        dimension.min_y() + 8,
        dimension.min_y() + dimension.height() - 8,
    );
    // Require the destination chunk to be loaded before editing it.
    let loaded = source
        .with_loaded_chunk(x.div_euclid(16), z.div_euclid(16), |_| ())
        .is_some();
    if !loaded {
        return None;
    }
    let obsidian = ferrite_data::blocks::default_state(OBSIDIAN)?;
    let portal = portal_state(Axis::X)?;
    // A 2x3 interior at x..x+1, y..y+2, z, surrounded by obsidian and an air approach.
    for dx in -2..=3 {
        for dy in -2..=4 {
            if !PortalWorld::set_block(source, x + dx, y + dy, z, 0) {
                return None;
            }
        }
    }
    for dx in -1..=2 {
        for dy in -1..=3 {
            let edge = dx == -1 || dx == 2 || dy == -1 || dy == 3;
            let state = if edge { obsidian } else { portal };
            if !PortalWorld::set_block(source, x + dx, y + dy, z, state) {
                return None;
            }
        }
    }
    Some([f64::from(x) + 1.0, f64::from(y), f64::from(z) + 0.5])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MapWorld {
        blocks: std::cell::RefCell<HashMap<(i32, i32, i32), u16>>,
    }

    impl Default for MapWorld {
        fn default() -> Self {
            Self {
                blocks: std::cell::RefCell::new(HashMap::new()),
            }
        }
    }

    impl PortalWorld for MapWorld {
        fn block(&self, x: i32, y: i32, z: i32) -> Option<u16> {
            self.blocks.borrow().get(&(x, y, z)).copied().or(Some(0))
        }
        fn set_block(&self, x: i32, y: i32, z: i32, state: u16) -> bool {
            self.blocks.borrow_mut().insert((x, y, z), state);
            true
        }
    }

    fn obsidian() -> u16 {
        ferrite_data::blocks::default_state(OBSIDIAN).expect("obsidian state")
    }

    fn frame_world(width: i32, height: i32) -> MapWorld {
        let world = MapWorld::default();
        for x in -1..=width {
            for y in -1..=height {
                let edge = x == -1 || x == width || y == -1 || y == height;
                if edge {
                    world.set_block(x, y, 0, obsidian());
                }
            }
        }
        world
    }

    #[test]
    fn a_two_by_three_frame_lights() {
        let world = frame_world(2, 3);
        let blocks = frame_blocks(&world, [0, 0, 0]).expect("valid frame");
        assert_eq!(blocks.len(), 6);
        assert!(blocks.iter().all(|(_, state)| is_portal(*state)));
    }

    #[test]
    fn a_wrong_frame_does_not_light() {
        let world = frame_world(2, 3);
        // Punch a hole in the frame.
        world.set_block(2, 1, 0, 0);
        assert!(frame_blocks(&world, [0, 0, 0]).is_none());
        // A too-short frame is rejected too.
        let short = MapWorld::default();
        for x in -1..=2 {
            short.set_block(x, -1, 0, obsidian());
        }
        short.set_block(-1, 0, 0, obsidian());
        short.set_block(2, 0, 0, obsidian());
        short.set_block(-1, 1, 0, obsidian());
        short.set_block(2, 1, 0, obsidian());
        for x in -1..=2 {
            short.set_block(x, 2, 0, obsidian());
        }
        assert!(frame_blocks(&short, [0, 0, 0]).is_none());
    }

    #[test]
    fn nether_coordinates_scale_eight_to_one() {
        assert_eq!(
            scale(
                Dimension::Overworld,
                Dimension::Nether,
                [800.0, 64.0, -160.0]
            ),
            [100.0, 64.0, -20.0]
        );
        assert_eq!(
            scale(
                Dimension::Nether,
                Dimension::Overworld,
                [100.0, 64.0, -20.0]
            ),
            [800.0, 64.0, -160.0]
        );
        assert_eq!(counterpart(Dimension::Overworld), Some(Dimension::Nether));
        assert_eq!(counterpart(Dimension::Nether), Some(Dimension::Overworld));
        assert_eq!(counterpart(Dimension::End), None);
    }

    #[test]
    fn frames_can_be_z_aligned() {
        let world = MapWorld::default();
        for z in -1..=2 {
            for y in -1..=3 {
                let edge = z == -1 || z == 2 || y == -1 || y == 3;
                if edge {
                    world.set_block(0, y, z, obsidian());
                }
            }
        }
        let blocks = frame_blocks(&world, [0, 0, 0]).expect("z frame");
        assert_eq!(blocks.len(), 6);
    }
}
