//! Gravity blocks (vanilla `FallingBlock`): sand, gravel, concrete powder, anvils and the
//! suspicious variants become `FallingBlockEntity` two ticks (`FallingBlock.getDelay`) after
//! `onPlace`/`neighborChanged` finds them unsupported.
//!
//! Only a block whose chunk this region holds is converted; the falling entity then lives in
//! the region's ordinary entity store, so tracking, region hand-off and physics are shared
//! with items.

use super::{anvil::AnvilChunkSource, fluid::RegionFluidWorld, region::Tracking};
use ferrite_entity::{EntityStore, falling};
use ferrite_fluid::FluidWorld;
use ferrite_math::Vec3;
use ferrite_region::ChunkPos;
use std::collections::{HashMap, HashSet};

/// Vanilla `FallingBlock.getDelay`.
pub const FALL_DELAY: u32 = 2;

/// Whether a block state falls when unsupported. Vanilla's `FallingBlock` subclasses:
/// `SandBlock` (sand, red sand, suspicious sand), `GravelBlock` (gravel, suspicious gravel),
/// `ConcretePowderBlock` and `AnvilBlock`.
pub fn is_gravity(state: u16) -> bool {
    let Some((name, _)) = ferrite_data::blocks::state(state) else {
        return false;
    };
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    matches!(
        name,
        "sand"
            | "red_sand"
            | "gravel"
            | "suspicious_sand"
            | "suspicious_gravel"
            | "anvil"
            | "chipped_anvil"
            | "damaged_anvil"
            | "dragon_egg"
    ) || name.ends_with("_concrete_powder")
}

/// Vanilla `ConcretePowderBlock.canSolidify`: the state holds water.
fn holds_water(state: u16) -> bool {
    ferrite_fluid::fluid_state(u32::from(state)).kind == ferrite_fluid::FluidKind::Water
}

/// The concrete a powder block at `at` turns into, vanilla `ConcretePowderBlock.shouldSolidify`:
/// the block itself holds water, or a side or the top touches water that does not sit behind a
/// sturdy face (the underside only counts when the powder's own cell is wet).
pub fn solidified(
    state: u16,
    at: [i32; 3],
    block: impl Fn([i32; 3]) -> Option<u16>,
) -> Option<u16> {
    use ferrite_math::Direction;
    let (name, _) = ferrite_data::blocks::state(state)?;
    let concrete = name.strip_suffix("_powder")?;
    let target = ferrite_data::blocks::default_state(concrete)?;
    let own_wet = holds_water(state);
    let wet = own_wet
        || [
            Direction::West,
            Direction::East,
            Direction::Down,
            Direction::Up,
            Direction::North,
            Direction::South,
        ]
        .into_iter()
        .any(|direction| {
            if direction == Direction::Down && !own_wet {
                return false;
            }
            let (dx, dy, dz) = direction.step();
            let Some(neighbor) = block([at[0] + dx, at[1] + dy, at[2] + dz]) else {
                return false;
            };
            if !holds_water(neighbor) {
                return false;
            }
            let face = match direction.opposite() {
                Direction::West => 0,
                Direction::East => 1,
                Direction::Down => 2,
                Direction::Up => 3,
                Direction::North => 4,
                Direction::South => 5,
            };
            ferrite_data::block_props::face_occlusion(neighbor, face).is_empty()
        });
    wet.then_some(target)
}

/// Vanilla `FallingBlock.isFree`: `isAir() || is(FIRE) || liquid() || canBeReplaced()`.
///
/// `liquid()` is true for the standalone water and lava blocks only: a waterlogged slab or
/// stairs carries water but is a solid block, so a gravity block resting on it stays put.
/// Replaceable blocks (grass, snow layers, vines, ...) are free, so sand falls through them.
pub fn is_free(state: Option<u16>) -> bool {
    let Some(state) = state else {
        return false;
    };
    if usize::from(state) >= ferrite_data::blocks::STATE_COUNT {
        return false;
    }
    let Some((name, _)) = ferrite_data::blocks::state(state) else {
        return false;
    };
    let props = ferrite_data::block_props::get(state);
    props.is_air
        || ferrite_fluid::is_liquid_block(u32::from(state))
        || props.replaceable
        || ferrite_data::tags::contains("block", "fire", name)
}

