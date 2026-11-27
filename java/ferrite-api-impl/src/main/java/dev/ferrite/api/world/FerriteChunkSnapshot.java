package dev.ferrite.api.world;

import java.util.Objects;
import net.kyori.adventure.key.Key;
import org.bukkit.Bukkit;
import org.bukkit.ChunkSnapshot;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.World;
import org.bukkit.block.Biome;
import org.bukkit.block.data.BlockData;

/**
 * Immutable {@link ChunkSnapshot} captured from a loaded {@link FerriteWorld}.
 *
 * <p>Blocks, biomes, biome temperature and the capture time are copied eagerly. The facade
 * does not model per-block light, so the light accessors report {@code 0}; plugins that need
 * lighting must read the live blocks instead.
 */
public final class FerriteChunkSnapshot implements ChunkSnapshot {

    private static final int WIDTH = 16;
    private static final int HEIGHT = 16;
    private static final int LAYER = WIDTH * HEIGHT;

    private final String worldName;
    private final Key worldKey;
    private final int x;
    private final int z;
    private final int minY;
    private final int maxY;
    private final Material[] blocks;
    private final Biome[] biomes;
    private final double[] biomeTemperatures;
    private final long captureFullTime;

    private FerriteChunkSnapshot(
            String worldName,
            Key worldKey,
            int x,
            int z,
            int minY,
            int maxY,
            Material[] blocks,
            Biome[] biomes,
            double[] biomeTemperatures,
            long captureFullTime) {
        this.worldName = worldName;
        this.worldKey = worldKey;
        this.x = x;
        this.z = z;
        this.minY = minY;
        this.maxY = maxY;
        this.blocks = blocks;
        this.biomes = biomes;
        this.biomeTemperatures = biomeTemperatures;
        this.captureFullTime = captureFullTime;
    }

    static FerriteChunkSnapshot capture(
            FerriteWorld world, int x, int z, boolean includeBiome, boolean includeTempRain) {
        Objects.requireNonNull(world, "world");
        int minY = world.getMinHeight();
        int maxY = world.getMaxHeight();
        int height = maxY - minY;
        Material[] blocks = new Material[height * LAYER];
        Biome[] biomes = includeBiome ? new Biome[blocks.length] : null;
        double[] temperatures = includeBiome && includeTempRain ? new double[blocks.length] : null;
        int baseX = x << 4;
        int baseZ = z << 4;
        for (int y = minY; y < maxY; y++) {
            int layer = (y - minY) * LAYER;
            for (int dz = 0; dz < HEIGHT; dz++) {
                for (int dx = 0; dx < WIDTH; dx++) {
                    int index = layer + (dz << 4) + dx;
                    blocks[index] = world.getBlockAt(baseX + dx, y, baseZ + dz).getType();
                    if (biomes != null) {
                        Biome biome = world.getBiome(baseX + dx, y, baseZ + dz);
                        biomes[index] = biome;
                        if (temperatures != null) {
                            temperatures[index] = temperatureOf(biome);
                        }
                    }
                }
            }
        }
        return new FerriteChunkSnapshot(
                world.getName(),
                Key.key(NamespacedKey.MINECRAFT, world.getName().toLowerCase(java.util.Locale.ROOT)),
                x,
                z,
                minY,
                maxY,
                blocks,
                biomes,
                temperatures,
                world.getFullTime());
    }

    static FerriteChunkSnapshot empty(
            FerriteWorld world, int x, int z, boolean includeBiome, boolean includeTempRain) {
        Objects.requireNonNull(world, "world");
        int minY = world.getMinHeight();
        int maxY = world.getMaxHeight();
        int size = (maxY - minY) * LAYER;
        Material[] blocks = new Material[size];
        java.util.Arrays.fill(blocks, Material.AIR);
        Biome[] biomes = null;
        double[] temperatures = null;
        if (includeBiome) {
            Biome biome = defaultBiome(world);
            biomes = new Biome[size];
            java.util.Arrays.fill(biomes, biome);
            if (includeTempRain) {
                temperatures = new double[size];
                java.util.Arrays.fill(temperatures, temperatureOf(biome));
            }
        }
        return new FerriteChunkSnapshot(
                world.getName(),
                Key.key(NamespacedKey.MINECRAFT, world.getName().toLowerCase(java.util.Locale.ROOT)),
                x,
                z,
                minY,
                maxY,
                blocks,
                biomes,
                temperatures,
                world.getFullTime());
    }

