//! Independently generated regression vectors for the seeded positional random
//! factories (`LegacyPositionalRandomFactory` and `XoroshiroPositionalRandomFactory`).
//!
//! Every constant in this file was emitted by `tests/java/NoiseReference.java` from the
//! local Minecraft 26.3 server jar (`random_regression_legacy` and
//! `random_regression_xoroshiro`); none are hand-computed. The fixture is generated at
//! test time by `reference.rs` when the server jar and Java 25 are available, but the
//! tables below are checked in, so these tests never need either.
//!
//! Coverage: negative and extreme coordinates, distinct named factories, repeatability
//! of `at`/`from_hash_of`, `from_seed` with negative/extreme seeds, and interleaving of
//! factory calls with multiple draws from the returned sources.

use ferrite_noise::{
    LegacyPositionalRandomFactory, PositionalRandomFactory, XoroshiroPositionalRandomFactory,
};

/// Must match `NoiseReference.REG_X`/`REG_Y`/`REG_Z`.
const COORDINATES: [(i32, i32, i32); 6] = [
    (-1_000, -64, -1_500),
    (-17, -1, -33),
    (0, 0, 0),
    (1, 5, 7),
    (12_345, 319, 98_765),
    (i32::MIN, i32::MAX, -1),
];

/// Must match `NoiseReference.REG_NAMES`.
const NAMES: [&str; 6] = [
    "minecraft:aquifer",
    "minecraft:terrain",
    "minecraft:feature_a",
    "ferrite:test",
    "octave_-8",
    "minecraft:overworld",
];

/// Must match `NoiseReference.REG_SEEDS`.
const SEEDS: [i64; 6] = [-1, 0, 1, 123_456_789, i64::MIN, i64::MAX];

/// `LegacyRandomSource.LegacyPositionalRandomFactory(0x0BADF00DCAFEBABE)`.
const LEGACY_SEED: i64 = 0x0BAD_F00D_CAFE_BABE;
/// `XoroshiroPositionalRandomFactory(0x0123456789ABCDEF, 0x0FEDCBA987654321)`.
const XOROSHIRO_SEED: (i64, i64) = (0x0123_4567_89AB_CDEF, 0x0FED_CBA9_8765_4321);

// ---- legacy factory reference tables --------------------------------------

const LEGACY_AT_DOUBLE: [u64; 6] = [
    0x3fb29d7284abd798,
    0x3fcb00e288c6849c,
    0x3fe66092f77f0593,
    0x3fd20feba9f87d46,
    0x3fee7677db8a5835,
    0x3fefec89027e4d6a,
];

const LEGACY_AT_DOUBLE_REPEAT: [u64; 6] = [
    0x3fb29d7284abd798,
    0x3fcb00e288c6849c,
    0x3fe66092f77f0593,
    0x3fd20feba9f87d46,
    0x3fee7677db8a5835,
    0x3fefec89027e4d6a,
];

const LEGACY_AT_INT: [i32; 6] = [
    312_308_414,
    906_085_678,
    -1_291_544_645,
    1_212_133_046,
    -206_323_998,
    -10_205_136,
];

const LEGACY_AT_LONG: [u64; 6] = [
    0x129d72be12af5e7d,
    0x3601c52e463424f8,
    0xb30497baefe0b276,
    0x483faeb59f87d46b,
    0xf3b3bee2714b06b1,
    0xff6448304fc9ad41,
];

const LEGACY_AT_GAUSSIAN: [u64; 6] = [
    0xbfcd423a56d20304,
    0xbff744d9efa8d21d,
    0x3fd87cab0df1732b,
    0xbff6a0485ab37b19,
    0x3ff5bed8ecf749c1,
    0x4004b1324763948a,
];

const LEGACY_NAMED_DOUBLE: [u64; 6] = [
    0x3feb4acc1e6d8630,
    0x3fec0822eb8ef720,
    0x3feb884d579bb49b,
    0x3fcf901d6247e054,
    0x3fb7b9ab255d9fb0,
    0x3fc226d2833a491c,
];

const LEGACY_NAMED_DOUBLE_REPEAT: [u64; 6] = [
    0x3feb4acc1e6d8630,
    0x3fec0822eb8ef720,
    0x3feb884d579bb49b,
    0x3fcf901d6247e054,
    0x3fb7b9ab255d9fb0,
    0x3fc226d2833a491c,
];

