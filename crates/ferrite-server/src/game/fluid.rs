//! Region-bounded fluid access to loaded chunk columns.
use super::{
    anvil::AnvilChunkSource,
    fx::{FxMap, FxSet},
    region::{Signal, Tracking},
};
use ferrite_fluid::FluidWorld;
use ferrite_net::game::{Outbound, OutboundSender};
use ferrite_region::ChunkPos;
use std::{cell::RefCell, collections::HashMap, sync::Arc};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

type CachedChunkSlot = Option<((i32, i32), Arc<ferrite_world::Chunk>)>;
type PendingUpdates = FxMap<Uuid, (OutboundSender, Vec<Arc<[u8]>>)>;

/// Per-tick chunk read cache backed by shared [`Arc<ferrite_world::Chunk>`] handles and a
/// 2x2 direct-mapped slot front-cache so neighbor boundary lookups skip hashing.
#[derive(Default)]
pub(super) struct ChunkReadCache {
    slots: [CachedChunkSlot; 4],
    chunks: FxMap<(i32, i32), Arc<ferrite_world::Chunk>>,
}

impl ChunkReadCache {
    #[inline]
    fn slot_index(cx: i32, cz: i32) -> usize {
        ((cx & 1) | ((cz & 1) << 1)) as usize
    }

    pub(super) fn chunk_arc(
        &mut self,
        source: &AnvilChunkSource,
        cx: i32,
        cz: i32,
    ) -> Option<&Arc<ferrite_world::Chunk>> {
        let idx = Self::slot_index(cx, cz);
        if !self.slots[idx]
            .as_ref()
            .is_some_and(|(k, _)| *k == (cx, cz))
        {
            let arc = match self.chunks.get(&(cx, cz)) {
                Some(cached) => Arc::clone(cached),
                None => {
                    let loaded = source.chunk_arc(cx, cz)?;
                    self.chunks.insert((cx, cz), Arc::clone(&loaded));
                    loaded
                }
            };
            self.slots[idx] = Some(((cx, cz), arc));
        }
        self.slots[idx].as_ref().map(|(_, c)| c)
    }

