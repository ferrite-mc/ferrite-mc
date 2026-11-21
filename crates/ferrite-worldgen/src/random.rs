//! Vanilla-compatible PRNGs (`Xoroshiro128++` and `LegacyRandom`).

use md5::{Digest, Md5};

/// SplitMix64 mixing function (Stafford's Mix13).
#[inline]
pub fn mix_stafford13(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Vanilla's `XoroshiroRandomSource` (`Xoroshiro128++`).
#[derive(Debug, Clone)]
pub struct XoroshiroRandom {
    lo: u64,
    hi: u64,
}

impl XoroshiroRandom {
    /// Creates a new `XoroshiroRandom` from a 64-bit world seed, matching vanilla's
    /// `RandomSupport.upgradeSeedTo128bit`.
    pub fn from_seed(seed: u64) -> Self {
        let lo_unmixed = seed ^ 0x6a09_e667_f3bc_c909;
        let hi_unmixed = lo_unmixed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        Self::from_128bit(mix_stafford13(lo_unmixed), mix_stafford13(hi_unmixed))
    }

    /// Creates a new `XoroshiroRandom` from a 128-bit state, ensuring it is never all-zero.
    pub fn from_128bit(lo: u64, hi: u64) -> Self {
        if (lo | hi) == 0 {
            Self {
                lo: 0x9e37_79b9_7f4a_7c15,
                hi: 0x6a09_e667_f3bc_c909,
            }
        } else {
            Self { lo, hi }
        }
    }

    /// Creates a `XoroshiroRandom` from the MD5 hash of `name` (vanilla `seedFromHashOf`).
    pub fn from_hash_of(name: &str) -> Self {
        let digest = Md5::digest(name.as_bytes());
        let lo = u64::from_be_bytes(digest[0..8].try_into().unwrap());
        let hi = u64::from_be_bytes(digest[8..16].try_into().unwrap());
        Self::from_128bit(lo, hi)
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let s0 = self.lo;
        let mut s1 = self.hi;
        let result = s0.wrapping_add(s1).rotate_left(17).wrapping_add(s0);

        s1 ^= s0;
        self.lo = s0.rotate_left(49) ^ s1 ^ (s1 << 21);
        self.hi = s1.rotate_left(28);
        result
    }

    #[inline]
    pub fn next_i32(&mut self) -> i32 {
        self.next_u64() as i32
    }

    #[inline]
    pub fn next_i32_bound(&mut self, bound: i32) -> i32 {
        debug_assert!(bound > 0);
        let bound_u64 = bound as u64;
        let mut r = (self.next_u64() as u32) as u64;
        let mut m = r * bound_u64;
        let mut l = (m as u32) as u64;
        if l < bound_u64 {
            let bound_u32 = bound as u32;
            let t = ((!bound_u32).wrapping_add(1) % bound_u32) as u64;
            while l < t {
                r = (self.next_u64() as u32) as u64;
                m = r * bound_u64;
                l = (m as u32) as u64;
            }
        }
        (m >> 32) as i32
    }

    #[inline]
    pub fn next_f64(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64) * 1.110_223_024_625_156_5e-16
    }

    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32) * 5.960_464_5e-8
    }

    #[inline]
    pub fn fork(&mut self) -> Self {
        let lo = self.next_u64();
        let hi = self.next_u64();
        Self::from_128bit(lo, hi)
    }

    #[inline]
    pub fn fork_positional(&mut self) -> PositionalRandomFactory {
        PositionalRandomFactory {
            lo: self.next_u64(),
            hi: self.next_u64(),
        }
    }
}

/// Vanilla's `XoroshiroRandomSource.XoroshiroPositionalRandomFactory`.
#[derive(Debug, Clone, Copy)]
pub struct PositionalRandomFactory {
    lo: u64,
    hi: u64,
}

impl PositionalRandomFactory {
    #[inline]
    pub fn at(&self, x: i32, y: i32, z: i32) -> XoroshiroRandom {
        let pos_seed = get_seed(x, y, z);
        XoroshiroRandom::from_128bit(pos_seed ^ self.lo, self.hi)
    }

    pub fn from_hash_of(&self, name: &str) -> XoroshiroRandom {
        let digest = Md5::digest(name.as_bytes());
        let lo = u64::from_be_bytes(digest[0..8].try_into().unwrap());
        let hi = u64::from_be_bytes(digest[8..16].try_into().unwrap());
        XoroshiroRandom::from_128bit(lo ^ self.lo, hi ^ self.hi)
    }
}

/// Vanilla's `Mth.getSeed(int x, int y, int z)`.
#[inline]
pub fn get_seed(x: i32, y: i32, z: i32) -> u64 {
    let i =
        (x as i64).wrapping_mul(3_129_871) ^ ((z as i64).wrapping_mul(116_129_781)) ^ (y as i64);
    let i = i
        .wrapping_mul(i)
        .wrapping_mul(42_317_861)
        .wrapping_add(i.wrapping_mul(11));
    (i >> 16) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xoroshiro_deterministic_from_seed() {
        let mut r1 = XoroshiroRandom::from_seed(12345);
        let mut r2 = XoroshiroRandom::from_seed(12345);
        for _ in 0..16 {
            assert_eq!(r1.next_u64(), r2.next_u64());
        }
    }

    #[test]
    fn xoroshiro_from_hash_of_is_nonzero_and_deterministic() {
        let mut r1 = XoroshiroRandom::from_hash_of("minecraft:temperature");
        let mut r2 = XoroshiroRandom::from_hash_of("minecraft:temperature");
        assert_eq!(r1.next_u64(), r2.next_u64());
        assert_ne!(r1.next_f64(), r2.next_f64() + 1.0);
    }
}