    private static Biome defaultBiome(FerriteWorld world) {
        return switch (world.getEnvironment()) {
            case NETHER -> FerriteWorld.resolveBiome("minecraft:nether_wastes");
            case THE_END -> FerriteWorld.resolveBiome("minecraft:the_end");
            default -> FerriteWorld.resolveBiome("minecraft:plains");
        };
    }

    private static double temperatureOf(Biome biome) {
        if (biome instanceof dev.ferrite.api.registry.FerriteBiome ferriteBiome) {
            return ferriteBiome.temperature();
        }
        return 0.5;
    }

    @Override
    public int getX() {
        return x;
    }

    @Override
    public int getZ() {
        return z;
    }

    @Override
    public String getWorldName() {
        return worldName;
    }

    @Override
    public Key getWorldKey() {
        return worldKey;
    }

    @Override
    public Material getBlockType(int x, int y, int z) {
        return blocks[index(x, y, z)];
    }

    @Override
    public BlockData getBlockData(int x, int y, int z) {
        return Bukkit.createBlockData(getBlockType(x, y, z));
    }

    @Override
    @Deprecated
    @SuppressWarnings("deprecation")
    public int getData(int x, int y, int z) {
        return 0;
    }

    @Override
    public int getBlockSkyLight(int x, int y, int z) {
        // The facade models block types and biomes, not the light engine.
        checkIndex(x, y, z);
        return 0;
    }

    @Override
    public int getBlockEmittedLight(int x, int y, int z) {
        checkIndex(x, y, z);
        return 0;
    }

    @Override
    public int getHighestBlockYAt(int x, int z) {
        checkColumn(x, z);
        for (int y = maxY - 1; y >= minY; y--) {
            if (!blocks[((y - minY) << 8) | (z << 4) | x].isAir()) {
                return y;
            }
        }
        return minY;
    }

    @Override
    @Deprecated
    public Biome getBiome(int x, int z) {
        return getBiome(x, Math.max(minY, Math.min(64, maxY - 1)), z);
    }

    @Override
    public Biome getBiome(int x, int y, int z) {
        if (biomes == null) {
            return null;
        }
        return biomes[index(x, y, z)];
    }

    @Override
    @Deprecated
    public double getRawBiomeTemperature(int x, int z) {
        return getRawBiomeTemperature(x, Math.max(minY, Math.min(64, maxY - 1)), z);
    }

    @Override
    public double getRawBiomeTemperature(int x, int y, int z) {
        if (biomeTemperatures == null) {
            Biome biome = getBiome(x, y, z);
            return biome == null ? 0.5 : temperatureOf(biome);
        }
        return biomeTemperatures[index(x, y, z)];
    }

    @Override
    public long getCaptureFullTime() {
        return captureFullTime;
    }

    @Override
    public boolean isSectionEmpty(int sy) {
        int startY = sy << 4;
        int endY = startY + 16;
        if (endY <= minY || startY >= maxY) {
            return true;
        }
        for (int y = Math.max(startY, minY); y < Math.min(endY, maxY); y++) {
            int layer = (y - minY) << 8;
            for (int index = layer; index < layer + LAYER; index++) {
                if (!blocks[index].isAir()) {
                    return false;
                }
            }
        }
        return true;
    }

    @Override
    public boolean contains(BlockData block) {
        Objects.requireNonNull(block, "block");
        Material material = block.getMaterial();
        for (Material candidate : blocks) {
            if (candidate == material) {
                return true;
            }
        }
        return false;
    }

    @Override
    public boolean contains(Biome biome) {
        if (biome == null || biomes == null) {
            return false;
        }
        for (Biome candidate : biomes) {
            if (biome.equals(candidate)) {
                return true;
            }
        }
        return false;
    }

    private int index(int x, int y, int z) {
        checkIndex(x, y, z);
        return ((y - minY) << 8) | (z << 4) | x;
    }

    private void checkIndex(int x, int y, int z) {
        checkColumn(x, z);
        if (y < minY || y >= maxY) {
            throw new IndexOutOfBoundsException(
                    "y " + y + " is outside " + minY + ".." + (maxY - 1));
        }
    }

    private static void checkColumn(int x, int z) {
        if (x < 0 || x > 15 || z < 0 || z > 15) {
            throw new IndexOutOfBoundsException(
                    "chunk-relative coordinates out of range 0..15: (" + x + ", " + z + ")");
        }
    }

    @Override
    public String toString() {
        return "FerriteChunkSnapshot{world=" + worldName + ",x=" + x + ",z=" + z + "}";
    }
}