    pub(super) fn block(
        &mut self,
        source: &AnvilChunkSource,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<u32> {
        let cx = x.div_euclid(16);
        let cz = z.div_euclid(16);
        self.chunk_arc(source, cx, cz)?
            .block(x.rem_euclid(16) as u8, y, z.rem_euclid(16) as u8)
    }

    pub(super) fn invalidate(&mut self, cx: i32, cz: i32) {
        let idx = Self::slot_index(cx, cz);
        if self.slots[idx]
            .as_ref()
            .is_some_and(|(k, _)| *k == (cx, cz))
        {
            self.slots[idx] = None;
        }
        self.chunks.remove(&(cx, cz));
    }
}

pub(super) struct RegionFluidWorld<'a> {
    pub(super) source: &'a AnvilChunkSource,
    held: &'a HashMap<ChunkPos, usize>,
    tracking: &'a Tracking,
    chunks: RefCell<ChunkReadCache>,
    water_conversion: bool,
    lava_conversion: bool,
    /// The `spread_vines` game rule, read by the random-tick vine handler.
    spread_vines: bool,
    /// Where cross-region placements and redstone wakes are handed to the world loop.
    pub(super) signals: Option<UnboundedSender<Signal>>,
    /// Blocks flowing water destroyed (`WaterFluid.beforeDestroyingBlock` drops their loot).
    pub(super) washed: Vec<([i32; 3], u16)>,
    /// When set, block updates are queued per viewer and sent as one batch by [`Self::flush_updates`].
    batch_updates: bool,
    pending_updates: PendingUpdates,
    chunk_viewers: FxMap<(i32, i32), Vec<(Uuid, OutboundSender)>>,
    dirty_chunks: FxSet<(i32, i32)>,
    write_gen: u32,
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
            chunks: RefCell::new(ChunkReadCache::default()),
            water_conversion,
            lava_conversion,
            spread_vines: true,
            signals: None,
            washed: Vec::new(),
            batch_updates: false,
            pending_updates: FxMap::default(),
            chunk_viewers: FxMap::default(),
            dirty_chunks: FxSet::default(),
            write_gen: 0,
        }
    }
    #[inline]
    pub(super) fn write_gen(&self) -> u32 {
        self.write_gen
    }
    /// Queues fluid block updates per viewer instead of sending one message per block.
    pub fn with_batched_updates(mut self) -> Self {
        self.batch_updates = true;
        self
    }
    /// Sends the queued fluid block updates as one batch per viewer.
    pub fn flush_updates(&mut self) {
        if !self.dirty_chunks.is_empty() {
            let coords: Vec<(i32, i32)> = self.dirty_chunks.drain().collect();
            if let Err(error) = self.source.reencode_dirty_chunks(&coords) {
                tracing::warn!(%error, "deferred fluid chunk re-encode failed");
            }
        }
        for (_, (outbound, packets)) in std::mem::take(&mut self.pending_updates) {
            if !packets.is_empty() {
                let _ = outbound.send(Outbound::Batch(packets));
            }
        }
    }
    /// Lets the world hand cross-region writes to the chunk owner instead of dropping them.
    pub fn with_signals(mut self, signals: UnboundedSender<Signal>) -> Self {
        self.signals = Some(signals);
        self
    }
    /// Sets the `spread_vines` game rule for random ticks.
    pub fn with_spread_vines(mut self, spread_vines: bool) -> Self {
        self.spread_vines = spread_vines;
        self
    }
    pub(super) fn brightness(&self, x: i32, y: i32, z: i32) -> u8 {
        if self.owns(x, z) {
            self.source.light_level(x, y, z).unwrap_or(0)
        } else {
            0
        }
    }
    pub(super) fn owns(&self, x: i32, z: i32) -> bool {
        self.held
            .contains_key(&ChunkPos(x.div_euclid(16), z.div_euclid(16)))
    }
    pub(super) fn spread_vines(&self) -> bool {
        self.spread_vines
    }
    pub(super) fn read_block_opt(&self, x: i32, y: i32, z: i32) -> Option<u32> {
        self.chunks.borrow_mut().block(self.source, x, y, z)
    }
    pub(super) fn cached_chunk_arc(&self, cx: i32, cz: i32) -> Option<Arc<ferrite_world::Chunk>> {
        self.chunks
            .borrow_mut()
            .chunk_arc(self.source, cx, cz)
            .cloned()
    }
}
impl FluidWorld for RegionFluidWorld<'_> {
    fn block(&self, x: i32, y: i32, z: i32) -> u32 {
        // A loaded chunk another region owns is still the authoritative world state. Reading
        // it is safe and lets a fluid decide to spread across the boundary; only writes move
        // through the region scheduler.
        if let Some(state) = self.read_block_opt(x, y, z) {
            return state;
        }
        u32::from(ferrite_data::blocks::default_state("stone").expect("vanilla stone"))
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: u32) {
        if !self.owns(x, z) {
            // Folia ownership: only the owning region writes its chunks. A placement into a
            // loaded neighbour is handed to that region through the world loop.
            if let (Some(signals), Some(expected)) = (&self.signals, self.read_block_opt(x, y, z)) {
                let _ = signals.send(Signal::FluidPlace {
                    dimension: self.source.dimension(),
                    x,
                    y,
                    z,
                    expected,
                    state,
                });
            }
            return;
        }
        let Some(old) = self.read_block_opt(x, y, z) else {
            return;
        };
        if old == state {
            return;
        }
        let cx = x.div_euclid(16);
        let cz = z.div_euclid(16);
        let write_res = if self.batch_updates {
            self.source
                .compare_set_block_deferred(x, y, z, Some(old), state)
        } else {
            self.source.compare_set_block(x, y, z, Some(old), state)
        };
        if let Err(error) = write_res {
            tracing::warn!(x,y,z,%error,"fluid block update failed");
            return;
        }
        if self.batch_updates {
            self.dirty_chunks.insert((cx, cz));
        }
        self.chunks.get_mut().invalidate(cx, cz);
        self.write_gen = self.write_gen.wrapping_add(1);
        // `WaterFluid.beforeDestroyingBlock`: water that takes the place of a plant, torch,
        // button or similar block drops what it would have dropped when broken. Lava only
        // burns it, and waterlogging keeps the block.
        if ferrite_fluid::fluid_state(old).kind == ferrite_fluid::FluidKind::Empty
            && ferrite_fluid::is_liquid_block(state)
            && ferrite_fluid::fluid_state(state).kind == ferrite_fluid::FluidKind::Water
            && let Ok(old_id) = u16::try_from(old)
            && ferrite_data::blocks::state(old_id).is_some_and(|(name, _)| !name.ends_with("air"))
        {
            self.washed.push(([x, y, z], old_id));
        }
        let packet = ferrite_protocol::packets::play::BlockUpdate {
            location: ferrite_protocol::types::BlockPos::new(x, y, z).pack(),
            block_state: ferrite_protocol::VarInt(state as i32),
        };
        let chunk = (x.div_euclid(16), z.div_euclid(16));
        let message = ferrite_net::game::packet(&packet);
        let Outbound::Packet(bytes) = message else {
            return;
        };
        if self.batch_updates {
            let pending = &mut self.pending_updates;
            let world_id = self.source.world_id();
            let viewers = self.chunk_viewers.entry(chunk).or_insert_with(|| {
                let observers = self.tracking.lock().unwrap();
                observers
                    .iter()
                    .filter(|(_, (world, chunks, _))| *world == world_id && chunks.contains(&chunk))
                    .map(|(uuid, (_, _, outbound))| (*uuid, outbound.clone()))
                    .collect()
            });
            for (uuid, outbound) in viewers.iter() {
                pending
                    .entry(*uuid)
                    .or_insert_with(|| (outbound.clone(), Vec::new()))
                    .1
                    .push(Arc::clone(&bytes));
            }
        } else {
            super::region::broadcast_tracked_chunk(
                &self.tracking.lock().unwrap(),
                self.source.world_id(),
                chunk,
                &Outbound::Packet(bytes),
            );
        }
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
    async fn water_that_replaces_a_plant_records_it_for_its_drops() {
        let source = super::super::anvil::AnvilChunkSource::new_memory();
        loop {
            if let Some(result) = source.get(0, 0, 0) {
                result.unwrap();
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let held = HashMap::from([(ferrite_region::ChunkPos(0, 0), 1)]);
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let id = |name: &str| u32::from(ferrite_data::blocks::default_state(name).unwrap());
        source.set_block(1, 65, 1, id("oak_sapling")).unwrap();
        source.set_block(2, 65, 1, id("oak_sapling")).unwrap();
        let mut world = RegionFluidWorld::new(&source, &held, &tracking);
        world.set_block(1, 65, 1, id("water"));
        // Lava only burns the plant: no drops.
        world.set_block(2, 65, 1, id("lava"));
        // Replacing air (or flowing water) over water records nothing.
        world.set_block(3, 65, 1, id("water"));
        assert_eq!(world.washed, vec![([1, 65, 1], id("oak_sapling") as u16)]);
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
                expected,
                state,
            } => {
                assert_eq!(dimension, source.dimension());
                assert_eq!(expected, before);
                assert_eq!((x, y, z, state), (16, 65, 0, water));
            }
            _ => panic!("unexpected hand-off signal"),
        }
    }

    async fn two_loaded_chunks() -> Arc<AnvilChunkSource> {
        let source = AnvilChunkSource::new_memory();
        for chunk in [0, 1] {
            loop {
                if let Some(result) = source.get(chunk, 0, 0) {
                    result.unwrap();
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
        let water = u32::from(ferrite_data::blocks::default_state("water").unwrap());
        let stone = u32::from(ferrite_data::blocks::default_state("stone").unwrap());
        let air = u32::from(ferrite_data::blocks::default_state("air").unwrap());
        for x in 12..=20 {
            source.set_block(x, 64, 0, stone).unwrap();
            source.set_block(x, 65, 0, air).unwrap();
        }
        source.set_block(15, 65, 0, water).unwrap();
        source
    }

    #[tokio::test]
    async fn delayed_fluid_handoff_does_not_overwrite_a_new_solid_block() {
        let source = two_loaded_chunks().await;
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let stone = u32::from(ferrite_data::blocks::default_state("stone").unwrap());
        let water = u32::from(ferrite_data::blocks::default_state("water").unwrap());
        let mut east = super::super::region::Region::default();
        east.hold_chunk(ChunkPos(1, 0));
        // The sending region saw air; the destination changed before its message arrived.
        let expected = source.block(16, 65, 0).unwrap();
        source.set_block(16, 65, 0, stone).unwrap();
        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();
        east.apply_fluid_place(&source, &tracking, signals, (16, 65, 0), expected, water);
        assert_eq!(source.block(16, 65, 0), Some(stone));
    }

    #[tokio::test]
    async fn a_handed_off_water_washes_a_plant_and_drops_its_loot() {
        let source = two_loaded_chunks().await;
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let sapling = u32::from(ferrite_data::blocks::default_state("oak_sapling").unwrap());
        let water = u32::from(ferrite_data::blocks::default_state("water").unwrap());
        source.set_block(16, 66, 1, sapling).unwrap();
        let mut east = super::super::region::Region::default();
        east.hold_chunk(ChunkPos(1, 0));
        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();
        east.apply_fluid_place(&source, &tracking, signals, (16, 66, 1), sapling, water);
        assert_eq!(source.block(16, 66, 1), Some(water));
        assert_eq!(
            east.entities.len(),
            1,
            "the owner that applied the hand-off also drops the washed sapling"
        );
    }

    /// Same region: one region holds both chunks, so water crosses the chunk border directly.
    #[tokio::test]
    async fn water_crosses_a_chunk_border_inside_one_region() {
        let source = two_loaded_chunks().await;
        let held = HashMap::from([(ChunkPos(0, 0), 1), (ChunkPos(1, 0), 1)]);
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let mut world = RegionFluidWorld::new(&source, &held, &tracking);
        let mut ticks = ferrite_fluid::FluidTicks::default();
        ticks.schedule((15, 65, 0), 0);
        ticks.run_until(&mut world, 30);
        let flowed = ferrite_fluid::fluid_state(source.block(16, 65, 0).unwrap());
        assert_eq!(flowed.kind, ferrite_fluid::FluidKind::Water);
        assert!(!flowed.source);
    }

    /// Cross region: the east chunk is loaded but owned by another region. The west region may
    /// only hand the placement over; the east region applies it and keeps the water flowing.
    #[tokio::test]
    async fn water_crosses_a_region_border_through_the_hand_off() {
        let source = two_loaded_chunks().await;
        let air = source.block(16, 65, 0).unwrap();
        let tracking = Arc::new(Mutex::new(HashMap::new()));
        let (signals, mut received) = tokio::sync::mpsc::unbounded_channel();

        let mut west = super::super::region::Region::default();
        west.hold_chunk(ChunkPos(0, 0));
        west.schedule_fluid((15, 65, 0));
        west.tick_fluids(&source, &tracking, signals, true, false);
        assert_eq!(
            source.block(16, 65, 0),
            Some(air),
            "west must not write east"
        );
        let mut placed = None;
        while let Ok(signal) = received.try_recv() {
            if let Signal::FluidPlace { x, y, z, state, .. } = signal
                && (x, y, z) == (16, 65, 0)
            {
                placed = Some(state);
            }
        }
        let state = placed.expect("a hand-off for the east cell");

        let mut east = super::super::region::Region::default();
        east.hold_chunk(ChunkPos(1, 0));
        let (place_signals, _place_rx) = tokio::sync::mpsc::unbounded_channel();
        east.apply_fluid_place(&source, &tracking, place_signals, (16, 65, 0), air, state);
        assert_eq!(source.block(16, 65, 0), Some(state));
        let (signals, _unused) = tokio::sync::mpsc::unbounded_channel();
        for _ in 0..30 {
            east.tick_fluids(&source, &tracking, signals.clone(), true, false);
        }
        let further = ferrite_fluid::fluid_state(source.block(17, 65, 0).unwrap());
        assert_eq!(
            further.kind,
            ferrite_fluid::FluidKind::Water,
            "east keeps flowing"
        );
    }
}