const LEGACY_NAMED_INT: [i32; 6] = [
    -631_873_300,
    -532_605_067,
    -599_627_119,
    1_059_076_824,
    398_043_946,
    609_068_341,
];

const LEGACY_SEED_DOUBLE: [u64; 6] = [
    0x3fd1365b2708722c,
    0x3fe764168ea6ca89,
    0x3fe7635aa8cdc4e6,
    0x3fe53fccd61b45a8,
    0x3fe764168ea6ca89,
    0x3fd1365b2708722c,
];

const LEGACY_INTERLEAVED_DOUBLE: [u64; 30] = [
    0x3fb29d7284abd798,
    0x3feb4acc1e6d8630,
    0x3fd1365b2708722c,
    0x3fb29d7284abd798,
    0x3feb4acc1e6d8630,
    0x3fcb00e288c6849c,
    0x3fec0822eb8ef720,
    0x3fe764168ea6ca89,
    0x3fcb00e288c6849c,
    0x3fec0822eb8ef720,
    0x3fe66092f77f0593,
    0x3feb884d579bb49b,
    0x3fe7635aa8cdc4e6,
    0x3fe66092f77f0593,
    0x3feb884d579bb49b,
    0x3fd20feba9f87d46,
    0x3fcf901d6247e054,
    0x3fe53fccd61b45a8,
    0x3fd20feba9f87d46,
    0x3fcf901d6247e054,
    0x3fee7677db8a5835,
    0x3fb7b9ab255d9fb0,
    0x3fe764168ea6ca89,
    0x3fee7677db8a5835,
    0x3fb7b9ab255d9fb0,
    0x3fefec89027e4d6a,
    0x3fc226d2833a491c,
    0x3fd1365b2708722c,
    0x3fefec89027e4d6a,
    0x3fc226d2833a491c,
];

const LEGACY_INTERLEAVED_INT: [i32; 30] = [
    312_308_414,
    -631_873_300,
    1_155_099_827,
    312_308_414,
    -631_873_300,
    906_085_678,
    -532_605_067,
    -1_155_484_576,
    906_085_678,
    -532_605_067,
    -1_291_544_645,
    -599_627_119,
    -1_155_869_325,
    -1_291_544_645,
    -599_627_119,
    1_212_133_046,
    1_059_076_824,
    -1_442_945_365,
    1_212_133_046,
    1_059_076_824,
    -206_323_998,
    398_043_946,
    -1_155_484_576,
    -206_323_998,
    398_043_946,
    -10_205_136,
    609_068_341,
    1_155_099_827,
    -10_205_136,
    609_068_341,
];

const LEGACY_INTERLEAVED_LONG: [u64; 30] = [
    0x129d72be12af5e7d,
    0xda5660ebcdb0c60a,
    0x44d96cb3708722c3,
    0x129d72be12af5e7d,
    0xda5660ebcdb0c60a,
    0x3601c52e463424f8,
    0xe041177571dee40d,
    0xbb20b45fd4d95138,
    0x3601c52e463424f8,
    0xe041177571dee40d,
    0xb30497baefe0b276,
    0xdc426a90f3769374,
    0xbb1ad57319b89cd8,
    0xb30497baefe0b276,
    0xdc426a90f3769374,
    0x483faeb59f87d46b,
    0x3f203ad8123f02ae,
    0xa9fe66aac368b501,
    0x483faeb59f87d46b,
    0x3f203ad8123f02ae,
    0xf3b3bee2714b06b1,
    0x17b9ab2995767ece,
    0xbb20b45fd4d95138,
    0xf3b3bee2714b06b1,
    0x17b9ab2995767ece,
    0xff6448304fc9ad41,
    0x244da53519d248e0,
    0x44d96cb3708722c3,
    0xff6448304fc9ad41,
    0x244da53519d248e0,
];

// ---- xoroshiro factory reference tables -----------------------------------

const XOROSHIRO_AT_DOUBLE: [u64; 6] = [
    0x3fe60d12c965943a,
    0x3fe60d6124414160,
    0x3fc1a2b3c4d5e5f8,
    0x3fe72b756eea8b30,
    0x3fd097d864f9c43a,
    0x3fed2da393856072,
];

