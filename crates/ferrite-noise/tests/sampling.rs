//! Regression coverage that runs without Java or the vanilla server jar.
//!
//! The `*_BITS` tables were dumped from the 26.3 server jar by
//! `tests/java/NoiseReference.java` (the `sampling_regressions` section), so they are
//! independent of this crate's implementation. `REGRESSION_COORDS` and
//! `REGRESSION_BLOCK_COORDS` mirror the harness and include lattice points, negative
//! coordinates and both sides of the wrap-window boundary.
//!
//! The remaining tests pin properties the port must preserve exactly: zero-offset
//! sampling at the origin, wrap-window folding, order-independent repeated sampling and
//! reuse of one sampler instance (or one positional factory) across many positions.

use ferrite_noise::noise::{Noise, NormalNoise};
use ferrite_noise::{
    BlendedNoise, LegacyPositionalRandomFactory, LegacyRandomSource, PerlinNoise,
    PositionalRandomFactory, SimplexNoise, SmearedPerlinNoise, XoroshiroPositionalRandomFactory,
    XoroshiroRandomSource,
};

/// Half of `GradientNoise.wrap`'s 2^25 round-off step, i.e. the radius of the window
/// that wrapped coordinates land in.
const HALF_WINDOW: f64 = 16_777_216.0;
/// `GradientNoise.wrap`'s round-off step (`2^25`).
const ROUND_OFF: f64 = 33_554_432.0;

/// Must stay in sync with `NoiseReference.REGRESSION_COORDS`.
const REGRESSION_COORDS: [[f64; 3]; 12] = [
    [0.0, 0.0, 0.0],
    [-1.0, -1.0, -1.0],
    [-17.0, -64.0, -33.0],
    [-1_000_000.0, 0.0, 1_000_000.0],
    [-16_777_216.0, 5.0, -16_777_216.0],
    [16_777_215.5, 0.0, -16_777_215.5],
    [16_777_216.0, 1.25, -16_777_216.0],
    [16_777_216.5, -0.5, 33_554_432.0],
    [-33_554_432.5, 0.0, 25_165_824.0],
    [0.5, -0.5, 0.5],
    [100.25, -30.75, -400.125],
    [25_165_825.5, -25_165_826.5, -12.0],
];

/// Must stay in sync with the block coordinates in `NoiseReference.blendedAtField`.
const REGRESSION_BLOCK_COORDS: [[i32; 3]; 12] = [
    [0, 0, 0],
    [-1, -1, -1],
    [-17, -64, -33],
    [-1_000_000, 0, 1_000_000],
    [-16_777_216, 5, -16_777_216],
    [16_777_215, 0, -16_777_215],
    [16_777_216, 1, -16_777_216],
    [16_777_216, -1, 33_554_432],
    [-33_554_432, 0, 25_165_824],
    [1, -1, 1],
    [100, -30, -400],
    [25_165_825, -25_165_826, -12],
];

/// `f64::from(PerlinNoise::get(...))` bit patterns from the Java harness.
const PERLIN_BITS: [u64; 12] = [
    0x3fd2359060000000,
    0xbf97057c00000000,
    0x3fcdab7340000000,
    0x3fdb3a3120000000,
    0x3fd1aa5ec0000000,
    0x3fcdae6b00000000,
    0x3fe171c700000000,
    0xbfc652a640000000,
    0x3fd9030540000000,
    0x3fb2cd8b00000000,
    0xbf99366400000000,
    0x3fa12b62c0000000,
];

/// `f64::from(SmearedPerlinNoise::get(...))` bit patterns from the Java harness.
const SMEARED_BITS: [u64; 12] = [
    0xbf7e830000000000,
    0xbfae25d600000000,
    0xbf8cfea800000000,
    0x3fc94cb080000000,
    0xbfcacd8a80000000,
    0xbfe1253700000000,
    0xbf85a6e400000000,
    0xbfd7bd1c80000000,
    0xbfc97e2280000000,
    0xbf95528600000000,
    0x3fd4d8b5a0000000,
    0xbfa47a0400000000,
];

/// `f64::from(SimplexNoise::get_2d(...))` bit patterns from the Java harness.
const SIMPLEX_2D_BITS: [u64; 12] = [
    0xbf8c81c5c0000000,
    0x3fe5ddd380000000,
    0x3fdbb4b680000000,
    0xbfd60ad2a0000000,
    0x3fdbb66f40000000,
    0xbfdecc09a0000000,
    0xbf8c81c660000000,
    0xbfe412b8a0000000,
    0xbfc1acd080000000,
    0x3fda9c21a0000000,
    0xbfbe626a00000000,
    0x3fe3077160000000,
];

