// Clean-room Ferrite NMS test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
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
import java.lang.reflect.Proxy;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;
import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.phys.Vec3;
import org.bukkit.Location;
import org.bukkit.World;
import org.bukkit.craftbukkit.entity.CraftEntity;
import org.bukkit.craftbukkit.entity.StubCraftEntity;
import org.bukkit.entity.EntityType;
import org.bukkit.util.Vector;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class EntityTest {
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
    void readsPositionRotationVelocityAndRelationships() {
        Entity entity = Entity.of(FakeNmsHost.ENTITY_ID);
        assertEquals(FakeNmsHost.ENTITY_ID, entity.getUUID());
        assertEquals("minecraft:armor_stand", entity.getTypeKey());
        assertEquals(3.0, entity.getX());
        assertEquals(64.0, entity.getY());
        assertEquals(4.0, entity.getZ());
        assertEquals(45.0F, entity.getYRot());
        assertEquals(5.0F, entity.getXRot());
        assertEquals(0.1, entity.getDeltaMovement().x());
        assertEquals(0.2, entity.getDeltaMovement().y());
        assertEquals(0.3, entity.getDeltaMovement().z());
        assertEquals(FakeNmsHost.WORLD_ID, entity.level().getUUID());
        assertEquals(1, entity.getPassengers().size());
        assertEquals(FakeNmsHost.PASSENGER_ID, entity.getPassengers().get(0).getUUID());
        assertNotNull(entity.getVehicle());
        assertEquals(FakeNmsHost.VEHICLE_ID, entity.getVehicle().getUUID());
        assertNull(Entity.of(FakeNmsHost.VEHICLE_ID).getVehicle());
    }

    @Test
    void unknownEntitiesFailWithoutANullHandle() {
        NmsHostException error = assertThrows(NmsHostException.class,
                () -> Entity.of(UUID.randomUUID()));
        assertEquals("unknown_entity", error.code());
    }

    @Test
    void teleportIsAnAsyncOwnerRoutedWrite() throws Exception {
        Entity entity = Entity.of(FakeNmsHost.ENTITY_ID);
        entity.teleportAsync(10.5, 70.0, -4.5, 180.0F, -10.0F).get(5, TimeUnit.SECONDS);
        assertEquals(
                new FakeNmsHost.Teleport(
                        FakeNmsHost.ENTITY_ID, FakeNmsHost.WORLD_ID, 10.5, 70.0, -4.5, 180.0F, -10.0F),
                host.teleports.get(0));
    }

    @Test
    void crossWorldTeleportNeedsAnExplicitTargetWorld() throws Exception {
        Entity entity = Entity.of(FakeNmsHost.ENTITY_ID);
        ServerLevel target = ServerLevel.of(FakeNmsHost.WORLD_ID);
        entity.teleportAsync(target, 0.0, 80.0, 0.0, 0.0F, 0.0F).get(5, TimeUnit.SECONDS);
        assertEquals(FakeNmsHost.WORLD_ID, host.teleports.get(0).worldId());
        assertTrue(host.teleports.get(0).x() == 0.0);
    }

    @Test
    void entityPositionBlockPositionAndRelationshipFlagsMatchSnapshot() {
        Entity entity = Entity.of(FakeNmsHost.ENTITY_ID);
        assertEquals(new Vec3(3.0, 64.0, 4.0), entity.position());
        assertEquals(new BlockPos(3, 64, 4), entity.blockPosition());
        assertTrue(entity.isPassenger());
        assertTrue(entity.isVehicle());
        assertEquals(List.of(FakeNmsHost.PASSENGER_ID), entity.passengerIds());
        assertTrue(entity.owns(FakeNmsHost.ENTITY_ID));
        assertFalse(entity.owns(FakeNmsHost.PLAYER_ID));

        Entity vehicle = Entity.of(FakeNmsHost.VEHICLE_ID);
        assertFalse(vehicle.isPassenger());
        assertTrue(vehicle.isVehicle());
    }

    @Test
    void craftEntityGetHandleAndFirstSliceAccessorsUseHostSnapshots() {
        CraftEntity craft = new StubCraftEntity(FakeNmsHost.ENTITY_ID);
        Entity handle = craft.getHandle();
        assertEquals(FakeNmsHost.ENTITY_ID, handle.getUUID());
        assertSame(NmsBridge.entity(craft), NmsBridge.entity(craft));

        Location location = craft.getLocation();
        assertNull(location.getWorld());
        assertEquals(3.0, location.getX());
        assertEquals(64.0, location.getY());
        assertEquals(4.0, location.getZ());
        assertEquals(45.0F, location.getYaw());
        assertEquals(5.0F, location.getPitch());

        Location reused = new Location(null, 0, 0, 0);
        assertSame(reused, craft.getLocation(reused));
        assertEquals(3.0, reused.getX());
        assertNull(craft.getLocation(null));

        assertEquals(new Vector(0.1, 0.2, 0.3), craft.getVelocity());
        assertEquals(EntityType.ARMOR_STAND, craft.getType());

        assertNull(new StubCraftEntity(FakeNmsHost.VEHICLE_ID).getVehicle());
        assertTrue(new StubCraftEntity(FakeNmsHost.PASSENGER_ID).getPassengers().isEmpty());

        UnsupportedOperationException worldError =
                assertThrows(UnsupportedOperationException.class, craft::getWorld);
        assertTrue(worldError.getMessage().contains("CraftEntity#getWorld"), worldError.getMessage());

        UnsupportedOperationException vehicleError =
                assertThrows(UnsupportedOperationException.class, craft::getVehicle);
        assertTrue(vehicleError.getMessage().contains("CraftEntity#getVehicle"), vehicleError.getMessage());

        UnsupportedOperationException passengersError =
                assertThrows(UnsupportedOperationException.class, craft::getPassengers);
        assertTrue(
                passengersError.getMessage().contains("CraftEntity#getPassengers"),
                passengersError.getMessage());
    }

    @Test
    void craftEntityTeleportAsyncValidatesTargetWorldAndRefusesSyncWrites() throws Exception {
        UUID secondWorldId = UUID.fromString("00000000-0000-0000-0000-000000000099");
        host.worlds.put(
                secondWorldId,
                new WorldSnapshot(
                        secondWorldId, "world_nether", "minecraft:the_nether", "NETHER", 99L, 0L, 0.0, 64.0, 0.0, 0, 256));

        CraftEntity craft = new StubCraftEntity(FakeNmsHost.ENTITY_ID);
        World targetWorld = worldProxy(secondWorldId, "world_nether");
        assertTrue(craft.teleportAsync(new Location(targetWorld, 12.0, 72.0, -8.0, 90.0F, 15.0F))
                .get(5, TimeUnit.SECONDS));
        assertEquals(
                new FakeNmsHost.Teleport(
                        FakeNmsHost.ENTITY_ID, secondWorldId, 12.0, 72.0, -8.0, 90.0F, 15.0F),
                host.teleports.get(0));

        World missingWorld = worldProxy(UUID.randomUUID(), "missing");
        NmsHostException unknownWorld = assertThrows(
                NmsHostException.class,
                () -> craft.teleportAsync(new Location(missingWorld, 0.0, 64.0, 0.0)));
        assertEquals("unknown_world", unknownWorld.code());

        assertThrows(IllegalArgumentException.class, () -> craft.teleportAsync(null));
        assertThrows(IllegalArgumentException.class, () -> craft.teleportAsync(new Location(null, 0, 64, 0)));
        assertThrows(
                IllegalArgumentException.class,
                () -> craft.teleportAsync(new Location(targetWorld, Double.NaN, 64.0, 0.0)));

        UnsupportedOperationException syncTeleport =
                assertThrows(UnsupportedOperationException.class, () -> craft.teleport(new Location(targetWorld, 0, 64, 0)));
        assertTrue(syncTeleport.getMessage().contains("CraftEntity#teleport"), syncTeleport.getMessage());
        assertTrue(syncTeleport.getMessage().contains("teleportAsync"), syncTeleport.getMessage());

        UnsupportedOperationException setVelocity =
                assertThrows(UnsupportedOperationException.class, () -> craft.setVelocity(new Vector(1, 0, 0)));
        assertTrue(setVelocity.getMessage().contains("CraftEntity#setVelocity"), setVelocity.getMessage());

        Entity handle = craft.getHandle();
        assertTrue(assertThrows(UnsupportedOperationException.class, () -> handle.setPos(0, 64, 0))
                .getMessage().contains("Entity#setPos"));
        assertTrue(assertThrows(UnsupportedOperationException.class, () -> handle.teleportTo(ServerLevel.of(secondWorldId), 0, 64, 0))
                .getMessage().contains("Entity#teleportTo"));
        assertTrue(assertThrows(UnsupportedOperationException.class, () -> handle.setDeltaMovement(Vec3.ZERO))
                .getMessage().contains("Entity#setDeltaMovement"));
        assertTrue(assertThrows(UnsupportedOperationException.class, handle::getBukkitEntity)
                .getMessage().contains("Entity#getBukkitEntity"));
    }

    @Test
    void asyncEntityTeleportCompletesOnRegionThreadWithoutBlockingCaller() throws Exception {
        CompletableFuture<Void> pending = new CompletableFuture<>();
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
                host.teleport(entityId, worldId, x, y, z, yaw, pitch);
                return pending;
            }

            @Override
            public CompletableFuture<Void> setGameMode(UUID playerId, String gameMode) {
                return host.setGameMode(playerId, gameMode);
            }

            @Override
            public CompletableFuture<Void> setHealth(UUID playerId, float health) {
                return host.setHealth(playerId, health);
            }
        };

        NmsHostBinding.install(deferredHost);
        try {
            CraftEntity craft = new StubCraftEntity(FakeNmsHost.ENTITY_ID);
            CompletableFuture<Boolean> future =
                    craft.teleportAsync(new Location(worldProxy(FakeNmsHost.WORLD_ID, "world"), 5.0, 66.0, 5.0));
            assertFalse(future.isDone(), "teleportAsync must return immediately without waiting for region tick");

            host.region.submit(() -> {
                        completionThread.set(Thread.currentThread());
                        pending.complete(null);
                    })
                    .get(5, TimeUnit.SECONDS);
            assertTrue(future.get(5, TimeUnit.SECONDS));
            assertEquals("ferrite-region-0", completionThread.get().getName());
            assertNotEquals(Thread.currentThread().getName(), completionThread.get().getName());
        } finally {
            NmsHostBinding.install(host);
        }
    }

    @Test
    void entitySnapshotRejectsInvalidFieldsAndSelfCycles() {
        assertThrows(
                IllegalArgumentException.class,
                () -> new EntitySnapshot(
                        FakeNmsHost.ENTITY_ID, " ", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, 0, 0, 0, null, List.of()));
        assertThrows(
                IllegalArgumentException.class,
                () -> new EntitySnapshot(
                        FakeNmsHost.ENTITY_ID, "minecraft:pig", FakeNmsHost.WORLD_ID, Double.NaN, 64, 0, 0, 0, 0, 0, 0, null, List.of()));
        assertThrows(
                IllegalArgumentException.class,
                () -> new EntitySnapshot(
                        FakeNmsHost.ENTITY_ID, "minecraft:pig", FakeNmsHost.WORLD_ID, 0, 64, 0, Float.POSITIVE_INFINITY, 0, 0, 0, 0, null, List.of()));
        assertThrows(
                IllegalArgumentException.class,
                () -> new EntitySnapshot(
                        FakeNmsHost.ENTITY_ID, "minecraft:pig", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, 0, Double.NaN, 0, null, List.of()));
        assertThrows(
                IllegalArgumentException.class,
                () -> new EntitySnapshot(
                        FakeNmsHost.ENTITY_ID, "minecraft:pig", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, 0, 0, 0, FakeNmsHost.ENTITY_ID, List.of()));
        assertThrows(
                IllegalArgumentException.class,
                () -> new EntitySnapshot(
                        FakeNmsHost.ENTITY_ID, "minecraft:pig", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, 0, 0, 0, null, List.of(FakeNmsHost.ENTITY_ID)));
    }

    private static World worldProxy(UUID worldId, String name) {
        return (World) Proxy.newProxyInstance(
                World.class.getClassLoader(),
                new Class<?>[] {World.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "getUID" -> worldId;
                    case "getName" -> name;
                    case "hashCode" -> System.identityHashCode(proxy);
                    case "equals" -> proxy == (args == null ? null : args[0]);
                    case "toString" -> "WorldProxy{" + name + "}";
                    default -> throw new UnsupportedOperationException("WorldProxy#" + method.getName());
                });
    }
}
