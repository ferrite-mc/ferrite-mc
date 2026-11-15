package dev.ferrite.server;

import net.kyori.adventure.key.Key;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.block.Biome;
import org.bukkit.block.data.BlockData;

/**
 * A chunk snapshot. Blocks and light are copied from the host when the snapshot is captured;
 * {@code World#getEmptyChunkSnapshot} uses the empty form, whose blocks are all air. Biome data
 * is either copied with the chunk or, for the empty form, read live from the host.
 */
final class FerriteChunkSnapshot implements org.bukkit.ChunkSnapshot {
    private record Captured(String[] blockPalette, Material[] materials, int[] blocks,
                            byte[] skyLight, byte[] blockLight, String[] biomePalette,
                            int[] biomes, int[] highest) {}

    private final int x, z, minHeight, maxHeight;
    private final String worldName;
    private final String worldId;
    private final Key worldKey;
    private final boolean includeMaxBlockY, includeBiome, includeBiomeTempRain;
    private final long fullTime;
    private final Captured captured;
    /** Distinct biomes in the captured cells; computed once for {@link #contains(Biome)}. */
    private volatile java.util.Set<Biome> biomes;

    FerriteChunkSnapshot(int x, int z, String worldName, String worldId, Key worldKey, int minHeight, int maxHeight, long fullTime) {
        this(x, z, worldName, worldId, worldKey, minHeight, maxHeight, fullTime, true, true, true, null);
    }

    FerriteChunkSnapshot(int x, int z, String worldName, String worldId, Key worldKey,
                         int minHeight, int maxHeight, long fullTime, boolean includeBiome) {
        this(x, z, worldName, worldId, worldKey, minHeight, maxHeight, fullTime, true, includeBiome, includeBiome, null);
    }

    /** An all-air snapshot from {@code World#getEmptyChunkSnapshot}. */
    static FerriteChunkSnapshot empty(int x, int z, String worldName, String worldId, Key worldKey,
                                      int minHeight, int maxHeight, long fullTime,
                                      boolean includeMaxBlockY, boolean includeBiome) {
        return new FerriteChunkSnapshot(x, z, worldName, worldId, worldKey, minHeight, maxHeight, fullTime,
                includeMaxBlockY, includeBiome, includeBiome, null);
    }

    /** Builds a snapshot from the line format produced by the Rust host. */
    static FerriteChunkSnapshot capture(int x, int z, String worldName, String worldId, Key worldKey,
                                        int minHeight, int maxHeight, long fullTime,
                                        boolean includeMaxBlockY, boolean includeBiome,
                                        boolean includeBiomeTempRain, String packed) {
        return new FerriteChunkSnapshot(x, z, worldName, worldId, worldKey, minHeight, maxHeight, fullTime,
                includeMaxBlockY, includeBiome, includeBiomeTempRain, parse(packed, minHeight, maxHeight));
    }

    private FerriteChunkSnapshot(int x, int z, String worldName, String worldId, Key worldKey,
                                 int minHeight, int maxHeight, long fullTime, boolean includeMaxBlockY,
                                 boolean includeBiome, boolean includeBiomeTempRain, Captured captured) {
        this.x = x; this.z = z; this.worldName = worldName; this.worldId = worldId; this.worldKey = worldKey;
        this.minHeight = minHeight; this.maxHeight = maxHeight; this.fullTime = fullTime;
        this.includeMaxBlockY = includeMaxBlockY; this.includeBiome = includeBiome;
        this.includeBiomeTempRain = includeBiomeTempRain; this.captured = captured;
    }

    private static Captured parse(String packed, int expectedMin, int expectedMax) {
        java.util.Objects.requireNonNull(packed, "packed snapshot");
        String[] lines = packed.split("\n", -1);
        int at = 0;
        if (!line(lines, at++).equals("1")) throw new IllegalArgumentException("Unknown chunk snapshot format");
        int min = Integer.parseInt(line(lines, at++));
        int max = Integer.parseInt(line(lines, at++));
        if (min != expectedMin || max != expectedMax) {
            throw new IllegalArgumentException("Chunk snapshot height " + min + ".." + max
                    + " does not match the world's " + expectedMin + ".." + expectedMax);
        }
        int paletteCount = count(line(lines, at++), "block palette");
        String[] blockPalette = new String[paletteCount];
        for (int i = 0; i < paletteCount; i++) blockPalette[i] = line(lines, at++);
        int blockCount = count(line(lines, at++), "block count");
        int expectedBlocks = (max - min) * 256;
        if (blockCount != expectedBlocks) {
            throw new IllegalArgumentException("Chunk snapshot has " + blockCount + " blocks, expected " + expectedBlocks);
        }
        int[] blocks = indexes(line(lines, at++), blockCount, paletteCount);
        byte[] skyLight = light(line(lines, at++), blockCount);
        byte[] blockLight = light(line(lines, at++), blockCount);
        int biomePaletteCount = count(line(lines, at++), "biome palette");
        String[] biomePalette = new String[biomePaletteCount];
        for (int i = 0; i < biomePaletteCount; i++) biomePalette[i] = line(lines, at++);
        int biomeCount = count(line(lines, at++), "biome count");
        int expectedBiomes = ((max - min) / 4) * 16;
        if (biomeCount != expectedBiomes) {
            throw new IllegalArgumentException("Chunk snapshot has " + biomeCount + " biome cells, expected " + expectedBiomes);
        }
        int[] biomes = indexes(line(lines, at++), biomeCount, biomePaletteCount);
        int heightCount = count(line(lines, at++), "height count");
        if (heightCount != 256) throw new IllegalArgumentException("Chunk snapshot has " + heightCount + " height columns");
        int[] highest = numbers(line(lines, at++), 256);
        Material[] materials = new Material[blockPalette.length];
        for (int i = 0; i < blockPalette.length; i++) materials[i] = material(blockPalette[i]);
        return new Captured(blockPalette, materials, blocks, skyLight, blockLight, biomePalette, biomes, highest);
    }

