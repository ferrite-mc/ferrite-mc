//! Bounded, world-independent block and sky light propagation.
//!
//! One engine holds a 3x3 chunk neighborhood. The caller supplies blocks and
//! loaded-chunk status and must notify it after changes. Light storage includes
//! one section above and below the world. Pending edge chunks identify missing
//! neighbors which must be rechecked when loaded. Queue entries encode a local
//! position and permitted directions in one `u64`; no per-step allocation occurs.
//! Vanilla properties include the exact face occlusion shapes.
mod pool;
mod properties;
pub use pool::{
    LightJob, LightPool, LightPoolConfig, LightPoolError, LightSnapshot, chunk_neighbourhood,
};
pub use properties::{LightProperties, vanilla_properties};
use std::collections::{HashSet, VecDeque};

const WIDTH: usize = 48;
const PLANE: usize = WIDTH * WIDTH;
const DIRECTIONS: [(i32, i32, i32); 6] = [
    (-1, 0, 0),
    (1, 0, 0),
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
];
const OPPOSITE: [usize; 6] = [1, 0, 3, 2, 5, 4];

pub trait LightBlockView {
    fn state(&self, x: i32, y: i32, z: i32) -> u16;
    fn is_chunk_loaded(&self, _chunk_x: i32, _chunk_z: i32) -> bool {
        true
    }
    fn properties(&self, state: u16) -> LightProperties {
        vanilla_properties(state)
    }
    fn section_is_empty(&self, _chunk_x: i32, _chunk_z: i32, _section_index: usize) -> bool {
        false
    }
    fn fill_section_properties(
        &self,
        chunk_x: i32,
        chunk_z: i32,
        _section_index: usize,
        base_y: i32,
        out: &mut [LightProperties; 4096],
    ) {
        let bx = chunk_x * 16;
        let bz = chunk_z * 16;
        for ly in 0..16usize {
            let wy = base_y + ly as i32;
            let row = ly * 256;
            for lz in 0..16usize {
                let wz = bz + lz as i32;
                let off = row + lz * 16;
                for lx in 0..16usize {
                    out[off + lx] = self.properties(self.state(bx + lx as i32, wy, wz));
                }
            }
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightKind {
    Sky,
    Block,
}

/// A zero or full section stores no payload. Null marks an unloaded chunk.
#[derive(Clone, PartialEq, Eq)]
pub enum NibbleArray {
    Null,
    Zero,
    Full,
    Data(Box<[u8; 2048]>),
}
impl std::fmt::Debug for NibbleArray {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => f.write_str("Null"),
            Self::Zero => f.write_str("Zero"),
            Self::Full => f.write_str("Full"),
            Self::Data(_) => f.write_str("Data(..)"),
        }
    }
}
impl NibbleArray {
    pub fn get(&self, index: usize) -> Option<u8> {
        assert!(index < 4096);
        match self {
            Self::Null => None,
            Self::Zero => Some(0),
            Self::Full => Some(15),
            Self::Data(data) => Some((data[index / 2] >> ((index & 1) * 4)) & 15),
        }
    }
    pub fn set(&mut self, index: usize, value: u8) {
        assert!(index < 4096 && value < 16);
        let old = self.get(index).expect("cannot write unloaded section");
        if old == value {
            return;
        }
        if !matches!(self, Self::Data(_)) {
            *self = Self::Data(Box::new([if old == 15 { 255 } else { 0 }; 2048]));
        }
        if let Self::Data(data) = self {
            let shift = (index & 1) * 4;
            data[index / 2] = (data[index / 2] & !(15 << shift)) | (value << shift);
        }
    }
    /// Compact uniform arrays after a lighting pass.
    pub fn compact(&mut self) {
        if let Self::Data(data) = self {
            if data.iter().all(|b| *b == 0) {
                *self = Self::Zero;
            } else if data.iter().all(|b| *b == 255) {
                *self = Self::Full;
            }
        }
    }
}
struct Section {
    sky: NibbleArray,
    block: NibbleArray,
}

pub struct LightEngine {
    origin_x: i32,
    origin_z: i32,
    min_y: i32,
    height: usize,
    sky_enabled: bool,
    sections: Vec<Section>,
    blocks: Vec<LightProperties>,
    sky_sources: Vec<u8>,
    section_has_emission: Vec<bool>,
    highest_non_air_sy: Option<usize>,
    loaded: [bool; 9],
    pending: HashSet<(i32, i32)>,
    queue: VecDeque<u64>,
    removal: VecDeque<(u64, u8)>,
    section_buf: Vec<LightProperties>,
}
impl LightEngine {
    /// `min_section..=max_section` is the buildable range; the engine also
    /// stores an extra light section above and below it.
    pub fn new(
        center_x: i32,
        center_z: i32,
        min_section: i32,
        max_section: i32,
        sky_enabled: bool,
    ) -> Self {
        assert!(min_section <= max_section);
        let height = usize::try_from(i64::from(max_section) - i64::from(min_section) + 1)
            .unwrap()
            .checked_mul(16)
            .unwrap();
        assert!(height <= 65536);
        let origin_x = center_x
            .checked_sub(1)
            .and_then(|x| x.checked_mul(16))
            .expect("chunk x out of range");
        let origin_z = center_z
            .checked_sub(1)
            .and_then(|z| z.checked_mul(16))
            .expect("chunk z out of range");
        let min_y = min_section.checked_mul(16).expect("section y out of range");
        let sections = (0..(height / 16 + 2) * 9)
            .map(|_| Section {
                sky: NibbleArray::Null,
                block: NibbleArray::Null,
            })
            .collect();
        Self {
            origin_x,
            origin_z,
            min_y,
            height,
            sky_enabled,
            sections,
            blocks: vec![LightProperties::OPAQUE; height * PLANE],
            sky_sources: vec![0; height * PLANE],
            section_has_emission: vec![false; (height / 16) * 9],
            highest_non_air_sy: None,
            loaded: [false; 9],
            pending: HashSet::new(),
            queue: VecDeque::new(),
            removal: VecDeque::new(),
            section_buf: vec![LightProperties::AIR; 4096],
        }
    }
    /// Moves the engine to another centre chunk, discarding all block, light,
    /// queue and pending-edge state. The engine must be relit (or given a
    /// `section_changed`/`check_edges` notification) before its light is read.
    /// The section range and sky setting are unchanged.
    pub fn recenter(&mut self, center_x: i32, center_z: i32) {
        self.origin_x = center_x
            .checked_sub(1)
            .and_then(|x| x.checked_mul(16))
            .expect("chunk x out of range");
        self.origin_z = center_z
            .checked_sub(1)
            .and_then(|z| z.checked_mul(16))
            .expect("chunk z out of range");
        for section in &mut self.sections {
            section.sky = NibbleArray::Null;
            section.block = NibbleArray::Null;
        }
        self.loaded = [false; 9];
        self.pending.clear();
        self.queue.clear();
        self.removal.clear();
    }
    fn section_index(&self, x: usize, y: i32, z: usize) -> Option<(usize, usize)> {
        let sy = i64::from(y) - i64::from(self.min_y) + 16;
        if x >= WIDTH || z >= WIDTH || sy < 0 || sy >= self.height as i64 + 32 {
            return None;
        }
        let sy = sy as usize;
        let section = (sy / 16) * 9 + (z / 16) * 3 + x / 16;
        Some((section, ((sy % 16) * 256) + ((z % 16) * 16) + (x % 16)))
    }
    fn local(&self, x: i32, y: i32, z: i32) -> Option<(usize, usize, usize)> {
        let x = i64::from(x) - i64::from(self.origin_x);
        let y = i64::from(y) - i64::from(self.min_y);
        let z = i64::from(z) - i64::from(self.origin_z);
        if !(0..48).contains(&x) || !(0..48).contains(&z) || y < 0 || y >= self.height as i64 {
            None
        } else {
            Some((x as usize, y as usize, z as usize))
        }
    }
    fn index(x: usize, y: usize, z: usize) -> usize {
        y * PLANE + z * WIDTH + x
    }
    fn unpack(index: usize) -> (usize, usize, usize) {
        (index % WIDTH, index / PLANE, (index % PLANE) / WIDTH)
    }
    fn level(&self, kind: LightKind, index: usize) -> u8 {
        let (x, y, z) = Self::unpack(index);
        let section = (y / 16 + 1) * 9 + (z / 16) * 3 + x / 16;
        let offset = (y % 16) * 256 + (z % 16) * 16 + x % 16;
        match kind {
            LightKind::Sky => &self.sections[section].sky,
            LightKind::Block => &self.sections[section].block,
        }
        .get(offset)
        .unwrap_or(0)
    }
    fn set(&mut self, kind: LightKind, index: usize, value: u8) {
        let (x, y, z) = Self::unpack(index);
        let section = (y / 16 + 1) * 9 + (z / 16) * 3 + x / 16;
        let offset = (y % 16) * 256 + (z % 16) * 16 + x % 16;
        match kind {
            LightKind::Sky => &mut self.sections[section].sky,
            LightKind::Block => &mut self.sections[section].block,
        }
        .set(offset, value);
    }
    pub fn get(&self, kind: LightKind, x: i32, y: i32, z: i32) -> Option<u8> {
        let x = i64::from(x) - i64::from(self.origin_x);
        let z = i64::from(z) - i64::from(self.origin_z);
        if !(0..48).contains(&x) || !(0..48).contains(&z) {
            return None;
        }
        let (section, offset) = self.section_index(x as usize, y, z as usize)?;
        match kind {
            LightKind::Sky => &self.sections[section].sky,
            LightKind::Block => &self.sections[section].block,
        }
        .get(offset)
    }
    pub fn section(
        &self,
        kind: LightKind,
        chunk_x: i32,
        section_y: i32,
        chunk_z: i32,
    ) -> Option<&NibbleArray> {
        let x = i64::from(chunk_x) * 16 - i64::from(self.origin_x);
        let z = i64::from(chunk_z) * 16 - i64::from(self.origin_z);
        if !(0..48).contains(&x) || !(0..48).contains(&z) {
            return None;
        }
        let y = section_y.checked_mul(16)?;
        let (section, _) = self.section_index(x as usize, y, z as usize)?;
        Some(match kind {
            LightKind::Sky => &self.sections[section].sky,
            LightKind::Block => &self.sections[section].block,
        })
    }
    pub fn pending_edges(&self) -> &HashSet<(i32, i32)> {
        &self.pending
    }
    pub fn take_pending_edges(&mut self) -> HashSet<(i32, i32)> {
        std::mem::take(&mut self.pending)
    }
    fn reset(&mut self, world: &impl LightBlockView) {
        self.pending.clear();
        self.queue.clear();
        self.removal.clear();
        self.section_has_emission.fill(false);
        self.highest_non_air_sy = None;
        let num_sections = self.height / 16;
        let base_cx = self.origin_x / 16;
        let base_cz = self.origin_z / 16;
        let mut buf: [LightProperties; 4096] = self
            .section_buf
            .as_slice()
            .try_into()
            .expect("section_buf has length 4096");
        for cz_i in 0..3 {
            let cz = base_cz + cz_i as i32;
            let z_off = cz_i * 16;
            for cx_i in 0..3 {
                let cx = base_cx + cx_i as i32;
                let x_off = cx_i * 16;
                let chunk_slot = cz_i * 3 + cx_i;
                let loaded = world.is_chunk_loaded(cx, cz);
                self.loaded[chunk_slot] = loaded;
                for sy in 0..num_sections + 2 {
                    let s = &mut self.sections[sy * 9 + chunk_slot];
                    s.block = if loaded {
                        NibbleArray::Zero
                    } else {
                        NibbleArray::Null
                    };
                    s.sky = if !loaded {
                        NibbleArray::Null
                    } else if self.sky_enabled && sy == num_sections + 1 {
                        NibbleArray::Full
                    } else {
                        NibbleArray::Zero
                    };
                }
                for sy in 0..num_sections {
                    let sec_base_y = sy * 16;
                    if !loaded {
                        for ly in 0..16 {
                            let y_off = (sec_base_y + ly) * PLANE + z_off * WIDTH + x_off;
                            for lz in 0..16 {
                                self.blocks[y_off + lz * WIDTH..y_off + lz * WIDTH + 16]
                                    .fill(LightProperties::OPAQUE);
                            }
                        }
                        continue;
                    }
                    if world.section_is_empty(cx, cz, sy) {
                        for ly in 0..16 {
                            let y_off = (sec_base_y + ly) * PLANE + z_off * WIDTH + x_off;
                            for lz in 0..16 {
                                self.blocks[y_off + lz * WIDTH..y_off + lz * WIDTH + 16]
                                    .fill(LightProperties::AIR);
                            }
                        }
                        continue;
                    }
                    world.fill_section_properties(
                        cx,
                        cz,
                        sy,
                        self.min_y + sec_base_y as i32,
                        &mut buf,
                    );
                    let mut has_emission = false;
                    let mut has_non_air = false;
                    for ly in 0..16 {
                        let y_off = (sec_base_y + ly) * PLANE + z_off * WIDTH + x_off;
                        for lz in 0..16 {
                            let src = &buf[ly * 256 + lz * 16..ly * 256 + lz * 16 + 16];
                            let dst = &mut self.blocks[y_off + lz * WIDTH..y_off + lz * WIDTH + 16];
                            dst.copy_from_slice(src);
                            for p in src {
                                if p.emission > 0 {
                                    has_emission = true;
                                }
                                if *p != LightProperties::AIR {
                                    has_non_air = true;
                                }
                            }
                        }
                    }
                    self.section_has_emission[sy * 9 + chunk_slot] = has_emission;
                    if has_non_air {
                        self.highest_non_air_sy =
                            Some(self.highest_non_air_sy.map_or(sy, |prev| prev.max(sy)));
                    }
                }
            }
        }
        self.section_buf.copy_from_slice(&buf);
    }
    /// Recompute all loaded sections in this 3x3 neighborhood.
    pub fn relight(&mut self, world: &impl LightBlockView) {
        self.reset(world);
        let num_sections = self.height / 16;
        for sy in 0..num_sections {
            for cz_i in 0..3 {
                let z_off = cz_i * 16;
                for cx_i in 0..3 {
                    let chunk_slot = cz_i * 3 + cx_i;
                    if !self.section_has_emission[sy * 9 + chunk_slot] {
                        continue;
                    }
                    let x_off = cx_i * 16;
                    let sec_base_y = sy * 16;
                    for ly in 0..16 {
                        for lz in 0..16 {
                            let row = (sec_base_y + ly) * PLANE + (z_off + lz) * WIDTH + x_off;
                            for lx in 0..16 {
                                let i = row + lx;
                                let emission = self.blocks[i].emission.min(15);
                                if emission > 0 {
                                    self.set(LightKind::Block, i, emission);
                                    self.queue.push_back(((i as u64) << 6) | 63);
                                }
                            }
                        }
                    }
                }
            }
        }
        self.propagate(LightKind::Block);
        if self.sky_enabled {
            self.relight_sky();
        }
        self.compact();
    }
    fn relight_sky(&mut self) {
        self.queue.clear();
        self.sky_sources.fill(0);
        for s in &mut self.sections {
            if !matches!(s.sky, NibbleArray::Null) {
                s.sky = NibbleArray::Zero;
            }
        }
        let num_sections = self.height / 16;
        for chunk in 0..9 {
            if self.loaded[chunk] {
                self.sections[(num_sections + 1) * 9 + chunk].sky = NibbleArray::Full;
            }
        }
        let scan_top_sy = self
            .highest_non_air_sy
            .map_or(0, |sy| (sy + 1).min(num_sections));
        let all_loaded = self.loaded.iter().all(|&b| b);
        if all_loaded && scan_top_sy < num_sections {
            // Every chunk in the 3x3 is loaded and all sections >= `scan_top_sy` are uniform AIR:
            // fill `Full` (15) directly without 4096*N queue operations.
            for sy in scan_top_sy..num_sections {
                for chunk in 0..9 {
                    self.sections[(sy + 1) * 9 + chunk].sky = NibbleArray::Full;
                }
            }
            let start_y = scan_top_sy * 16;
            self.sky_sources[start_y * PLANE..self.height * PLANE].fill(15);
            // If `scan_top_sy == 0`, the world has no non-air blocks at all and `sections` are all Full;
            // still keep pending edges up-to-date on outer borders.
            let border_cx_low = (self.origin_x - 1).div_euclid(16);
            let border_cx_high = (self.origin_x + WIDTH as i32).div_euclid(16);
            let border_cz_low = (self.origin_z - 1).div_euclid(16);
            let border_cz_high = (self.origin_z + WIDTH as i32).div_euclid(16);
            let base_cx = self.origin_x / 16;
            let base_cz = self.origin_z / 16;
            if scan_top_sy < num_sections {
                for i in 0..3 {
                    self.pending.insert((border_cx_low, base_cz + i));
                    self.pending.insert((border_cx_high, base_cz + i));
                    self.pending.insert((base_cx + i, border_cz_low));
                    self.pending.insert((base_cx + i, border_cz_high));
                }
            }
        }
        let scan_height = if all_loaded {
            scan_top_sy * 16
        } else {
            self.height
        };
        for z in 0..WIDTH {
            for x in 0..WIDTH {
                if !self.loaded[(z / 16) * 3 + x / 16] {
                    continue;
                }
                let mut value = 15u8;
                let mut above = LightProperties::AIR;
                for y in (0..scan_height).rev() {
                    let i = Self::index(x, y, z);
                    let p = self.blocks[i];
                    if properties::occludes(above.faces[2], p.faces[3], 2) {
                        value = 0;
                    } else {
                        value = value.saturating_sub(if value == 15 {
                            p.opacity.min(15)
                        } else {
                            p.opacity.clamp(1, 15)
                        });
                    }
                    self.sky_sources[i] = value;
                    if value > 0 {
                        self.set(LightKind::Sky, i, value);
                        self.queue.push_back(((i as u64) << 6) | 63);
                    }
                    above = p;
                }
            }
        }
        self.propagate(LightKind::Sky);
    }
    fn propagate(&mut self, kind: LightKind) {
        while let Some(item) = self.queue.pop_front() {
            let i = (item >> 6) as usize;
            let mask = item as u8 & 63;
            let level = self.level(kind, i);
            if level <= 1 {
                continue;
            }
            let (x, y, z) = Self::unpack(i);
            let from = self.blocks[i];
            for (direction, &(dx, dy, dz)) in DIRECTIONS.iter().enumerate() {
                if mask & (1 << direction) == 0 {
                    continue;
                }
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                let nz = z as i32 + dz;
                if !(0..48).contains(&nx) || !(0..48).contains(&nz) {
                    let wx = self.origin_x + x as i32 + dx;
                    let wz = self.origin_z + z as i32 + dz;
                    self.pending.insert((wx.div_euclid(16), wz.div_euclid(16)));
                    continue;
                }
                if ny < 0 || ny >= self.height as i32 {
                    continue;
                }
                let cx = nx as usize / 16;
                let cz = nz as usize / 16;
                if !self.loaded[cz * 3 + cx] {
                    self.pending.insert((
                        (self.origin_x + nx).div_euclid(16),
                        (self.origin_z + nz).div_euclid(16),
                    ));
                    continue;
                }
                let n = Self::index(nx as usize, ny as usize, nz as usize);
                let target = self.blocks[n];
                let value = level.saturating_sub(target.opacity.max(1));
                if value <= self.level(kind, n) {
                    continue;
                }
                if properties::occludes(
                    if from.emission > 0 {
                        0
                    } else {
                        from.faces[direction]
                    },
                    target.faces[OPPOSITE[direction]],
                    direction,
                ) {
                    continue;
                }
                self.set(kind, n, value);
                self.queue
                    .push_back(((n as u64) << 6) | u64::from(63u8 & !(1 << OPPOSITE[direction])));
            }
        }
    }
    fn remove_and_relight(&mut self, kind: LightKind) {
        while let Some((item, old)) = self.removal.pop_front() {
            let i = (item >> 6) as usize;
            let (x, y, z) = Self::unpack(i);
            let source = match kind {
                LightKind::Sky => self.sky_sources[i],
                LightKind::Block => self.blocks[i].emission.min(15),
            };
            if source > self.level(kind, i) {
                self.set(kind, i, source);
                self.queue.push_back((i as u64) << 6 | 63);
            }
            for (dir, &(dx, dy, dz)) in DIRECTIONS.iter().enumerate() {
                if item as u8 & (1 << dir) == 0 {
                    continue;
                }
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                let nz = z as i32 + dz;
                if !(0..48).contains(&nx)
                    || !(0..48).contains(&nz)
                    || ny < 0
                    || ny >= self.height as i32
                {
                    continue;
                }
                if !self.loaded[(nz as usize / 16) * 3 + nx as usize / 16] {
                    continue;
                }
                let n = Self::index(nx as usize, ny as usize, nz as usize);
                let value = self.level(kind, n);
                if value == 0 {
                    continue;
                }
                if value < old {
                    self.set(kind, n, 0);
                    self.removal.push_back((
                        (n as u64) << 6 | u64::from(63u8 & !(1 << OPPOSITE[dir])),
                        value,
                    ));
                } else {
                    self.queue.push_back((n as u64) << 6 | 63);
                }
            }
        }
        self.propagate(kind);
    }
    /// Notify the engine after a block changes; decrease and increase queues
    /// repair both light layers. Only the affected direct-sky column is rescanned.
    pub fn block_changed(&mut self, world: &impl LightBlockView, x: i32, y: i32, z: i32) {
        let Some((lx, ly, lz)) = self.local(x, y, z) else {
            return;
        };
        if !self.loaded[(lz / 16) * 3 + lx / 16] {
            return;
        }
        let index = Self::index(lx, ly, lz);
        self.blocks[index] = world.properties(world.state(x, y, z));
        let old = self.level(LightKind::Block, index);
        self.set(LightKind::Block, index, 0);
        self.removal.push_back(((index as u64) << 6 | 63, old));
        self.remove_and_relight(LightKind::Block);
        if self.sky_enabled {
            let mut value = 15u8;
            let mut above = LightProperties::AIR;
            for py in (0..self.height).rev() {
                let i = Self::index(lx, py, lz);
                let p = self.blocks[i];
                if properties::occludes(above.faces[2], p.faces[3], 2) {
                    value = 0;
                } else {
                    value = value.saturating_sub(if value == 15 {
                        p.opacity.min(15)
                    } else {
                        p.opacity.clamp(1, 15)
                    });
                }
                let old_source = std::mem::replace(&mut self.sky_sources[i], value);
                let old_level = self.level(LightKind::Sky, i);
                if (old_source > value && old_level <= old_source) || i == index {
                    self.set(LightKind::Sky, i, 0);
                    self.removal.push_back(((i as u64) << 6 | 63, old_level));
                } else if value > old_level {
                    self.set(LightKind::Sky, i, value);
                    self.queue.push_back((i as u64) << 6 | 63);
                }
                above = p;
            }
            self.remove_and_relight(LightKind::Sky);
        }
        self.compact();
    }
    /// Recheck after section occupancy or neighbor availability changes.
    pub fn section_changed(&mut self, world: &impl LightBlockView) {
        self.relight(world);
    }
    pub fn check_edges(&mut self, world: &impl LightBlockView) {
        self.relight(world);
    }
    fn compact(&mut self) {
        for section in &mut self.sections {
            section.sky.compact();
            section.block.compact();
        }
    }
}