const XOROSHIRO_AT_DOUBLE_REPEAT: [u64; 6] = [
    0x3fe60d12c965943a,
    0x3fe60d6124414160,
    0x3fc1a2b3c4d5e5f8,
    0x3fe72b756eea8b30,
    0x3fd097d864f9c43a,
    0x3fed2da393856072,
];

const XOROSHIRO_AT_INT: [i32; 6] = [
    748_803_695,
    168_494_122,
    -1_412_698_095,
    1_415_152_090,
    -418_320_688,
    721_655_528,
];

const XOROSHIRO_AT_LONG: [u64; 6] = [
    0xb068964b2ca1d66f,
    0xb06b09220a0b042a,
    0x23456789abcbf011,
    0xb95bab77545981da,
    0x425f6193e710eed0,
    0xe96d1c9c2b0396e8,
];

const XOROSHIRO_AT_GAUSSIAN: [u64; 6] = [
    0x3fff07be3de5ec99,
    0x3fde3205dcda3d8c,
    0xbfdc442767612738,
    0x3fe6c565e3edb9bf,
    0xbfdd18c41cf762c4,
    0xbff9b5775834c354,
];

const XOROSHIRO_NAMED_DOUBLE: [u64; 6] = [
    0x3fed006c6162fc0b,
    0x3fda4d8dfb7a5b2e,
    0x3fddb60bd399470c,
    0x3fd817d90570b3f4,
    0x3fe1f185e78a5a8f,
    0x3fe004ac20d0112d,
];

const XOROSHIRO_NAMED_DOUBLE_REPEAT: [u64; 6] = [
    0x3fed006c6162fc0b,
    0x3fda4d8dfb7a5b2e,
    0x3fddb60bd399470c,
    0x3fd817d90570b3f4,
    0x3fe1f185e78a5a8f,
    0x3fe004ac20d0112d,
];

const XOROSHIRO_NAMED_INT: [i32; 6] = [
    400_580_238,
    -378_749_651,
    1_696_348_082,
    -1_026_568_098,
    1_389_657_107,
    -2_138_477_937,
];

const XOROSHIRO_SEED_DOUBLE: [u64; 6] = [
    0x3feb97530eca8641,
    0x3fc1a2b3c4d5e5f8,
    0x3fc1a2b3c4d5e3f8,
    0x3fc1a2b1e256a610,
    0x3fe468acf135797e,
    0x3fd72ea61d950c82,
];

const XOROSHIRO_INTERLEAVED_DOUBLE: [u64; 30] = [
    0x3fe60d12c965943a,
    0x3fed006c6162fc0b,
    0x3feb97530eca8641,
    0x3fe60d12c965943a,
    0x3fed006c6162fc0b,
    0x3fe60d6124414160,
    0x3fda4d8dfb7a5b2e,
    0x3fc1a2b3c4d5e5f8,
    0x3fe60d6124414160,
    0x3fda4d8dfb7a5b2e,
    0x3fc1a2b3c4d5e5f8,
    0x3fddb60bd399470c,
    0x3fc1a2b3c4d5e3f8,
    0x3fc1a2b3c4d5e5f8,
    0x3fddb60bd399470c,
    0x3fe72b756eea8b30,
    0x3fd817d90570b3f4,
    0x3fc1a2b1e256a610,
    0x3fe72b756eea8b30,
    0x3fd817d90570b3f4,
    0x3fd097d864f9c43a,
    0x3fe1f185e78a5a8f,
    0x3fe468acf135797e,
    0x3fd097d864f9c43a,
    0x3fe1f185e78a5a8f,
    0x3fed2da393856072,
    0x3fe004ac20d0112d,
    0x3fd72ea61d950c82,
    0x3fed2da393856072,
    0x3fe004ac20d0112d,
];

const XOROSHIRO_INTERLEAVED_INT: [i32; 30] = [
    748_803_695,
    400_580_238,
    1_412_567_021,
    748_803_695,
    400_580_238,
    168_494_122,
    -378_749_651,
    -1_412_698_095,
    168_494_122,
    -378_749_651,
    -1_412_698_095,
    1_696_348_082,
    -1_412_960_240,
    -1_412_698_095,
    1_696_348_082,
    1_415_152_090,
    -1_026_568_098,
    -1_387_519_204,
    1_415_152_090,
    -1_026_568_098,
    -418_320_688,
    1_389_657_107,
    -1_412_698_095,
    -418_320_688,
    1_389_657_107,
    721_655_528,
    -2_138_477_937,
    1_412_567_021,
    721_655_528,
    -2_138_477_937,
];

