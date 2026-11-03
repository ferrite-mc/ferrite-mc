import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.function.BooleanSupplier;
import java.util.function.DoubleSupplier;
import java.util.function.IntSupplier;
import java.util.function.LongSupplier;

import net.minecraft.SharedConstants;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.PositionalRandomFactory;
import net.minecraft.world.level.levelgen.RandomSupport;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;
import net.minecraft.world.level.levelgen.densityfunction.DensitySampler;
import net.minecraft.world.level.levelgen.synth.BlendedNoise;
import net.minecraft.world.level.levelgen.synth.Noise;
import net.minecraft.world.level.levelgen.synth.NormalNoise;
import net.minecraft.world.level.levelgen.synth.PerlinNoise;
import net.minecraft.world.level.levelgen.synth.SimplexNoise;
import net.minecraft.world.level.levelgen.synth.SmearedPerlinNoise;

/**
 * Dumps reference values for ferrite-noise from the local 26.3 server jar.
 *
 * <p>Usage: {@code java NoiseReference <output.json>}. The output is read by
 * {@code crates/ferrite-noise/tests/reference.rs}; nothing here is committed.
 *
 * <p>Run this with {@code -XX:+UnlockDiagnosticVMOptions -XX:-UseLibmIntrinsic}:
 * the JVM is allowed to replace {@code Math.log} with a platform libm intrinsic
 * that differs from {@code StrictMath.log} in the last bit, which would make
 * {@code nextGaussian()} depend on the host libm. Disabling that intrinsic pins
 * the reference to fdlibm, the deterministic behaviour of the Java spec.
 */
public final class NoiseReference {
    private static final int SAMPLES = 10_000;
    private static final long LEGACY_SEED = 0x123456789ABCDEFL;
    private static final long XOROSHIRO_SEED = 0x0FEDCBA987654321L;

    /**
     * Negative, lattice and wrap-window coordinates checked without a fixture by
     * {@code tests/sampling.rs}. Kept in sync with {@code REGRESSION_COORDS} there.
     */
    /**
     * Positional-factory regression rows: negative and extreme coordinates, plus one
     * distinct name and seed per row. Checked by {@code tests/random_regression.rs}.
     */
    private static final int[] REG_X = {-1_000, -17, 0, 1, 12_345, Integer.MIN_VALUE};
    private static final int[] REG_Y = {-64, -1, 0, 5, 319, Integer.MAX_VALUE};
    private static final int[] REG_Z = {-1_500, -33, 0, 7, 98_765, -1};
    private static final String[] REG_NAMES = {
        "minecraft:aquifer",
        "minecraft:terrain",
        "minecraft:feature_a",
        "ferrite:test",
        "octave_-8",
        "minecraft:overworld",
    };
    private static final long[] REG_SEEDS = {
        -1L, 0L, 1L, 123_456_789L, Long.MIN_VALUE, Long.MAX_VALUE,
    };

    private interface IndexDouble {
        double get(int index);
    }

    private interface IndexInt {
        int get(int index);
    }

    private interface IndexLong {
        long get(int index);
    }

    private static final double[][] REGRESSION_COORDS = {
        {0.0, 0.0, 0.0},
        {-1.0, -1.0, -1.0},
        {-17.0, -64.0, -33.0},
        {-1_000_000.0, 0.0, 1_000_000.0},
        {-16_777_216.0, 5.0, -16_777_216.0},
        {16_777_215.5, 0.0, -16_777_215.5},
        {16_777_216.0, 1.25, -16_777_216.0},
        {16_777_216.5, -0.5, 33_554_432.0},
        {-33_554_432.5, 0.0, 25_165_824.0},
        {0.5, -0.5, 0.5},
        {100.25, -30.75, -400.125},
        {25_165_825.5, -25_165_826.5, -12.0},
    };

    private interface FloatSupplier {
        float getAsFloat();
    }

    private interface SectionWriter {
        void write(StringBuilder out);
    }

    private NoiseReference() {}

