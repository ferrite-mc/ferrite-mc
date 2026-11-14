//! A small, fast random source for entity behaviour (spawn offsets, spin, push-out speed).
//! Nothing here needs vanilla's exact sequence, only its distributions.

use std::sync::atomic::{AtomicU64, Ordering};

/// SplitMix64.
#[derive(Debug, Clone)]
pub struct Rng(u64);

static SEEDS: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);

impl Default for Rng {
    /// A fresh generator, seeded from the clock and a process-wide counter.
    fn default() -> Self {
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        Self::new(time ^ SEEDS.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed))
    }
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u32 << 24) as f32
    }

    /// Uniform in `[min, max)`, vanilla `Mth.nextDouble`.
    pub fn range(&mut self, min: f64, max: f64) -> f64 {
        min + self.next_f64() * (max - min)
    }

    /// Uniform in `0..bound` (`bound` must be positive).
    pub fn next_int(&mut self, bound: i32) -> i32 {
        (self.next_u64() % bound.max(1) as u64) as i32
    }

    /// Standard normal, by the Box-Muller transform (vanilla `Random.nextGaussian`).
    pub fn next_gaussian(&mut self) -> f64 {
        let u1 = self.next_f64().max(f64::MIN_POSITIVE);
        let u2 = self.next_f64();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_stay_in_range() {
        let mut rng = Rng::new(1);
        for _ in 0..1000 {
            assert!((0.0..1.0).contains(&rng.next_f64()));
            assert!((0.0..1.0).contains(&rng.next_f32()));
            assert!((-0.25..0.25).contains(&rng.range(-0.25, 0.25)));
            assert!((0..7).contains(&rng.next_int(7)));
        }
        assert_ne!(Rng::default().next_u64(), Rng::default().next_u64());
    }
}
