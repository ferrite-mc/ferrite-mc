// Clean-room Ferrite NMS test support. No Mojang code or assets.
package dev.ferrite.nms.test;

import dev.ferrite.nms.FerriteNmsVersion;
import dev.ferrite.nms.host.BlockStateSnapshot;
import dev.ferrite.nms.host.EntitySnapshot;
import dev.ferrite.nms.host.HostVersion;
import dev.ferrite.nms.host.NmsHost;
import dev.ferrite.nms.host.NmsHostException;
import dev.ferrite.nms.host.PlayerSnapshot;
import dev.ferrite.nms.host.WorldSnapshot;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;

/** In-memory host with a single named region executor; writes complete only when a test says so. */
public final class FakeNmsHost implements NmsHost {
    public static final UUID WORLD_ID = UUID.fromString("00000000-0000-0000-0000-000000000001");
    public static final UUID PLAYER_ID = UUID.fromString("00000000-0000-0000-0000-000000000003");
    public static final UUID ENTITY_ID = UUID.fromString("00000000-0000-0000-0000-000000000004");
    public static final UUID PASSENGER_ID = UUID.fromString("00000000-0000-0000-0000-000000000005");
    public static final UUID VEHICLE_ID = UUID.fromString("00000000-0000-0000-0000-000000000006");

    public final Map<UUID, WorldSnapshot> worlds = new HashMap<>();
    public final Map<UUID, PlayerSnapshot> players = new HashMap<>();
    public final Map<UUID, EntitySnapshot> entities = new HashMap<>();
    public final Map<String, Integer> registryIds = new HashMap<>();
    public final List<BlockWrite> blockWrites = new CopyOnWriteArrayList<>();
    public final List<Teleport> teleports = new CopyOnWriteArrayList<>();
    public final List<GameModeWrite> gameModeWrites = new CopyOnWriteArrayList<>();
    public final List<HealthWrite> healthWrites = new CopyOnWriteArrayList<>();
    public final AtomicReference<Thread> completionThread = new AtomicReference<>();
    public final ExecutorService region = Executors.newSingleThreadExecutor(runnable -> {
        Thread thread = new Thread(runnable, "ferrite-region-0");
        thread.setDaemon(true);
        return thread;
    });

    public volatile HostVersion version = FerriteNmsVersion.hostVersion();
    private volatile CompletableFuture<Void> pendingBlockWrite;

    public FakeNmsHost() {
        worlds.put(
                WORLD_ID,
                new WorldSnapshot(
                        WORLD_ID, "world", "minecraft:overworld", "NORMAL", 1234L, 6000L, 0.5, 64.0, 0.5, -64, 320));
        players.put(
                PLAYER_ID,
                new PlayerSnapshot(
                        PLAYER_ID, "Steve", WORLD_ID, 1.5, 65.0, 2.5, 90.0F, 10.0F, "SURVIVAL", 20.0F, 20));
        entities.put(
                ENTITY_ID,
                new EntitySnapshot(
                        ENTITY_ID,
                        "minecraft:armor_stand",
                        WORLD_ID,
                        3.0,
                        64.0,
                        4.0,
                        45.0F,
                        5.0F,
                        0.1,
                        0.2,
                        0.3,
                        VEHICLE_ID,
                        List.of(PASSENGER_ID)));
        entities.put(
                PASSENGER_ID,
                new EntitySnapshot(
                        PASSENGER_ID, "minecraft:pig", WORLD_ID, 3.0, 64.0, 4.0, 0.0F, 0.0F, 0.0, 0.0, 0.0, ENTITY_ID, List.of()));
        entities.put(
                VEHICLE_ID,
                new EntitySnapshot(
                        VEHICLE_ID, "minecraft:oak_boat", WORLD_ID, 3.0, 64.0, 4.0, 0.0F, 0.0F, 0.0, 0.0, 0.0, null, List.of(ENTITY_ID)));
        registryIds.put("minecraft:stone", 1);
        registryIds.put(
                "minecraft:oak_stairs[facing=north,half=top,shape=straight,waterlogged=true]", 5463);
    }

    @Override
    public HostVersion version() {
        return version;
    }

    @Override
    public WorldSnapshot world(UUID worldId) {
        WorldSnapshot world = worlds.get(worldId);
        if (world == null) {
            throw new NmsHostException("unknown_world", "world " + worldId + " is not loaded");
        }
        return world;
    }