    public static void main(String[] args) throws IOException {
        if (args.length != 1) {
            System.err.println("usage: NoiseReference <output.json>");
            System.exit(2);
        }

        // BlendedNoise implements DensityFunction, whose static initialiser needs
        // the vanilla registries to be bootstrapped, and bootstrapping needs a
        // detected game version.
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        StringBuilder out = new StringBuilder(8 << 20);
        out.append('{');
        boolean[] firstSection = {true};
        section(out, firstSection, "meta", "\"samples\":" + SAMPLES + ",\"minecraft\":\"26.3\"");

        // ---- random sources -------------------------------------------------
        LegacyRandomSource legacyInt = new LegacyRandomSource(LEGACY_SEED);
        LegacyRandomSource legacyBound = new LegacyRandomSource(LEGACY_SEED);
        LegacyRandomSource legacyLong = new LegacyRandomSource(LEGACY_SEED);
        LegacyRandomSource legacyDouble = new LegacyRandomSource(LEGACY_SEED);
        LegacyRandomSource legacyFloat = new LegacyRandomSource(LEGACY_SEED);
        LegacyRandomSource legacyGaussian = new LegacyRandomSource(LEGACY_SEED);
        LegacyRandomSource legacyBoolean = new LegacyRandomSource(LEGACY_SEED);
        section(out, firstSection, "legacy_random_sequences",
                intsField("next_int", legacyInt::nextInt, SAMPLES),
                intsField("next_int_bound_1000", () -> legacyBound.nextInt(1000), SAMPLES),
                longsField("next_long", legacyLong::nextLong, SAMPLES),
                doublesField("next_double", legacyDouble::nextDouble, SAMPLES),
                floatsField("next_float", legacyFloat::nextFloat, SAMPLES),
                doublesField("next_gaussian", legacyGaussian::nextGaussian, SAMPLES),
                booleansField("next_boolean", legacyBoolean::nextBoolean, SAMPLES));

        XoroshiroRandomSource xoroInt = new XoroshiroRandomSource(XOROSHIRO_SEED);
        XoroshiroRandomSource xoroBound = new XoroshiroRandomSource(XOROSHIRO_SEED);
        XoroshiroRandomSource xoroLong = new XoroshiroRandomSource(XOROSHIRO_SEED);
        XoroshiroRandomSource xoroDouble = new XoroshiroRandomSource(XOROSHIRO_SEED);
        XoroshiroRandomSource xoroFloat = new XoroshiroRandomSource(XOROSHIRO_SEED);
        XoroshiroRandomSource xoroGaussian = new XoroshiroRandomSource(XOROSHIRO_SEED);
        XoroshiroRandomSource xoroBoolean = new XoroshiroRandomSource(XOROSHIRO_SEED);
        section(out, firstSection, "xoroshiro_random_sequences",
                intsField("next_int", xoroInt::nextInt, SAMPLES),
                intsField("next_int_bound_1000", () -> xoroBound.nextInt(1000), SAMPLES),
                longsField("next_long", xoroLong::nextLong, SAMPLES),
                doublesField("next_double", xoroDouble::nextDouble, SAMPLES),
                floatsField("next_float", xoroFloat::nextFloat, SAMPLES),
                doublesField("next_gaussian", xoroGaussian::nextGaussian, SAMPLES),
                booleansField("next_boolean", xoroBoolean::nextBoolean, SAMPLES));

        // ---- positional factories ------------------------------------------
        PositionalRandomFactory legacyFactory =
                new LegacyRandomSource.LegacyPositionalRandomFactory(0x0BADF00DCAFEBABEL);
        PositionalRandomFactory xoroFactory =
                new XoroshiroRandomSource.XoroshiroPositionalRandomFactory(
                        0x0123456789ABCDEFL, 0x0FEDCBA987654321L);
        section(out, firstSection, "legacy_positional",
                doublesField("at", () -> legacyFactory.at(posX(0), posY(0), posZ(0)).nextDouble(), 1));
        section(out, firstSection, "positional_legacy",
                positionsField("at", legacyFactory),
                hashesField("from_hash_of", legacyFactory),
                seedsField("from_seed", legacyFactory));
        section(out, firstSection, "positional_xoroshiro",
                positionsField("at", xoroFactory),
                hashesField("from_hash_of", xoroFactory),
                seedsField("from_seed", xoroFactory));

        // ---- noise primitives ----------------------------------------------
        PerlinNoise perlin = new PerlinNoise(new LegacyRandomSource(1234L));
        section(out, firstSection, "perlin",
                noise3dField("samples", perlin));

        SmearedPerlinNoise smeared = new SmearedPerlinNoise(new LegacyRandomSource(5678L), 1.5);
        section(out, firstSection, "smeared_perlin",
                noise3dField("samples", smeared));

        SimplexNoise simplex = new SimplexNoise(new LegacyRandomSource(9012L));
        section(out, firstSection, "simplex",
                noise2dField("samples_2d", simplex),
                noise3dField("samples_3d", simplex));

        NormalNoise normal = NormalNoise.builder()
                .setBaseAmplitude(1.0)
                .setBaseOctave(-8)
                .setOctaveCount(5)
                .setNormalize(true)
                .setAmplitudeModifier(1, 0.5)
                .setAmplitudeModifier(3, 0.0)
                .build();
        Noise normalNoise = normal.create(new XoroshiroRandomSource(3456L));
        section(out, firstSection, "normal_noise",
                noise3dField("samples", normalNoise));

        NormalNoise legacyNormal = NormalNoise.builder()
                .setBaseAmplitude(1.0)
                .setBaseOctave(-6)
                .setOctaveCount(4)
                .setLegacyNormalization()
                .build();
        Noise legacyNormalNoise = legacyNormal.createForLegacyNetherBiome(new LegacyRandomSource(7890L));
        section(out, firstSection, "legacy_normal_noise",
                noise3dField("samples", legacyNormalNoise));

        BlendedNoise blended = new BlendedNoise(1.0, 1.0, 80.0, 160.0, 8.0);
        DensitySampler blendedSampler = blended.compileSampler(new XoroshiroRandomSource(2468L));
        section(out, firstSection, "blended_noise",
                blendedField(blendedSampler));

        // A small coordinate table (negative, lattice and wrap-window points) that
        // the Rust tests hardcode, so boundary behaviour is checked without Java.
        section(out, firstSection, "sampling_regressions",
                noise3dAtField("perlin", perlin, REGRESSION_COORDS),
                noise3dAtField("smeared_perlin", smeared, REGRESSION_COORDS),
                noise2dAtField("simplex_2d", simplex, REGRESSION_COORDS),
                noise3dAtField("simplex_3d", simplex, REGRESSION_COORDS),
                noise3dAtField("normal_noise", normalNoise, REGRESSION_COORDS),
                noise3dAtField("legacy_normal_noise", legacyNormalNoise, REGRESSION_COORDS),
                blendedAtField("blended_noise", blendedSampler));

        // Positional random factories: negative coordinates, distinct named factories,
        // repeatability and interleaving. tests/random_regression.rs hardcodes these.
        section(out, firstSection, "random_regression_legacy",
                doublesAt("at_double", i -> legacyFactory.at(REG_X[i], REG_Y[i], REG_Z[i]).nextDouble()),
                doublesAt("at_double_repeat", i -> legacyFactory.at(REG_X[i], REG_Y[i], REG_Z[i]).nextDouble()),
                intsAt("at_int", i -> legacyFactory.at(REG_X[i], REG_Y[i], REG_Z[i]).nextInt()),
                longsAt("at_long", i -> legacyFactory.at(REG_X[i], REG_Y[i], REG_Z[i]).nextLong()),
                doublesAt("at_gaussian", i -> legacyFactory.at(REG_X[i], REG_Y[i], REG_Z[i]).nextGaussian()),
                doublesAt("named_double", i -> legacyFactory.fromHashOf(REG_NAMES[i]).nextDouble()),
                doublesAt("named_double_repeat", i -> legacyFactory.fromHashOf(REG_NAMES[i]).nextDouble()),
                intsAt("named_int", i -> legacyFactory.fromHashOf(REG_NAMES[i]).nextInt()),
                doublesAt("seed_double", i -> legacyFactory.fromSeed(REG_SEEDS[i]).nextDouble()),
                interleavedDoubles("interleaved_double", legacyFactory),
                interleavedInts("interleaved_int", legacyFactory),
                interleavedLongs("interleaved_long", legacyFactory));

        section(out, firstSection, "random_regression_xoroshiro",
                doublesAt("at_double", i -> xoroFactory.at(REG_X[i], REG_Y[i], REG_Z[i]).nextDouble()),
                doublesAt("at_double_repeat", i -> xoroFactory.at(REG_X[i], REG_Y[i], REG_Z[i]).nextDouble()),
                intsAt("at_int", i -> xoroFactory.at(REG_X[i], REG_Y[i], REG_Z[i]).nextInt()),
                longsAt("at_long", i -> xoroFactory.at(REG_X[i], REG_Y[i], REG_Z[i]).nextLong()),
                doublesAt("at_gaussian", i -> xoroFactory.at(REG_X[i], REG_Y[i], REG_Z[i]).nextGaussian()),
                doublesAt("named_double", i -> xoroFactory.fromHashOf(REG_NAMES[i]).nextDouble()),
                doublesAt("named_double_repeat", i -> xoroFactory.fromHashOf(REG_NAMES[i]).nextDouble()),
                intsAt("named_int", i -> xoroFactory.fromHashOf(REG_NAMES[i]).nextInt()),
                doublesAt("seed_double", i -> xoroFactory.fromSeed(REG_SEEDS[i]).nextDouble()),
                interleavedDoubles("interleaved_double", xoroFactory),
                interleavedInts("interleaved_int", xoroFactory),
                interleavedLongs("interleaved_long", xoroFactory));

        section(out, firstSection, "random_support",
                longsField("upgrade_unmixed_lo", () -> RandomSupport.upgradeSeedTo128bitUnmixed(LEGACY_SEED).seedLo(), 1),
                longsField("upgrade_unmixed_hi", () -> RandomSupport.upgradeSeedTo128bitUnmixed(LEGACY_SEED).seedHi(), 1),
                longsField("upgrade_lo", () -> RandomSupport.upgradeSeedTo128bit(LEGACY_SEED).seedLo(), 1),
                longsField("upgrade_hi", () -> RandomSupport.upgradeSeedTo128bit(LEGACY_SEED).seedHi(), 1),
                longsField("hash_lo", () -> RandomSupport.seedFromHashOf("minecraft:overworld").seedLo(), 1),
                longsField("hash_hi", () -> RandomSupport.seedFromHashOf("minecraft:overworld").seedHi(), 1),
                longsField("mix_stafford_13", () -> RandomSupport.mixStafford13(LEGACY_SEED), 1));

        out.append('}');
        Path output = Path.of(args[0]);
        if (output.getParent() != null) {
            Files.createDirectories(output.getParent());
        }
        Files.writeString(output, out.toString(), StandardCharsets.UTF_8);
        System.out.println("wrote " + output.toAbsolutePath() + " (" + out.length() + " chars)");
    }