/// `f64::from(SimplexNoise::get_3d(...))` bit patterns from the Java harness.
const SIMPLEX_3D_BITS: [u64; 12] = [
    0x3fd9506b60000000,
    0xbfd94794c0000000,
    0xbfc5ab7a20000000,
    0xbfc3a3d060000000,
    0x3fe1c98980000000,
    0xbfc0993480000000,
    0x3fd36ae400000000,
    0x3fc798fb40000000,
    0xbfdffd1c80000000,
    0xbfae371640000000,
    0x3fe2b84960000000,
    0x3fd04559a0000000,
];

/// NormalNoise::create(seed 3456) bit patterns from the Java harness.
const NORMAL_BITS: [u64; 12] = [
    0x3fa39e0880000000,
    0x3fa360ed00000000,
    0xbfc784dda0000000,
    0x3fa092db40000000,
    0xbfd68b2c20000000,
    0xbfd6d39720000000,
    0xbfd6da9900000000,
    0xbfc0e402e0000000,
    0x3f72dc1f00000000,
    0x3fa62b6de0000000,
    0x3fa97433a0000000,
    0xbfb144bee0000000,
];

/// `create_for_legacy_nether_biome` (seed 7890) bit patterns from the Java harness.
const LEGACY_NORMAL_BITS: [u64; 12] = [
    0xbfcb3bfbc0000000,
    0xbfc0e52ba0000000,
    0xbfacb1cbe0000000,
    0x3fc2a7f5a0000000,
    0xbfb1596940000000,
    0x3f914266e0000000,
    0xbfa63ebe20000000,
    0x3fda295180000000,
    0x3fd73b38c0000000,
    0xbfca90ae00000000,
    0x3fb5167e40000000,
    0x3fb26bfe20000000,
];

/// `BlendedNoise::compile_sampler(seed 2468)` f32 bit patterns from the Java harness.
const BLENDED_BITS: [u32; 12] = [
    0xbe78db3f, 0xbea5558e, 0x3eabf8b0, 0xbd70113e, 0x3da6d174, 0xbddb6bfe, 0xbd3e1de7, 0x3eaca607,
    0x3e8cad96, 0xbdf370b4, 0xbed7e07c, 0xbe2372c0,
];

fn perlin() -> PerlinNoise {
    PerlinNoise::new(&mut LegacyRandomSource::new(1234))
}

fn smeared() -> SmearedPerlinNoise {
    SmearedPerlinNoise::new(&mut LegacyRandomSource::new(5678), 1.5)
}

fn simplex() -> SimplexNoise {
    SimplexNoise::new(&mut LegacyRandomSource::new(9012))
}

fn normal_noise_parameters() -> NormalNoise {
    NormalNoise::builder()
        .set_base_amplitude(1.0)
        .set_base_octave(-8)
        .set_octave_count(5)
        .set_normalize(true)
        .set_amplitude_modifier(1, 0.5)
        .set_amplitude_modifier(3, 0.0)
        .build()
}

fn legacy_normal_noise_parameters() -> NormalNoise {
    NormalNoise::builder()
        .set_base_amplitude(1.0)
        .set_base_octave(-6)
        .set_octave_count(4)
        .set_legacy_normalization()
        .build()
}

fn blended() -> BlendedNoise {
    BlendedNoise::new(1.0, 1.0, 80.0, 160.0, 8.0)
}

#[track_caller]
fn assert_repeated(name: &str, expected: &[u64], mut sample: impl FnMut([f64; 3]) -> u64) {
    let mut first = [0_u64; REGRESSION_COORDS.len()];
    for pass in 0..3 {
        for (index, coordinate) in REGRESSION_COORDS.iter().enumerate() {
            let bits = sample(*coordinate);
            if pass == 0 {
                first[index] = bits;
            }
            assert_eq!(
                bits, expected[index],
                "{name} at {coordinate:?}: {bits:#018x} != Java {:#018x}",
                expected[index]
            );
            assert_eq!(
                bits, first[index],
                "{name} at {coordinate:?} changed between sampling passes"
            );
        }
    }
    // Reverse order exercises reuse: no hidden per-instance cache may change a sample.
    for (index, coordinate) in REGRESSION_COORDS.iter().enumerate().rev() {
        assert_eq!(
            sample(*coordinate),
            first[index],
            "{name} at {coordinate:?} changed when sampled in reverse order"
        );
    }
}

