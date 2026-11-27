//! Chunk storage primitives.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteKind {
    Blocks,
    Biomes,
}
impl PaletteKind {
    pub const fn len(self) -> usize {
        match self {
            Self::Blocks => 4096,
            Self::Biomes => 64,
        }
    }
    pub const fn is_empty(self) -> bool {
        false
    }
    const fn min_bits(self) -> u8 {
        match self {
            Self::Blocks => 4,
            Self::Biomes => 1,
        }
    }
    const fn max_bits(self) -> u8 {
        match self {
            Self::Blocks => 8,
            Self::Biomes => 3,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PaletteError {
    #[error("invalid global bit width")]
    InvalidBits,
    #[error("index out of bounds")]
    Index,
    #[error("value does not fit global bit width")]
    Value,
}

/// Packed entries use the Java layout: no entry crosses a 64-bit word.
#[derive(Debug, Clone)]
pub struct PalettedContainer {
    kind: PaletteKind,
    global_bits: u8,
    bits: u8,
    palette: Vec<u32>,
    counts: Vec<(u32, u16)>,
    data: Vec<u64>,
}
impl PalettedContainer {
    pub fn new(kind: PaletteKind, value: u32, global_bits: u8) -> Result<Self, PaletteError> {
        if global_bits <= kind.max_bits() || global_bits > 32 {
            return Err(PaletteError::InvalidBits);
        }
        if u64::from(value) >= (1u64 << global_bits) {
            return Err(PaletteError::Value);
        }
        Ok(Self {
            kind,
            global_bits,
            bits: 0,
            palette: vec![value],
            counts: vec![(value, kind.len() as u16)],
            data: Vec::new(),
        })
    }
    pub const fn bits_per_entry(&self) -> u8 {
        self.bits
    }
    pub const fn global_bits(&self) -> u8 {
        self.global_bits
    }
    pub fn packed_data(&self) -> &[u64] {
        &self.data
    }
    pub(crate) fn from_nbt_parts(
        kind: PaletteKind,
        global_bits: u8,
        bits: u8,
        palette: Vec<u32>,
        counts: Vec<(u32, u16)>,
        data: Vec<u64>,
    ) -> Self {
        debug_assert!(global_bits > kind.max_bits() && global_bits <= 32);
        debug_assert!(bits >= kind.min_bits() && bits < global_bits);
        Self {
            kind,
            global_bits,
            bits,
            palette,
            counts,
            data,
        }
    }
    /// Constructs a [`PalettedContainer`] directly from a dense slice of `u16` states,
    /// avoiding per-block palette repacks and count searches during bulk worldgen.
    pub fn from_states_u16(
        kind: PaletteKind,
        states: &[u16],
        global_bits: u8,
    ) -> Result<Self, PaletteError> {
        if states.len() != kind.len() {
            return Err(PaletteError::Index);
        }
        if global_bits <= kind.max_bits() || global_bits > 32 {
            return Err(PaletteError::InvalidBits);
        }
        let first = states[0];
        if u64::from(first) >= (1u64 << global_bits) {
            return Err(PaletteError::Value);
        }
        let Some(first_diff) = states[1..].iter().position(|&s| s != first).map(|i| i + 1) else {
            return Self::new(kind, u32::from(first), global_bits);
        };

        let mut counts_u16: Vec<(u16, u16)> = Vec::with_capacity(16);
        counts_u16.push((first, first_diff as u16));
        let mut last_state = first;
        let mut last_idx = 0usize;
        for &state in &states[first_diff..] {
            if state == last_state {
                counts_u16[last_idx].1 += 1;
            } else if let Some(pos) = counts_u16.iter().position(|&(s, _)| s == state) {
                counts_u16[pos].1 += 1;
                last_state = state;
                last_idx = pos;
            } else {
                if u64::from(state) >= (1u64 << global_bits) {
                    return Err(PaletteError::Value);
                }
                last_state = state;
                last_idx = counts_u16.len();
                counts_u16.push((state, 1));
            }
        }
        counts_u16.sort_unstable_by_key(|&(s, _)| s);

        let counts: Vec<(u32, u16)> = counts_u16.iter().map(|&(s, c)| (u32::from(s), c)).collect();
        let palette: Vec<u32> = counts.iter().map(|&(k, _)| k).collect();
        let bits = {
            let b = (usize::BITS - (palette.len() - 1).leading_zeros()) as u8;
            let b = b.max(kind.min_bits());
            if b > kind.max_bits() { global_bits } else { b }
        };
        let per = 64 / usize::from(bits);
        let mut data = vec![0u64; kind.len().div_ceil(per)];

        if bits == global_bits {
            let mut word_idx = 0usize;
            let mut lane = 0usize;
            let mut word = 0u64;
            for &s in states {
                word |= u64::from(s) << (lane * usize::from(bits));
                lane += 1;
                if lane == per {
                    data[word_idx] = word;
                    word_idx += 1;
                    word = 0;
                    lane = 0;
                }
            }
            if lane > 0 {
                data[word_idx] = word;
            }
        } else {
            // Try a 64-entry direct hash LUT first so common worldgen sections (<=16 states)
            // pack all 4,096 blocks with O(1) table lookup and zero per-block search branches.
            // `u32::MAX` is not a representable state id, unlike `u16::MAX`, so the
            // empty marker can never collide with a real entry.
            let mut lut = [(u32::MAX, 0u8); 64];
            let mut lut_ok = counts_u16.len() <= 32;
            if lut_ok {
                for (idx, &(s, _)) in counts_u16.iter().enumerate() {
                    let h = (usize::from(s).wrapping_mul(0x9e37) >> 10) & 63;
                    if lut[h].0 != u32::MAX {
                        lut_ok = false;
                        break;
                    }
                    lut[h] = (u32::from(s), idx as u8);
                }
            }
            if lut_ok && bits == 4 {
                for (dst, chunk16) in data.iter_mut().zip(states.as_chunks::<16>().0) {
                    let mut w = 0u64;
                    for (lane, &s) in chunk16.iter().enumerate() {
                        let h = (usize::from(s).wrapping_mul(0x9e37) >> 10) & 63;
                        w |= u64::from(lut[h].1) << (lane * 4);
                    }
                    *dst = w;
                }
            } else if lut_ok {
                let mut word_idx = 0usize;
                let mut lane = 0usize;
                let mut word = 0u64;
                for &s in states {
                    let h = (usize::from(s).wrapping_mul(0x9e37) >> 10) & 63;
                    let raw = u64::from(lut[h].1);
                    word |= raw << (lane * usize::from(bits));
                    lane += 1;
                    if lane == per {
                        data[word_idx] = word;
                        word_idx += 1;
                        word = 0;
                        lane = 0;
                    }
                }
                if lane > 0 {
                    data[word_idx] = word;
                }
            } else {
                let mut word_idx = 0usize;
                let mut lane = 0usize;
                let mut word = 0u64;
                // `u32::MAX` is outside the `u16` state range, so it can never be an MRU hit
                // for a real state (unlike `u16::MAX`, which is a valid id).
                let mut mru_state = u32::MAX;
                let mut mru_idx = 0u64;
                for &s in states {
                    let raw = if u32::from(s) == mru_state {
                        mru_idx
                    } else {
                        let idx = if counts_u16.len() <= 16 {
                            counts_u16.iter().position(|&(k, _)| k == s).unwrap() as u64
                        } else {
                            counts_u16.binary_search_by_key(&s, |&(k, _)| k).unwrap() as u64
                        };
                        mru_state = u32::from(s);
                        mru_idx = idx;
                        idx
                    };
                    word |= raw << (lane * usize::from(bits));
                    lane += 1;
                    if lane == per {
                        data[word_idx] = word;
                        word_idx += 1;
                        word = 0;
                        lane = 0;
                    }
                }
                if lane > 0 {
                    data[word_idx] = word;
                }
            }
        }

        Ok(Self {
            kind,
            global_bits,
            bits,
            palette,
            counts,
            data,
        })
    }

    /// Packs all `kind.len()` biome or block entries in one pass, using a zero-data single-value
    /// palette when every entry is identical.
    pub fn from_values_u32(
        kind: PaletteKind,
        values: &[u32],
        global_bits: u8,
    ) -> Result<Self, PaletteError> {
        if values.len() != kind.len() {
            return Err(PaletteError::Index);
        }
        if global_bits <= kind.max_bits() || global_bits > 32 {
            return Err(PaletteError::InvalidBits);
        }
        let first = values[0];
        if u64::from(first) >= (1u64 << global_bits) {
            return Err(PaletteError::Value);
        }
        if values[1..].iter().all(|&v| v == first) {
            return Self::new(kind, first, global_bits);
        }

        let mut counts: Vec<(u32, u16)> = Vec::with_capacity(8);
        let mut last_val = first;
        let mut last_idx = 0usize;
        counts.push((first, 1));
        for &val in &values[1..] {
            if val == last_val {
                counts[last_idx].1 += 1;
            } else if let Some(pos) = counts.iter().position(|&(k, _)| k == val) {
                counts[pos].1 += 1;
                last_val = val;
                last_idx = pos;
            } else {
                if u64::from(val) >= (1u64 << global_bits) {
                    return Err(PaletteError::Value);
                }
                last_val = val;
                last_idx = counts.len();
                counts.push((val, 1));
            }
        }
        counts.sort_unstable_by_key(|&(k, _)| k);
        let palette: Vec<u32> = counts.iter().map(|&(k, _)| k).collect();
        let bits = {
            let b = (usize::BITS - (palette.len() - 1).leading_zeros()) as u8;
            let b = b.max(kind.min_bits());
            if b > kind.max_bits() { global_bits } else { b }
        };
        let per = 64 / usize::from(bits);
        let mut data = vec![0u64; kind.len().div_ceil(per)];

        let mut word_idx = 0usize;
        let mut lane = 0usize;
        let mut word = 0u64;
        // `u64::MAX` is outside the `u32` value range and therefore never an MRU hit.
        let mut mru_val = u64::MAX;
        let mut mru_raw = 0u64;
        let small_palette = counts.len() <= 16;
        for &v in values {
            let raw = if bits == global_bits {
                u64::from(v)
            } else if u64::from(v) == mru_val {
                mru_raw
            } else {
                let idx = if small_palette {
                    counts.iter().position(|&(k, _)| k == v).unwrap() as u64
                } else {
                    counts.binary_search_by_key(&v, |&(k, _)| k).unwrap() as u64
                };
                mru_val = u64::from(v);
                mru_raw = idx;
                idx
            };
            word |= raw << (lane * usize::from(bits));
            lane += 1;
            if lane == per {
                data[word_idx] = word;
                word_idx += 1;
                word = 0;
                lane = 0;
            }
        }
        if lane > 0 {
            data[word_idx] = word;
        }

        Ok(Self {
            kind,
            global_bits,
            bits,
            palette,
            counts,
            data,
        })
    }
    /// Frequencies already accumulated when the container was decoded or changed.
    pub(crate) fn value_counts(&self) -> impl Iterator<Item = (u32, usize)> + '_ {
        self.counts
            .iter()
            .map(|&(value, count)| (value, usize::from(count)))
    }
    pub fn palette(&self) -> &[u32] {
        &self.palette
    }
    #[inline]
    fn has_count(&self, value: u32) -> bool {
        self.counts
            .binary_search_by_key(&value, |&(k, _)| k)
            .is_ok()
    }
    pub fn get(&self, index: usize) -> Option<u32> {
        if index >= self.kind.len() {
            return None;
        }
        if self.bits == 0 {
            return Some(self.palette[0]);
        }
        let raw = lane_read(self.bits, &self.data, index);
        if self.bits == self.global_bits {
            Some(raw)
        } else {
            self.palette.get(raw as usize).copied()
        }
    }
    pub fn set(&mut self, index: usize, value: u32) -> Result<(), PaletteError> {
        if index >= self.kind.len() {
            return Err(PaletteError::Index);
        }
        if u64::from(value) >= (1u64 << self.global_bits) {
            return Err(PaletteError::Value);
        }
        let old = self.get(index).expect("internal palette invariant");
        if old == value {
            return Ok(());
        }
        let old_idx = self
            .counts
            .binary_search_by_key(&old, |&(k, _)| k)
            .expect("old value in counts");
        let remove_old = self.counts[old_idx].1 == 1;
        if remove_old {
            self.counts.remove(old_idx);
        } else {
            self.counts[old_idx].1 -= 1;
        }
        let new_value = match self.counts.binary_search_by_key(&value, |&(k, _)| k) {
            Ok(idx) => {
                self.counts[idx].1 += 1;
                false
            }
            Err(idx) => {
                self.counts.insert(idx, (value, 1));
                true
            }
        };

        if self.counts.len() == 1 {
            self.palette.clear();
            self.palette.push(value);
            self.bits = 0;
            self.data.clear();
        } else if self.bits == self.global_bits {
            if remove_old || new_value {
                self.palette.clear();
                self.palette.extend(self.counts.iter().map(|&(k, _)| k));
            }
            self.write(index, value);
        } else if self.bits == 0 {
            // All old entries are palette index zero; initialize the first indirect word array.
            self.bits = self.kind.min_bits();
            self.palette.push(value);
            let per = 64 / usize::from(self.bits);
            self.data.resize(self.kind.len().div_ceil(per), 0);
            self.data.fill(0);
            self.write(index, 1);
        } else {
            let slot = self
                .palette
                .iter()
                .position(|&v| v == value)
                .or_else(|| self.palette.iter().position(|&v| !self.has_count(v)));
            let raw = if let Some(slot) = slot {
                self.palette[slot] = value;
                slot
            } else if self.palette.len() < (1usize << self.bits) {
                self.palette.push(value);
                self.palette.len() - 1
            } else {
                let len = self.kind.len();
                let mut buf = [0u32; 4096];
                self.unpack_into(&mut buf[..len]);
                buf[index] = value;
                self.repack(&buf[..len]);
                return Ok(());
            };
            self.write(index, raw as u32);
        }
        Ok(())
    }
    fn unpack_into(&self, dst: &mut [u32]) {
        if self.bits == 0 {
            dst.fill(self.palette[0]);
            return;
        }
        let bpe = usize::from(self.bits);
        let per = 64 / bpe;
        let mask = (1u64 << bpe) - 1;
        let mut wi = 0usize;
        let mut lane = 0usize;
        let mut word = self.data[0];
        let direct = self.bits == self.global_bits;
        for out in dst.iter_mut() {
            let raw = (word & mask) as usize;
            *out = if direct {
                raw as u32
            } else {
                self.palette[raw]
            };
            lane += 1;
            if lane == per {
                lane = 0;
                wi += 1;
                if wi < self.data.len() {
                    word = self.data[wi];
                }
            } else {
                word >>= bpe;
            }
        }
    }
    fn repack(&mut self, values: &[u32]) {
        self.palette.clear();
        self.palette.extend(self.counts.iter().map(|&(k, _)| k));
        let count = self.palette.len();
        self.bits = if count == 1 {
            0
        } else {
            let bits = (usize::BITS - (count - 1).leading_zeros()) as u8;
            let bits = bits.max(self.kind.min_bits());
            if bits > self.kind.max_bits() {
                self.global_bits
            } else {
                bits
            }
        };
        if self.bits == 0 {
            self.data.clear();
            return;
        }
        let bpe = usize::from(self.bits);
        let per = 64 / bpe;
        self.data.resize(self.kind.len().div_ceil(per), 0);
        let direct = self.bits == self.global_bits;
        let small_palette = self.palette.len() <= 16;
        let mut word_idx = 0usize;
        let mut lane = 0usize;
        let mut word = 0u64;
        // `u64::MAX` is outside the `u32` value range and therefore never an MRU hit.
        let mut mru_val = u64::MAX;
        let mut mru_raw = 0u64;
        for &value in values {
            let raw = if direct {
                u64::from(value)
            } else if u64::from(value) == mru_val {
                mru_raw
            } else {
                let idx = if small_palette {
                    self.palette.iter().position(|&v| v == value).unwrap() as u64
                } else {
                    self.palette.binary_search(&value).unwrap() as u64
                };
                mru_val = u64::from(value);
                mru_raw = idx;
                idx
            };
            word |= raw << (lane * bpe);
            lane += 1;
            if lane == per {
                self.data[word_idx] = word;
                word_idx += 1;
                word = 0;
                lane = 0;
            }
        }
        if lane > 0 {
            self.data[word_idx] = word;
        }
    }
    fn write(&mut self, index: usize, value: u32) {
        lane_write(self.bits, &mut self.data, index, value);
    }
}

impl PalettedContainer {
    /// Encode a paletted container in the 1.21.5+ network layout.
    pub fn encode_network(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_network_into(&mut out);
        out
    }

    /// Appends the network-encoded palette and data array into `out` without an intermediate `Vec`.
    pub fn encode_network_into(&self, out: &mut Vec<u8>) {
        out.reserve(1 + self.data.len() * 8 + self.palette.len() * 2);
        out.push(self.bits);
        if self.bits == 0 {
            put_varint(out, self.palette[0]);
        } else if self.bits != self.global_bits {
            put_varint(out, self.palette.len() as u32);
            for &value in &self.palette {
                put_varint(out, value);
            }
        }
        let byte_len = self.data.len() * 8;
        if byte_len > 0 {
            out.reserve(byte_len);
            let start = out.len();
            let spare = &mut out.spare_capacity_mut()[..byte_len];
            for (dst, &word) in spare.as_chunks_mut::<8>().0.iter_mut().zip(&self.data) {
                let be = word.to_be_bytes();
                dst[0].write(be[0]);
                dst[1].write(be[1]);
                dst[2].write(be[2]);
                dst[3].write(be[3]);
                dst[4].write(be[4]);
                dst[5].write(be[5]);
                dst[6].write(be[6]);
                dst[7].write(be[7]);
            }
            // SAFETY: `reserve(byte_len)` succeeded and all `byte_len` bytes were initialized above.
            unsafe {
                out.set_len(start + byte_len);
            }
        }
    }
}
/// Reads one packed entry from a container of width `bits`.
///
/// The width is matched to a const generic so `index / per` and the lane shift
/// fold to multiply-shift instructions; a runtime `per` would divide once per
/// entry on every block read.
#[inline(always)]
fn lane_read(bits: u8, data: &[u64], index: usize) -> u32 {
    debug_assert!((1..=32).contains(&bits), "packed width out of range");
    match bits {
        1 => bits_read::<1>(data, index),
        2 => bits_read::<2>(data, index),
        3 => bits_read::<3>(data, index),
        4 => bits_read::<4>(data, index),
        5 => bits_read::<5>(data, index),
        6 => bits_read::<6>(data, index),
        7 => bits_read::<7>(data, index),
        8 => bits_read::<8>(data, index),
        16 => bits_read::<16>(data, index),
        32 => bits_read::<32>(data, index),
        _ => {
            let per = 64 / usize::from(bits);
            ((data[index / per] >> ((index % per) * usize::from(bits))) & ((1u64 << bits) - 1))
                as u32
        }
    }
}

/// Writes one packed entry; see [`lane_read`] for why the width is a constant.
#[inline(always)]
fn lane_write(bits: u8, data: &mut [u64], index: usize, value: u32) {
    debug_assert!((1..=32).contains(&bits), "packed width out of range");
    match bits {
        1 => bits_write::<1>(data, index, value),
        2 => bits_write::<2>(data, index, value),
        3 => bits_write::<3>(data, index, value),
        4 => bits_write::<4>(data, index, value),
        5 => bits_write::<5>(data, index, value),
        6 => bits_write::<6>(data, index, value),
        7 => bits_write::<7>(data, index, value),
        8 => bits_write::<8>(data, index, value),
        16 => bits_write::<16>(data, index, value),
        32 => bits_write::<32>(data, index, value),
        _ => {
            let per = 64 / usize::from(bits);
            let shift = (index % per) * usize::from(bits);
            let mask = ((1u64 << bits) - 1) << shift;
            let word = &mut data[index / per];
            *word = (*word & !mask) | (u64::from(value) << shift);
        }
    }
}

#[inline(always)]
fn bits_read<const BITS: usize>(data: &[u64], index: usize) -> u32 {
    // `BITS` is a const generic, so this folds to a constant after monomorphization.
    let per = 64 / BITS;
    ((data[index / per] >> ((index % per) * BITS)) & ((1u64 << BITS) - 1)) as u32
}

#[inline(always)]
fn bits_write<const BITS: usize>(data: &mut [u64], index: usize, value: u32) {
    let per = 64 / BITS;
    let shift = (index % per) * BITS;
    let mask = ((1u64 << BITS) - 1) << shift;
    let word = &mut data[index / per];
    *word = (*word & !mask) | (u64::from(value) << shift);
}

fn put_varint(out: &mut Vec<u8>, mut value: u32) {
    loop {
        if value & !0x7f == 0 {
            out.push(value as u8);
            break;
        }
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
}

impl PalettedContainer {
    /// Repack the values using the smallest valid palette.
    pub fn compact(&mut self) {
        if self.bits == 0 {
            return;
        }
        let len = self.kind.len();
        let mut buf = [0u32; 4096];
        self.unpack_into(&mut buf[..len]);
        self.repack(&buf[..len]);
    }
}
mod chunk;
pub use chunk::{
    BlockEntity, BlockFlags, BlockInfo, Chunk, ChunkError, ChunkSection, HeightmapKind,
};
mod network;
pub use network::{
    LightData, LightSection, NetworkError, encode_chunk_packet, encode_chunk_packet_into,
};
mod fworld;
pub use fworld::{
    BlobCompression, BlobError, CompressedChunkRecord, Fworld, FworldColdStore,
    MAX_FWORLD_UNCOMPRESSED_BYTES,
};
mod region;
pub use region::{RegionCompression, RegionError, RegionFile};
mod registry;
pub use registry::{BlockState, BlockStateRegistry, RegistryError, RuntimeRegistry};
impl PalettedContainer {
    pub fn from_values(
        kind: PaletteKind,
        values: &[u32],
        global_bits: u8,
    ) -> Result<Self, PaletteError> {
        if values.len() != kind.len() {
            return Err(PaletteError::Index);
        }
        // Validate the width before shifting by it: a caller-supplied `global_bits`
        // of 64 or more would otherwise overflow the shift below.
        if global_bits <= kind.max_bits() || global_bits > 32 {
            return Err(PaletteError::InvalidBits);
        }
        let first = values[0];
        if u64::from(first) >= (1u64 << global_bits) {
            return Err(PaletteError::Value);
        }
        if values.iter().all(|&v| v == first) {
            return Self::new(kind, first, global_bits);
        }
        let mut this = Self::new(kind, first, global_bits)?;
        this.counts.clear();
        for &value in values {
            if u64::from(value) >= (1u64 << global_bits) {
                return Err(PaletteError::Value);
            }
            match this.counts.binary_search_by_key(&value, |&(k, _)| k) {
                Ok(idx) => this.counts[idx].1 += 1,
                Err(idx) => this.counts.insert(idx, (value, 1)),
            }
        }
        this.repack(values);
        Ok(this)
    }
}
mod chunk_nbt;
pub use chunk_nbt::{ChunkNbtError, DATA_VERSION, chunk_from_nbt, chunk_to_nbt};

#[cfg(test)]
mod frequency_tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn frequencies_track_single_indirect_and_direct_palettes() {
        let mut p = PalettedContainer::new(PaletteKind::Blocks, 0, 16).unwrap();
        assert_eq!(p.value_counts().collect::<Vec<_>>(), vec![(0, 4096)]);
        for i in 0..20 {
            p.set(i, i as u32 + 1).unwrap();
        }
        assert_ne!(p.bits_per_entry(), 16);
        assert_eq!(p.value_counts().map(|(_, n)| n).sum::<usize>(), 4096);
        assert_eq!(
            p.value_counts().find(|(value, _)| *value == 0),
            Some((0, 4076))
        );
        for i in 20..300 {
            p.set(i, i as u32 + 1).unwrap();
        }
        assert_eq!(p.bits_per_entry(), 16);
        let counts: BTreeMap<_, _> = p.value_counts().collect();
        let mut expected = BTreeMap::new();
        for i in 0..4096 {
            *expected.entry(p.get(i).unwrap()).or_insert(0) += 1;
        }
        assert_eq!(counts, expected);
        for i in 0..300 {
            p.set(i, 0).unwrap();
        }
        assert_eq!(p.value_counts().collect::<Vec<_>>(), vec![(0, 4096)]);
    }

    #[test]
    fn from_states_u16_matches_incremental_sets() {
        for distinct in [1usize, 5, 24, 300] {
            let mut states = vec![0u16; 4096];
            for (i, s) in states.iter_mut().enumerate() {
                *s = ((i * 7 + 3) % distinct) as u16;
            }
            let fast =
                PalettedContainer::from_states_u16(PaletteKind::Blocks, &states, 16).unwrap();
            let mut slow = PalettedContainer::new(PaletteKind::Blocks, 0, 16).unwrap();
            for (i, &s) in states.iter().enumerate() {
                slow.set(i, u32::from(s)).unwrap();
            }
            for i in 0..4096 {
                assert_eq!(
                    fast.get(i),
                    slow.get(i),
                    "mismatch at {i} for distinct={distinct}"
                );
            }
            assert_eq!(
                fast.value_counts().collect::<BTreeMap<_, _>>(),
                slow.value_counts().collect::<BTreeMap<_, _>>()
            );
        }
    }

    // Regression: state id 65,535 was the hash table's empty marker, so a section
    // containing it could pack a collision bucket with the wrong palette index.
    #[test]
    fn from_states_u16_supports_the_maximum_state_id() {
        let max = u16::MAX;
        let bucket = |s: u16| (usize::from(s).wrapping_mul(0x9e37) >> 10) & 63;
        let other = (1u16..u16::MAX)
            .find(|&s| bucket(s) == bucket(max))
            .expect("a colliding state id exists");
        let mut states = vec![0u16; 4096];
        for (i, state) in states.iter_mut().enumerate() {
            *state = if i % 2 == 0 { max } else { other };
        }
        let container =
            PalettedContainer::from_states_u16(PaletteKind::Blocks, &states, 16).unwrap();
        for (i, &state) in states.iter().enumerate() {
            assert_eq!(
                container.get(i),
                Some(u32::from(state)),
                "state mismatch at {i}"
            );
        }
    }

    // Regression: the MRU cache used `u32::MAX` as its empty sentinel, which is a
    // valid value when the global width is 32 bits.
    #[test]
    fn from_values_u32_supports_the_maximum_value() {
        let mut values = [0u32; 64];
        for (i, value) in values.iter_mut().enumerate() {
            *value = if i % 2 == 0 { u32::MAX } else { 0 };
        }
        let container =
            PalettedContainer::from_values_u32(PaletteKind::Biomes, &values, 32).unwrap();
        for (i, &value) in values.iter().enumerate() {
            assert_eq!(container.get(i), Some(value), "value mismatch at {i}");
        }
    }

    #[test]
    fn from_values_supports_the_maximum_value() {
        let mut values = [0u32; 64];
        for (i, value) in values.iter_mut().enumerate() {
            *value = if i % 2 == 0 { u32::MAX } else { 7 };
        }
        let container = PalettedContainer::from_values(PaletteKind::Biomes, &values, 32).unwrap();
        for (i, &value) in values.iter().enumerate() {
            assert_eq!(container.get(i), Some(value), "value mismatch at {i}");
        }
    }

    #[test]
    fn from_values_rejects_unusable_global_bits_without_overflowing() {
        let error = PalettedContainer::from_values(PaletteKind::Biomes, &[0u32; 64], 64)
            .expect_err("64 global bits cannot be encoded");
        assert_eq!(error, PaletteError::InvalidBits);
    }

    #[test]
    fn from_values_u32_matches_incremental_sets() {
        for distinct in [1usize, 3, 12] {
            let mut values = [0u32; 64];
            for (i, v) in values.iter_mut().enumerate() {
                *v = ((i * 5 + 1) % distinct) as u32;
            }
            let fast = PalettedContainer::from_values_u32(PaletteKind::Biomes, &values, 6).unwrap();
            let mut slow = PalettedContainer::new(PaletteKind::Biomes, 0, 6).unwrap();
            for (i, &v) in values.iter().enumerate() {
                slow.set(i, v).unwrap();
            }
            for i in 0..64 {
                assert_eq!(fast.get(i), slow.get(i), "mismatch at {i}");
            }
            assert_eq!(
                fast.value_counts().collect::<BTreeMap<_, _>>(),
                slow.value_counts().collect::<BTreeMap<_, _>>()
            );
        }
    }
}