    // ---- section helpers ---------------------------------------------------

    private static void section(StringBuilder out, boolean[] first, String name, String... fields) {
        if (!first[0]) {
            out.append(',');
        }
        first[0] = false;
        out.append('"').append(name).append("\":{");
        for (int i = 0; i < fields.length; i++) {
            if (i > 0) {
                out.append(',');
            }
            out.append(fields[i]);
        }
        out.append('}');
    }

    private static String intsField(String key, IntSupplier supplier, int count) {
        StringBuilder out = new StringBuilder(count * 12 + 32);
        out.append('"').append(key).append("\":[");
        for (int i = 0; i < count; i++) {
            if (i > 0) {
                out.append(',');
            }
            out.append(supplier.getAsInt());
        }
        return out.append(']').toString();
    }

    private static String longsField(String key, LongSupplier supplier, int count) {
        StringBuilder out = new StringBuilder(count * 20 + 32);
        out.append('"').append(key).append("\":[");
        for (int i = 0; i < count; i++) {
            if (i > 0) {
                out.append(',');
            }
            out.append('"').append(hex(supplier.getAsLong())).append('"');
        }
        return out.append(']').toString();
    }

    private static String doublesField(String key, DoubleSupplier supplier, int count) {
        StringBuilder out = new StringBuilder(count * 22 + 32);
        out.append('"').append(key).append("\":[");
        for (int i = 0; i < count; i++) {
            if (i > 0) {
                out.append(',');
            }
            out.append('"').append(hex(Double.doubleToRawLongBits(supplier.getAsDouble()))).append('"');
        }
        return out.append(']').toString();
    }

