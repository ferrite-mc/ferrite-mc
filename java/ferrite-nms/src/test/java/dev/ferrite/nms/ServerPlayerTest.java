// Clean-room Ferrite NMS test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.nms.bridge.NmsBridge;
import dev.ferrite.nms.host.BlockStateSnapshot;
import dev.ferrite.nms.host.EntitySnapshot;
import dev.ferrite.nms.host.HostVersion;
import dev.ferrite.nms.host.NmsHost;
import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.NmsHostException;
import dev.ferrite.nms.host.PlayerSnapshot;
import dev.ferrite.nms.host.WorldSnapshot;
import dev.ferrite.nms.test.FakeNmsHost;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.entity.EntitySnapshotFactory;
import org.bukkit.GameMode;
import org.bukkit.Location;
import org.bukkit.craftbukkit.entity.CraftPlayer;
import org.bukkit.craftbukkit.entity.StubCraftPlayer;
import org.bukkit.entity.EntityType;
import org.bukkit.util.Vector;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class ServerPlayerTest {
    private FakeNmsHost host;

    @BeforeEach
    void installFakeHost() {
        host = new FakeNmsHost();
        NmsHostBinding.install(host);
    }

    @AfterEach
    void removeFakeHost() {
        NmsBridge.close();
        try {
            NmsHostBinding.uninstall(host);
        } catch (IllegalStateException ignored) {
            // Deliberately uninstalled by a test.
        }
        host.close();
    }

    @Test
    void readsThePlayerSnapshot() {
        ServerPlayer player = ServerPlayer.of(FakeNmsHost.PLAYER_ID);
        assertEquals(FakeNmsHost.PLAYER_ID, player.getUUID());
        assertEquals("Steve", player.getName());
        assertEquals("minecraft:player", player.getTypeKey());
        assertEquals("SURVIVAL", player.gameMode());
        assertEquals(20.0F, player.getHealth());
        assertEquals(20, player.getFoodLevel());
        assertEquals(1.5, player.getX());
        assertEquals(65.0, player.getY());
        assertEquals(2.5, player.getZ());
        assertEquals(90.0F, player.getYRot());
        assertEquals(10.0F, player.getXRot());
        assertEquals(FakeNmsHost.WORLD_ID, player.level().getUUID());
        assertTrue(player.passengerIds().isEmpty());
    }

    @Test
    void unknownPlayersFailWithTheHostCode() {
        NmsHostException error = assertThrows(NmsHostException.class,
                () -> ServerPlayer.of(UUID.randomUUID()));
        assertEquals("unknown_player", error.code());
    }

    @Test
    void safeWritesAreAsynchronousHostOperations() throws Exception {
        ServerPlayer player = ServerPlayer.of(FakeNmsHost.PLAYER_ID);
        player.setHealthAsync(15.0F).get(5, TimeUnit.SECONDS);
        player.setGameModeAsync("CREATIVE").get(5, TimeUnit.SECONDS);

        assertEquals(new FakeNmsHost.HealthWrite(FakeNmsHost.PLAYER_ID, 15.0F), host.healthWrites.get(0));
        assertEquals(new FakeNmsHost.GameModeWrite(FakeNmsHost.PLAYER_ID, "CREATIVE"), host.gameModeWrites.get(0));
    }

    @Test
    void invalidWritesAreRefusedBeforeTouchingTheHost() {
        ServerPlayer player = ServerPlayer.of(FakeNmsHost.PLAYER_ID);
        assertThrows(IllegalArgumentException.class, () -> player.setHealthAsync(-1.0F));
        assertThrows(IllegalArgumentException.class, () -> player.setHealthAsync(Float.NaN));
        assertThrows(IllegalArgumentException.class, () -> player.setGameModeAsync(" "));
        assertThrows(IllegalArgumentException.class, () -> player.setGameModeAsync("NOT_A_MODE"));
        assertTrue(host.healthWrites.isEmpty());
        assertTrue(host.gameModeWrites.isEmpty());
    }

    @Test
    void craftPlayerGetHandleAndFirstSliceAccessorsUseHostSnapshots() {
        CraftPlayer craft = new StubCraftPlayer(FakeNmsHost.PLAYER_ID);
        ServerPlayer handle = craft.getHandle();
        assertEquals(FakeNmsHost.PLAYER_ID, handle.getUUID());
        assertEquals("Steve", craft.getName());
        assertEquals("Steve", handle.getScoreboardName());
        assertEquals(GameMode.SURVIVAL, craft.getGameMode());
        assertEquals(20.0, craft.getHealth());
        assertEquals(20, craft.getFoodLevel());
        assertEquals(EntityType.PLAYER, craft.getType());

        Location loc = craft.getLocation();
        assertEquals(1.5, loc.getX());
        assertEquals(65.0, loc.getY());
        assertEquals(2.5, loc.getZ());
        assertEquals(90.0F, loc.getYaw());
        assertEquals(10.0F, loc.getPitch());
        assertEquals(new Vector(0.0, 0.0, 0.0), craft.getVelocity());
        assertNull(craft.getVehicle());
        assertTrue(craft.getPassengers().isEmpty());

        ServerPlayer bridged = NmsBridge.player(craft);
        assertSame(bridged, NmsBridge.entity(craft));
        assertEquals(FakeNmsHost.PLAYER_ID, bridged.getUUID());
    }

    @Test
    void serverPlayerNormalizesGameModeAndRoutesAsyncWritesOnRegionThread() throws Exception {
        CompletableFuture<Void> pendingHealth = new CompletableFuture<>();
        CompletableFuture<Void> pendingGameMode = new CompletableFuture<>();
        AtomicReference<Thread> completionThread = new AtomicReference<>();

        NmsHost deferredHost = new NmsHost() {
            @Override
            public HostVersion version() {
                return host.version();
            }

            @Override
            public WorldSnapshot world(UUID worldId) {
                return host.world(worldId);
            }

            @Override
            public BlockStateSnapshot blockState(UUID worldId, int x, int y, int z) {
                return host.blockState(worldId, x, y, z);
            }

            @Override
            public boolean isChunkLoaded(UUID worldId, int chunkX, int chunkZ) {
                return host.isChunkLoaded(worldId, chunkX, chunkZ);
            }

            @Override
            public CompletableFuture<Void> setBlockState(UUID worldId, int x, int y, int z, int globalStateId) {
                return host.setBlockState(worldId, x, y, z, globalStateId);
            }

            @Override
            public int registryBlockStateId(String blockStateText) {
                return host.registryBlockStateId(blockStateText);
            }

            @Override
            public PlayerSnapshot player(UUID playerId) {
                return host.player(playerId);
            }

            @Override
            public EntitySnapshot entity(UUID entityId) {
                return host.entity(entityId);
            }

            @Override
            public CompletableFuture<Void> teleport(
                    UUID entityId, UUID worldId, double x, double y, double z, float yaw, float pitch) {
                return host.teleport(entityId, worldId, x, y, z, yaw, pitch);
            }

            @Override
            public CompletableFuture<Void> setGameMode(UUID playerId, String gameMode) {
                host.setGameMode(playerId, gameMode);
                return pendingGameMode;
            }

            @Override
            public CompletableFuture<Void> setHealth(UUID playerId, float health) {
                host.setHealth(playerId, health);
                return pendingHealth;
            }
        };

        NmsHostBinding.install(deferredHost);
        try {
            ServerPlayer player = ServerPlayer.of(FakeNmsHost.PLAYER_ID);
            CompletableFuture<Void> healthFuture = player.setHealthAsync(12.5F);
            CompletableFuture<Void> modeFuture = player.setGameModeAsync("adventure");
            CompletableFuture<Void> enumModeFuture = player.setGameModeAsync(GameMode.SPECTATOR);

            assertFalse(healthFuture.isDone(), "setHealthAsync must not block for region tick");
            assertFalse(modeFuture.isDone(), "setGameModeAsync must not block for region tick");
            assertEquals(new FakeNmsHost.HealthWrite(FakeNmsHost.PLAYER_ID, 12.5F), host.healthWrites.get(0));
            assertEquals(new FakeNmsHost.GameModeWrite(FakeNmsHost.PLAYER_ID, "ADVENTURE"), host.gameModeWrites.get(0));
            assertEquals(new FakeNmsHost.GameModeWrite(FakeNmsHost.PLAYER_ID, "SPECTATOR"), host.gameModeWrites.get(1));

            host.region.submit(() -> {
                        completionThread.set(Thread.currentThread());
                        pendingHealth.complete(null);
                        pendingGameMode.complete(null);
                    })
                    .get(5, TimeUnit.SECONDS);

            healthFuture.get(5, TimeUnit.SECONDS);
            modeFuture.get(5, TimeUnit.SECONDS);
            enumModeFuture.get(5, TimeUnit.SECONDS);
            assertEquals("ferrite-region-0", completionThread.get().getName());
            assertNotEquals(Thread.currentThread().getName(), completionThread.get().getName());
        } finally {
            NmsHostBinding.install(host);
        }
    }

    @Test
    void serverPlayerAndCraftPlayerExplicitlyRefuseSyncAndUnsupportedOperations() {
        CraftPlayer craft = new StubCraftPlayer(FakeNmsHost.PLAYER_ID);
        ServerPlayer player = craft.getHandle();

        assertTrue(assertThrows(UnsupportedOperationException.class, () -> player.setGameMode("CREATIVE"))
                .getMessage().contains("ServerPlayer#setGameMode"));
        assertTrue(assertThrows(UnsupportedOperationException.class, () -> player.setHealth(10.0F))
                .getMessage().contains("ServerPlayer#setHealth"));
        assertTrue(assertThrows(UnsupportedOperationException.class, player::getConnection)
                .getMessage().contains("ServerPlayer#getConnection"));
        assertTrue(assertThrows(UnsupportedOperationException.class, () -> player.openMenu(null))
                .getMessage().contains("ServerPlayer#openMenu"));

        assertTrue(assertThrows(UnsupportedOperationException.class, () -> craft.setGameMode(GameMode.CREATIVE))
                .getMessage().contains("CraftPlayer#setGameMode"));
        assertTrue(assertThrows(UnsupportedOperationException.class, () -> craft.setHealth(10.0))
                .getMessage().contains("CraftPlayer#setHealth"));
        assertTrue(assertThrows(UnsupportedOperationException.class, () -> craft.setFoodLevel(10))
                .getMessage().contains("CraftPlayer#setFoodLevel"));
    }

    @Test
    void playerSnapshotAndEntitySnapshotFactoryValidateInputs() {
        assertThrows(NullPointerException.class, () -> EntitySnapshotFactory.forPlayer(null));
        assertThrows(
                IllegalArgumentException.class,
                () -> new PlayerSnapshot(
                        FakeNmsHost.PLAYER_ID, " ", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, "SURVIVAL", 20.0F, 20));
        assertThrows(
                IllegalArgumentException.class,
                () -> new PlayerSnapshot(
                        FakeNmsHost.PLAYER_ID, "Steve", FakeNmsHost.WORLD_ID, 0, Double.POSITIVE_INFINITY, 0, 0, 0, "SURVIVAL", 20.0F, 20));
        assertThrows(
                IllegalArgumentException.class,
                () -> new PlayerSnapshot(
                        FakeNmsHost.PLAYER_ID, "Steve", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, " ", 20.0F, 20));
        assertThrows(
                IllegalArgumentException.class,
                () -> new PlayerSnapshot(
                        FakeNmsHost.PLAYER_ID, "Steve", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, "SURVIVAL", -0.5F, 20));
        assertThrows(
                IllegalArgumentException.class,
                () -> new PlayerSnapshot(
                        FakeNmsHost.PLAYER_ID, "Steve", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, "SURVIVAL", 20.0F, 21));

        PlayerSnapshot snapshot = host.player(FakeNmsHost.PLAYER_ID);
        EntitySnapshot entity = EntitySnapshotFactory.forPlayer(snapshot);
        assertEquals(snapshot.id(), entity.id());
        assertEquals("minecraft:player", entity.typeKey());
        assertEquals(snapshot.worldId(), entity.worldId());
        assertEquals(snapshot.x(), entity.x());
        assertEquals(snapshot.y(), entity.y());
        assertEquals(snapshot.z(), entity.z());
        assertEquals(snapshot.yaw(), entity.yaw());
        assertEquals(snapshot.pitch(), entity.pitch());
        assertNull(entity.vehicleId());
        assertTrue(entity.passengers().isEmpty());
    }
}
