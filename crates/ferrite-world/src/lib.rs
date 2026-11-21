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
    pub fn packed_data(&self) -> &[u64] {
        &self.data
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
        let per = 64 / usize::from(self.bits);
        let raw = (self.data[index / per] >> ((index % per) * usize::from(self.bits)))
            & ((1u64 << self.bits) - 1);
        if self.bits == self.global_bits {
            Some(raw as u32)
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
            self.data = vec![0; self.kind.len().div_ceil(per)];
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
                let mut values: Vec<_> = (0..self.kind.len())
                    .map(|i| self.get(i).expect("internal palette invariant"))
                    .collect();
                values[index] = value;
                self.repack(&values);
                return Ok(());
            };
            self.write(index, raw as u32);
        }
        Ok(())
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
        let per = 64 / usize::from(self.bits);
        self.data = vec![0; self.kind.len().div_ceil(per)];
        for (i, &value) in values.iter().enumerate() {
            let raw = if self.bits == self.global_bits {
                value
            } else {
                self.palette.binary_search(&value).unwrap() as u32
            };
            self.write(i, raw);
        }
    }
    fn write(&mut self, index: usize, value: u32) {
        let per = 64 / usize::from(self.bits);
        let shift = (index % per) * usize::from(self.bits);
        let mask = ((1u64 << self.bits) - 1) << shift;
        let word = &mut self.data[index / per];
        *word = (*word & !mask) | (u64::from(value) << shift);
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
        for &word in &self.data {
            out.extend_from_slice(&word.to_be_bytes());
        }
    }
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
        let values: Vec<_> = (0..self.kind.len())
            .map(|i| self.get(i).expect("valid palette"))
            .collect();
        self.repack(&values);
    }
}
mod chunk;
pub use chunk::{
    BlockEntity, BlockFlags, BlockInfo, Chunk, ChunkError, ChunkSection, HeightmapKind,
};
mod network;
pub use network::{LightData, LightSection, NetworkError, encode_chunk_packet};
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
}