    private static String floatsField(String key, FloatSupplier supplier, int count) {
        StringBuilder out = new StringBuilder(count * 14 + 32);
        out.append('"').append(key).append("\":[");
        for (int i = 0; i < count; i++) {
            if (i > 0) {
                out.append(',');
            }
            out.append('"').append(hexInt(Float.floatToRawIntBits(supplier.getAsFloat()))).append('"');
        }
        return out.append(']').toString();
    }

    private static String booleansField(String key, BooleanSupplier supplier, int count) {
        StringBuilder out = new StringBuilder(count * 6 + 32);
        out.append('"').append(key).append("\":[");
        for (int i = 0; i < count; i++) {
            if (i > 0) {
                out.append(',');
            }
            out.append(supplier.getAsBoolean());
        }
        return out.append(']').toString();
    }

    private static String noise3dField(String key, Noise noise) {
        StringBuilder out = new StringBuilder(SAMPLES * 22 + 32);
        out.append('"').append(key).append("\":[");
        for (int i = 0; i < SAMPLES; i++) {
            double[] p = coords(i);
            if (i > 0) {
                out.append(',');
            }
            out.append('"').append(hex(Double.doubleToRawLongBits(noise.get(p[0], p[1], p[2])))).append('"');
        }
        return out.append(']').toString();
    }