#[test]
fn java_reference_matches_for_negative_and_boundary_coordinates() {
    let perlin = perlin();
    let smeared = smeared();
    let simplex = simplex();
    let mut normal_random = XoroshiroRandomSource::new(3456);
    let normal_noise = normal_noise_parameters().create(&mut normal_random);
    let mut legacy_random = LegacyRandomSource::new(7890);
    let legacy_noise =
        legacy_normal_noise_parameters().create_for_legacy_nether_biome(&mut legacy_random);
    let blended = blended().compile_sampler(&mut XoroshiroRandomSource::new(2468));

    for (index, [x, y, z]) in REGRESSION_COORDS.iter().enumerate() {
        assert_eq!(
            f64::from(perlin.get(*x, *y, *z)).to_bits(),
            PERLIN_BITS[index],
            "perlin at {x}, {y}, {z}"
        );
        assert_eq!(
            f64::from(smeared.get(*x, *y, *z)).to_bits(),
            SMEARED_BITS[index],
            "smeared perlin at {x}, {y}, {z}"
        );
        assert_eq!(
            f64::from(simplex.get_2d(*x, *z)).to_bits(),
            SIMPLEX_2D_BITS[index],
            "simplex 2d at {x}, {z}"
        );
        assert_eq!(
            f64::from(simplex.get(*x, *y, *z)).to_bits(),
            SIMPLEX_3D_BITS[index],
            "simplex 3d at {x}, {y}, {z}"
        );
        assert_eq!(
            f64::from(normal_noise.get(*x, *y, *z)).to_bits(),
            NORMAL_BITS[index],
            "normal noise at {x}, {y}, {z}"
        );
        assert_eq!(
            f64::from(legacy_noise.get(*x, *y, *z)).to_bits(),
            LEGACY_NORMAL_BITS[index],
            "legacy normal noise at {x}, {y}, {z}"
        );
    }

    for (index, [x, y, z]) in REGRESSION_BLOCK_COORDS.iter().enumerate() {
        assert_eq!(
            blended.sample_value(*x, *y, *z).to_bits(),
            BLENDED_BITS[index],
            "blended noise at {x}, {y}, {z}"
        );
    }
}

#[test]
fn repeated_sampling_is_bitwise_stable() {
    let perlin = perlin();
    assert_repeated("perlin", &PERLIN_BITS, |[x, y, z]| {
        f64::from(perlin.get(x, y, z)).to_bits()
    });

    let smeared = smeared();
    assert_repeated("smeared perlin", &SMEARED_BITS, |[x, y, z]| {
        f64::from(smeared.get(x, y, z)).to_bits()
    });

    let simplex = simplex();
    assert_repeated("simplex 2d", &SIMPLEX_2D_BITS, |[x, _, z]| {
        f64::from(simplex.get_2d(x, z)).to_bits()
    });
    assert_repeated("simplex 3d", &SIMPLEX_3D_BITS, |[x, y, z]| {
        f64::from(simplex.get(x, y, z)).to_bits()
    });

    let mut normal_random = XoroshiroRandomSource::new(3456);
    let normal_noise = normal_noise_parameters().create(&mut normal_random);
    assert_repeated("normal noise", &NORMAL_BITS, |[x, y, z]| {
        f64::from(normal_noise.get(x, y, z)).to_bits()
    });

    let mut legacy_random = LegacyRandomSource::new(7890);
    let legacy_noise =
        legacy_normal_noise_parameters().create_for_legacy_nether_biome(&mut legacy_random);
    assert_repeated("legacy normal noise", &LEGACY_NORMAL_BITS, |[x, y, z]| {
        f64::from(legacy_noise.get(x, y, z)).to_bits()
    });
}

#[test]
fn blended_noise_reuse_matches_the_java_reference() {
    let sampler = blended().compile_sampler(&mut XoroshiroRandomSource::new(2468));
    let mut first = [0_u32; REGRESSION_BLOCK_COORDS.len()];
    for pass in 0..3 {
        for (index, [x, y, z]) in REGRESSION_BLOCK_COORDS.iter().enumerate() {
            let bits = sampler.sample_value(*x, *y, *z).to_bits();
            if pass == 0 {
                first[index] = bits;
            }
            assert_eq!(bits, BLENDED_BITS[index], "blended noise at {x}, {y}, {z}");
            assert_eq!(
                bits, first[index],
                "blended noise at {x}, {y}, {z} changed between sampling passes"
            );
        }
    }
    let fresh = blended().compile_sampler(&mut XoroshiroRandomSource::new(2468));
    for (index, [x, y, z]) in REGRESSION_BLOCK_COORDS.iter().enumerate() {
        assert_eq!(
            fresh.sample_value(*x, *y, *z).to_bits(),
            first[index],
            "fresh blended sampler at {x}, {y}, {z}"
        );
    }
}
#[test]
fn zero_offset_legacy_simplex_samples_the_origin_as_exact_zero() {
    // `new_legacy(_, true)` zeroes the coordinate offsets, so the origin really is a
    // lattice point. Every Simplex corner either has a zero gradient dot product or a
    // negative distance contribution there, so the result must be exactly +0.0.
    // (The ordinary constructor adds offsets, so integer inputs are *not* lattice
    // points and must not be expected to return zero.)
    let simplex = SimplexNoise::new_legacy(&mut LegacyRandomSource::new(9012), true);
    assert_eq!(simplex.get_2d(0.0, 0.0).to_bits(), 0);
    assert_eq!(simplex.get(0.0, 0.0, 0.0).to_bits(), 0);
}

