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
    /// Replaces all 64 biome cells in this section in one pass.
    pub fn set_biomes_u32(&mut self, values: &[u32; 64]) -> Result<(), ChunkError> {
        self.biomes = PalettedContainer::from_values_u32(
            PaletteKind::Biomes,
            values,
            self.biomes.global_bits(),
        )?;
        Ok(())
    }
    /// Replaces this section's biome container with a pre-built container.
    pub fn set_biomes_container(&mut self, biomes: PalettedContainer) {
        debug_assert_eq!(biomes.kind, PaletteKind::Biomes);
        self.biomes = biomes;
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
    pub(crate) heights: Box<[[u16; 256]; 6]>,
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
            heights: Box::new([[0u16; 256]; 6]),
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
        for map in &mut *self.heights {
            map.fill(0);
        }
        let Some(top_section) = self
            .sections
            .iter()
            .rposition(|s| s.block_count != 0 || s.fluid_count != 0)
        else {
            return;
        };
        let mut remaining = [0b111111u8; 256];
        let mut unfinished = 256usize;

        for s in (0..=top_section).rev() {
            let sec = &self.sections[s];
            if sec.block_count == 0 && sec.fluid_count == 0 {
                continue;
            }
            let bits = sec.blocks.bits_per_entry();
            if bits == 0 {
                let state = sec.blocks.palette()[0];
                if state == 0 {
                    continue;
                }
                let m = flags_heightmap_mask(info.flags(state));
                if m == 0 {
                    continue;
                }
                let h = (s * 16 + 16) as u16;
                for (col, rem) in remaining.iter_mut().enumerate() {
                    let hit = *rem & m;
                    if hit != 0 {
                        *rem &= !hit;
                        if *rem == 0 {
                            unfinished -= 1;
                        }
                        let mut b = hit;
                        while b != 0 {
                            let k = b.trailing_zeros() as usize;
                            self.heights[k][col] = h;
                            b &= b - 1;
                        }
                    }
                }
                if unfinished == 0 {
                    return;
                }
            } else if bits <= 8 {
                let mut pal_mask = [0u8; 256];
                let mut any_non_zero = false;
                for (idx, &state) in sec.blocks.palette().iter().enumerate() {
                    if state != 0 {
                        let m = flags_heightmap_mask(info.flags(state));
                        pal_mask[idx] = m;
                        any_non_zero |= m != 0;
                    }
                }
                if !any_non_zero {
                    continue;
                }
                let packed = sec.blocks.packed_data();
                if bits == 4 {
                    let zero_mask = pal_mask[0];
                    for ly in (0..16usize).rev() {
                        let h = (s * 16 + ly + 1) as u16;
                        let row_words = &packed[ly * 16..(ly + 1) * 16];
                        for (wi, &word) in row_words.iter().enumerate() {
                            if word == 0 && zero_mask == 0 {
                                continue;
                            }
                            let base_col = wi * 16;
                            let mut w = word;
                            for lane in 0..16 {
                                let m = pal_mask[(w & 0xF) as usize];
                                w >>= 4;
                                if m == 0 {
                                    continue;
                                }
                                let col = base_col + lane;
                                let hit = remaining[col] & m;
                                if hit != 0 {
                                    remaining[col] &= !hit;
                                    if remaining[col] == 0 {
                                        unfinished -= 1;
                                    }
                                    let mut b = hit;
                                    while b != 0 {
                                        let k = b.trailing_zeros() as usize;
                                        self.heights[k][col] = h;
                                        b &= b - 1;
                                    }
                                }
                            }
                        }
                        if unfinished == 0 {
                            return;
                        }
                    }
                } else {
                    let bpe = usize::from(bits);
                    let per = 64 / bpe;
                    let entry_mask = (1u64 << bpe) - 1;
                    for ly in (0..16usize).rev() {
                        let h = (s * 16 + ly + 1) as u16;
                        let start_idx = ly * 256;
                        let mut wi = start_idx / per;
                        let mut lane = start_idx % per;
                        let mut w = packed[wi] >> (lane * bpe);
                        for col in 0..256 {
                            let m = pal_mask[(w & entry_mask) as usize];
                            lane += 1;
                            if lane == per {
                                lane = 0;
                                wi += 1;
                                if col < 255 {
                                    w = packed[wi];
                                }
                            } else {
                                w >>= bpe;
                            }
                            if m == 0 {
                                continue;
                            }
                            let hit = remaining[col] & m;
                            if hit != 0 {
                                remaining[col] &= !hit;
                                if remaining[col] == 0 {
                                    unfinished -= 1;
                                }
                                let mut b = hit;
                                while b != 0 {
                                    let k = b.trailing_zeros() as usize;
                                    self.heights[k][col] = h;
                                    b &= b - 1;
                                }
                            }
                        }
                        if unfinished == 0 {
                            return;
                        }
                    }
                }
            } else {
                for ly in (0..16usize).rev() {
                    let h = (s * 16 + ly + 1) as u16;
                    let base = ly * 256;
                    for (col, rem) in remaining.iter_mut().enumerate() {
                        if *rem == 0 {
                            continue;
                        }
                        let state = sec.blocks.get(base + col).unwrap();
                        if state == 0 {
                            continue;
                        }
                        let m = flags_heightmap_mask(info.flags(state));
                        let hit = *rem & m;
                        if hit != 0 {
                            *rem &= !hit;
                            if *rem == 0 {
                                unfinished -= 1;
                            }
                            let mut b = hit;
                            while b != 0 {
                                let k = b.trailing_zeros() as usize;
                                self.heights[k][col] = h;
                                b &= b - 1;
                            }
                        }
                    }
                    if unfinished == 0 {
                        return;
                    }
                }
            }
        }
    }
    /// Computes all six vanilla heightmaps directly from a flat `blocks` buffer laid out as
    /// `blocks[y_idx * 256 + z * 16 + x]`, avoiding 4,096 paletted reads per section.
    pub fn compute_heightmaps_from_u16(&mut self, blocks: &[u16], info: &impl BlockInfo) {
        for map in &mut *self.heights {
            map.fill(0);
        }
        let Some(top_section) = self
            .sections
            .iter()
            .rposition(|s| s.block_count != 0 || s.fluid_count != 0)
        else {
            return;
        };
        let total_h = (blocks.len() / 256).min((top_section + 1) * 16);
        if total_h == 0 {
            return;
        }
        let mut remaining = [0b111111u8; 256];
        let mut unfinished = 256usize;
        let mut last_state = 0u16;
        let mut last_mask = 0u8;
        for y_idx in (0..total_h).rev() {
            let row = &blocks[y_idx * 256..(y_idx + 1) * 256];
            let h = (y_idx + 1) as u16;
            for col in 0..256 {
                let rem = remaining[col];
                if rem == 0 {
                    continue;
                }
                let state = row[col];
                if state == 0 {
                    continue;
                }
                let m = if state == last_state {
                    last_mask
                } else {
                    let computed = flags_heightmap_mask(info.flags(u32::from(state)));
                    last_state = state;
                    last_mask = computed;
                    computed
                };
                let hit = rem & m;
                if hit != 0 {
                    remaining[col] = rem & !hit;
                    if remaining[col] == 0 {
                        unfinished -= 1;
                    }
                    let mut b = hit;
                    while b != 0 {
                        let k = b.trailing_zeros() as usize;
                        self.heights[k][col] = h;
                        b &= b - 1;
                    }
                }
            }
            if unfinished == 0 {
                break;
            }
        }
    }
    pub fn heights(&self, kind: HeightmapKind) -> &[u16] {
        &self.heights[kind as usize]
    }
}

#[inline]
fn flags_heightmap_mask(flags: BlockFlags) -> u8 {
    if flags.air && !flags.fluid && !flags.motion_blocking {
        return 0;
    }
    let mut mask = 0u8;
    for kind in HeightmapKind::ALL {
        if kind.includes(flags) {
            mask |= 1u8 << (kind as u8);
        }
    }
    mask
}