const XOROSHIRO_INTERLEAVED_LONG: [u64; 30] = [
    0xb068964b2ca1d66f,
    0xe803630b17e05e8e,
    0xdcba987654320fed,
    0xb068964b2ca1d66f,
    0xe803630b17e05e8e,
    0xb06b09220a0b042a,
    0x693637ede96cbd2d,
    0x23456789abcbf011,
    0xb06b09220a0b042a,
    0x693637ede96cbd2d,
    0x23456789abcbf011,
    0x76d82f4e651c37b2,
    0x23456789abc7f010,
    0x23456789abcbf011,
    0x76d82f4e651c37b2,
    0xb95bab77545981da,
    0x605f6415c2cfd05e,
    0x234563c4ad4c231c,
    0xb95bab77545981da,
    0x605f6415c2cfd05e,
    0x425f6193e710eed0,
    0x8f8c2f3c52d47c13,
    0xa3456789abcbf011,
    0x425f6193e710eed0,
    0x8f8c2f3c52d47c13,
    0xe96d1c9c2b0396e8,
    0x8025610680896a8f,
    0x5cba987654320fed,
    0xe96d1c9c2b0396e8,
    0x8025610680896a8f,
];

// ---- sampling helpers -----------------------------------------------------

fn at_doubles(factory: &dyn PositionalRandomFactory) -> Vec<u64> {
    COORDINATES
        .iter()
        .map(|&(x, y, z)| factory.at(x, y, z).next_double().to_bits())
        .collect()
}

fn at_ints(factory: &dyn PositionalRandomFactory) -> Vec<i32> {
    COORDINATES
        .iter()
        .map(|&(x, y, z)| factory.at(x, y, z).next_int())
        .collect()
}

fn at_longs(factory: &dyn PositionalRandomFactory) -> Vec<u64> {
    COORDINATES
        .iter()
        .map(|&(x, y, z)| factory.at(x, y, z).next_long() as u64)
        .collect()
}

fn at_gaussians(factory: &dyn PositionalRandomFactory) -> Vec<u64> {
    COORDINATES
        .iter()
        .map(|&(x, y, z)| factory.at(x, y, z).next_gaussian().to_bits())
        .collect()
}

fn named_doubles(factory: &dyn PositionalRandomFactory) -> Vec<u64> {
    NAMES
        .iter()
        .map(|name| factory.from_hash_of(name).next_double().to_bits())
        .collect()
}

fn named_ints(factory: &dyn PositionalRandomFactory) -> Vec<i32> {
    NAMES
        .iter()
        .map(|name| factory.from_hash_of(name).next_int())
        .collect()
}

fn seed_doubles(factory: &dyn PositionalRandomFactory) -> Vec<u64> {
    SEEDS
        .iter()
        .map(|&seed| factory.from_seed(seed).next_double().to_bits())
        .collect()
}

/// The exact call order of `NoiseReference.interleavedDoubles`.
fn interleaved_doubles(factory: &dyn PositionalRandomFactory) -> Vec<u64> {
    let mut values = Vec::with_capacity(COORDINATES.len() * 5);
    for (index, &(x, y, z)) in COORDINATES.iter().enumerate() {
        values.push(factory.at(x, y, z).next_double().to_bits());
        values.push(factory.from_hash_of(NAMES[index]).next_double().to_bits());
        values.push(factory.from_seed(SEEDS[index]).next_double().to_bits());
        values.push(factory.at(x, y, z).next_double().to_bits());
        values.push(factory.from_hash_of(NAMES[index]).next_double().to_bits());
    }
    values
}

/// The exact call order of `NoiseReference.interleavedInts`.
fn interleaved_ints(factory: &dyn PositionalRandomFactory) -> Vec<i32> {
    let mut values = Vec::with_capacity(COORDINATES.len() * 5);
    for (index, &(x, y, z)) in COORDINATES.iter().enumerate() {
        values.push(factory.at(x, y, z).next_int());
        values.push(factory.from_hash_of(NAMES[index]).next_int());
        values.push(factory.from_seed(SEEDS[index]).next_int());
        values.push(factory.at(x, y, z).next_int());
        values.push(factory.from_hash_of(NAMES[index]).next_int());
    }
    values
}