    private static String noise2dField(String key, Noise noise) {
        StringBuilder out = new StringBuilder(SAMPLES * 22 + 32);
        out.append('"').append(key).append("\":[");
        for (int i = 0; i < SAMPLES; i++) {
            double[] p = coords(i);
            if (i > 0) {
                out.append(',');
            }
            out.append('"').append(hex(Double.doubleToRawLongBits(noise.get(p[0], p[2])))).append('"');
        }
        return out.append(']').toString();
    }

    private static String positionsField(String key, PositionalRandomFactory factory) {
        StringBuilder out = new StringBuilder(SAMPLES * 22 + 32);
        out.append('"').append(key).append("\":[");
        for (int i = 0; i < SAMPLES; i++) {
            if (i > 0) {
                out.append(',');
            }
            double value = factory.at(posX(i), posY(i), posZ(i)).nextDouble();
            out.append('"').append(hex(Double.doubleToRawLongBits(value))).append('"');
        }
        return out.append(']').toString();
    }

    private static String hashesField(String key, PositionalRandomFactory factory) {
        StringBuilder out = new StringBuilder(SAMPLES * 22 + 32);
        out.append('"').append(key).append("\":[");
        for (int i = 0; i < SAMPLES; i++) {
            if (i > 0) {
                out.append(',');
            }
            double value = factory.fromHashOf("minecraft:feature_" + i).nextDouble();
            out.append('"').append(hex(Double.doubleToRawLongBits(value))).append('"');
        }
        return out.append(']').toString();
    }

    private static String seedsField(String key, PositionalRandomFactory factory) {
        StringBuilder out = new StringBuilder(SAMPLES * 22 + 32);
        out.append('"').append(key).append("\":[");
        for (int i = 0; i < SAMPLES; i++) {
            if (i > 0) {
                out.append(',');
            }
            double value = factory.fromSeed(123456789L + i).nextDouble();
            out.append('"').append(hex(Double.doubleToRawLongBits(value))).append('"');
        }
        return out.append(']').toString();
    }