    private static String line(String[] lines, int at) {
        if (at < 0 || at >= lines.length) throw new IllegalArgumentException("Truncated chunk snapshot");
        return lines[at];
    }

    private static int count(String text, String what) {
        int value = Integer.parseInt(text);
        if (value < 0) throw new IllegalArgumentException("Negative " + what);
        return value;
    }

    private static int[] indexes(String text, int expected, int limit) {
        if (text.isEmpty()) {
            if (expected != 0) throw new IllegalArgumentException("Chunk snapshot field is empty, expected " + expected);
            return new int[0];
        }
        String[] parts = text.split(",", -1);
        if (parts.length != expected) {
            throw new IllegalArgumentException("Chunk snapshot field has " + parts.length + " values, expected " + expected);
        }
        int[] out = new int[expected];
        for (int i = 0; i < expected; i++) {
            out[i] = Integer.parseInt(parts[i]);
            if (out[i] < 0 || out[i] >= limit) throw new IllegalArgumentException("Chunk snapshot index " + out[i] + " is out of range");
        }
        return out;
    }

    /** Heights may be negative, so they cannot use the palette-index bounds check. */
    private static int[] numbers(String text, int expected) {
        if (text.isEmpty()) {
            if (expected != 0) throw new IllegalArgumentException("Chunk snapshot field is empty, expected " + expected);
            return new int[0];
        }
        String[] parts = text.split(",", -1);
        if (parts.length != expected) {
            throw new IllegalArgumentException("Chunk snapshot field has " + parts.length + " values, expected " + expected);
        }
        int[] out = new int[expected];
        for (int i = 0; i < expected; i++) out[i] = Integer.parseInt(parts[i]);
        return out;
    }

    private static byte[] light(String text, int expected) {
        if (text.length() != expected) {
            throw new IllegalArgumentException("Chunk snapshot light has " + text.length() + " values, expected " + expected);
        }
        byte[] out = new byte[expected];
        for (int i = 0; i < expected; i++) {
            int value = Character.digit(text.charAt(i), 16);
            if (value < 0 || value > 15) throw new IllegalArgumentException("Invalid light value " + text.charAt(i));
            out[i] = (byte) value;
        }
        return out;
    }

    private static Material material(String state) {
        int open = state.indexOf('[');
        String id = open < 0 ? state : state.substring(0, open);
        if (!id.contains(":")) id = "minecraft:" + id;
        var material = Material.matchMaterial(id);
        if (material == null) throw new IllegalArgumentException("Unknown block type: " + state);
        return material;
    }

    private void check(int bx, int by, int bz) {
        if (bx < 0 || bx > 15 || bz < 0 || bz > 15) throw new IndexOutOfBoundsException("x/z must be within 0..15");
        if (by < minHeight || by >= maxHeight) throw new IndexOutOfBoundsException("y out of world height");
    }

    private int index(int bx, int by, int bz) {
        return ((by - minHeight) * 16 + bz) * 16 + bx;
    }

    private String biomeKey(int bx, int by, int bz) {
        check(bx, by, bz);
        if (!includeBiome) throw new UnsupportedOperationException("Ferrite: this chunk snapshot was created without biome data");
        if (captured != null) {
            int cell = (((by - minHeight) >> 2) * 16) + ((bz >> 2) * 4) + (bx >> 2);
            return captured.biomePalette()[captured.biomes()[cell]];
        }
        var key = Native.biomeAt(worldId, (x << 4) + bx, by, (z << 4) + bz);
        if (key == null) throw new UnsupportedOperationException("Ferrite: no biome data at " + ((x << 4) + bx) + "," + by + "," + ((z << 4) + bz));
        return key;
    }

