//! Vanilla-compatible Perlin (`ImprovedNoise`) and `NormalNoise` (`DoublePerlinNoise`).

use crate::random::XoroshiroRandom;

/// Vanilla's `ImprovedNoise` (3D Perlin noise with permutation table and random origin offset).
#[derive(Debug, Clone)]
pub struct ImprovedNoise {
    pub xo: f64,
    pub yo: f64,
    pub zo: f64,
    p: [u8; 256],
}

const GRADIENT: [[f64; 3]; 16] = [
    [1.0, 1.0, 0.0],
    [-1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0],
    [-1.0, 0.0, 1.0],
    [1.0, 0.0, -1.0],
    [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0],
    [0.0, -1.0, 1.0],
    [0.0, 1.0, -1.0],
    [0.0, -1.0, -1.0],
    [1.0, 1.0, 0.0],
    [0.0, -1.0, 1.0],
    [-1.0, 1.0, 0.0],
    [0.0, -1.0, -1.0],
];

#[inline]
fn smoothstep(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

#[inline]
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

#[inline]
fn grad_dot(hash: u8, x: f64, y: f64, z: f64) -> f64 {
    let g = GRADIENT[(hash & 15) as usize];
    g[0] * x + g[1] * y + g[2] * z
}

impl ImprovedNoise {
    pub fn new(rng: &mut XoroshiroRandom) -> Self {
        let xo = rng.next_f64() * 256.0;
        let yo = rng.next_f64() * 256.0;
        let zo = rng.next_f64() * 256.0;
        let mut p = [0u8; 256];
        for (i, slot) in p.iter_mut().enumerate() {
            *slot = i as u8;
        }
        for i in 0..256 {
            let j = rng.next_i32_bound((256 - i) as i32) as usize + i;
            p.swap(i, j);
        }
        Self { xo, yo, zo, p }
    }

    #[inline]
    fn p(&self, i: i32) -> u8 {
        self.p[(i & 255) as usize]
    }

    pub fn sample(&self, x: f64, y: f64, z: f64) -> f64 {
        self.sample_with_y(x, y, z, 0.0, 0.0)
    }

    /// Vanilla's `ImprovedNoise.noise(x, y, z, yScale, yMax)` supporting Y-smearing for `BlendedNoise`.
    pub fn sample_with_y(&self, x: f64, y: f64, z: f64, y_scale: f64, y_max: f64) -> f64 {
        let xr = x + self.xo;
        let yr = y + self.yo;
        let zr = z + self.zo;

        let xf = xr.floor();
        let yf = yr.floor();
        let zf = zr.floor();

        let xi = xf as i32;
        let yi = yf as i32;
        let zi = zf as i32;

        let dx = xr - xf;
        let dy = yr - yf;
        let dz = zr - zf;

        let dy_smear = if y_scale != 0.0 {
            let f = if y_max >= 0.0 && y_max < dy {
                y_max
            } else {
                dy
            };
            (f / y_scale + 1.0e-7).floor() * y_scale
        } else {
            0.0
        };
        let dy_grad = dy - dy_smear;

        let u = smoothstep(dx);
        let v = smoothstep(dy);
        let w = smoothstep(dz);

        let a = self.p(xi) as i32;
        let aa = self.p(a + yi) as i32;
        let ab = self.p(a + yi + 1) as i32;
        let b = self.p(xi + 1) as i32;
        let ba = self.p(b + yi) as i32;
        let bb = self.p(b + yi + 1) as i32;

        let x00 = grad_dot(self.p(aa + zi), dx, dy_grad, dz);
        let x10 = grad_dot(self.p(ba + zi), dx - 1.0, dy_grad, dz);
        let x01 = grad_dot(self.p(ab + zi), dx, dy_grad - 1.0, dz);
        let x11 = grad_dot(self.p(bb + zi), dx - 1.0, dy_grad - 1.0, dz);

        let y00 = grad_dot(self.p(aa + zi + 1), dx, dy_grad, dz - 1.0);
        let y10 = grad_dot(self.p(ba + zi + 1), dx - 1.0, dy_grad, dz - 1.0);
        let y01 = grad_dot(self.p(ab + zi + 1), dx, dy_grad - 1.0, dz - 1.0);
        let y11 = grad_dot(self.p(bb + zi + 1), dx - 1.0, dy_grad - 1.0, dz - 1.0);

        let l0 = lerp(u, x00, x10);
        let l1 = lerp(u, x01, x11);
        let l2 = lerp(u, y00, y10);
        let l3 = lerp(u, y01, y11);

        let m0 = lerp(v, l0, l1);
        let m1 = lerp(v, l2, l3);

        lerp(w, m0, m1)
    }
}

/// Vanilla's `PerlinNoise` (multi-octave `ImprovedNoise`).
#[derive(Debug, Clone)]
pub struct PerlinNoise {
    octaves: Vec<Option<ImprovedNoise>>,
    amplitudes: Vec<f64>,
    lowest_freq_input_factor: f64,
    lowest_freq_value_factor: f64,
}

impl PerlinNoise {
    pub fn new(rng: &mut XoroshiroRandom, first_octave: i32, amplitudes: &[f64]) -> Self {
        let factory = rng.fork_positional();
        let mut octaves = Vec::with_capacity(amplitudes.len());
        for (i, &amp) in amplitudes.iter().enumerate() {
            if amp != 0.0 {
                let octave_idx = first_octave + i as i32;
                let mut oct_rng = factory.from_hash_of(&format!("octave_{octave_idx}"));
                octaves.push(Some(ImprovedNoise::new(&mut oct_rng)));
            } else {
                octaves.push(None);
            }
        }
        let lowest_freq_input_factor = 2.0_f64.powi(first_octave);
        let lowest_freq_value_factor = 2.0_f64.powi(amplitudes.len() as i32 - 1)
            / (2.0_f64.powi(amplitudes.len() as i32) - 1.0);
        Self {
            octaves,
            amplitudes: amplitudes.to_vec(),
            lowest_freq_input_factor,
            lowest_freq_value_factor,
        }
    }

    /// Vanilla's legacy `PerlinNoise` initialization used by `BlendedNoise` (`old_blended_noise`),
    /// which samples `ImprovedNoise` octaves sequentially from highest frequency (`0`) down to `first_octave`.
    pub fn new_legacy_for_blended(
        rng: &mut XoroshiroRandom,
        first_octave: i32,
        count: usize,
    ) -> Self {
        let mut octaves = vec![None; count];
        if count > 0 {
            octaves[count - 1] = Some(ImprovedNoise::new(rng));
            for i in (0..count - 1).rev() {
                octaves[i] = Some(ImprovedNoise::new(rng));
            }
        }
        let lowest_freq_input_factor = 2.0_f64.powi(first_octave);
        let lowest_freq_value_factor =
            2.0_f64.powi(count as i32 - 1) / (2.0_f64.powi(count as i32) - 1.0);
        Self {
            octaves,
            amplitudes: vec![1.0; count],
            lowest_freq_input_factor,
            lowest_freq_value_factor,
        }
    }

    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        let mut value = 0.0;
        let mut input_factor = self.lowest_freq_input_factor;
        let mut value_factor = self.lowest_freq_value_factor;

        for (octave, &amp) in self.octaves.iter().zip(self.amplitudes.iter()) {
            if let Some(noise) = octave {
                let nx = wrap(x * input_factor);
                let ny = wrap(y * input_factor);
                let nz = wrap(z * input_factor);
                value += amp * noise.sample(nx, ny, nz) * value_factor;
            }
            input_factor *= 2.0;
            value_factor *= 0.5;
        }
        value
    }
}

