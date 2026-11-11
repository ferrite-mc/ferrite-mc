//! Region-bounded fluid access to loaded chunk columns.
use super::{
    anvil::AnvilChunkSource,
    region::{Signal, Tracking},
};
use ferrite_fluid::FluidWorld;
use ferrite_region::ChunkPos;
use std::collections::HashMap;
use tokio::sync::mpsc::UnboundedSender;

pub(super) struct RegionFluidWorld<'a> {
    pub(super) source: &'a AnvilChunkSource,
    held: &'a HashMap<ChunkPos, usize>,
    tracking: &'a Tracking,
    water_conversion: bool,
    lava_conversion: bool,
    /// Where cross-region placements and redstone wakes are handed to the world loop.
    pub(super) signals: Option<UnboundedSender<Signal>>,
}
impl<'a> RegionFluidWorld<'a> {
    pub fn new(
        source: &'a AnvilChunkSource,
        held: &'a HashMap<ChunkPos, usize>,
        tracking: &'a Tracking,
    ) -> Self {
        Self::with_source_rules(source, held, tracking, true, false)
    }
    pub fn with_source_rules(
        source: &'a AnvilChunkSource,
        held: &'a HashMap<ChunkPos, usize>,
        tracking: &'a Tracking,
        water_conversion: bool,
        lava_conversion: bool,
    ) -> Self {
        Self {
            source,
            held,
            tracking,
            water_conversion,
            lava_conversion,
            signals: None,
        }
    }
    /// Lets the world hand cross-region writes to the chunk owner instead of dropping them.
    pub fn with_signals(mut self, signals: UnboundedSender<Signal>) -> Self {
        self.signals = Some(signals);
        self
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
        // A loaded chunk another region owns is still the authoritative world state. Reading
        // it is safe and lets a fluid decide to spread across the boundary; only writes move
        // through the region scheduler.
        if let Some(state) = self.source.block(x, y, z) {
            return state;
        }
        u32::from(ferrite_data::blocks::default_state("stone").expect("vanilla stone"))
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) {
        if !self.owns(x, z) {
            // Folia ownership: only the owning region writes its chunks. A placement into a
            // loaded neighbour is handed to that region through the world loop.
            if let (Some(signals), Some(_)) = (&self.signals, self.source.block(x, y, z)) {
                let _ = signals.send(Signal::FluidPlace {
                    dimension: self.source.dimension(),
                    x,
                    y,
                    z,
                    state,
                });
            }
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
        super::region::broadcast_tracked_chunk(
            &self.tracking.lock().unwrap(),
            self.source.world_id(),
            chunk,
            &ferrite_net::game::packet(&packet),
        );
    }
    fn ultrawarm(&self) -> bool {
        self.source.dimension() == super::dimension::Dimension::Nether
    }
    fn lava_conversion(&self) -> bool {
        self.lava_conversion
    }
    fn water_conversion(&self) -> bool {
        self.water_conversion
    }
    fn owns(&self, x: i32, z: i32) -> bool {
        RegionFluidWorld::owns(self, x, z)
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

    #[tokio::test]
    async fn fluid_world_uses_dimension_and_source_conversion_rules() {
        use super::super::dimension::Dimension;

        let overworld = AnvilChunkSource::new_memory_dimension(None, Dimension::Overworld);
        let nether = AnvilChunkSource::new_memory_dimension(None, Dimension::Nether);
        let held = HashMap::new();
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let normal = RegionFluidWorld::with_source_rules(&overworld, &held, &tracking, false, true);
        let hot = RegionFluidWorld::with_source_rules(&nether, &held, &tracking, true, false);

        assert!(!normal.ultrawarm());
        assert!(hot.ultrawarm());
        assert!(!normal.water_conversion());
        assert!(normal.lava_conversion());
        assert!(hot.water_conversion());
        assert!(!hot.lava_conversion());
        assert_eq!(
            ferrite_fluid::tick_delay(ferrite_fluid::FluidKind::Lava, normal.ultrawarm()),
            30
        );
        assert_eq!(
            ferrite_fluid::tick_delay(ferrite_fluid::FluidKind::Lava, hot.ultrawarm()),
            10
        );
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
        // A loaded neighbour is readable as a snapshot, but still not writable by this region.
        assert_eq!(world.block(16, 65, 0), before);
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
    async fn cross_region_fluid_writes_are_handed_off() {
        let source = super::super::anvil::AnvilChunkSource::new_memory();
        loop {
            if let Some(result) = source.get(1, 0, 0) {
                result.unwrap();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let before = source.block(16, 65, 0).unwrap();
        let held = HashMap::from([(ferrite_region::ChunkPos(0, 0), 1)]);
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let (signals, mut received) = tokio::sync::mpsc::unbounded_channel();
        let mut world = RegionFluidWorld::new(&source, &held, &tracking).with_signals(signals);
        let water = u32::from(ferrite_data::blocks::default_state("water").unwrap());
        world.set_block(16, 65, 0, water);
        assert_eq!(source.block(16, 65, 0), Some(before));
        match received.try_recv().expect("hand-off signal") {
            Signal::FluidPlace {
                dimension,
                x,
                y,
                z,
                state,
            } => {
                assert_eq!(dimension, source.dimension());
                assert_eq!((x, y, z, state), (16, 65, 0, water));
            }
            _ => panic!("unexpected hand-off signal"),
        }
    }
}
