// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package org.bukkit.craftbukkit;

import java.util.List;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.level.block.state.BlockState;

/**
 * Compatibility base for Lane 2's world implementation. It adds only the NMS handle; every
 * Bukkit {@code World} method remains Lane 2's responsibility.
 */
public abstract class CraftWorld implements org.bukkit.World {
    protected CraftWorld() {}

    /** A snapshot handle bound to this world's UID; it cannot read or write another world. */
    public final ServerLevel getHandle() {
        UUID worldId = getUID();
        return ServerLevel.of(worldId);
    }

    // The Bukkit World methods themselves stay Lane 2's responsibility. These helpers expose the
    // selected NMS surface directly so a plugin that already has a CraftWorld does not need to
    // retain a second handle. Each call re-reads the immutable host world snapshot.

    /**
     * Reads a block state in this world. Outside-height coordinates fail explicitly.
     *
     * <p>The name carries the {@code NMS} prefix because {@code World} inherits
     * {@code RegionAccessor#getBlockState(int, int, int)} with a different Bukkit return type.
     */
    public final BlockState getNMSBlockState(int x, int y, int z) {
        return getHandle().getBlockState(x, y, z);
    }

    public final BlockState getNMSBlockState(BlockPos pos) {
        return getHandle().getBlockState(pos);
    }

    public final boolean isChunkLoaded(int chunkX, int chunkZ) {
        return getHandle().isChunkLoaded(chunkX, chunkZ);
    }

    /** Routes a block write to this world's owning region. */
    public final CompletableFuture<Void> setBlockAsync(BlockPos pos, BlockState state) {
        return getHandle().setBlockAsync(pos, state);
    }

    /** Coordinate overload of {@link #setBlockAsync(BlockPos, BlockState)}. */
    public final CompletableFuture<Void> setBlockAsync(int x, int y, int z, BlockState state) {
        return getHandle().setBlockAsync(x, y, z, state);
    }

    /** One loaded non-player entity, or null when the host reports it as unknown. */
    public final Entity getNMSEntity(UUID entityId) {
        return getHandle().getEntity(entityId);
    }

    /** Non-player entities the host reports as loaded in this world. */
    public final List<Entity> getNMSEntities() {
        return getHandle().getEntities();
    }

    /** One online player in this world, or null when the host reports it as unknown. */
    public final ServerPlayer getNMSPlayer(UUID playerId) {
        return getHandle().getPlayer(playerId);
    }

    /** Online players the host reports as present in this world. */
    public final List<ServerPlayer> getNMSPlayers() {
        return getHandle().getPlayers();
    }
}
