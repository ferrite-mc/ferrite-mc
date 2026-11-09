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
    ) || name.ends_with("_concrete_powder")
}

/// Vanilla `FallingBlock.isFree`: air, a liquid or fire.
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
    props.is_air || props.fluid.is_some() || ferrite_data::tags::contains("block", "fire", name)
}

/// Scheduled gravity checks for one region.
#[derive(Debug, Default)]
pub(super) struct Gravity {
    clock: u32,
    due: Vec<([i32; 3], u32)>,
    scheduled: HashSet<[i32; 3]>,
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