    private static String noise3dAtField(String key, Noise noise, double[][] points) {
        StringBuilder out = new StringBuilder(points.length * 22 + 32);
        out.append('"').append(key).append("\":[");
        for (int i = 0; i < points.length; i++) {
            if (i > 0) {
                out.append(',');
            }
            double value = noise.get(points[i][0], points[i][1], points[i][2]);
            out.append('"').append(hex(Double.doubleToRawLongBits(value))).append('"');
        }
        return out.append(']').toString();
    }

    private static String noise2dAtField(String key, Noise noise, double[][] points) {
        StringBuilder out = new StringBuilder(points.length * 22 + 32);
        out.append('"').append(key).append("\":[");
        for (int i = 0; i < points.length; i++) {
            if (i > 0) {
                out.append(',');
            }
            double value = noise.get(points[i][0], points[i][2]);
            out.append('"').append(hex(Double.doubleToRawLongBits(value))).append('"');
        }
        return out.append(']').toString();
    }

    private static String blendedAtField(String key, DensitySampler sampler) {
        int[][] points = {
            {0, 0, 0},
            {-1, -1, -1},
            {-17, -64, -33},
            {-1_000_000, 0, 1_000_000},
            {-16_777_216, 5, -16_777_216},
            {16_777_215, 0, -16_777_215},
            {16_777_216, 1, -16_777_216},
            {16_777_216, -1, 33_554_432},
            {-33_554_432, 0, 25_165_824},
            {1, -1, 1},
            {100, -30, -400},
            {25_165_825, -25_165_826, -12},
        };
        StringBuilder out = new StringBuilder(points.length * 14 + 32);
        out.append('"').append(key).append("\":[");
        for (int i = 0; i < points.length; i++) {
            if (i > 0) {
                out.append(',');
            }
            float value = sampler.sampleValue(null, points[i][0], points[i][1], points[i][2]);
            out.append('"').append(hexInt(Float.floatToRawIntBits(value))).append('"');
        }
        return out.append(']').toString();
    }

    private static String blendedField(DensitySampler sampler) {
        StringBuilder out = new StringBuilder(SAMPLES * 22 + 32);
        out.append("\"samples\":[");
        for (int i = 0; i < SAMPLES; i++) {
            if (i > 0) {
                out.append(',');
            }
            float value = sampler.sampleValue(null, blendedX(i), blendedY(i), blendedZ(i));
            out.append('"').append(hexInt(Float.floatToRawIntBits(value))).append('"');
        }
        return out.append(']').toString();
    }

    // ---- positional-factory regression helpers ----------------------------

    private static String doublesAt(String key, IndexDouble supplier) {
        StringBuilder out = new StringBuilder(REG_X.length * 22 + 32);
        out.append('"').append(key).append("\":[");
        for (int i = 0; i < REG_X.length; i++) {
            if (i > 0) {
                out.append(',');
            }
            double value = supplier.get(i);
            out.append('"').append(hex(Double.doubleToRawLongBits(value))).append('"');
        }
        return out.append(']').toString();
    }

    private static String intsAt(String key, IndexInt supplier) {
        StringBuilder out = new StringBuilder(REG_X.length * 12 + 32);
        out.append('"').append(key).append("\":[");
        for (int i = 0; i < REG_X.length; i++) {
            if (i > 0) {
                out.append(',');
            }
            out.append(supplier.get(i));
        }
        return out.append(']').toString();
    }

    private static String longsAt(String key, IndexLong supplier) {
        StringBuilder out = new StringBuilder(REG_X.length * 20 + 32);
        out.append('"').append(key).append("\":[");
        for (int i = 0; i < REG_X.length; i++) {
            if (i > 0) {
                out.append(',');
            }
            out.append('"').append(hex(supplier.get(i))).append('"');
        }
        return out.append(']').toString();
    }