/// Scheduled gravity checks for one region.
#[derive(Debug, Default)]
pub(super) struct Gravity {
    clock: u32,
    due: Vec<([i32; 3], u32)>,
    pub(super) scheduled: HashSet<[i32; 3]>,
}

impl Gravity {
    /// Vanilla `Level.scheduleTick(pos, block, 2)`.
    pub fn schedule(&mut self, pos: [i32; 3]) {
        if self.scheduled.insert(pos) {
            self.due.push((pos, self.clock.wrapping_add(FALL_DELAY)));
        }
    }

    /// Keeps the checks whose block stays in this region's sections, as regions split.
    pub fn split(&mut self, keep: impl Fn(i32, i32) -> bool) -> Self {
        let mut out = Self {
            clock: self.clock,
            due: Vec::new(),
            scheduled: HashSet::new(),
        };
        let mut moved = Vec::new();
        self.due.retain(|&(pos, due)| {
            if keep(pos[0], pos[2]) {
                moved.push((pos, due));
                false
            } else {
                true
            }
        });
        for (pos, due) in moved {
            self.scheduled.remove(&pos);
            out.scheduled.insert(pos);
            out.due.push((pos, due));
        }
        out
    }

    /// Merges another region's checks. Due times are absolute in the source region's clock,
    /// so each one is re-based on this region's clock by its remaining delay.
    pub fn merge(&mut self, other: Self) {
        let base = self.clock;
        for (pos, due) in other.due {
            let remaining = due.saturating_sub(other.clock);
            if self.scheduled.insert(pos) {
                self.due.push((pos, base.saturating_add(remaining)));
            }
        }
    }