/// Vanilla's `PerlinNoise.wrap(double)` to keep coordinates within `[-16777216, 16777216]`.
#[inline]
fn wrap(v: f64) -> f64 {
    v - (v / 33_554_432.0 + 0.5).floor() * 33_554_432.0
}

/// Vanilla's `NormalNoise` (`DoublePerlinNoiseSampler`).
#[derive(Debug, Clone)]
pub struct NormalNoise {
    value_factor: f64,
    first: PerlinNoise,
    second: PerlinNoise,
}

impl NormalNoise {
    pub fn new(rng: &mut XoroshiroRandom, first_octave: i32, amplitudes: &[f64]) -> Self {
        let first = PerlinNoise::new(rng, first_octave, amplitudes);
        let second = PerlinNoise::new(rng, first_octave, amplitudes);

        let mut min_nonzero = usize::MAX;
        let mut max_nonzero = 0usize;
        for (i, &amp) in amplitudes.iter().enumerate() {
            if amp != 0.0 {
                min_nonzero = min_nonzero.min(i);
                max_nonzero = max_nonzero.max(i);
            }
        }
        let span = if min_nonzero <= max_nonzero {
            (max_nonzero - min_nonzero) as f64
        } else {
            0.0
        };
        let expected_deviation = 0.1 * (1.0 + 1.0 / (span + 1.0));
        let value_factor = (1.0 / 6.0) / expected_deviation;

        Self {
            value_factor,
            first,
            second,
        }
    }