    private static String interleavedDoubles(String key, PositionalRandomFactory factory) {
        int count = REG_X.length * 5;
        StringBuilder out = new StringBuilder(count * 22 + 32);
        out.append('"').append(key).append("\":[");
        boolean first = true;
        for (int i = 0; i < REG_X.length; i++) {
            double[] values = {
                factory.at(REG_X[i], REG_Y[i], REG_Z[i]).nextDouble(),
                factory.fromHashOf(REG_NAMES[i]).nextDouble(),
                factory.fromSeed(REG_SEEDS[i]).nextDouble(),
                factory.at(REG_X[i], REG_Y[i], REG_Z[i]).nextDouble(),
                factory.fromHashOf(REG_NAMES[i]).nextDouble(),
            };
            for (double value : values) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                out.append('"').append(hex(Double.doubleToRawLongBits(value))).append('"');
            }
        }
        return out.append(']').toString();
    }

    private static String interleavedInts(String key, PositionalRandomFactory factory) {
        int count = REG_X.length * 5;
        StringBuilder out = new StringBuilder(count * 12 + 32);
        out.append('"').append(key).append("\":[");
        boolean first = true;
        for (int i = 0; i < REG_X.length; i++) {
            int[] values = {
                factory.at(REG_X[i], REG_Y[i], REG_Z[i]).nextInt(),
                factory.fromHashOf(REG_NAMES[i]).nextInt(),
                factory.fromSeed(REG_SEEDS[i]).nextInt(),
                factory.at(REG_X[i], REG_Y[i], REG_Z[i]).nextInt(),
                factory.fromHashOf(REG_NAMES[i]).nextInt(),
            };
            for (int value : values) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                out.append(value);
            }
        }
        return out.append(']').toString();
    }

    private static String interleavedLongs(String key, PositionalRandomFactory factory) {
        int count = REG_X.length * 5;
        StringBuilder out = new StringBuilder(count * 20 + 32);
        out.append('"').append(key).append("\":[");
        boolean first = true;
        for (int i = 0; i < REG_X.length; i++) {
            long[] values = {
                factory.at(REG_X[i], REG_Y[i], REG_Z[i]).nextLong(),
                factory.fromHashOf(REG_NAMES[i]).nextLong(),
                factory.fromSeed(REG_SEEDS[i]).nextLong(),
                factory.at(REG_X[i], REG_Y[i], REG_Z[i]).nextLong(),
                factory.fromHashOf(REG_NAMES[i]).nextLong(),
            };
            for (long value : values) {
                if (!first) {
                    out.append(',');
                }
                first = false;
                out.append('"').append(hex(value)).append('"');
            }
        }
        return out.append(']').toString();
    }

    // ---- deterministic coordinates ----------------------------------------

    private static double[] coords(int i) {
        double x = (i % 4096) * 0.3125 - 640.0;
        double y = (i % 211) * 0.25 - 26.0;
        double z = (i / 4096) * 0.5 - 12.0;
        if (i % 1000 == 0) {
            x = 25_165_824.0 + 1.5;
            y = -25_165_824.0 - 2.5;
        }
        return new double[] {x, y, z};
    }

    private static int posX(int i) {
        return (i * 13) % 2000 - 1000;
    }

    private static int posY(int i) {
        return (i * 7) % 128 - 64;
    }

    private static int posZ(int i) {
        return (i * 29) % 3000 - 1500;
    }

    private static int blendedX(int i) {
        return (i * 37) % 4000 - 2000;
    }

    private static int blendedY(int i) {
        return (i * 17) % 128 - 64;
    }

    private static int blendedZ(int i) {
        return (i * 7) % 3000 - 1500;
    }

    // ---- hex helpers -------------------------------------------------------

    private static String hex(long value) {
        String text = Long.toHexString(value);
        return "0000000000000000".substring(text.length()) + text;
    }

    private static String hexInt(int value) {
        String text = Integer.toHexString(value);
        return "00000000".substring(text.length()) + text;
    }
}
