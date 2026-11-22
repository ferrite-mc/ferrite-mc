//! Server-side bridge to [`ferrite_light`] for single-chunk initial light and
//! parallel [`LightPool`] batch relighting with batch-wide atomic publication.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ferrite_light::{
    LightBlockView, LightEngine, LightJob, LightKind, LightPool, LightPoolConfig, LightProperties,
    LightSnapshot, NibbleArray, chunk_neighbourhood, vanilla_properties,
};
use ferrite_world::{Chunk, ChunkSection, LightData, LightSection};

use super::Dimension;

#[inline]
fn section_empty(section: &ChunkSection) -> bool {
    section.block_count() == 0 && section.fluid_count() == 0
}

fn fill_from_chunk_section(chunk: &Chunk, section_index: usize, out: &mut [LightProperties; 4096]) {
    let Some(section) = chunk.sections().get(section_index) else {
        out.fill(LightProperties::AIR);
        return;
    };
    if section_empty(section) {
        out.fill(LightProperties::AIR);
        return;
    }
    let blocks = section.blocks();
    let bits = blocks.bits_per_entry();
    let pal = blocks.palette();
    if bits == 0 {
        out.fill(vanilla_properties(pal.first().copied().unwrap_or(0) as u16));
        return;
    }
    if bits <= 8 && pal.len() <= 256 {
        let mut table = [LightProperties::AIR; 256];
        for (i, &state) in pal.iter().enumerate() {
            table[i] = vanilla_properties(state as u16);
        }
        let per = 64 / usize::from(bits);
        let mask = (1u64 << bits) - 1;
        let data = blocks.packed_data();
        let mut idx = 0usize;
        for &word in data {
            let mut w = word;
            let count = per.min(4096 - idx);
            for _ in 0..count {
                let raw = (w & mask) as usize;
                out[idx] = table[raw];
                w >>= bits;
                idx += 1;
            }
        }
        return;
    }
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = vanilla_properties(blocks.get(i).unwrap_or(0) as u16);
    }
}

/// Immutable block snapshot across a batch's 3x3 chunk neighbourhoods.
pub struct SnapshotBlockView {
    chunks: HashMap<(i32, i32), Arc<Chunk>>,
}

impl SnapshotBlockView {
    pub fn from_chunks(chunks: HashMap<(i32, i32), Arc<Chunk>>) -> Self {
        Self { chunks }
    }
}

impl LightBlockView for SnapshotBlockView {
    fn state(&self, x: i32, y: i32, z: i32) -> u16 {
        let cx = x.div_euclid(16);
        let cz = z.div_euclid(16);
        let Some(chunk) = self.chunks.get(&(cx, cz)) else {
            return 0;
        };
        chunk
            .block(x.rem_euclid(16) as u8, y, z.rem_euclid(16) as u8)
            .unwrap_or(0) as u16
    }

    fn is_chunk_loaded(&self, chunk_x: i32, chunk_z: i32) -> bool {
        self.chunks.contains_key(&(chunk_x, chunk_z))
    }

    fn section_is_empty(&self, chunk_x: i32, chunk_z: i32, section_index: usize) -> bool {
        self.chunks
            .get(&(chunk_x, chunk_z))
            .and_then(|c| c.sections().get(section_index))
            .is_none_or(section_empty)
    }

    fn fill_section_properties(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        section_index: usize,
        _base_y: i32,
        out: &mut [LightProperties; 4096],
    ) {
        let Some(chunk) = self.chunks.get(&(chunk_x, chunk_z)) else {
            out.fill(LightProperties::OPAQUE);
            return;
        };
        fill_from_chunk_section(chunk, section_index, out);
    }
}

struct SingleChunkView<'a> {
    cx: i32,
    cz: i32,
    chunk: &'a Chunk,
}

impl LightBlockView for SingleChunkView<'_> {
    fn state(&self, x: i32, y: i32, z: i32) -> u16 {
        if x.div_euclid(16) != self.cx || z.div_euclid(16) != self.cz {
            return 0;
        }
        self.chunk
            .block(x.rem_euclid(16) as u8, y, z.rem_euclid(16) as u8)
            .unwrap_or(0) as u16
    }

    fn is_chunk_loaded(&self, chunk_x: i32, chunk_z: i32) -> bool {
        chunk_x == self.cx && chunk_z == self.cz
    }

    fn section_is_empty(&self, chunk_x: i32, chunk_z: i32, section_index: usize) -> bool {
        chunk_x == self.cx
            && chunk_z == self.cz
            && self
                .chunk
                .sections()
                .get(section_index)
                .is_none_or(section_empty)
    }

    fn fill_section_properties(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        section_index: usize,
        _base_y: i32,
        out: &mut [LightProperties; 4096],
    ) {
        if chunk_x != self.cx || chunk_z != self.cz {
            out.fill(LightProperties::OPAQUE);
            return;
        }
        fill_from_chunk_section(self.chunk, section_index, out);
    }
}

