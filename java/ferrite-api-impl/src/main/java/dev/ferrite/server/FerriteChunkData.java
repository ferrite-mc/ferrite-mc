package dev.ferrite.server;

import org.bukkit.HeightMap;
import org.bukkit.Material;
import org.bukkit.block.Biome;
import org.bukkit.block.data.BlockData;
import org.bukkit.generator.ChunkGenerator.ChunkData;
import org.bukkit.material.MaterialData;

/** A 16 x height x 16 block buffer a chunk generator fills, held in memory. */
@SuppressWarnings("deprecation")
final class FerriteChunkData implements ChunkData {
    private final int minHeight, maxHeight;
    private final BlockData[] blocks;

    FerriteChunkData(int minHeight, int maxHeight) {
        this.minHeight = minHeight;
        this.maxHeight = maxHeight;
        this.blocks = new BlockData[(maxHeight - minHeight) * 256];
    }

    private boolean inside(int x, int y, int z) { return x >= 0 && x < 16 && z >= 0 && z < 16 && y >= minHeight && y < maxHeight; }
    private int index(int x, int y, int z) { return ((y - minHeight) * 16 + z) * 16 + x; }

    @Override public int getMinHeight() { return minHeight; }
    @Override public int getMaxHeight() { return maxHeight; }
    @Override public Biome getBiome(int x, int y, int z) { throw new UnsupportedOperationException("Ferrite: not implemented: ChunkData#getBiome"); }

    @Override public void setBlock(int x, int y, int z, Material material) { setBlock(x, y, z, java.util.Objects.requireNonNull(material, "material").createBlockData()); }
    @Override public void setBlock(int x, int y, int z, MaterialData material) { setBlock(x, y, z, material.getItemType().createBlockData()); }
    @Override public void setBlock(int x, int y, int z, BlockData data) {
        java.util.Objects.requireNonNull(data, "data");
        if (inside(x, y, z)) blocks[index(x, y, z)] = data.getMaterial().isAir() ? null : data.clone();
    }

    @Override public void setRegion(int xMin, int yMin, int zMin, int xMax, int yMax, int zMax, Material material) { setRegion(xMin, yMin, zMin, xMax, yMax, zMax, material.createBlockData()); }
    @Override public void setRegion(int xMin, int yMin, int zMin, int xMax, int yMax, int zMax, MaterialData material) { setRegion(xMin, yMin, zMin, xMax, yMax, zMax, material.getItemType().createBlockData()); }
    /** The maximum corner is exclusive, as in Bukkit. */
    @Override public void setRegion(int xMin, int yMin, int zMin, int xMax, int yMax, int zMax, BlockData data) {
        for (int y = Math.max(yMin, minHeight); y < Math.min(yMax, maxHeight); y++)
            for (int z = Math.max(zMin, 0); z < Math.min(zMax, 16); z++)
                for (int x = Math.max(xMin, 0); x < Math.min(xMax, 16); x++) setBlock(x, y, z, data);
    }

    @Override public Material getType(int x, int y, int z) { return getBlockData(x, y, z).getMaterial(); }
    @Override public MaterialData getTypeAndData(int x, int y, int z) { return new MaterialData(getType(x, y, z)); }
    @Override public BlockData getBlockData(int x, int y, int z) {
        var data = inside(x, y, z) ? blocks[index(x, y, z)] : null;
        return data == null ? Material.AIR.createBlockData() : data.clone();
    }
    @Override public byte getData(int x, int y, int z) { return 0; }

    /** One above the highest block that is not air; the minimum height for an empty column. Every heightmap type counts any non-air block. */
    @Override public int getHeight(HeightMap heightMap, int x, int z) {
        java.util.Objects.requireNonNull(heightMap, "heightMap");
        if (x < 0 || x >= 16 || z < 0 || z >= 16) return minHeight;
        for (int y = maxHeight - 1; y >= minHeight; y--) if (blocks[index(x, y, z)] != null) return y + 1;
        return minHeight;
    }
}
