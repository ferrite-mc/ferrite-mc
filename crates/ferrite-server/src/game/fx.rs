use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

/// Zero-dependency FxHash hasher for fast internal chunk, UUID, and player maps.
#[derive(Default)]
pub(crate) struct FxHasher(u64);

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(b)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.0 = (self.0.rotate_left(5) ^ u64::from(i)).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.0 = (self.0.rotate_left(5) ^ u64::from(i)).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.0 = (self.0.rotate_left(5) ^ u64::from(i)).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    #[inline]
    fn write_i32(&mut self, i: i32) {
        self.write_u32(i as u32);
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    #[inline]
    fn write_u128(&mut self, i: u128) {
        self.write_u64(i as u64);
        self.write_u64((i >> 64) as u64);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

pub(crate) type FxBuildHasher = BuildHasherDefault<FxHasher>;
pub(crate) type FxMap<K, V> = HashMap<K, V, FxBuildHasher>;
pub(crate) type FxSet<K> = HashSet<K, FxBuildHasher>;
