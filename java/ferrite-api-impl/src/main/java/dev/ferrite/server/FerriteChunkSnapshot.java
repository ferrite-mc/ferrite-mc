package dev.ferrite.server;

import org.bukkit.Material;
import org.bukkit.block.Biome;
import org.bukkit.block.data.BlockData;

/** A snapshot of a chunk that holds nothing but air. Light levels are always 0 and biomes are not held. */
final class FerriteChunkSnapshot implements org.bukkit.ChunkSnapshot {
    private final int x, z, minHeight, maxHeight;
    private final String worldName;
    private final long fullTime;

    FerriteChunkSnapshot(int x, int z, String worldName, int minHeight, int maxHeight, long fullTime) {
        this.x = x; this.z = z; this.worldName = worldName; this.minHeight = minHeight; this.maxHeight = maxHeight; this.fullTime = fullTime;
    }

    private void check(int bx, int by, int bz) {
        if (bx < 0 || bx > 15 || bz < 0 || bz > 15) throw new IndexOutOfBoundsException("x/z must be within 0..15");
        if (by < minHeight || by >= maxHeight) throw new IndexOutOfBoundsException("y out of world height");
    }

    @Override public int getX() { return x; }
    @Override public int getZ() { return z; }
    @Override public String getWorldName() { return worldName; }
    @Override public net.kyori.adventure.key.Key getWorldKey() { throw new UnsupportedOperationException("Ferrite: not implemented: ChunkSnapshot#getWorldKey"); }
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
    @Override public Biome getBiome(int bx, int bz) { throw new UnsupportedOperationException("Ferrite: not implemented: ChunkSnapshot#getBiome"); }
    @Override public Biome getBiome(int bx, int by, int bz) { throw new UnsupportedOperationException("Ferrite: not implemented: ChunkSnapshot#getBiome"); }
    @Override public double getRawBiomeTemperature(int bx, int bz) { throw new UnsupportedOperationException("Ferrite: not implemented: ChunkSnapshot#getRawBiomeTemperature"); }
    @Override public double getRawBiomeTemperature(int bx, int by, int bz) { throw new UnsupportedOperationException("Ferrite: not implemented: ChunkSnapshot#getRawBiomeTemperature"); }
    @Override public long getCaptureFullTime() { return fullTime; }
    @Override public boolean isSectionEmpty(int section) { return true; }
    @Override public boolean contains(BlockData block) { return block.getMaterial().isAir(); }
    @Override public boolean contains(Biome biome) { throw new UnsupportedOperationException("Ferrite: not implemented: ChunkSnapshot#contains"); }
}