    /// Runs the checks that are due. Returns the positions whose block turned into a falling
    /// block entity, which are new block changes for the neighbor system.
    pub fn tick(
        &mut self,
        source: &AnvilChunkSource,
        held: &HashMap<ChunkPos, usize>,
        tracking: &Tracking,
        entities: &mut EntityStore,
    ) -> Vec<[i32; 3]> {
        self.clock = self.clock.wrapping_add(1);
        let mut removed = Vec::new();
        let mut world = RegionFluidWorld::new(source, held, tracking);
        let air = ferrite_data::blocks::default_state("air").expect("vanilla air");
        let mut index = 0;
        while index < self.due.len() {
            if self.due[index].1 > self.clock {
                index += 1;
                continue;
            }
            let (pos, _) = self.due.swap_remove(index);
            self.scheduled.remove(&pos);
            if !world.owns(pos[0], pos[2]) {
                continue;
            }
            let Some(state) = source
                .block(pos[0], pos[1], pos[2])
                .and_then(|s| u16::try_from(s).ok())
            else {
                continue;
            };
            if !is_gravity(state) {
                continue;
            }
            let below = source
                .block(pos[0], pos[1] - 1, pos[2])
                .and_then(|s| u16::try_from(s).ok());
            if !is_free(below) {
                continue;
            }
            // Vanilla `FallingBlock.tick`: remove the block, then `FallingBlockEntity.fall`.
            world.set_block(pos[0], pos[1], pos[2], u32::from(air));
            entities.insert(falling::new_falling(
                Vec3::new(
                    f64::from(pos[0]) + 0.5,
                    f64::from(pos[1]),
                    f64::from(pos[2]) + 0.5,
                ),
                state,
            ));
            removed.push(pos);
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_entity::EntityStore;
    use std::collections::HashMap;

    fn state(name: &str) -> u16 {
        ferrite_data::blocks::default_state(name).unwrap()
    }

    #[test]
    fn gravity_blocks_are_the_vanilla_falling_blocks() {
        for name in [
            "sand",
            "red_sand",
            "gravel",
            "suspicious_sand",
            "suspicious_gravel",
            "anvil",
            "chipped_anvil",
            "damaged_anvil",
            "white_concrete_powder",
            "black_concrete_powder",
            "dragon_egg",
        ] {
            assert!(is_gravity(state(name)), "{name}");
        }
        for name in ["stone", "dirt", "torch", "glass"] {
            assert!(!is_gravity(state(name)), "{name}");
        }
        assert!(is_free(Some(state("air"))));
        assert!(is_free(Some(state("water"))));
        assert!(is_free(Some(state("fire"))));
        assert!(!is_free(Some(state("stone"))));
        assert!(!is_free(None));
    }

    #[test]
    fn concrete_powder_solidifies_beside_water_but_not_behind_a_sturdy_face() {
        let powder = state("red_concrete_powder");
        let at = [0, 64, 0];
        let air = state("air");
        let water = state("water");
        let stone = state("stone");
        let world = |wet: Option<[i32; 3]>| {
            move |pos: [i32; 3]| Some(if Some(pos) == wet { water } else { air })
        };
        let concrete = ferrite_data::blocks::default_state("red_concrete").unwrap();
        assert_eq!(
            solidified(powder, at, world(None)),
            None,
            "dry powder stays"
        );
        assert_eq!(
            solidified(powder, at, world(Some([1, 64, 0]))),
            Some(concrete)
        );
        assert_eq!(
            solidified(powder, at, world(Some([0, 65, 0]))),
            Some(concrete)
        );
        assert_eq!(
            solidified(powder, at, world(Some([0, 63, 0]))),
            None,
            "water under dry powder does not count"
        );
        assert_eq!(solidified(stone, at, world(Some([1, 64, 0]))), None);
        // Waterlogged powder cannot exist, but a wet cell is a wet cell.
        assert_eq!(solidified(powder, at, |_| Some(stone)), None);
    }

    /// Vanilla `isFree` includes `canBeReplaced()`: sand falls through grass and snow layers.
    #[test]
    fn replaceable_blocks_are_free_to_fall_into() {
        assert!(is_free(Some(state("short_grass"))));
        assert!(is_free(Some(state("vine"))));
    }

    /// A waterlogged slab holds water but is not a liquid block, so it supports a falling block.
    #[test]
    fn waterlogged_blocks_are_not_free() {
        let wet = ferrite_data::blocks::state_id(
            "oak_slab",
            &[("type", "bottom"), ("waterlogged", "true")],
        )
        .unwrap();
        assert!(ferrite_fluid::fluid_state(u32::from(wet)).kind != ferrite_fluid::FluidKind::Empty);
        assert!(!is_free(Some(wet)));
    }

    /// Every region counts its own ticks, so an entry merged from a region that has ticked a
    /// different number of times must keep its remaining delay, not its absolute due tick.
    #[test]
    fn merge_keeps_remaining_delay_across_different_region_clocks() {
        let mut left = Gravity {
            clock: 100,
            ..Gravity::default()
        };
        let mut right = Gravity {
            clock: 10,
            ..Gravity::default()
        };
        right.schedule([1, 70, 0]);
        left.merge(right);
        assert_eq!(left.due, vec![([1, 70, 0], 102)]);
    }

    #[tokio::test]
    async fn unsupported_sand_becomes_a_falling_entity_after_two_ticks() {
        let source = AnvilChunkSource::new_memory();
        loop {
            if let Some(result) = source.get(0, 0, 0) {
                result.unwrap();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let air = u32::from(state("air"));
        let sand = u32::from(state("sand"));
        source.set_block(0, 70, 0, sand).unwrap();
        source.set_block(0, 69, 0, air).unwrap();
        let mut gravity = Gravity::default();
        gravity.schedule([0, 70, 0]);
        let held = HashMap::from([(ChunkPos(0, 0), 1)]);
        let tracking = std::sync::Arc::new(std::sync::Mutex::new(HashMap::new()));
        let mut entities = EntityStore::default();
        assert!(
            gravity
                .tick(&source, &held, &tracking, &mut entities)
                .is_empty()
        );
        let removed = gravity.tick(&source, &held, &tracking, &mut entities);
        assert_eq!(removed, vec![[0, 70, 0]]);
        assert_eq!(source.block(0, 70, 0), Some(air));
        assert_eq!(entities.len(), 1);
        let key = entities.keys()[0];
        assert_eq!(entities.data(key).unwrap().block_state, Some(state("sand")));
        assert_eq!(entities.pos(key).unwrap().y, 70.0);
    }
}
