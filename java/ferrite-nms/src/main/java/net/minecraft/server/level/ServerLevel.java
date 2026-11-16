// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.server.level;

import dev.ferrite.nms.NmsUnsupported;
import dev.ferrite.nms.host.BlockStateSnapshot;
import dev.ferrite.nms.host.NmsHost;
import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.WorldSnapshot;
import java.util.Objects;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.Vec3;
import org.bukkit.Material;

/**
 * Snapshot handle to a Rust world. It stores only the world ID and the last read snapshot; block
 * reads ask the host every time, and writes are routed to the owning region asynchronously.
 */
public final class ServerLevel {
    private final UUID id;
    private final WorldSnapshot snapshot;

    private ServerLevel(WorldSnapshot snapshot) {
        this.id = snapshot.id();
        this.snapshot = snapshot;
    }

    /** Binds to an existing world; an unknown world ID fails in the host, not here. */
    public static ServerLevel of(UUID worldId) {
        Objects.requireNonNull(worldId, "worldId");
        return new ServerLevel(NmsHostBinding.require().world(worldId));
    }

    public UUID getUUID() {
        return id;
    }

    public String getName() {
        return snapshot.name();
    }

    public String dimensionKey() {
        return snapshot.dimensionKey();
    }

    public String environment() {
        return snapshot.environment();
    }

    public long getSeed() {
        return snapshot.seed();
    }

    public long getDayTime() {
        return snapshot.dayTime();
    }

    public Vec3 getSpawn() {
        return new Vec3(snapshot.spawnX(), snapshot.spawnY(), snapshot.spawnZ());
    }

    public BlockPos getSpawnPos() {
        return new BlockPos(
                (int) Math.floor(snapshot.spawnX()),
                (int) Math.floor(snapshot.spawnY()),
                (int) Math.floor(snapshot.spawnZ()));
    }

    public int getMinY() {
        return snapshot.minY();
    }

    public int getMaxY() {
        return snapshot.maxY();
    }

    public ServerChunkCache getChunkSource() {
        return new ServerChunkCache(this);
    }

    public boolean isChunkLoaded(int chunkX, int chunkZ) {
        return host().isChunkLoaded(id, chunkX, chunkZ);
    }

    public BlockState getBlockState(BlockPos pos) {
        Objects.requireNonNull(pos, "pos");
        return getBlockState(pos.x, pos.y, pos.z);
    }

    public BlockState getBlockState(int x, int y, int z) {
        BlockStateSnapshot state = host().blockState(id, x, y, z);
        Material material = Material.matchMaterial(state.name());
        if (material == null) {
            throw new IllegalStateException(
                    "The Ferrite host returned block state " + state.asString()
                            + " which has no 26.3 Bukkit material");
        }
        return new BlockState(new Block(material), state.asString(), state.globalId());
    }

    /**
     * Synchronous writes are deliberately refused: completing one would require parking the
     * calling Java thread until the owning region ticks.
     */
    public boolean setBlock(BlockPos pos, BlockState state) {
        throw NmsUnsupported.operation(
                "net.minecraft.server.level.ServerLevel",
                "setBlock",
                "a synchronous write would block the calling Java thread on a region; use "
                        + "setBlockAsync(BlockPos, BlockState) so the host can route the write to the owning region");
    }

    /** Routes a block write to the owning region; the future completes on that region thread. */
    public CompletableFuture<Void> setBlockAsync(BlockPos pos, BlockState state) {
        Objects.requireNonNull(pos, "pos");
        Objects.requireNonNull(state, "state");
        return host().setBlockState(id, pos.x, pos.y, pos.z, state.resolveGlobalId());
    }

    /** The world ID this handle is allowed to touch; it can never widen to another world. */
    public boolean owns(UUID worldId) {
        return id.equals(worldId);
    }

    private static NmsHost host() {
        return NmsHostBinding.require();
    }

    @Override
    public String toString() {
        return "ServerLevel{" + id + ", " + snapshot.name() + "}";
    }
}