#[test]
fn perlin_folds_large_coordinates_into_the_wrap_window() {
    let perlin = perlin();
    // Hand-computed `GradientNoise.wrap` results; `wrap` subtracts
    // floor(x / 2^25 + 0.5) * 2^25.
    let pairs: [(f64, f64); 8] = [
        (16_777_215.5, 16_777_215.5),
        (16_777_216.0, -16_777_216.0),
        (33_554_432.0, 0.0),
        (33_554_432.5, 0.5),
        (-33_554_432.5, -0.5),
        (50_331_648.0, -16_777_216.0),
        (25_165_825.5, -8_388_606.5),
        (-25_165_826.5, 8_388_605.5),
    ];
    for (raw, folded) in pairs {
        assert!(
            folded.abs() <= HALF_WINDOW,
            "test table folds {raw} outside the window"
        );
        assert_eq!(
            perlin.get(raw, 3.25, -7.5).to_bits(),
            perlin.get(folded, 3.25, -7.5).to_bits(),
            "x = {raw} should fold to {folded}"
        );
        assert_eq!(
            perlin.get(3.25, raw, -7.5).to_bits(),
            perlin.get(3.25, folded, -7.5).to_bits(),
            "y = {raw} should fold to {folded}"
        );
        assert_eq!(
            perlin.get(3.25, -7.5, raw).to_bits(),
            perlin.get(3.25, -7.5, folded).to_bits(),
            "z = {raw} should fold to {folded}"
        );
    }
}

#[test]
fn perlin_aliases_once_per_round_off_inside_the_window() {
    let perlin = perlin();
    // Coordinates inside the window are used as-is, so shifting by one 2^25
    // round-off step must fold back to the same sample.
    for x in [
        0.0,
        0.5,
        -0.5,
        12_345.75,
        -12_345.75,
        16_000_000.25,
        -16_000_000.25,
    ] {
        let base = perlin.get(x, 1.5, -2.5).to_bits();
        assert_eq!(
            perlin.get(x + ROUND_OFF, 1.5, -2.5).to_bits(),
            base,
            "x = {x} + 2^25"
        );
        assert_eq!(
            perlin.get(x - ROUND_OFF, 1.5, -2.5).to_bits(),
            base,
            "x = {x} - 2^25"
        );
    }
}

#[test]
fn sampler_reuse_is_order_independent_and_matches_fresh_instances() {
    let parameters = normal_noise_parameters();
    let reused = parameters.create(&mut XoroshiroRandomSource::new(3456));
    let forward: Vec<u64> = REGRESSION_COORDS
        .iter()
        .map(|[x, y, z]| f64::from(reused.get(*x, *y, *z)).to_bits())
        .collect();
    assert_eq!(forward, NORMAL_BITS, "fresh sampler differs from Java");

    // Creating another stack must not disturb the first one, and vice versa.
    let fresh = parameters.create(&mut XoroshiroRandomSource::new(3456));
    for (index, [x, y, z]) in REGRESSION_COORDS.iter().enumerate().rev() {
        assert_eq!(
            f64::from(reused.get(*x, *y, *z)).to_bits(),
            forward[index],
            "reused sampler at {x}, {y}, {z}"
        );
        assert_eq!(
            f64::from(fresh.get(*x, *y, *z)).to_bits(),
            forward[index],
            "second sampler at {x}, {y}, {z}"
        );
    }
}

#[test]
fn positional_factories_can_be_reused_for_the_same_position() {
    let xoroshiro =
        XoroshiroPositionalRandomFactory::new(0x0123_4567_89AB_CDEF, 0x0FED_CBA9_8765_4321);
    let first = xoroshiro.at(123, -64, -456).next_double().to_bits();
    let second = xoroshiro.at(123, -64, -456).next_double().to_bits();
    assert_eq!(first, second, "xoroshiro at() is not reusable");

    let hash_first = xoroshiro
        .from_hash_of("minecraft:aquifer")
        .next_double()
        .to_bits();
    let hash_second = xoroshiro
        .from_hash_of("minecraft:aquifer")
        .next_double()
        .to_bits();
    assert_eq!(
        hash_first, hash_second,
        "xoroshiro from_hash_of() is not reusable"
    );

    let legacy = LegacyPositionalRandomFactory::new(0x0BAD_F00D_CAFE_BABE);
    let legacy_first = legacy.at(-17, 5, 33).next_double().to_bits();
    let legacy_second = legacy.at(-17, 5, 33).next_double().to_bits();
    assert_eq!(legacy_first, legacy_second, "legacy at() is not reusable");
}
