// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms.host;

import java.util.List;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;

/**
 * The state port every NMS wrapper uses.
 *
 * <p><strong>This is a Lane 2/3 port, not Lane 1's wire contract.</strong> Contract v1
 * ({@code docs/plugin-b-host-contract.md}) defines JNI, lifecycle, events, the scheduler and
 * {@code HostApi}, but no world/block/player/registry ops. §12 says Lane 3 uses Lane 2; Lane 2's
 * facade implementation (or a future contract version) is therefore the intended provider of
 * this port. It must preserve the contract's naming, error codes, timeout and owner-thread rules.
 *
 * <p>Reads are synchronous snapshots. Writes return a future completed by the provider on the
 * owning region/server thread; the caller's Java thread is never parked waiting for a region tick.
 */
public interface NmsHost extends AutoCloseable {
    /** The Minecraft/protocol/data version this host serves. */
    HostVersion version();

    /** Reads one world. Unknown world IDs must throw {@link NmsHostException}. */
    WorldSnapshot world(UUID worldId) throws NmsHostException;

    /** Reads a block state, including its 26.3 global state ID. */
    BlockStateSnapshot blockState(UUID worldId, int x, int y, int z) throws NmsHostException;

    boolean isChunkLoaded(UUID worldId, int chunkX, int chunkZ) throws NmsHostException;

    /**
     * Routes a block write to the world's owning region. The returned future completes on that
     * region thread. The state ID is the canonical 26.3 global block-state ID.
     */
    CompletableFuture<Void> setBlockState(UUID worldId, int x, int y, int z, int globalStateId);

    /** Resolves a canonical block-state string to the 26.3 global state ID. */
    int registryBlockStateId(String blockStateText) throws NmsHostException;

    /** Reads an online player. Unknown player IDs must throw {@link NmsHostException}. */
    PlayerSnapshot player(UUID playerId) throws NmsHostException;

    /** Reads an entity. Unknown entity IDs must throw {@link NmsHostException}. */
    EntitySnapshot entity(UUID entityId) throws NmsHostException;

    /**
     * Lists the non-player entities currently loaded in {@code worldId}.
     *
     * <p>The default implementation is an explicit capability refusal so existing host adapters
     * keep compiling. A host that can enumerate a world must override it; returning an empty list
     * is not a substitute for implementing it because an empty world and a missing capability are
     * different answers. Every returned snapshot must belong to {@code worldId}.
     */
    default List<EntitySnapshot> entities(UUID worldId) throws NmsHostException {
        world(worldId);
        throw new NmsHostException(
                "unsupported_snapshot",
                "The attached Ferrite host does not implement world entity listing for "
                        + worldId
                        + "; implement NmsHost.entities(UUID) and include the optional EntityState "
                        + "block to serve the NMS CraftWorld entity surface");
    }

    /**
     * Lists the online players currently in {@code worldId}.
     *
     * <p>Like {@link #entities(UUID)} this defaults to an explicit capability refusal, and every
     * returned snapshot must belong to {@code worldId}.
     */
    default List<PlayerSnapshot> players(UUID worldId) throws NmsHostException {
        world(worldId);
        throw new NmsHostException(
                "unsupported_snapshot",
                "The attached Ferrite host does not implement world player listing for "
                        + worldId
                        + "; implement NmsHost.players(UUID) and include the optional PlayerState "
                        + "block to serve the NMS CraftWorld player surface");
    }

    /** Routes a teleport to the entity's owning region. */
    CompletableFuture<Void> teleport(
            UUID entityId, UUID worldId, double x, double y, double z, float yaw, float pitch);

    /** Routes a game-mode change to the player's owning region. */
    CompletableFuture<Void> setGameMode(UUID playerId, String gameMode);

    /** Routes a health change to the player's owning region. */
    CompletableFuture<Void> setHealth(UUID playerId, float health);

    @Override
    default void close() {}
}