    private Biome biome(int bx, int by, int bz) {
        var key = biomeKey(bx, by, bz);
        var biome = org.bukkit.Registry.BIOME.get(NamespacedKey.fromString(key));
        if (biome == null) throw new IllegalStateException("Unknown biome " + key);
        return biome;
    }

    @Override public int getX() { return x; }
    @Override public int getZ() { return z; }
    @Override public String getWorldName() { return worldName; }
    @Override public Key getWorldKey() { return worldKey; }

    @Override public Material getBlockType(int bx, int by, int bz) {
        check(bx, by, bz);
        return captured == null ? Material.AIR : captured.materials()[captured.blocks()[index(bx, by, bz)]];
    }

    @Override public BlockData getBlockData(int bx, int by, int bz) {
        check(bx, by, bz);
        if (captured == null) return Material.AIR.createBlockData();
        return FerriteBlockData.create(captured.blockPalette()[captured.blocks()[index(bx, by, bz)]]);
    }

    @Override public int getData(int bx, int by, int bz) { check(bx, by, bz); return 0; }

    @Override public int getBlockSkyLight(int bx, int by, int bz) {
        check(bx, by, bz);
        return captured == null ? 0 : captured.skyLight()[index(bx, by, bz)];
    }

    @Override public int getBlockEmittedLight(int bx, int by, int bz) {
        check(bx, by, bz);
        return captured == null ? 0 : captured.blockLight()[index(bx, by, bz)];
    }

    @Override public int getHighestBlockYAt(int bx, int bz) {
        if (bx < 0 || bx > 15 || bz < 0 || bz > 15) throw new IndexOutOfBoundsException("x/z must be within 0..15");
        // The empty form is already known to be all air, so its column height is unchanged.
        if (captured == null) return minHeight;
        if (!includeMaxBlockY) throw new UnsupportedOperationException("Ferrite: this chunk snapshot was created without maximum block heights");
        return captured.highest()[bz * 16 + bx];
    }

    @Override public Biome getBiome(int bx, int by, int bz) { return biome(bx, by, bz); }

    /** The deprecated 2D query reads the biome at y=0, as CraftBukkit does. */
    @Override public Biome getBiome(int bx, int bz) { return getBiome(bx, 0, bz); }

    @Override public double getRawBiomeTemperature(int bx, int by, int bz) {
        if (!includeBiomeTempRain) throw new UnsupportedOperationException("Ferrite: this chunk snapshot was created without biome temperature data");
        var key = biomeKey(bx, by, bz);
        String reply = (String) HostBridge.call("World", worldId, "biomeClimate(String)", String.class, key);
        return Double.parseDouble(reply.split(String.valueOf(HostBridge.ITEM))[0]);
    }

    @Override public double getRawBiomeTemperature(int bx, int bz) { return getRawBiomeTemperature(bx, 0, bz); }
    @Override public long getCaptureFullTime() { return fullTime; }

    @Override public boolean isSectionEmpty(int section) {
        if (captured == null) return true;
        int sections = (maxHeight - minHeight) / 16;
        if (section < 0 || section >= sections) throw new IndexOutOfBoundsException("section out of world height");
        int firstY = minHeight + section * 16;
        for (int by = firstY; by < firstY + 16; by++) {
            for (int bz = 0; bz < 16; bz++) {
                for (int bx = 0; bx < 16; bx++) {
                    if (!captured.materials()[captured.blocks()[index(bx, by, bz)]].isAir()) return false;
                }
            }
        }
        return true;
    }

    @Override public boolean contains(BlockData block) {
        java.util.Objects.requireNonNull(block, "block");
        if (captured == null) return block.getMaterial().isAir();
        for (String state : captured.blockPalette()) if (block.getAsString().equals(state)) return true;
        return false;
    }

    @Override public boolean contains(Biome biome) {
        java.util.Objects.requireNonNull(biome, "biome");
        if (!includeBiome) return false;
        var cached = biomes;
        if (cached == null) {
            var found = new java.util.LinkedHashSet<Biome>();
            if (captured != null) {
                for (String key : captured.biomePalette()) {
                    var value = org.bukkit.Registry.BIOME.get(NamespacedKey.fromString(key));
                    if (value != null) found.add(value);
                }
            } else {
                // Vanilla stores biomes per 4x4x4 cell; sampling each cell's first block is enough.
                for (int by = minHeight; by < maxHeight; by += 4) {
                    for (int bx = 0; bx < 16; bx += 4) {
                        for (int bz = 0; bz < 16; bz += 4) {
                            var key = Native.biomeAt(worldId, (x << 4) + bx, by, (z << 4) + bz);
                            if (key == null) continue;
                            var value = org.bukkit.Registry.BIOME.get(NamespacedKey.fromString(key));
                            if (value != null) found.add(value);
                        }
                    }
                }
            }
            cached = java.util.Set.copyOf(found);
            biomes = cached;
        }
        return cached.contains(biome);
    }
}
