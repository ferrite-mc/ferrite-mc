use crate::{PaletteError, PaletteKind, PalettedContainer};
use ferrite_nbt::Compound;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlockFlags {
    pub air: bool,
    pub motion_blocking: bool,
    pub fluid: bool,
    pub leaves: bool,
}
pub trait BlockInfo {
    fn flags(&self, state: u32) -> BlockFlags;
    fn randomly_ticking(&self, _state: u32) -> bool {
        false
    }
    fn fluid_randomly_ticking(&self, _state: u32) -> bool {
        false
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HeightmapKind {
    WorldSurfaceWg = 0,
    WorldSurface = 1,
    OceanFloorWg = 2,
    OceanFloor = 3,
    MotionBlocking = 4,
    MotionBlockingNoLeaves = 5,
}
impl HeightmapKind {
    pub const ALL: [Self; 6] = [
        Self::WorldSurfaceWg,
        Self::WorldSurface,
        Self::OceanFloorWg,
        Self::OceanFloor,
        Self::MotionBlocking,
        Self::MotionBlockingNoLeaves,
    ];
    fn includes(self, flags: BlockFlags) -> bool {
        match self {
            Self::WorldSurface | Self::WorldSurfaceWg => !flags.air,
            Self::OceanFloor | Self::OceanFloorWg => flags.motion_blocking,
            Self::MotionBlocking => flags.motion_blocking || flags.fluid,
            Self::MotionBlockingNoLeaves => (flags.motion_blocking || flags.fluid) && !flags.leaves,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum ChunkError {
    #[error(transparent)]
    Palette(#[from] PaletteError),
    #[error("invalid dimension height or bounds")]
    Bounds,
}
#[derive(Debug, Clone)]
pub struct ChunkSection {
    pub(crate) blocks: PalettedContainer,
    pub(crate) biomes: PalettedContainer,
    pub(crate) block_count: u16,
    pub(crate) fluid_count: u16,
    pub(crate) random_blocks: u16,
    pub(crate) random_fluids: u16,
}
impl ChunkSection {
    pub fn new(air: u32, biome: u32, block_bits: u8, biome_bits: u8) -> Result<Self, ChunkError> {
        Ok(Self {
            blocks: PalettedContainer::new(PaletteKind::Blocks, air, block_bits)?,
            biomes: PalettedContainer::new(PaletteKind::Biomes, biome, biome_bits)?,
            block_count: 0,
            fluid_count: 0,
            random_blocks: 0,
            random_fluids: 0,
        })
    }
    pub fn block_count(&self) -> u16 {
        self.block_count
    }
    pub fn fluid_count(&self) -> u16 {
        self.fluid_count
    }
    pub fn randomly_ticking_block_count(&self) -> u16 {
        self.random_blocks
    }
    pub fn randomly_ticking_fluid_count(&self) -> u16 {
        self.random_fluids
    }
    pub fn is_randomly_ticking(&self) -> bool {
        self.random_blocks != 0 || self.random_fluids != 0
    }
    pub fn blocks(&self) -> &PalettedContainer {
        &self.blocks
    }
    pub fn biomes(&self) -> &PalettedContainer {
        &self.biomes
    }
    pub fn set_biome(&mut self, index: usize, value: u32) -> Result<(), ChunkError> {
        Ok(self.biomes.set(index, value)?)
    }
    /// Replaces all 4096 block states in this section in one pass, preserving its biomes.
    pub fn set_blocks_u16(
        &mut self,
        states: &[u16],
        info: &impl BlockInfo,
    ) -> Result<(), ChunkError> {
        let container = PalettedContainer::from_states_u16(
            PaletteKind::Blocks,
            states,
            self.blocks.global_bits(),
        )?;
        let mut block_count = 0u16;
        let mut fluid_count = 0u16;
        let mut random_blocks = 0u16;
        let mut random_fluids = 0u16;
        for (state, count) in container.value_counts() {
            let count = count as u16;
            let flags = info.flags(state);
            if !flags.air {
                block_count += count;
            }
            if flags.fluid {
                fluid_count += count;
            }
            if info.randomly_ticking(state) {
                random_blocks += count;
            }
            if info.fluid_randomly_ticking(state) {
                random_fluids += count;
            }
        }
        self.blocks = container;
        self.block_count = block_count;
        self.fluid_count = fluid_count;
        self.random_blocks = random_blocks;
        self.random_fluids = random_fluids;
        Ok(())
    }
    pub fn set_block(
        &mut self,
        index: usize,
        value: u32,
        info: &impl BlockInfo,
    ) -> Result<(), ChunkError> {
        let old = self.blocks.get(index).ok_or(ChunkError::Bounds)?;
        if old == value {
            return Ok(());
        }
        self.blocks.set(index, value)?;
        for (count, was, is) in [
            (
                &mut self.random_blocks,
                info.randomly_ticking(old),
                info.randomly_ticking(value),
            ),
            (
                &mut self.random_fluids,
                info.fluid_randomly_ticking(old),
                info.fluid_randomly_ticking(value),
            ),
        ] {
            match (was, is) {
                (false, true) => *count += 1,
                (true, false) => *count -= 1,
                _ => {}
            }
        }
        let old_flags = info.flags(old);
        let new_flags = info.flags(value);
        match (old_flags.fluid, new_flags.fluid) {
            (false, true) => self.fluid_count += 1,
            (true, false) => self.fluid_count -= 1,
            _ => {}
        }
        match (old_flags.air, new_flags.air) {
            (true, false) => self.block_count += 1,
            (false, true) => self.block_count -= 1,
            _ => {}
        }
        Ok(())
    }
    pub fn encode_network(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_network_into(&mut out);
        out
    }

    /// Appends the network-encoded section into `out` without intermediate allocations.
    pub fn encode_network_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.block_count.to_be_bytes());
        out.extend_from_slice(&self.fluid_count.to_be_bytes());
        self.blocks.encode_network_into(out);
        self.biomes.encode_network_into(out);
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct BlockEntity {
    pub type_id: u32,
    pub data: Compound,
}
#[derive(Debug, Clone)]
pub struct Chunk {
    pub(crate) min_y: i32,
    pub(crate) height: u32,
    pub(crate) sections: Vec<ChunkSection>,
    pub(crate) heights: [Vec<u16>; 6],
    pub block_entities: BTreeMap<(u8, i32, u8), BlockEntity>,
    pub(crate) source: Option<Compound>,
}
impl Chunk {
    pub fn new(
        min_y: i32,
        height: u32,
        air: u32,
        biome: u32,
        block_bits: u8,
        biome_bits: u8,
    ) -> Result<Self, ChunkError> {
        if min_y % 16 != 0
            || height == 0
            || !height.is_multiple_of(16)
            || height > u16::MAX as u32
            || min_y.checked_add(height as i32).is_none()
        {
            return Err(ChunkError::Bounds);
        }
        let mut sections = Vec::with_capacity((height / 16) as usize);
        for _ in 0..height / 16 {
            sections.push(ChunkSection::new(air, biome, block_bits, biome_bits)?);
        }
        Ok(Self {
            min_y,
            height,
            sections,
            heights: std::array::from_fn(|_| vec![0; 256]),
            block_entities: BTreeMap::new(),
            source: None,
        })
    }
    pub fn min_y(&self) -> i32 {
        self.min_y
    }
    pub fn height_blocks(&self) -> u32 {
        self.height
    }
    pub fn sections(&self) -> &[ChunkSection] {
        &self.sections
    }
    pub fn sections_mut(&mut self) -> &mut [ChunkSection] {
        &mut self.sections
    }
    fn coords(&self, x: u8, y: i32, z: u8) -> Result<(usize, usize), ChunkError> {
        if x >= 16 || z >= 16 || y < self.min_y || y >= self.min_y + self.height as i32 {
            return Err(ChunkError::Bounds);
        }
        let local = (y - self.min_y) as usize;
        Ok((
            local / 16,
            (local % 16) * 256 + usize::from(z) * 16 + usize::from(x),
        ))
    }
    pub fn block(&self, x: u8, y: i32, z: u8) -> Option<u32> {
        let (s, i) = self.coords(x, y, z).ok()?;
        self.sections[s].blocks.get(i)
    }
    pub fn height(&self, kind: HeightmapKind, x: u8, z: u8) -> Option<u16> {
        if x >= 16 || z >= 16 {
            None
        } else {
            Some(self.heights[kind as usize][usize::from(z) * 16 + usize::from(x)])
        }
    }
    /// Sets a block state without updating heightmaps; call [`Self::recompute_heightmaps`]
    /// once after bulk terrain writes.
    pub fn set_block_raw(
        &mut self,
        x: u8,
        y: i32,
        z: u8,
        value: u32,
        info: &impl BlockInfo,
    ) -> Result<(), ChunkError> {
        let (s, i) = self.coords(x, y, z)?;
        self.sections[s].set_block(i, value, info)
    }

    pub fn set_block(
        &mut self,
        x: u8,
        y: i32,
        z: u8,
        value: u32,
        info: &impl BlockInfo,
    ) -> Result<(), ChunkError> {
        let (s, i) = self.coords(x, y, z)?;
        let old = self.sections[s].blocks.get(i).ok_or(ChunkError::Bounds)?;
        self.sections[s].set_block(i, value, info)?;
        let old_flags = info.flags(old);
        let flags = info.flags(value);
        let column = usize::from(z) * 16 + usize::from(x);
        let h = (y - self.min_y + 1) as u16;
        for kind in HeightmapKind::ALL {
            let previous = self.heights[kind as usize][column];
            if kind.includes(flags) && h > previous {
                self.heights[kind as usize][column] = h;
            } else if kind.includes(old_flags) && !kind.includes(flags) && h == previous {
                let mut new_height = 0;
                for below in (self.min_y..y).rev() {
                    if kind.includes(info.flags(self.block(x, below, z).unwrap())) {
                        new_height = (below - self.min_y + 1) as u16;
                        break;
                    }
                }
                self.heights[kind as usize][column] = new_height;
            }
        }
        Ok(())
    }
    pub fn recompute_heightmaps(&mut self, info: &impl BlockInfo) {
        for map in &mut self.heights {
            map.fill(0);
        }
        let Some(top_section) = self
            .sections
            .iter()
            .rposition(|s| s.block_count != 0 || s.fluid_count != 0)
        else {
            return;
        };
        for z in 0..16u8 {
            for x in 0..16u8 {
                let col = usize::from(z) * 16 + usize::from(x);
                let mut remaining = 0b111111u8;
                'sections: for s in (0..=top_section).rev() {
                    let sec = &self.sections[s];
                    if sec.block_count == 0 && sec.fluid_count == 0 {
                        continue;
                    }
                    let sec_base_y = self.min_y + (s as i32) * 16;
                    let xz_offset = usize::from(z) * 16 + usize::from(x);
                    for ly in (0..16usize).rev() {
                        let state = sec.blocks.get(ly * 256 + xz_offset).unwrap();
                        let flags = info.flags(state);
                        if flags.air && !flags.fluid && !flags.motion_blocking {
                            continue;
                        }
                        let h = (sec_base_y + ly as i32 - self.min_y + 1) as u16;
                        for kind in HeightmapKind::ALL {
                            let bit = 1u8 << (kind as u8);
                            if (remaining & bit) != 0 && kind.includes(flags) {
                                self.heights[kind as usize][col] = h;
                                remaining &= !bit;
                            }
                        }
                        if remaining == 0 {
                            break 'sections;
                        }
                    }
                }
            }
        }
    }
    pub fn heights(&self, kind: HeightmapKind) -> &[u16] {
        &self.heights[kind as usize]
    }
}
