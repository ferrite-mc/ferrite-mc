// Clean-room Ferrite NMS test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.nms.bridge.NmsBridge;
import dev.ferrite.nms.host.EntitySnapshot;
import dev.ferrite.nms.host.NmsHost;
import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.NmsHostException;
import dev.ferrite.nms.host.PlayerSnapshot;
import dev.ferrite.nms.test.FakeNmsHost;
import java.lang.reflect.Proxy;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.TimeUnit;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.entity.EntitySnapshotFactory;
import org.bukkit.GameMode;
import org.bukkit.Location;
import org.bukkit.World;
import org.bukkit.craftbukkit.entity.CraftPlayer;
import org.bukkit.entity.EntityType;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class ServerPlayerTest {
    private FakeNmsHost host;

    @BeforeEach
    void installFakeHost() {
        NmsBridge.close();
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
        assertEquals("Steve", player.getScoreboardName());
        assertEquals("minecraft:player", player.getTypeKey());
        assertEquals("SURVIVAL", player.gameMode());
        assertEquals(GameMode.SURVIVAL, player.bukkitGameMode());
        assertEquals(20.0F, player.getHealth());
        assertEquals(20, player.getFoodLevel());
        assertEquals(1.5, player.getX());
        assertEquals(65.0, player.getY());
        assertEquals(2.5, player.getZ());
        assertEquals(90.0F, player.getYRot());
        assertEquals(10.0F, player.getXRot());
        assertEquals(7, player.getId());
        assertFalse(player.isRemoved());
        assertEquals(1200, player.tickCount());
        assertTrue(player.isOnline());
        assertTrue(player.isValid());
        assertEquals(37, player.ping());
        assertEquals("en_us", player.locale());
        assertEquals(5.0F, player.saturation());
        assertEquals(0.2F, player.exhaustion());
        assertEquals(12, player.experienceLevel());
        assertEquals(0.5F, player.experienceProgress());
        assertEquals(345, player.totalExperience());
        assertEquals(0.1F, player.walkSpeed());
        assertEquals(0.05F, player.flySpeed());
        assertFalse(player.mayFly());
        assertFalse(player.isFlying());
        assertFalse(player.isSneaking());
        assertTrue(player.isSprinting());
        assertEquals(300, player.getRemainingAir());
        assertEquals(300, player.getMaximumAir());
        assertEquals("Steve", player.displayName());
        assertEquals("Steve", player.playerListName());
        assertEquals(6000L, player.playerTime());
        assertEquals(0L, player.playerTimeOffset());
        assertTrue(player.isPlayerTimeRelative());
        assertEquals(FakeNmsHost.WORLD_ID, player.level().getUUID());
        assertTrue(player.passengerIds().isEmpty());
        assertNull(player.getVehicle());
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
        player.setGameModeAsync(GameMode.ADVENTURE).get(5, TimeUnit.SECONDS);
        player.setGameModeAsync("spectator").get(5, TimeUnit.SECONDS);

        assertEquals(new FakeNmsHost.HealthWrite(FakeNmsHost.PLAYER_ID, 15.0F), host.healthWrites.get(0));
        assertEquals(new FakeNmsHost.GameModeWrite(FakeNmsHost.PLAYER_ID, "CREATIVE"), host.gameModeWrites.get(0));
        assertEquals(new FakeNmsHost.GameModeWrite(FakeNmsHost.PLAYER_ID, "ADVENTURE"), host.gameModeWrites.get(1));
        assertEquals(new FakeNmsHost.GameModeWrite(FakeNmsHost.PLAYER_ID, "SPECTATOR"), host.gameModeWrites.get(2));
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
    void playerSnapshotValidationAndFactoryPreserveInvariants() {
        UUID id = UUID.randomUUID();
        assertThrows(
                IllegalArgumentException.class,
                () -> new PlayerSnapshot(id, " ", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, "SURVIVAL", 20.0F, 20));
        assertThrows(
                IllegalArgumentException.class,
                () -> new PlayerSnapshot(id, "Alex", FakeNmsHost.WORLD_ID, Double.NaN, 64, 0, 0, 0, "SURVIVAL", 20.0F, 20));
        assertThrows(
                IllegalArgumentException.class,
                () -> new PlayerSnapshot(id, "Alex", FakeNmsHost.WORLD_ID, 0, 64, 0, Float.NaN, 0, "SURVIVAL", 20.0F, 20));
        assertThrows(
                IllegalArgumentException.class,
                () -> new PlayerSnapshot(id, "Alex", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, " ", 20.0F, 20));
        assertThrows(
                IllegalArgumentException.class,
                () -> new PlayerSnapshot(id, "Alex", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, "SURVIVAL", -0.5F, 20));
        assertThrows(
                IllegalArgumentException.class,
                () -> new PlayerSnapshot(id, "Alex", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, "SURVIVAL", 20.0F, 21));

        assertThrows(NullPointerException.class, () -> EntitySnapshotFactory.forPlayer(null));
        PlayerSnapshot snap = host.player(FakeNmsHost.PLAYER_ID);
        EntitySnapshot entitySnap = EntitySnapshotFactory.forPlayer(snap);
        assertEquals(snap.id(), entitySnap.id());
        assertEquals("minecraft:player", entitySnap.typeKey());
        assertEquals(snap.worldId(), entitySnap.worldId());
        assertEquals(snap.x(), entitySnap.x());
        assertEquals(snap.y(), entitySnap.y());
        assertEquals(snap.z(), entitySnap.z());
        assertNull(entitySnap.vehicleId());
        assertTrue(entitySnap.passengers().isEmpty());
    }

    @Test
    void serverPlayerExplicitRefusalsAndUnknownGameModeAreExplicit() {
        ServerPlayer player = ServerPlayer.of(FakeNmsHost.PLAYER_ID);
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> player.setGameMode(GameMode.CREATIVE)),
                "ServerPlayer#setGameMode");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> player.setGameMode("CREATIVE")),
                "ServerPlayer#setGameMode");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> player.setHealth(10.0F)),
                "ServerPlayer#setHealth");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, player::getConnection),
                "ServerPlayer#getConnection");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> player.openMenu(null)),
                "ServerPlayer#openMenu");

        UUID bogusPlayerId = UUID.randomUUID();
        host.players.put(
                bogusPlayerId,
                new PlayerSnapshot(
                        bogusPlayerId, "Bogus", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, "UNKNOWN_MODE", 20.0F, 20));
        ServerPlayer bogus = ServerPlayer.of(bogusPlayerId);
        assertThrows(IllegalStateException.class, bogus::bukkitGameMode);
    }

    @Test
    void craftPlayerReadsHandleBridgesCanonicallyAndRefusesSyncWrites() throws Exception {
        CraftPlayer craftPlayer = EntityTest.instantiateCraftSubclass(CraftPlayer.class, FakeNmsHost.PLAYER_ID);
        assertEquals(FakeNmsHost.PLAYER_ID, craftPlayer.getHandle().getUUID());
        assertSame(NmsBridge.player(craftPlayer), NmsBridge.entity(craftPlayer));

        assertEquals("Steve", craftPlayer.getName());
        assertEquals(GameMode.SURVIVAL, craftPlayer.getGameMode());
        assertEquals(20.0, craftPlayer.getHealth());
        assertEquals(20, craftPlayer.getFoodLevel());
        assertEquals(EntityType.PLAYER, craftPlayer.getType());
        assertEquals(7, craftPlayer.getEntityId());
        assertFalse(craftPlayer.isDead());
        assertTrue(craftPlayer.isValid());
        assertTrue(craftPlayer.isOnline());
        assertEquals(37, craftPlayer.getPing());
        assertEquals("en_us", craftPlayer.getLocale());
        assertEquals(5.0F, craftPlayer.getSaturation());
        assertEquals(0.2F, craftPlayer.getExhaustion());
        assertEquals(12, craftPlayer.getLevel());
        assertEquals(0.5F, craftPlayer.getExp());
        assertEquals(345, craftPlayer.getTotalExperience());
        assertEquals(0.1F, craftPlayer.getWalkSpeed());
        assertEquals(0.05F, craftPlayer.getFlySpeed());
        assertFalse(craftPlayer.getAllowFlight());
        assertFalse(craftPlayer.isFlying());
        assertFalse(craftPlayer.isSneaking());
        assertTrue(craftPlayer.isSprinting());
        assertEquals(300, craftPlayer.getRemainingAir());
        assertEquals(300, craftPlayer.getMaximumAir());
        assertEquals("Steve", craftPlayer.getDisplayName());
        assertEquals("Steve", craftPlayer.getPlayerListName());
        assertEquals(6000L, craftPlayer.getPlayerTime());
        assertEquals(0L, craftPlayer.getPlayerTimeOffset());
        assertTrue(craftPlayer.isPlayerTimeRelative());
        assertEquals(1.5, craftPlayer.getLocation().getX());
        assertEquals(65.0, craftPlayer.getLocation().getY());
        assertEquals(2.5, craftPlayer.getLocation().getZ());
        assertFalse(craftPlayer.isInsideVehicle());
        assertNull(craftPlayer.getVehicle());
        assertTrue(craftPlayer.getPassengers().isEmpty());

        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setGameMode(GameMode.CREATIVE)),
                "CraftPlayer#setGameMode");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setHealth(10.0)),
                "CraftPlayer#setHealth");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setFoodLevel(10)),
                "CraftPlayer#setFoodLevel");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setDisplayName("x")),
                "CraftPlayer#setDisplayName");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setPlayerListName("x")),
                "CraftPlayer#setPlayerListName");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setSneaking(true)),
                "CraftPlayer#setSneaking");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setSprinting(true)),
                "CraftPlayer#setSprinting");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setFlying(true)),
                "CraftPlayer#setFlying");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setAllowFlight(true)),
                "CraftPlayer#setAllowFlight");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setWalkSpeed(0.2F)),
                "CraftPlayer#setWalkSpeed");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setFlySpeed(0.2F)),
                "CraftPlayer#setFlySpeed");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setLevel(2)),
                "CraftPlayer#setLevel");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setExp(0.2F)),
                "CraftPlayer#setExp");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setTotalExperience(2)),
                "CraftPlayer#setTotalExperience");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setSaturation(1.0F)),
                "CraftPlayer#setSaturation");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setExhaustion(1.0F)),
                "CraftPlayer#setExhaustion");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setRemainingAir(10)),
                "CraftPlayer#setRemainingAir");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setMaximumAir(10)),
                "CraftPlayer#setMaximumAir");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.setPlayerTime(0L, true)),
                "CraftPlayer#setPlayerTime");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, craftPlayer::resetPlayerTime),
                "CraftPlayer#resetPlayerTime");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> craftPlayer.teleport(craftPlayer.getLocation())),
                "CraftEntity#teleport");

        World world = EntityTest.worldProxy(FakeNmsHost.WORLD_ID);
        Location target = new Location(world, 5.0, 70.0, -5.0, 0.0F, 0.0F);
        assertEquals(Boolean.TRUE, craftPlayer.teleportAsync(target).get(5, TimeUnit.SECONDS));
        assertEquals(
                new FakeNmsHost.Teleport(
                        FakeNmsHost.PLAYER_ID, FakeNmsHost.WORLD_ID, 5.0, 70.0, -5.0, 0.0F, 0.0F),
                host.teleports.get(0));
    }

    @Test
    void playerStateIsOptionalAndOfflinePlayersReportFalse() throws Exception {
        UUID statelessId = UUID.randomUUID();
        host.players.put(
                statelessId,
                new PlayerSnapshot(
                        statelessId, "Alex", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, "SURVIVAL", 20.0F, 20));
        ServerPlayer stateless = ServerPlayer.of(statelessId);
        assertEquals("Alex", stateless.displayName());
        assertEquals("Alex", stateless.playerListName());
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, stateless::ping),
                "ServerPlayer#ping");
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, stateless::locale),
                "ServerPlayer#locale");
        CraftPlayer craftStateless = EntityTest.instantiateCraftSubclass(CraftPlayer.class, statelessId);
        EntityTest.assertRefusal(
                assertThrows(UnsupportedOperationException.class, craftStateless::getPing),
                "ServerPlayer#ping");

        ServerPlayer player = ServerPlayer.of(FakeNmsHost.PLAYER_ID);
        assertTrue(player.isOnline());
        host.players.remove(FakeNmsHost.PLAYER_ID);
        assertFalse(player.isOnline());
        assertFalse(player.isValid());
        CraftPlayer craftPlayer = EntityTest.instantiateCraftSubclass(CraftPlayer.class, FakeNmsHost.PLAYER_ID);
        assertFalse(craftPlayer.isOnline());
        assertFalse(craftPlayer.isValid());
    }

    @Test
    void playerAsyncWritesPropagateRegionCompletionAndHostFailures() throws Exception {
        CompletableFuture<Void> pendingHealth = new CompletableFuture<>();
        CompletableFuture<Void> failingMode = new CompletableFuture<>();
        NmsHost asyncHost = (NmsHost) Proxy.newProxyInstance(
                NmsHost.class.getClassLoader(),
                new Class<?>[] {NmsHost.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "setHealth" -> pendingHealth;
                    case "setGameMode" -> failingMode;
                    default -> method.invoke(host, args);
                });
        NmsHostBinding.uninstall(host);
        NmsHostBinding.install(asyncHost);
        try {
            ServerPlayer player = ServerPlayer.of(FakeNmsHost.PLAYER_ID);
            CompletableFuture<Void> healthFuture = player.setHealthAsync(12.0F);
            assertFalse(healthFuture.isDone());
            host.region.submit(() -> pendingHealth.complete(null)).get(5, TimeUnit.SECONDS);
            healthFuture.get(5, TimeUnit.SECONDS);

            CompletableFuture<Void> modeFuture = player.setGameModeAsync(GameMode.CREATIVE);
            failingMode.completeExceptionally(new NmsHostException("player_offline", "disconnected"));
            ExecutionException ex = assertThrows(ExecutionException.class, () -> modeFuture.get(5, TimeUnit.SECONDS));
            assertTrue(ex.getCause() instanceof NmsHostException);
            assertEquals("player_offline", ((NmsHostException) ex.getCause()).code());
        } finally {
            NmsHostBinding.uninstall(asyncHost);
            NmsHostBinding.install(host);
        }
    }

    @Test
    void playerFoodLevelAsyncWriteUpdatesSnapshotAndValidatesRange() throws Exception {
        ServerPlayer player = ServerPlayer.of(FakeNmsHost.PLAYER_ID);
        player.setFoodLevelAsync(14).get(5, TimeUnit.SECONDS);
        assertEquals(14, ServerPlayer.of(FakeNmsHost.PLAYER_ID).getFoodLevel());
        assertThrows(IllegalArgumentException.class, () -> player.setFoodLevelAsync(-1));
        assertThrows(IllegalArgumentException.class, () -> player.setFoodLevelAsync(21));
    }
}