/// The exact call order of `NoiseReference.interleavedLongs`.
fn interleaved_longs(factory: &dyn PositionalRandomFactory) -> Vec<u64> {
    let mut values = Vec::with_capacity(COORDINATES.len() * 5);
    for (index, &(x, y, z)) in COORDINATES.iter().enumerate() {
        values.push(factory.at(x, y, z).next_long() as u64);
        values.push(factory.from_hash_of(NAMES[index]).next_long() as u64);
        values.push(factory.from_seed(SEEDS[index]).next_long() as u64);
        values.push(factory.at(x, y, z).next_long() as u64);
        values.push(factory.from_hash_of(NAMES[index]).next_long() as u64);
    }
    values
}

fn distinct_count<T: Ord + Clone>(values: &[T]) -> usize {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    sorted.len()
}

#[track_caller]
fn check_interleaving(
    factory: &dyn PositionalRandomFactory,
    expected_doubles: &[u64],
    expected_ints: &[i32],
    expected_longs: &[u64],
) {
    let doubles = interleaved_doubles(factory);
    let ints = interleaved_ints(factory);
    let longs = interleaved_longs(factory);
    assert_eq!(doubles.as_slice(), expected_doubles, "interleaved doubles");
    assert_eq!(ints.as_slice(), expected_ints, "interleaved ints");
    assert_eq!(longs.as_slice(), expected_longs, "interleaved longs");

    // Each interleaved `at`/named/seed call must equal the isolated table entries.
    let at_double = at_doubles(factory);
    let named_double = named_doubles(factory);
    let seed_double = seed_doubles(factory);
    for index in 0..COORDINATES.len() {
        let base = index * 5;
        assert_eq!(
            doubles[base], at_double[index],
            "first interleaved at() call for row {index}"
        );
        assert_eq!(
            doubles[base + 3],
            at_double[index],
            "repeated interleaved at() call for row {index}"
        );
        assert_eq!(
            doubles[base + 1],
            named_double[index],
            "first interleaved named call for row {index}"
        );
        assert_eq!(
            doubles[base + 4],
            named_double[index],
            "repeated interleaved named call for row {index}"
        );
        assert_eq!(
            doubles[base + 2],
            seed_double[index],
            "interleaved from_seed call for row {index}"
        );
    }
}

// ---- tests ----------------------------------------------------------------

#[test]
fn legacy_factory_matches_java_reference() {
    let factory = LegacyPositionalRandomFactory::new(LEGACY_SEED);
    assert_eq!(at_doubles(&factory).as_slice(), LEGACY_AT_DOUBLE);
    assert_eq!(at_ints(&factory).as_slice(), LEGACY_AT_INT);
    assert_eq!(at_longs(&factory).as_slice(), LEGACY_AT_LONG);
    assert_eq!(at_gaussians(&factory).as_slice(), LEGACY_AT_GAUSSIAN);
    assert_eq!(named_doubles(&factory).as_slice(), LEGACY_NAMED_DOUBLE);
    assert_eq!(named_ints(&factory).as_slice(), LEGACY_NAMED_INT);
    assert_eq!(seed_doubles(&factory).as_slice(), LEGACY_SEED_DOUBLE);
}

#[test]
fn xoroshiro_factory_matches_java_reference() {
    let factory = XoroshiroPositionalRandomFactory::new(XOROSHIRO_SEED.0, XOROSHIRO_SEED.1);
    assert_eq!(at_doubles(&factory).as_slice(), XOROSHIRO_AT_DOUBLE);
    assert_eq!(at_ints(&factory).as_slice(), XOROSHIRO_AT_INT);
    assert_eq!(at_longs(&factory).as_slice(), XOROSHIRO_AT_LONG);
    assert_eq!(at_gaussians(&factory).as_slice(), XOROSHIRO_AT_GAUSSIAN);
    assert_eq!(named_doubles(&factory).as_slice(), XOROSHIRO_NAMED_DOUBLE);
    assert_eq!(named_ints(&factory).as_slice(), XOROSHIRO_NAMED_INT);
    assert_eq!(seed_doubles(&factory).as_slice(), XOROSHIRO_SEED_DOUBLE);
}

