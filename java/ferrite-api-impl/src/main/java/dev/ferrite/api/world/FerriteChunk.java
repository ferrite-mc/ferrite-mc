package dev.ferrite.api.world;

import dev.ferrite.api.internal.Unsupported;
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
        throw Unsupported.exception("Chunk#getChunkSnapshot");
    }

    @Override
    public BlockState[] getTileEntities(boolean useSnapshot) {
        throw Unsupported.exception("Chunk#getTileEntities");
    }

    @Override
    public Collection<BlockState> getTileEntities(
            Predicate<? super Block> blockPredicate, boolean useSnapshot) {
        throw Unsupported.exception("Chunk#getTileEntities");
    }

    @Override
    public boolean unload(boolean save) {
        throw Unsupported.exception("Chunk#unload");
    }

    @Override
    public boolean unload() {
        throw Unsupported.exception("Chunk#unload");
    }

    @Override
    public boolean isSlimeChunk() {
        throw Unsupported.exception("Chunk#isSlimeChunk");
    }

    @Override
    public boolean isForceLoaded() {
        throw Unsupported.exception("Chunk#isForceLoaded");
    }

    @Override
    public void setForceLoaded(boolean forced) {
        throw Unsupported.exception("Chunk#setForceLoaded");
    }

    @Override
    public boolean addPluginChunkTicket(Plugin plugin) {
        throw Unsupported.exception("Chunk#addPluginChunkTicket");
    }

    @Override
    public boolean removePluginChunkTicket(Plugin plugin) {
        throw Unsupported.exception("Chunk#removePluginChunkTicket");
    }

    @Override
    public Collection<Plugin> getPluginChunkTickets() {
        throw Unsupported.exception("Chunk#getPluginChunkTickets");
    }

    @Override
    public long getInhabitedTime() {
        throw Unsupported.exception("Chunk#getInhabitedTime");
    }

    @Override
    public void setInhabitedTime(long ticks) {
        throw Unsupported.exception("Chunk#setInhabitedTime");
    }

    @Override
    public boolean contains(BlockData block) {
        throw Unsupported.exception("Chunk#contains");
    }

    @Override
    public boolean contains(Biome biome) {
        throw Unsupported.exception("Chunk#contains");
    }

    @Override
    public Collection<GeneratedStructure> getStructures() {
        throw Unsupported.exception("Chunk#getStructures");
    }

    @Override
    public Collection<GeneratedStructure> getStructures(Structure structure) {
        throw Unsupported.exception("Chunk#getStructures");
    }

    @Override
    public PersistentDataContainer getPersistentDataContainer() {
        throw Unsupported.exception("Chunk#getPersistentDataContainer");
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
