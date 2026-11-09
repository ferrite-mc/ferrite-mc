package dev.ferrite.server;

import org.bukkit.Material;
import org.bukkit.block.Biome;
import org.bukkit.block.data.BlockData;

/**
 * A snapshot of a chunk that holds nothing but air. Block contents and light are not captured, so
 * every block reads as air; biomes come from the host's live biome data.
 */
final class FerriteChunkSnapshot implements org.bukkit.ChunkSnapshot {
    private final int x, z, minHeight, maxHeight;
    private final String worldName;
    private final String worldId;
    private final net.kyori.adventure.key.Key worldKey;
    private final long fullTime;

    FerriteChunkSnapshot(int x, int z, String worldName, String worldId, net.kyori.adventure.key.Key worldKey, int minHeight, int maxHeight, long fullTime) {
        this.x = x; this.z = z; this.worldName = worldName; this.worldId = worldId; this.worldKey = worldKey;
        this.minHeight = minHeight; this.maxHeight = maxHeight; this.fullTime = fullTime;
    }

    private void check(int bx, int by, int bz) {
        if (bx < 0 || bx > 15 || bz < 0 || bz > 15) throw new IndexOutOfBoundsException("x/z must be within 0..15");
        if (by < minHeight || by >= maxHeight) throw new IndexOutOfBoundsException("y out of world height");
    }

    @Override public int getX() { return x; }
    @Override public int getZ() { return z; }
    @Override public String getWorldName() { return worldName; }
    @Override public net.kyori.adventure.key.Key getWorldKey() { return worldKey; }
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
    @Override public Biome getBiome(int bx, int by, int bz) {
        check(bx, by, bz);
        String key = Native.biomeAt(worldId, x * 16 + bx, by, z * 16 + bz);
        if (key == null) throw new UnsupportedOperationException("Ferrite: no biome data at chunk " + x + "," + z);
        Biome biome = org.bukkit.Registry.BIOME.get(org.bukkit.NamespacedKey.fromString(key));
        if (biome == null) throw new IllegalStateException("Unknown biome " + key);
        return biome;
    }

    /** The deprecated 2D query reads the biome at y=0, as CraftBukkit does. */
    @Override public Biome getBiome(int bx, int bz) { return getBiome(bx, 0, bz); }
    /** The biome's vanilla `temperature` value, without the altitude cooling a live block applies. */
    @Override public double getRawBiomeTemperature(int bx, int by, int bz) {
        check(bx, by, bz);
        String biome = Native.biomeAt(worldId, x * 16 + bx, by, z * 16 + bz);
        if (biome == null) throw new UnsupportedOperationException("Ferrite: no biome data at chunk " + x + "," + z);
        String reply = (String) HostBridge.call("World", worldId, "biomeClimate(String)", String.class, biome);
        return Double.parseDouble(reply.split(String.valueOf(HostBridge.ITEM))[0]);
    }
    @Override public double getRawBiomeTemperature(int bx, int bz) { return getRawBiomeTemperature(bx, 0, bz); }
    @Override public long getCaptureFullTime() { return fullTime; }
    @Override public boolean isSectionEmpty(int section) { return true; }
    @Override public boolean contains(BlockData block) { return block.getMaterial().isAir(); }
    @Override public boolean contains(Biome biome) { throw new UnsupportedOperationException("Ferrite: not implemented: ChunkSnapshot#contains"); }
}
