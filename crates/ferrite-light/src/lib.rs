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
use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
};

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
    fn section_is_opaque(&self, _chunk_x: i32, _chunk_z: i32, _section_index: usize) -> bool {
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
    Data(Arc<[u8; 2048]>),
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
            *self = Self::Data(Arc::new([if old == 15 { 255 } else { 0 }; 2048]));
        }
        if let Self::Data(data) = self {
            let shift = (index & 1) * 4;
            let bytes = Arc::make_mut(data);
            bytes[index / 2] = (bytes[index / 2] & !(15 << shift)) | (value << shift);
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

const SECTION_MIXED: u8 = 0;
const SECTION_OPAQUE: u8 = 1;
const SECTION_AIR: u8 = 2;

pub struct LightEngine {
    origin_x: i32,
    origin_z: i32,
    min_y: i32,
    height: usize,
    sky_enabled: bool,
    sections: Vec<Section>,
    blocks: Vec<LightProperties>,
    sky_sources: Vec<u8>,
    sky_sources_dirty: bool,
    section_has_emission: Vec<bool>,
    section_state: Vec<u8>,
    highest_non_air_sy: Option<usize>,
    loaded: [bool; 9],
    pending: HashSet<(i32, i32)>,
    queue: VecDeque<u64>,
    removal: VecDeque<(u64, u8)>,
    section_buf: Box<[LightProperties; 4096]>,
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
            sky_sources: Vec::new(),
            sky_sources_dirty: true,
            section_has_emission: vec![false; (height / 16) * 9],
            section_state: vec![SECTION_OPAQUE; (height / 16) * 9],
            highest_non_air_sy: None,
            loaded: [false; 9],
            pending: HashSet::new(),
            queue: VecDeque::new(),
            removal: VecDeque::new(),
            section_buf: Box::new([LightProperties::AIR; 4096]),
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
    #[inline]
    fn pack(x: usize, y: usize, z: usize, mask: u8) -> u64 {
        // `y` is a local height index in `0..height`; the engine accepts heights up to
        // 65,536, so it needs all 16 bits. A narrower field silently aliases tall columns.
        (x as u64) | ((z as u64) << 6) | ((y as u64) << 12) | (u64::from(mask & 63) << 28)
    }
    #[inline]
    fn unpack_item(item: u64) -> (usize, usize, usize, u8) {
        let x = (item & 63) as usize;
        let z = ((item >> 6) & 63) as usize;
        let y = ((item >> 12) & 0xffff) as usize;
        let mask = ((item >> 28) & 63) as u8;
        (x, y, z, mask)
    }
    #[inline]
    fn level_at(&self, kind: LightKind, x: usize, y: usize, z: usize) -> u8 {
        let section = (y / 16 + 1) * 9 + (z / 16) * 3 + x / 16;
        let offset = (y % 16) * 256 + (z % 16) * 16 + x % 16;
        match kind {
            LightKind::Sky => &self.sections[section].sky,
            LightKind::Block => &self.sections[section].block,
        }
        .get(offset)
        .unwrap_or(0)
    }
    #[inline]
    fn set_at(&mut self, kind: LightKind, x: usize, y: usize, z: usize, value: u8) {
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
        let buf = &mut *self.section_buf;
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
                    let sec_idx = sy * 9 + chunk_slot;
                    let sec_base_y = sy * 16;
                    if !loaded {
                        if self.section_state[sec_idx] != SECTION_OPAQUE {
                            for ly in 0..16 {
                                let y_off = (sec_base_y + ly) * PLANE + z_off * WIDTH + x_off;
                                for lz in 0..16 {
                                    self.blocks[y_off + lz * WIDTH..y_off + lz * WIDTH + 16]
                                        .fill(LightProperties::OPAQUE);
                                }
                            }
                            self.section_state[sec_idx] = SECTION_OPAQUE;
                        }
                        continue;
                    }
                    if world.section_is_empty(cx, cz, sy) {
                        if self.section_state[sec_idx] != SECTION_AIR {
                            for ly in 0..16 {
                                let y_off = (sec_base_y + ly) * PLANE + z_off * WIDTH + x_off;
                                for lz in 0..16 {
                                    self.blocks[y_off + lz * WIDTH..y_off + lz * WIDTH + 16]
                                        .fill(LightProperties::AIR);
                                }
                            }
                            self.section_state[sec_idx] = SECTION_AIR;
                        }
                        continue;
                    }
                    if world.section_is_opaque(cx, cz, sy) {
                        self.highest_non_air_sy =
                            Some(self.highest_non_air_sy.map_or(sy, |prev| prev.max(sy)));
                        if self.section_state[sec_idx] != SECTION_OPAQUE {
                            for ly in 0..16 {
                                let y_off = (sec_base_y + ly) * PLANE + z_off * WIDTH + x_off;
                                for lz in 0..16 {
                                    self.blocks[y_off + lz * WIDTH..y_off + lz * WIDTH + 16]
                                        .fill(LightProperties::OPAQUE);
                                }
                            }
                            self.section_state[sec_idx] = SECTION_OPAQUE;
                        }
                        continue;
                    }
                    self.section_state[sec_idx] = SECTION_MIXED;
                    world.fill_section_properties(cx, cz, sy, self.min_y + sec_base_y as i32, buf);
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
                    self.section_has_emission[sec_idx] = has_emission;
                    if has_non_air {
                        self.highest_non_air_sy =
                            Some(self.highest_non_air_sy.map_or(sy, |prev| prev.max(sy)));
                    }
                }
            }
        }
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
                                    self.set_at(
                                        LightKind::Block,
                                        x_off + lx,
                                        sec_base_y + ly,
                                        z_off + lz,
                                        emission,
                                    );
                                    self.queue.push_back(Self::pack(
                                        x_off + lx,
                                        sec_base_y + ly,
                                        z_off + lz,
                                        63,
                                    ));
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
        let scan_height = scan_top_sy * 16;
        self.sky_sources_dirty = true;
        if scan_top_sy < num_sections {
            // Every loaded chunk has uniform AIR across all sections >= `scan_top_sy`:
            // fill `Full` (15) directly without 4096*N queue operations.
            for sy in scan_top_sy..num_sections {
                for chunk in 0..9 {
                    if self.loaded[chunk] {
                        self.sections[(sy + 1) * 9 + chunk].sky = NibbleArray::Full;
                    }
                }
            }
            let base_cx = self.origin_x / 16;
            let base_cz = self.origin_z / 16;
            for cz_i in 0..3i32 {
                for cx_i in 0..3i32 {
                    if !self.loaded[(cz_i * 3 + cx_i) as usize] {
                        continue;
                    }
                    let cx = base_cx + cx_i;
                    let cz = base_cz + cz_i;
                    for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let ncx_i = cx_i + dx;
                        let ncz_i = cz_i + dz;
                        if !(0..3).contains(&ncx_i)
                            || !(0..3).contains(&ncz_i)
                            || !self.loaded[(ncz_i * 3 + ncx_i) as usize]
                        {
                            self.pending.insert((cx + dx, cz + dz));
                        }
                    }
                }
            }
        }
        let mut col_stop_y = [scan_height as u16; PLANE];
        for cz_i in 0..3 {
            let z_off = cz_i * 16;
            for cx_i in 0..3 {
                let chunk_slot = cz_i * 3 + cx_i;
                if !self.loaded[chunk_slot] {
                    continue;
                }
                let x_off = cx_i * 16;
                let chunk_top_sy = (0..scan_top_sy)
                    .rev()
                    .find(|&sy| self.section_state[sy * 9 + chunk_slot] != SECTION_AIR)
                    .map_or(0, |sy| sy + 1);
                let chunk_scan_height = chunk_top_sy * 16;
                let mut chunk_max_stop = 0usize;
                for z in z_off..z_off + 16 {
                    let row_off = z * WIDTH;
                    for x in x_off..x_off + 16 {
                        let mut stop = 0usize;
                        for y in (0..chunk_scan_height).rev() {
                            if self.blocks[Self::index(x, y, z)] != LightProperties::AIR {
                                stop = y + 1;
                                break;
                            }
                        }
                        col_stop_y[row_off + x] = stop as u16;
                        if stop > chunk_max_stop {
                            chunk_max_stop = stop;
                        }
                    }
                }
                let first_full_sy = chunk_max_stop.div_ceil(16).min(scan_top_sy);
                for sy in first_full_sy..scan_top_sy {
                    self.sections[(sy + 1) * 9 + chunk_slot].sky = NibbleArray::Full;
                }
                let partial_top = (first_full_sy * 16).min(scan_height);
                for z in z_off..z_off + 16 {
                    let row_off = z * WIDTH;
                    for x in x_off..x_off + 16 {
                        let stop = col_stop_y[row_off + x] as usize;
                        for y in stop..partial_top {
                            self.set_at(LightKind::Sky, x, y, z, 15);
                        }
                        if stop > 0 {
                            let mut value = 15u8;
                            let mut above = LightProperties::AIR;
                            for y in (0..stop).rev() {
                                let i = Self::index(x, y, z);
                                let p = self.blocks[i];
                                if properties::occludes(above.faces[2], p.faces[3], 2) {
                                    break;
                                }
                                value = value.saturating_sub(if value == 15 {
                                    p.opacity.min(15)
                                } else {
                                    p.opacity.clamp(1, 15)
                                });
                                if value == 0 {
                                    break;
                                }
                                self.set_at(LightKind::Sky, x, y, z, value);
                                self.queue.push_back(Self::pack(x, y, z, 63));
                                above = p;
                            }
                        }
                    }
                }
            }
        }
        let base_cx = self.origin_x / 16;
        let base_cz = self.origin_z / 16;
        for cz_i in 0..3 {
            let z_off = cz_i * 16;
            for cx_i in 0..3 {
                if !self.loaded[cz_i * 3 + cx_i] {
                    continue;
                }
                let x_off = cx_i * 16;
                for z in z_off..z_off + 16 {
                    let row_off = z * WIDTH;
                    for x in x_off..x_off + 16 {
                        let my_stop = col_stop_y[row_off + x] as usize;
                        if my_stop >= scan_height {
                            continue;
                        }
                        let mut max_nbr = my_stop;
                        for (dx, dz) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                            let nx = x as i32 + dx;
                            let nz = z as i32 + dz;
                            if !(0..48).contains(&nx) || !(0..48).contains(&nz) {
                                self.pending.insert((
                                    base_cx + nx.div_euclid(16),
                                    base_cz + nz.div_euclid(16),
                                ));
                                continue;
                            }
                            let ux = nx as usize;
                            let uz = nz as usize;
                            if !self.loaded[(uz / 16) * 3 + ux / 16] {
                                self.pending.insert((
                                    base_cx + (ux / 16) as i32,
                                    base_cz + (uz / 16) as i32,
                                ));
                                continue;
                            }
                            let ns = col_stop_y[uz * WIDTH + ux] as usize;
                            if ns > max_nbr {
                                max_nbr = ns;
                            }
                        }
                        let end_y = max_nbr.min(scan_height);
                        for y in my_stop..end_y {
                            self.queue.push_back(Self::pack(x, y, z, 0b11_0011));
                        }
                    }
                }
            }
        }
        self.propagate(LightKind::Sky);
    }
    fn ensure_sky_sources(&mut self) {
        if !self.sky_sources_dirty {
            return;
        }
        let total = self.height * PLANE;
        if self.sky_sources.len() != total {
            self.sky_sources.resize(total, 0);
        } else {
            self.sky_sources.fill(0);
        }
        for cz_i in 0..3 {
            let z_off = cz_i * 16;
            for cx_i in 0..3 {
                if !self.loaded[cz_i * 3 + cx_i] {
                    continue;
                }
                let x_off = cx_i * 16;
                for z in z_off..z_off + 16 {
                    for x in x_off..x_off + 16 {
                        let mut value = 15u8;
                        let mut above = LightProperties::AIR;
                        for y in (0..self.height).rev() {
                            let i = Self::index(x, y, z);
                            let p = self.blocks[i];
                            if properties::occludes(above.faces[2], p.faces[3], 2) {
                                break;
                            }
                            value = value.saturating_sub(if value == 15 {
                                p.opacity.min(15)
                            } else {
                                p.opacity.clamp(1, 15)
                            });
                            if value == 0 {
                                break;
                            }
                            self.sky_sources[i] = value;
                            above = p;
                        }
                    }
                }
            }
        }
        self.sky_sources_dirty = false;
    }
    fn propagate(&mut self, kind: LightKind) {
        while let Some(item) = self.queue.pop_front() {
            let (x, y, z, mask) = Self::unpack_item(item);
            let level = self.level_at(kind, x, y, z);
            if level <= 1 {
                continue;
            }
            let i = Self::index(x, y, z);
            let from = self.blocks[i];
            for (direction, &(dx, dy, dz)) in DIRECTIONS.iter().enumerate() {
                if mask & (1 << direction) == 0 {
                    continue;
                }
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                let nz = z as i32 + dz;
                if !(0..48).contains(&nx) || !(0..48).contains(&nz) {
                    let wx = self.origin_x + nx;
                    let wz = self.origin_z + nz;
                    self.pending.insert((wx.div_euclid(16), wz.div_euclid(16)));
                    continue;
                }
                if ny < 0 || ny >= self.height as i32 {
                    continue;
                }
                let ux = nx as usize;
                let uy = ny as usize;
                let uz = nz as usize;
                let cx = ux / 16;
                let cz = uz / 16;
                if !self.loaded[cz * 3 + cx] {
                    self.pending.insert((
                        (self.origin_x + nx).div_euclid(16),
                        (self.origin_z + nz).div_euclid(16),
                    ));
                    continue;
                }
                let n = Self::index(ux, uy, uz);
                let target = self.blocks[n];
                let value = level.saturating_sub(target.opacity.max(1));
                if value <= self.level_at(kind, ux, uy, uz) {
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
                self.set_at(kind, ux, uy, uz, value);
                self.queue
                    .push_back(Self::pack(ux, uy, uz, 63u8 & !(1 << OPPOSITE[direction])));
            }
        }
    }
    fn remove_and_relight(&mut self, kind: LightKind) {
        while let Some((item, old)) = self.removal.pop_front() {
            let (x, y, z, mask) = Self::unpack_item(item);
            let i = Self::index(x, y, z);
            let source = match kind {
                LightKind::Sky => self.sky_sources[i],
                LightKind::Block => self.blocks[i].emission.min(15),
            };
            if source > self.level_at(kind, x, y, z) {
                self.set_at(kind, x, y, z, source);
                self.queue.push_back(Self::pack(x, y, z, 63));
            }
            for (dir, &(dx, dy, dz)) in DIRECTIONS.iter().enumerate() {
                if mask & (1 << dir) == 0 {
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
                let ux = nx as usize;
                let uy = ny as usize;
                let uz = nz as usize;
                if !self.loaded[(uz / 16) * 3 + ux / 16] {
                    continue;
                }
                let value = self.level_at(kind, ux, uy, uz);
                if value == 0 {
                    continue;
                }
                if value < old {
                    self.set_at(kind, ux, uy, uz, 0);
                    self.removal
                        .push_back((Self::pack(ux, uy, uz, 63u8 & !(1 << OPPOSITE[dir])), value));
                } else {
                    self.queue.push_back(Self::pack(ux, uy, uz, 63));
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
        let new_properties = world.properties(world.state(x, y, z));
        let old_properties = self.blocks[index];
        // Most block updates do not change light at all (dirt <-> grass, farmland
        // moisture, redstone component states with the same shape). Skip both the
        // removal/relight queues and the full-section `compact` scan for them.
        if old_properties == new_properties {
            return;
        }
        let opacity_or_faces = old_properties.opacity != new_properties.opacity
            || old_properties.faces != new_properties.faces;
        let block_light_changed =
            opacity_or_faces || old_properties.emission != new_properties.emission;
        let sky_light_changed = opacity_or_faces;
        if self.sky_enabled && sky_light_changed {
            self.ensure_sky_sources();
        }
        self.blocks[index] = new_properties;
        self.section_state[(ly / 16) * 9 + (lz / 16) * 3 + lx / 16] = SECTION_MIXED;
        if block_light_changed {
            let old = self.level_at(LightKind::Block, lx, ly, lz);
            self.set_at(LightKind::Block, lx, ly, lz, 0);
            self.removal.push_back((Self::pack(lx, ly, lz, 63), old));
            self.remove_and_relight(LightKind::Block);
        }
        if self.sky_enabled && sky_light_changed {
            // A block change only affects direct sky light at and below its own layer:
            // the sources above it are a function of unchanged blocks. Resume the column
            // rescan from the layer above the change instead of the build limit.
            let (mut value, mut above) = if ly + 1 < self.height {
                let above_index = Self::index(lx, ly + 1, lz);
                (self.sky_sources[above_index], self.blocks[above_index])
            } else {
                (15u8, LightProperties::AIR)
            };
            for py in (0..=ly).rev() {
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
                let old_level = self.level_at(LightKind::Sky, lx, py, lz);
                if (old_source > value && old_level <= old_source) || i == index {
                    self.set_at(LightKind::Sky, lx, py, lz, 0);
                    self.removal
                        .push_back((Self::pack(lx, py, lz, 63), old_level));
                } else if value > old_level {
                    self.set_at(LightKind::Sky, lx, py, lz, value);
                    self.queue.push_back(Self::pack(lx, py, lz, 63));
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

#[cfg(test)]
mod tests {
    use super::*;

    struct SwapWorld {
        stone: u16,
        dirt: u16,
        dirt_active: bool,
        removed: bool,
    }

    impl LightBlockView for SwapWorld {
        fn state(&self, x: i32, y: i32, z: i32) -> u16 {
            if (x, y, z) == (8, 70, 8) {
                if self.removed {
                    0
                } else if self.dirt_active {
                    self.dirt
                } else {
                    self.stone
                }
            } else {
                0
            }
        }
    }

    /// A stone/dirt swap has identical light properties, so it must not relight the
    /// column; removing the block does change the properties and must.
    #[test]
    fn block_changed_skips_identical_properties_but_relights_a_removal() {
        let stone = ferrite_data::blocks::default_state("stone").unwrap();
        let dirt = ferrite_data::blocks::default_state("dirt").unwrap();
        assert_eq!(vanilla_properties(stone), vanilla_properties(dirt));
        let mut world = SwapWorld {
            stone,
            dirt,
            dirt_active: false,
            removed: false,
        };
        let mut engine = LightEngine::new(0, 0, -4, 19, true);
        engine.relight(&world);
        let probes = [(8, 69, 8), (8, 70, 8), (8, 71, 8), (7, 70, 8)];
        let levels = |engine: &LightEngine| {
            probes
                .iter()
                .map(|&(x, y, z)| {
                    (
                        engine.get(LightKind::Sky, x, y, z),
                        engine.get(LightKind::Block, x, y, z),
                    )
                })
                .collect::<Vec<_>>()
        };
        let before = levels(&engine);
        world.dirt_active = true;
        engine.block_changed(&world, 8, 70, 8);
        assert_eq!(
            levels(&engine),
            before,
            "same light properties must not relight"
        );
        world.removed = true;
        engine.block_changed(&world, 8, 70, 8);
        assert!(
            engine.get(LightKind::Sky, 8, 70, 8).unwrap() > 0,
            "removing the block must reopen the sky column"
        );
    }

    // Regression: the queue entry packed the local y into 12 bits, so a world
    // taller than 4,096 blocks (which `LightEngine::new` accepts) truncated the
    // height and updated light at the wrong layer.
    #[test]
    fn packed_positions_round_trip_the_full_height_range() {
        for (x, z) in [(0, 0), (47, 12), (63, 63)] {
            for y in [0usize, 1, 15, 4095, 4096, 5000, 65535] {
                let mask = 0b10_1011;
                let packed = LightEngine::pack(x, y, z, mask);
                assert_eq!(LightEngine::unpack_item(packed), (x, y, z, mask));
            }
        }
    }
}