    /// Creates a `NormalNoise` matching Minecraft 26.3's `NormalNoise(Parameters)` formula:
    /// octaves have `amp_i = base_amplitude * (2^(n-1)/(2^n-1)) * 0.5^i * mod_i`, and
    /// `normalizationFactor = (sum_amps / 3) / (sqrt(sum((0.2702247831245211 * |amp_i|)^2)) * sqrt(2))`.
    pub fn with_base_amplitude(
        rng: &mut XoroshiroRandom,
        first_octave: i32,
        amplitudes: &[f64],
        base_amplitude: f64,
    ) -> Self {
        let mut n = Self::new(rng, first_octave, amplitudes);
        let count = amplitudes.len();
        if count == 0 {
            return n;
        }
        let lfvf = 2.0_f64.powi(count as i32 - 1) / (2.0_f64.powi(count as i32) - 1.0);
        let mut amp_step = base_amplitude * lfvf;
        let mut sum_amps = 0.0_f64;
        let mut sum_sq = 0.0_f64;
        for &m in amplitudes {
            if m != 0.0 {
                let oct_amp = amp_step * m;
                sum_amps += oct_amp;
                let dev = 0.270_224_783_124_521_1 * oct_amp.abs();
                sum_sq += dev * dev;
            }
            amp_step *= 0.5;
        }
        let est_dev = sum_sq.sqrt();
        if est_dev > 0.0 {
            n.first.lowest_freq_value_factor = base_amplitude * lfvf;
            n.second.lowest_freq_value_factor = base_amplitude * lfvf;
            n.value_factor = (sum_amps / 3.0) / (est_dev * std::f64::consts::SQRT_2);
        }
        n
    }

    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        let x2 = x * 1.018_126_888_217_522_7;
        let y2 = y * 1.018_126_888_217_522_7;
        let z2 = z * 1.018_126_888_217_522_7;
        (self.first.get_value(x, y, z) + self.second.get_value(x2, y2, z2)) * self.value_factor
    }
}

/// Vanilla's `BlendedNoise` (`minecraft:old_blended_noise`).
#[derive(Debug, Clone)]
pub struct BlendedNoise {
    min_limit_noise: PerlinNoise,
    max_limit_noise: PerlinNoise,
    main_noise: PerlinNoise,
    xz_multiplier: f64,
    y_multiplier: f64,
    xz_factor: f64,
    y_factor: f64,
    smear_scale_multiplier: f64,
}

impl BlendedNoise {
    pub fn new(
        rng: &mut XoroshiroRandom,
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    ) -> Self {
        let min_limit_noise = PerlinNoise::new_legacy_for_blended(rng, -15, 16);
        let max_limit_noise = PerlinNoise::new_legacy_for_blended(rng, -15, 16);
        let main_noise = PerlinNoise::new_legacy_for_blended(rng, -7, 8);
        Self {
            min_limit_noise,
            max_limit_noise,
            main_noise,
            xz_multiplier: 684.412 * xz_scale,
            y_multiplier: 684.412 * y_scale,
            xz_factor,
            y_factor,
            smear_scale_multiplier,
        }
    }

    pub fn sample(&self, x: i32, y: i32, z: i32) -> f64 {
        let d0 = x as f64 * self.xz_multiplier;
        let d1 = y as f64 * self.y_multiplier;
        let d2 = z as f64 * self.xz_multiplier;
        let d3 = d0 / self.xz_factor;
        let d4 = d1 / self.y_factor;
        let d5 = d2 / self.xz_factor;
        let d6 = self.y_multiplier * self.smear_scale_multiplier;
        let d7 = d6 / self.y_factor;

        let mut d8 = 0.0;
        let mut d9 = 0.0;
        let mut d10 = 0.0;
        let mut d11 = 1.0;

        for oct in self.main_noise.octaves.iter().rev().take(8) {
            if let Some(oct) = oct {
                let sample = oct.sample_with_y(
                    wrap(d3 * d11),
                    wrap(d4 * d11),
                    wrap(d5 * d11),
                    d7 * d11,
                    d4 * d11,
                );
                d10 += sample / d11;
            }
            d11 /= 2.0;
        }

        let d21 = (d10 / 10.0 + 1.0) / 2.0;
        let flag1 = d21 >= 1.0;
        let flag2 = d21 <= 0.0;
        let mut d12 = 1.0;

        for j in 0..16 {
            let d13 = wrap(d0 * d12);
            let d14 = wrap(d1 * d12);
            let d15 = wrap(d2 * d12);
            if !flag1 && let Some(oct) = &self.min_limit_noise.octaves[15 - j] {
                d8 += oct.sample_with_y(d13, d14, d15, d6 * d12, d14) / d12;
            }
            if !flag2 && let Some(oct) = &self.max_limit_noise.octaves[15 - j] {
                d9 += oct.sample_with_y(d13, d14, d15, d6 * d12, d14) / d12;
            }
            d12 /= 2.0;
        }

        let clamped = lerp(d21.clamp(0.0, 1.0), d8 / 512.0, d9 / 512.0);
        clamped / 128.0
    }
}

