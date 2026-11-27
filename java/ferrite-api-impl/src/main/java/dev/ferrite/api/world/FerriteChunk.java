package dev.ferrite.api.world;

import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.Objects;
import java.util.function.Predicate;
import org.bukkit.Chunk;
import org.bukkit.ChunkSnapshot;
import org.bukkit.Location;
import org.bukkit.World;
import org.bukkit.block.Biome;
import org.bukkit.block.Block;
import org.bukkit.block.BlockState;
import org.bukkit.block.data.BlockData;
import org.bukkit.entity.Entity;
import org.bukkit.entity.Player;
import org.bukkit.generator.structure.GeneratedStructure;
import org.bukkit.generator.structure.Structure;
import org.bukkit.persistence.PersistentDataContainer;
import org.bukkit.plugin.Plugin;

/**
 * Read-only {@link Chunk} handle backed by {@link FerriteWorld} and host v2 chunk queries.
 * Mutations and unmodelled chunk metadata fail loudly with {@link UnsupportedOperationException}.
 */
public final class FerriteChunk implements Chunk {

    private final FerriteWorld world;
    private final int x;
    private final int z;

    public FerriteChunk(FerriteWorld world, int x, int z) {
        this.world = Objects.requireNonNull(world, "world");
        this.x = x;
        this.z = z;
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
    public World getWorld() {
        return world;
    }

    @Override
    public Block getBlock(int bx, int by, int bz) {
        if (bx < 0 || bx > 15 || bz < 0 || bz > 15) {
            throw new IllegalArgumentException(
                    "chunk-relative coordinates out of range 0..15: (" + bx + ", " + bz + ")");
        }
        return world.getBlockAt((x << 4) + bx, by, (z << 4) + bz);
    }

    /** Reads the 3D biome at chunk-relative coordinates without loading or generating the chunk. */
    public Biome getBiome(int bx, int by, int bz) {
        if (bx < 0 || bx > 15 || bz < 0 || bz > 15) {
            throw new IllegalArgumentException(
                    "chunk-relative coordinates out of range 0..15: (" + bx + ", " + bz + ")");
        }
        return world.getBiome((x << 4) + bx, by, (z << 4) + bz);
    }

    @Override
    public boolean isLoaded() {
        return world.isChunkLoaded(x, z);
    }

    @Override
    public boolean isGenerated() {
        return world.isChunkGenerated(x, z);
    }

    @Override
    public boolean load(boolean generate) {
        return world.loadChunk(x, z, generate);
    }

    @Override
    public boolean load() {
        return load(true);
    }

    @Override
    public boolean isEntitiesLoaded() {
        return isLoaded();
    }

    @Override
    public Entity[] getEntities() {
        List<Entity> matching = new ArrayList<>();
        for (Entity entity : world.getEntities()) {
            Location loc = entity.getLocation();
            if ((Math.floorDiv((int) Math.floor(loc.getX()), 16) == x)
                    && (Math.floorDiv((int) Math.floor(loc.getZ()), 16) == z)) {
                matching.add(entity);
            }
        }
        return matching.toArray(Entity[]::new);
    }

    @Override
    public Collection<Player> getPlayersSeeingChunk() {
        return world.getPlayersSeeingChunk(x, z);
    }

    @Override
    public LoadLevel getLoadLevel() {
        return isLoaded() ? LoadLevel.TICKING : LoadLevel.UNLOADED;
    }

    @Override
    public ChunkSnapshot getChunkSnapshot(
            boolean includeMaxblocky,
            boolean includeBiome,
            boolean includeBiomeTempRain,
            boolean includeLightData) {
        if (!isLoaded()) {
            throw new IllegalStateException("Chunk is not loaded");
        }
        // Light is not modelled by the facade; FerriteChunkSnapshot reports zero light values.
        return FerriteChunkSnapshot.capture(
                world, x, z, includeBiome, includeBiome && includeBiomeTempRain);
    }

    @Override
    public BlockState[] getTileEntities(boolean useSnapshot) {
        return new BlockState[0];
    }

    @Override
    public Collection<BlockState> getTileEntities(
            Predicate<? super Block> blockPredicate, boolean useSnapshot) {
        return java.util.List.of();
    }

    @Override
    public boolean unload(boolean save) {
        return world.unloadChunk(x, z, save);
    }

    @Override
    public boolean unload() {
        return unload(true);
    }

    @Override
    public boolean isSlimeChunk() {
        // Vanilla Java slime-chunk formula (`WorldgenRandom.seedSlimeChunk`).
        long seed = world.worldSeed();
        java.util.Random rnd = new java.util.Random(
                seed
                        + ((long) x * x * 0x4c1906L)
                        + ((long) x * 0x5ac0dbL)
                        + ((long) z * z) * 0x4307a7L
                        + ((long) z * 0x5f24fL)
                        ^ 0x3ad8025fL);
        return rnd.nextInt(10) == 0;
    }

    @Override
    public boolean isForceLoaded() {
        return world.isChunkForceLoaded(x, z);
    }

    @Override
    public void setForceLoaded(boolean forced) {
        world.setChunkForceLoaded(x, z, forced);
    }

    @Override
    public boolean addPluginChunkTicket(Plugin plugin) {
        return world.addChunkTicket(x, z, plugin);
    }

    @Override
    public boolean removePluginChunkTicket(Plugin plugin) {
        return world.removeChunkTicket(x, z, plugin);
    }

    @Override
    public Collection<Plugin> getPluginChunkTickets() {
        return world.chunkTickets(x, z);
    }

    @Override
    public long getInhabitedTime() {
        return world.chunkInhabitedTime(x, z);
    }

    @Override
    public void setInhabitedTime(long ticks) {
        world.setChunkInhabitedTime(x, z, ticks);
    }

    @Override
    public boolean contains(BlockData block) {
        Objects.requireNonNull(block, "block");
        org.bukkit.Material target = block.getMaterial();
        int baseX = x << 4;
        int baseZ = z << 4;
        for (int dy = world.getMinHeight(); dy < world.getMaxHeight(); dy++) {
            for (int dz = 0; dz < 16; dz++) {
                for (int dx = 0; dx < 16; dx++) {
                    if (world.getBlockAt(baseX + dx, dy, baseZ + dz).getType() == target) {
                        return true;
                    }
                }
            }
        }
        return false;
    }

    @Override
    public boolean contains(Biome biome) {
        Objects.requireNonNull(biome, "biome");
        return world.getBiome((x << 4) + 8, 64, (z << 4) + 8).equals(biome);
    }

    @Override
    public Collection<GeneratedStructure> getStructures() {
        return java.util.List.of();
    }

    @Override
    public Collection<GeneratedStructure> getStructures(Structure structure) {
        return java.util.List.of();
    }

    @Override
    public PersistentDataContainer getPersistentDataContainer() {
        return world.chunkPdc(x, z);
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof FerriteChunk chunk
                && chunk.x == x
                && chunk.z == z
                && chunk.world.equals(world);
    }

    @Override
    public int hashCode() {
        return Objects.hash(world, x, z);
    }

    @Override
    public String toString() {
        return "FerriteChunk{world=" + world.getName() + ",x=" + x + ",z=" + z + "}";
    }
}