    @Override
    public BlockStateSnapshot blockState(UUID worldId, int x, int y, int z) {
        world(worldId);
        if (x == 0 && y == 64 && z == 0) {
            return new BlockStateSnapshot(1, "minecraft:stone", "", false);
        }
        return new BlockStateSnapshot(0, "minecraft:air", "", true);
    }

    @Override
    public boolean isChunkLoaded(UUID worldId, int chunkX, int chunkZ) {
        world(worldId);
        return chunkX == 0 && chunkZ == 0;
    }

    @Override
    public CompletableFuture<Void> setBlockState(UUID worldId, int x, int y, int z, int globalStateId) {
        world(worldId);
        if (pendingBlockWrite != null && !pendingBlockWrite.isDone()) {
            throw new IllegalStateException("the previous fake block write was never completed");
        }
        blockWrites.add(new BlockWrite(worldId, x, y, z, globalStateId));
        pendingBlockWrite = new CompletableFuture<>();
        return pendingBlockWrite;
    }

    /** Completes the pending write on the fake region thread, like a real region tick would. */
    public void completeBlockWrite() {
        CompletableFuture<Void> future = pendingBlockWrite;
        if (future == null) {
            throw new IllegalStateException("no block write is pending");
        }
        try {
            region.submit(() -> {
                        completionThread.set(Thread.currentThread());
                        future.complete(null);
                    })
                    .get(5, TimeUnit.SECONDS);
        } catch (Exception error) {
            throw new IllegalStateException("could not complete the fake block write", error);
        }
    }

    public void failBlockWrite(Throwable failure) {
        CompletableFuture<Void> future = pendingBlockWrite;
        if (future == null) {
            throw new IllegalStateException("no block write is pending");
        }
        try {
            region.submit(() -> {
                        completionThread.set(Thread.currentThread());
                        future.completeExceptionally(failure);
                    })
                    .get(5, TimeUnit.SECONDS);
        } catch (Exception error) {
            throw new IllegalStateException("could not fail the fake block write", error);
        }
    }

    @Override
    public int registryBlockStateId(String blockStateText) {
        Integer id = registryIds.get(blockStateText);
        if (id == null) {
            throw new NmsHostException("unknown_block_state", "block state " + blockStateText + " is not registered");
        }
        return id;
    }

    @Override
    public PlayerSnapshot player(UUID playerId) {
        PlayerSnapshot player = players.get(playerId);
        if (player == null) {
            throw new NmsHostException("unknown_player", "player " + playerId + " is not online");
        }
        return player;
    }

    @Override
    public EntitySnapshot entity(UUID entityId) {
        EntitySnapshot entity = entities.get(entityId);
        if (entity == null) {
            throw new NmsHostException("unknown_entity", "entity " + entityId + " is not loaded");
        }
        return entity;
    }

    @Override
    public CompletableFuture<Void> teleport(
            UUID entityId, UUID worldId, double x, double y, double z, float yaw, float pitch) {
        playerOrEntity(entityId);
        world(worldId);
        teleports.add(new Teleport(entityId, worldId, x, y, z, yaw, pitch));
        return CompletableFuture.completedFuture(null);
    }

    @Override
    public CompletableFuture<Void> setGameMode(UUID playerId, String gameMode) {
        player(playerId);
        gameModeWrites.add(new GameModeWrite(playerId, gameMode));
        return CompletableFuture.completedFuture(null);
    }

    @Override
    public CompletableFuture<Void> setHealth(UUID playerId, float health) {
        player(playerId);
        healthWrites.add(new HealthWrite(playerId, health));
        return CompletableFuture.completedFuture(null);
    }

    @Override
    public void close() {
        region.shutdownNow();
    }

    private void playerOrEntity(UUID id) {
        if (players.containsKey(id)) {
            return;
        }
        entity(id);
    }

    public record BlockWrite(UUID worldId, int x, int y, int z, int globalStateId) {}

    public record Teleport(UUID entityId, UUID worldId, double x, double y, double z, float yaw, float pitch) {}

    public record GameModeWrite(UUID playerId, String gameMode) {}

    public record HealthWrite(UUID playerId, float health) {}
}