/// Vanilla 2D `SimplexNoise` used by End outer island generation (`EndIslandDensityFunction`).
#[derive(Debug, Clone)]
pub struct SimplexNoise {
    p: [u8; 256],
}

impl SimplexNoise {
    pub fn new(rng: &mut XoroshiroRandom) -> Self {
        let _xo = rng.next_f64() * 256.0;
        let _yo = rng.next_f64() * 256.0;
        let _zo = rng.next_f64() * 256.0;
        let mut p = [0u8; 256];
        for (i, slot) in p.iter_mut().enumerate() {
            *slot = i as u8;
        }
        for i in 0..256 {
            let j = rng.next_i32_bound((256 - i) as i32) as usize + i;
            p.swap(i, j);
        }
        Self { p }
    }

    #[inline]
    fn p(&self, i: i32) -> i32 {
        self.p[(i & 255) as usize] as i32
    }

    fn corner_noise(&self, g: i32, x: f64, y: f64) -> f64 {
        let t = 0.5 - x * x - y * y;
        if t < 0.0 {
            0.0
        } else {
            let t2 = t * t;
            let grad = GRADIENT[(g & 15) as usize];
            t2 * t2 * (grad[0] * x + grad[1] * y)
        }
    }

    pub fn sample_2d(&self, xin: f64, yin: f64) -> f64 {
        const F2: f64 = 0.366_025_403_784_438_6;
        const G2: f64 = 0.211_324_865_405_187_13;

        let s = (xin + yin) * F2;
        let i = (xin + s).floor() as i32;
        let j = (yin + s).floor() as i32;
        let t = (i + j) as f64 * G2;
        let x0 = xin - (i as f64 - t);
        let y0 = yin - (j as f64 - t);

        let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };
        let x1 = x0 - i1 as f64 + G2;
        let y1 = y0 - j1 as f64 + G2;
        let x2 = x0 - 1.0 + 2.0 * G2;
        let y2 = y0 - 1.0 + 2.0 * G2;

        let gi0 = self.p(i + self.p(j)) % 12;
        let gi1 = self.p(i + i1 + self.p(j + j1)) % 12;
        let gi2 = self.p(i + 1 + self.p(j + 1)) % 12;

        let n0 = self.corner_noise(gi0, x0, y0);
        let n1 = self.corner_noise(gi1, x1, y1);
        let n2 = self.corner_noise(gi2, x2, y2);
        70.0 * (n0 + n1 + n2)
    }

    /// Vanilla `EndIslandDensityFunction.getHeightValue` returning density in `[-0.84375, 0.5625]`.
    pub fn end_island_density(&self, block_x: i32, block_z: i32) -> f64 {
        let i = block_x.div_euclid(8);
        let j = block_z.div_euclid(8);
        let k = block_x.rem_euclid(8);
        let l = block_z.rem_euclid(8);
        let mut f = 100.0_f64 - ((k * k + l * l) as f64).sqrt() * 8.0;
        if (i as i64) * (i as i64) + (j as i64) * (j as i64) <= 4096 {
            f = f.clamp(-100.0, 80.0);
        } else {
            f = -100.0;
        }
        for dx in -12..=12 {
            for dz in -12..=12 {
                let cx = (i + dx) as i64;
                let cz = (j + dz) as i64;
                if cx * cx + cz * cz > 4096 && self.sample_2d(cx as f64, cz as f64) < -0.9 {
                    let size = ((cx.abs() * 3439 + cz.abs() * 147).rem_euclid(13) + 9) as f64;
                    let ddx = (k - dx * 2) as f64;
                    let ddz = (l - dz * 2) as f64;
                    let island =
                        (100.0 - (ddx * ddx + ddz * ddz).sqrt() * size).clamp(-100.0, 80.0);
                    f = f.max(island);
                }
            }
        }
        (f - 8.0) / 128.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_noise_is_bounded_and_deterministic() {
        let mut rng1 = XoroshiroRandom::from_seed(42);
        let mut rng2 = XoroshiroRandom::from_seed(42);
        let n1 = NormalNoise::new(&mut rng1, -7, &[1.0, 1.0, 0.5]);
        let n2 = NormalNoise::new(&mut rng2, -7, &[1.0, 1.0, 0.5]);
        for i in 0..10 {
            let v1 = n1.get_value(i as f64 * 13.5, 64.0, i as f64 * -7.25);
            let v2 = n2.get_value(i as f64 * 13.5, 64.0, i as f64 * -7.25);
            assert_eq!(v1, v2);
            assert!(v1.abs() < 2.5, "unexpectedly large noise value: {v1}");
        }
    }
}
