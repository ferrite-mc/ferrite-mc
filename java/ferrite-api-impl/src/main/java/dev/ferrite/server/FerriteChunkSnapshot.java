package dev.ferrite.server;

import net.kyori.adventure.key.Key;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.block.Biome;
import org.bukkit.block.data.BlockData;

/**
 * A snapshot of a chunk that holds nothing but air. Block contents and light are not captured, so
 * every block reads as air; biomes come from the host's live biome data when the snapshot was
 * requested with biome data. Without biome data, biome queries fail instead of inventing a value.
 */
final class FerriteChunkSnapshot implements org.bukkit.ChunkSnapshot {
    private final int x, z, minHeight, maxHeight;
    private final String worldName;
    private final String worldId;
    private final Key worldKey;
    private final boolean includeBiome;
    private final long fullTime;
    /** Distinct biomes sampled from the 4x4x4 biome cells when {@link #contains(Biome)} is called. */
    private volatile java.util.Set<Biome> biomes;

    FerriteChunkSnapshot(int x, int z, String worldName, String worldId, Key worldKey, int minHeight, int maxHeight, long fullTime) {
        this(x, z, worldName, worldId, worldKey, minHeight, maxHeight, fullTime, true);
    }

    FerriteChunkSnapshot(int x, int z, String worldName, String worldId, Key worldKey,
                         int minHeight, int maxHeight, long fullTime, boolean includeBiome) {
        this.x = x; this.z = z; this.worldName = worldName; this.worldId = worldId; this.worldKey = worldKey;
        this.minHeight = minHeight; this.maxHeight = maxHeight; this.fullTime = fullTime; this.includeBiome = includeBiome;
    }

    private void check(int bx, int by, int bz) {
        if (bx < 0 || bx > 15 || bz < 0 || bz > 15) throw new IndexOutOfBoundsException("x/z must be within 0..15");
        if (by < minHeight || by >= maxHeight) throw new IndexOutOfBoundsException("y out of world height");
    }

    private String biomeKey(int bx, int by, int bz) {
        check(bx, by, bz);
        if (!includeBiome) throw new UnsupportedOperationException("Ferrite: this chunk snapshot was created without biome data");
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
    @Override public Material getBlockType(int bx, int by, int bz) { check(bx, by, bz); return Material.AIR; }
    @Override public BlockData getBlockData(int bx, int by, int bz) { check(bx, by, bz); return Material.AIR.createBlockData(); }
    @Override public int getData(int bx, int by, int bz) { check(bx, by, bz); return 0; }
    @Override public int getBlockSkyLight(int bx, int by, int bz) { check(bx, by, bz); return 0; }
    @Override public int getBlockEmittedLight(int bx, int by, int bz) { check(bx, by, bz); return 0; }
    /** An empty column reports the world's minimum height. */
    @Override public int getHighestBlockYAt(int bx, int bz) {
        if (bx < 0 || bx > 15 || bz < 0 || bz > 15) throw new IndexOutOfBoundsException("x/z must be within 0..15");
        return minHeight;
    }

    /** The live biome at this column; a snapshot does not copy the biome map. */
    @Override public Biome getBiome(int bx, int by, int bz) { return biome(bx, by, bz); }

    /** The deprecated 2D query reads the biome at y=0, as CraftBukkit does. */
    @Override public Biome getBiome(int bx, int bz) { return getBiome(bx, 0, bz); }
    /** The biome's vanilla `temperature` value, without the altitude cooling a live block applies. */
    @Override public double getRawBiomeTemperature(int bx, int by, int bz) {
        var key = biomeKey(bx, by, bz);
        String reply = (String) HostBridge.call("World", worldId, "biomeClimate(String)", String.class, key);
        return Double.parseDouble(reply.split(String.valueOf(HostBridge.ITEM))[0]);
    }
    @Override public double getRawBiomeTemperature(int bx, int bz) { return getRawBiomeTemperature(bx, 0, bz); }
    @Override public long getCaptureFullTime() { return fullTime; }
    @Override public boolean isSectionEmpty(int section) { return true; }
    @Override public boolean contains(BlockData block) { return block.getMaterial().isAir(); }
    @Override public boolean contains(Biome biome) {
        java.util.Objects.requireNonNull(biome, "biome");
        if (!includeBiome) return false;
        var cached = biomes;
        if (cached == null) {
            var found = new java.util.LinkedHashSet<Biome>();
            // Vanilla stores biomes per 4x4x4 cell; sampling each cell's first block is enough to
            // see every distinct biome without walking all 98k block positions.
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
            cached = java.util.Set.copyOf(found);
            biomes = cached;
        }
        return cached.contains(biome);
    }
}