fn nibble_to_light_section(arr: &NibbleArray) -> LightSection {
    match arr {
        NibbleArray::Null | NibbleArray::Zero => LightSection::Empty,
        NibbleArray::Full => LightSection::Data(Box::new([0xff; 2048])),
        NibbleArray::Data(bytes) => LightSection::Data(bytes.clone()),
    }
}

/// Converts a [`LightSnapshot`] into protocol/Anvil [`LightData`].
pub fn snapshot_to_light_data(snapshot: &LightSnapshot) -> LightData {
    let count = snapshot.section_count();
    let mut sky = Vec::with_capacity(count);
    let mut block = Vec::with_capacity(count);
    for sy in snapshot.min_section() - 1..=snapshot.max_section() + 1 {
        let s_arr = snapshot
            .section(LightKind::Sky, sy)
            .unwrap_or(&NibbleArray::Zero);
        let b_arr = snapshot
            .section(LightKind::Block, sy)
            .unwrap_or(&NibbleArray::Zero);
        sky.push(nibble_to_light_section(s_arr));
        block.push(nibble_to_light_section(b_arr));
    }
    LightData { sky, block }
}

/// Computes initial sky and block light for a freshly generated standalone chunk using a
/// thread-local reusable [`LightEngine`].
pub fn compute_initial_light(
    dimension: Dimension,
    chunk_x: i32,
    chunk_z: i32,
    chunk: &Chunk,
) -> LightData {
    let min_section = chunk.min_y().div_euclid(16);
    let max_section = min_section + chunk.sections().len() as i32 - 1;
    let sky_enabled = dimension.has_skylight();

    thread_local! {
        static ENGINE: RefCell<Option<(i32, i32, bool, LightEngine)>> = const { RefCell::new(None) };
    }

    ENGINE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let engine = match slot.as_mut() {
            Some((min_s, max_s, sky, eng))
                if *min_s == min_section && *max_s == max_section && *sky == sky_enabled =>
            {
                eng.recenter(chunk_x, chunk_z);
                eng
            }
            _ => {
                *slot = Some((
                    min_section,
                    max_section,
                    sky_enabled,
                    LightEngine::new(chunk_x, chunk_z, min_section, max_section, sky_enabled),
                ));
                &mut slot.as_mut().expect("just inserted").3
            }
        };

        let view = SingleChunkView {
            cx: chunk_x,
            cz: chunk_z,
            chunk,
        };
        engine.relight(&view);

        let count = chunk.sections().len() + 2;
        let mut sky = Vec::with_capacity(count);
        let mut block = Vec::with_capacity(count);
        for sy in min_section - 1..=max_section + 1 {
            let s_arr = if sky_enabled {
                engine
                    .section(LightKind::Sky, chunk_x, sy, chunk_z)
                    .unwrap_or(&NibbleArray::Zero)
            } else {
                &NibbleArray::Zero
            };
            let b_arr = engine
                .section(LightKind::Block, chunk_x, sy, chunk_z)
                .unwrap_or(&NibbleArray::Zero);
            sky.push(nibble_to_light_section(s_arr));
            block.push(nibble_to_light_section(b_arr));
        }
        LightData { sky, block }
    })
}

/// Builds a [`LightPool`] configured for `dimension` and `workers` threads.
pub fn pool_for_dimension(dimension: Dimension, workers: usize) -> LightPool {
    let min_section = dimension.min_y().div_euclid(16);
    let max_section = min_section + (dimension.height() / 16) - 1;
    LightPool::new(LightPoolConfig::new(
        workers.max(1),
        min_section,
        max_section,
        dimension.has_skylight(),
    ))
}

/// Expands target chunk coordinates to include every chunk and its 3x3 neighbours.
pub fn expand_neighbourhoods(coords: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let mut set = HashSet::with_capacity(coords.len() * 9);
    for &(cx, cz) in coords {
        for pos in chunk_neighbourhood(cx, cz) {
            set.insert(pos);
        }
    }
    set.into_iter().collect()
}

/// Runs [`LightPool::relight`] over `chunks` for `coords`, returning `(coord, LightData)` pairs.
pub fn relight_batch_from_snapshot(
    pool: &LightPool,
    coords: &[(i32, i32)],
    chunks: HashMap<(i32, i32), Arc<Chunk>>,
) -> anyhow::Result<Vec<((i32, i32), LightData)>> {
    if coords.is_empty() {
        return Ok(Vec::new());
    }
    let jobs: Vec<LightJob> = coords.iter().map(|&(x, z)| LightJob::new(x, z)).collect();
    let view = SnapshotBlockView::from_chunks(chunks);
    let snapshots = pool
        .relight(&view, &jobs)
        .map_err(|e| anyhow::anyhow!("lighting pool failed: {e}"))?;
    Ok(snapshots
        .into_iter()
        .map(|snap| (snap.center(), snapshot_to_light_data(&snap)))
        .collect())
}
