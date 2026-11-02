//! Region-bounded fluid access to loaded chunk columns.
use super::{Dimension, anvil::AnvilChunkSource, region::Tracking};
use ferrite_fluid::FluidWorld;
use ferrite_region::ChunkPos;
use std::collections::HashMap;

pub(super) struct RegionFluidWorld<'a> {
    pub(super) source: &'a AnvilChunkSource,
    held: &'a HashMap<ChunkPos, usize>,
    tracking: &'a Tracking,
}
impl<'a> RegionFluidWorld<'a> {
    pub fn new(
        source: &'a AnvilChunkSource,
        held: &'a HashMap<ChunkPos, usize>,
        tracking: &'a Tracking,
    ) -> Self {
        Self {
            source,
            held,
            tracking,
        }
    }
    pub(super) fn brightness(&self, x: i32, y: i32, z: i32) -> u8 {
        if self.owns(x, z) {
            self.source.brightness(x, y, z).unwrap_or(0)
        } else {
            0
        }
    }
    pub(super) fn owns(&self, x: i32, z: i32) -> bool {
        self.held
            .contains_key(&ChunkPos(x.div_euclid(16), z.div_euclid(16)))
    }
}
impl FluidWorld for RegionFluidWorld<'_> {
    fn block(&self, x: i32, y: i32, z: i32) -> u32 {
        if self.owns(x, z)
            && let Some(state) = self.source.block(x, y, z)
        {
            return state;
        }
        u32::from(ferrite_data::blocks::default_state("stone").expect("vanilla stone"))
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) {
        if !self.owns(x, z) {
            return;
        }
        let Some(old) = self.source.block(x, y, z) else {
            return;
        };
        if old == state {
            return;
        }
        if let Err(error) = self.source.compare_set_block(x, y, z, Some(old), state) {
            tracing::warn!(x,y,z,%error,"fluid block update failed");
            return;
        }
        let packet = ferrite_protocol::packets::play::BlockUpdate {
            location: ferrite_protocol::types::BlockPos::new(x, y, z).pack(),
            block_state: ferrite_protocol::VarInt(state as i32),
        };
        let chunk = (x.div_euclid(16), z.div_euclid(16));
        for (loaded, tx) in self.tracking.lock().unwrap().values() {
            if loaded.contains(&chunk) {
                let _ = tx.send(ferrite_net::game::packet(&packet));
            }
        }
    }
    /// Vanilla's ultrawarm dimension property, spelled
    /// `minecraft:gameplay/fast_lava` in the 26.3 `dimension_type` data: only
    /// the nether gets the faster lava ticks, wider lava spread and shorter
    /// lava drop-off.
    fn ultrawarm(&self) -> bool {
        self.source.dimension() == Dimension::Nether
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrite_fluid::FluidWorld;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    async fn loaded_source(dimension: Dimension) -> Arc<AnvilChunkSource> {
        let source = AnvilChunkSource::new_memory_dimension(None, dimension);
        for _ in 0..2_000 {
            match source.get(0, 0, 0) {
                Some(result) => {
                    result.expect("chunk load");
                    return source;
                }
                None => tokio::time::sleep(std::time::Duration::from_millis(5)).await,
            }
        }
        panic!("chunk never loaded for {}", dimension.key());
    }

    #[tokio::test]
    async fn missing_and_unowned_chunks_are_solid_and_not_loaded() {
        let source = super::super::anvil::AnvilChunkSource::new_memory();
        let held = HashMap::from([(ferrite_region::ChunkPos(0, 0), 1)]);
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let mut world = RegionFluidWorld::new(&source, &held, &tracking);
        let stone = u32::from(ferrite_data::blocks::default_state("stone").unwrap());
        assert_eq!(world.block(0, 65, 0), stone);
        assert_eq!(world.block(16, 65, 0), stone);
        world.set_block(16, 65, 0, 0);
        assert_eq!(source.block(16, 65, 0), None);
        // A previously loaded but unheld cached chunk must remain inaccessible.
        loop {
            if let Some(result) = source.get(1, 0, 0) {
                result.unwrap();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let before = source.block(16, 65, 0).unwrap();
        assert_eq!(world.block(16, 65, 0), stone);
        world.set_block(
            16,
            65,
            0,
            u32::from(ferrite_data::blocks::default_state("water").unwrap()),
        );
        assert_eq!(source.block(16, 65, 0), Some(before));
    }
    #[tokio::test]
    async fn fluid_tick_never_loads_unheld_neighbor() {
        let source = super::super::anvil::AnvilChunkSource::new_memory();
        loop {
            if let Some(result) = source.get(0, 0, 0) {
                result.unwrap();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let water = u32::from(ferrite_data::blocks::default_state("water").unwrap());
        let stone = u32::from(ferrite_data::blocks::default_state("stone").unwrap());
        source.set_block(15, 65, 0, water).unwrap();
        source.set_block(15, 64, 0, stone).unwrap();
        assert_eq!(source.block(16, 65, 0), None);
        let held = HashMap::from([(ChunkPos(0, 0), 1)]);
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let mut world = RegionFluidWorld::new(&source, &held, &tracking);
        let mut ticks = ferrite_fluid::FluidTicks::default();
        ticks.schedule((15, 65, 0), 0);
        ticks.run_until(&mut world, 20);
        assert_eq!(source.block(16, 65, 0), None);
        assert_eq!(world.block(16, 65, 0), stone);
    }

    #[tokio::test]
    async fn ultrawarm_follows_the_nether_and_changes_lava_behavior() {
        let held = HashMap::from([(ChunkPos(0, 0), 1)]);
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let air = u32::from(ferrite_data::blocks::default_state("air").unwrap());
        let lava = u32::from(ferrite_data::blocks::default_state("lava").unwrap());
        let water = u32::from(ferrite_data::blocks::default_state("water").unwrap());
        for (dimension, ultrawarm) in [(Dimension::Overworld, false), (Dimension::Nether, true)] {
            let source = loaded_source(dimension).await;
            // Two isolated fluids: lava at y=64 and water at y=70, each with an
            // empty cell to flow into.
            for (y, state) in [(64, lava), (70, water)] {
                source.set_block(0, y, 0, state).unwrap();
                source.set_block(1, y, 0, air).unwrap();
                source.set_block(1, y + 1, 0, air).unwrap();
            }
            let world = RegionFluidWorld::new(&source, &held, &tracking);
            assert_eq!(
                world.ultrawarm(),
                ultrawarm,
                "{} must report vanilla fast_lava",
                dimension.key()
            );
            assert_eq!(
                ferrite_fluid::tick_delay(ferrite_fluid::FluidKind::Lava, world.ultrawarm()),
                if ultrawarm { 10 } else { 30 },
                "lava ticks faster in the nether"
            );
            assert_eq!(
                ferrite_fluid::get_new_liquid(
                    &world,
                    1,
                    64,
                    0,
                    ferrite_fluid::FluidKind::Lava,
                    false,
                )
                .amount,
                if ultrawarm { 7 } else { 6 },
                "nether lava keeps two levels, overworld lava drops three"
            );
            // Water is not ultrawarm-sensitive in either dimension.
            assert_eq!(
                ferrite_fluid::tick_delay(ferrite_fluid::FluidKind::Water, world.ultrawarm()),
                5
            );
            assert_eq!(
                ferrite_fluid::get_new_liquid(
                    &world,
                    1,
                    70,
                    0,
                    ferrite_fluid::FluidKind::Water,
                    false,
                )
                .amount,
                7,
                "water ignores the dimension's ultrawarm property"
            );
        }
    }
}