#[test]
fn negative_and_extreme_coordinates_are_repeatable() {
    let legacy = LegacyPositionalRandomFactory::new(LEGACY_SEED);
    assert_eq!(
        at_doubles(&legacy).as_slice(),
        LEGACY_AT_DOUBLE_REPEAT,
        "legacy at() is not repeatable"
    );
    assert_eq!(
        named_doubles(&legacy).as_slice(),
        LEGACY_NAMED_DOUBLE_REPEAT,
        "legacy from_hash_of() is not repeatable"
    );

    let xoroshiro = XoroshiroPositionalRandomFactory::new(XOROSHIRO_SEED.0, XOROSHIRO_SEED.1);
    assert_eq!(
        at_doubles(&xoroshiro).as_slice(),
        XOROSHIRO_AT_DOUBLE_REPEAT,
        "xoroshiro at() is not repeatable"
    );
    assert_eq!(
        named_doubles(&xoroshiro).as_slice(),
        XOROSHIRO_NAMED_DOUBLE_REPEAT,
        "xoroshiro from_hash_of() is not repeatable"
    );

    // A second factory built from the same seed reproduces every row, so nothing is
    // carried in process-global state either.
    let legacy_again = LegacyPositionalRandomFactory::new(LEGACY_SEED);
    assert_eq!(at_doubles(&legacy_again).as_slice(), LEGACY_AT_DOUBLE);
    assert_eq!(named_doubles(&legacy_again).as_slice(), LEGACY_NAMED_DOUBLE);
    let xoroshiro_again = XoroshiroPositionalRandomFactory::new(XOROSHIRO_SEED.0, XOROSHIRO_SEED.1);
    assert_eq!(at_doubles(&xoroshiro_again).as_slice(), XOROSHIRO_AT_DOUBLE);
    assert_eq!(
        named_doubles(&xoroshiro_again).as_slice(),
        XOROSHIRO_NAMED_DOUBLE
    );

    // The coordinate rows (including i32::MIN/i32::MAX) must map to different streams.
    assert_eq!(distinct_count(&at_doubles(&legacy)), COORDINATES.len());
    assert_eq!(distinct_count(&at_doubles(&xoroshiro)), COORDINATES.len());
}

#[test]
fn distinct_named_factories_are_independent() {
    let legacy = LegacyPositionalRandomFactory::new(LEGACY_SEED);
    let legacy_names = named_doubles(&legacy);
    assert_eq!(legacy_names.as_slice(), LEGACY_NAMED_DOUBLE);
    assert_eq!(
        distinct_count(&legacy_names),
        NAMES.len(),
        "distinct names produced the same legacy stream"
    );
    assert_eq!(
        distinct_count(&named_ints(&legacy)),
        NAMES.len(),
        "distinct names produced the same legacy int stream"
    );

    let xoroshiro = XoroshiroPositionalRandomFactory::new(XOROSHIRO_SEED.0, XOROSHIRO_SEED.1);
    let xoroshiro_names = named_doubles(&xoroshiro);
    assert_eq!(xoroshiro_names.as_slice(), XOROSHIRO_NAMED_DOUBLE);
    assert_eq!(
        distinct_count(&xoroshiro_names),
        NAMES.len(),
        "distinct names produced the same xoroshiro stream"
    );
    assert_eq!(
        distinct_count(&named_ints(&xoroshiro)),
        NAMES.len(),
        "distinct names produced the same xoroshiro int stream"
    );
}

#[test]
fn interleaving_matches_isolated_calls() {
    let legacy = LegacyPositionalRandomFactory::new(LEGACY_SEED);
    check_interleaving(
        &legacy,
        &LEGACY_INTERLEAVED_DOUBLE,
        &LEGACY_INTERLEAVED_INT,
        &LEGACY_INTERLEAVED_LONG,
    );

    let xoroshiro = XoroshiroPositionalRandomFactory::new(XOROSHIRO_SEED.0, XOROSHIRO_SEED.1);
    check_interleaving(
        &xoroshiro,
        &XOROSHIRO_INTERLEAVED_DOUBLE,
        &XOROSHIRO_INTERLEAVED_INT,
        &XOROSHIRO_INTERLEAVED_LONG,
    );
}
