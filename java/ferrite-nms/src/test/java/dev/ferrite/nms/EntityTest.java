// Clean-room Ferrite NMS test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.nms.bridge.NmsBridge;
import dev.ferrite.nms.host.EntitySnapshot;
import dev.ferrite.nms.host.EntityState;
import dev.ferrite.nms.host.NmsHost;
import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.NmsHostException;
import dev.ferrite.nms.test.FakeNmsHost;
import io.papermc.paper.entity.TeleportFlag;
import java.lang.classfile.ClassFile;
import java.lang.constant.ClassDesc;
import java.lang.constant.ConstantDescs;
import java.lang.constant.MethodTypeDesc;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.phys.Vec3;
import org.bukkit.Location;
import org.bukkit.World;
import org.bukkit.block.BlockFace;
import org.bukkit.craftbukkit.entity.CraftEntity;
import org.bukkit.entity.EntityType;
import org.bukkit.entity.Pose;
import org.bukkit.event.player.PlayerTeleportEvent;
import org.bukkit.util.Vector;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class EntityTest {
    private static final AtomicInteger GENERATED_COUNTER = new AtomicInteger();

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
        assertEquals(42, entity.getId());
        assertFalse(entity.isRemoved());
        assertEquals(200, entity.tickCount());
        assertEquals(0, entity.getFireTicks());
        assertEquals(0.0F, entity.getFallDistance());
        assertTrue(entity.onGround());
        assertEquals(0, entity.getPortalCooldown());
        assertEquals("STANDING", entity.poseName());
        assertTrue(entity.isValid());
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
    void entitySnapshotRejectsInvalidValuesAndDefensivelyCopiesPassengers() {
        UUID id = UUID.randomUUID();
        UUID passenger = UUID.randomUUID();
        List<UUID> mutablePassengers = new ArrayList<>();
        mutablePassengers.add(passenger);

        EntitySnapshot snapshot = new EntitySnapshot(
                id,
                "minecraft:pig",
                FakeNmsHost.WORLD_ID,
                1.0,
                64.0,
                2.0,
                0.0F,
                0.0F,
                0.0,
                0.0,
                0.0,
                null,
                mutablePassengers);
        mutablePassengers.clear();
        assertEquals(List.of(passenger), snapshot.passengers());
        assertThrows(UnsupportedOperationException.class, () -> snapshot.passengers().add(UUID.randomUUID()));

        assertThrows(
                IllegalArgumentException.class,
                () -> new EntitySnapshot(
                        id, " ", FakeNmsHost.WORLD_ID, 0, 0, 0, 0, 0, 0, 0, 0, null, List.of()));
        assertThrows(
                IllegalArgumentException.class,
                () -> new EntitySnapshot(
                        id, "minecraft:pig", FakeNmsHost.WORLD_ID, Double.NaN, 0, 0, 0, 0, 0, 0, 0, null, List.of()));
        assertThrows(
                IllegalArgumentException.class,
                () -> new EntitySnapshot(
                        id, "minecraft:pig", FakeNmsHost.WORLD_ID, 0, 0, 0, Float.POSITIVE_INFINITY, 0, 0, 0, 0, null, List.of()));
        assertThrows(
                IllegalArgumentException.class,
                () -> new EntitySnapshot(
                        id, "minecraft:pig", FakeNmsHost.WORLD_ID, 0, 0, 0, 0, 0, 0, Double.NEGATIVE_INFINITY, 0, null, List.of()));
        assertThrows(
                IllegalArgumentException.class,
                () -> new EntitySnapshot(
                        id, "minecraft:pig", FakeNmsHost.WORLD_ID, 0, 0, 0, 0, 0, 0, 0, 0, id, List.of()));
        assertThrows(
                IllegalArgumentException.class,
                () -> new EntitySnapshot(
                        id, "minecraft:pig", FakeNmsHost.WORLD_ID, 0, 0, 0, 0, 0, 0, 0, 0, null, List.of(id)));
    }

    @Test
    void entityHelpersExposePositionBlockPosOwnershipAndRefusals() {
        UUID negativeId = UUID.randomUUID();
        host.entities.put(
                negativeId,
                new EntitySnapshot(
                        negativeId,
                        "minecraft:cow",
                        FakeNmsHost.WORLD_ID,
                        -1.2,
                        63.9,
                        -0.1,
                        0.0F,
                        0.0F,
                        0.0,
                        0.0,
                        0.0,
                        null,
                        List.of()));

        Entity entity = Entity.of(FakeNmsHost.ENTITY_ID);
        assertEquals(new Vec3(3.0, 64.0, 4.0), entity.position());
        assertEquals(new BlockPos(3, 64, 4), entity.blockPosition());
        assertTrue(entity.isPassenger());
        assertTrue(entity.isVehicle());
        assertEquals(List.of(FakeNmsHost.PASSENGER_ID), entity.passengerIds());
        assertTrue(entity.owns(FakeNmsHost.ENTITY_ID));
        assertFalse(entity.owns(FakeNmsHost.PLAYER_ID));

        Entity negative = Entity.of(negativeId);
        assertEquals(new BlockPos(-2, 63, -1), negative.blockPosition());
        assertFalse(negative.isPassenger());
        assertFalse(negative.isVehicle());

        assertThrows(IllegalArgumentException.class, () -> entity.teleportAsync(Double.NaN, 64.0, 0.0, 0.0F, 0.0F));
        assertThrows(IllegalArgumentException.class, () -> entity.teleportAsync(0.0, 64.0, 0.0, Float.NaN, 0.0F));
        assertTrue(host.teleports.isEmpty());

        ServerLevel level = ServerLevel.of(FakeNmsHost.WORLD_ID);
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> entity.setPos(0, 64, 0)),
                "Entity#setPos");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> entity.teleportTo(level, 0, 64, 0)),
                "Entity#teleportTo");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> entity.setDeltaMovement(Vec3.ZERO)),
                "Entity#setDeltaMovement");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, entity::getBukkitEntity),
                "Entity#getBukkitEntity");
    }

    @Test
    void entityAsyncTeleportPropagatesRegionThreadCompletionAndFailures() throws Exception {
        CompletableFuture<Void> pending = new CompletableFuture<>();
        NmsHost asyncHost = (NmsHost) Proxy.newProxyInstance(
                NmsHost.class.getClassLoader(),
                new Class<?>[] {NmsHost.class},
                (proxy, method, args) -> {
                    if ("teleport".equals(method.getName())) {
                        return pending;
                    }
                    return method.invoke(host, args);
                });
        NmsHostBinding.uninstall(host);
        NmsHostBinding.install(asyncHost);
        try {
            Entity entity = Entity.of(FakeNmsHost.ENTITY_ID);
            CompletableFuture<Void> write = entity.teleportAsync(1.0, 65.0, 2.0, 0.0F, 0.0F);
            assertFalse(write.isDone());

            host.region.submit(() -> pending.complete(null)).get(5, TimeUnit.SECONDS);
            write.get(5, TimeUnit.SECONDS);

            CompletableFuture<Void> failing = new CompletableFuture<>();
            NmsHost failingHost = (NmsHost) Proxy.newProxyInstance(
                    NmsHost.class.getClassLoader(),
                    new Class<?>[] {NmsHost.class},
                    (proxy, method, args) -> {
                        if ("teleport".equals(method.getName())) {
                            return failing;
                        }
                        return method.invoke(host, args);
                    });
            NmsHostBinding.uninstall(asyncHost);
            NmsHostBinding.install(failingHost);
            CompletableFuture<Void> failedWrite = entity.teleportAsync(1.0, 65.0, 2.0, 0.0F, 0.0F);
            failing.completeExceptionally(new NmsHostException("region_unavailable", "region moved"));
            ExecutionException ex = assertThrows(ExecutionException.class, () -> failedWrite.get(5, TimeUnit.SECONDS));
            assertTrue(ex.getCause() instanceof NmsHostException);
            assertEquals("region_unavailable", ((NmsHostException) ex.getCause()).code());
        } finally {
            try {
                NmsHostBinding.install(host);
            } catch (IllegalStateException ignored) {
                // Reinstall only if uninstalled.
            }
        }
    }

    @Test
    void craftEntityReadsSnapshotFieldsResolvesTypeAndBridgesCanonically() throws Exception {
        CraftEntity armorStand = newCraftEntity(FakeNmsHost.ENTITY_ID);
        CraftEntity passenger = newCraftEntity(FakeNmsHost.PASSENGER_ID);
        CraftEntity vehicle = newCraftEntity(FakeNmsHost.VEHICLE_ID);

        assertEquals(FakeNmsHost.ENTITY_ID, armorStand.getHandle().getUUID());
        assertSame(NmsBridge.entity(armorStand), NmsBridge.entity(armorStand));
        assertEquals(EntityType.ARMOR_STAND, armorStand.getType());
        assertEquals(EntityType.PIG, passenger.getType());
        assertEquals(EntityType.OAK_BOAT, vehicle.getType());

        assertEquals(42, armorStand.getEntityId());
        assertFalse(armorStand.isDead());
        assertTrue(armorStand.isValid());
        assertEquals(200, armorStand.getTicksLived());
        assertEquals(0, armorStand.getFireTicks());
        assertEquals(0.0F, armorStand.getFallDistance());
        assertTrue(armorStand.isOnGround());
        assertEquals(0, armorStand.getPortalCooldown());
        assertEquals(Pose.STANDING, armorStand.getPose());
        assertEquals(BlockFace.SOUTH_WEST, armorStand.getFacing());
        assertEquals(BlockFace.SOUTH, passenger.getFacing());
        assertEquals(BlockFace.SOUTH, vehicle.getFacing());

        Location loc = armorStand.getLocation();
        assertNull(loc.getWorld());
        assertEquals(3.0, loc.getX());
        assertEquals(64.0, loc.getY());
        assertEquals(4.0, loc.getZ());
        assertEquals(45.0F, loc.getYaw());
        assertEquals(5.0F, loc.getPitch());

        Location reused = new Location(null, 0, 0, 0);
        assertSame(reused, armorStand.getLocation(reused));
        assertEquals(3.0, reused.getX());
        assertNull(armorStand.getLocation(null));

        assertEquals(new Vector(0.1, 0.2, 0.3), armorStand.getVelocity());
        assertTrue(armorStand.isInsideVehicle());
        assertFalse(vehicle.isInsideVehicle());
        assertNull(vehicle.getVehicle());
        assertTrue(passenger.getPassengers().isEmpty());

        World world = worldProxy(FakeNmsHost.WORLD_ID);
        Location target = new Location(world, 12.5, 72.0, -3.5, 90.0F, 15.0F);
        assertEquals(Boolean.TRUE, armorStand.teleportAsync(target).get(5, TimeUnit.SECONDS));
        assertEquals(
                new FakeNmsHost.Teleport(
                        FakeNmsHost.ENTITY_ID, FakeNmsHost.WORLD_ID, 12.5, 72.0, -3.5, 90.0F, 15.0F),
                host.teleports.get(0));
    }

    @Test
    void craftEntityUnknownEntityTypeFailsWithNmsHostExceptionNotUnknownEnumKeyError() throws Exception {
        UUID bogusId = UUID.randomUUID();
        host.entities.put(
                bogusId,
                new EntitySnapshot(
                        bogusId,
                        "minecraft:not_a_real_26_3_entity",
                        FakeNmsHost.WORLD_ID,
                        0.0,
                        64.0,
                        0.0,
                        0.0F,
                        0.0F,
                        0.0,
                        0.0,
                        0.0,
                        null,
                        List.of()));
        CraftEntity bogus = newCraftEntity(bogusId);
        NmsHostException error = assertThrows(NmsHostException.class, bogus::getType);
        assertEquals("unknown_entity_type", error.code());
    }

    @Test
    void craftEntityRefusesSyncMutationsBadTeleportsAndMissingFacades() throws Exception {
        CraftEntity entity = newCraftEntity(FakeNmsHost.ENTITY_ID);
        CraftEntity passenger = newCraftEntity(FakeNmsHost.PASSENGER_ID);
        World world = worldProxy(FakeNmsHost.WORLD_ID);
        Location validLoc = new Location(world, 1.0, 64.0, 2.0);

        assertRefusal(
                assertThrows(UnsupportedOperationException.class, entity::getWorld),
                "CraftEntity#getWorld");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, entity::getVehicle),
                "CraftEntity#getVehicle");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, entity::getPassengers),
                "CraftEntity#getPassengers");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> entity.setVelocity(new Vector())),
                "CraftEntity#setVelocity");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> entity.setFireTicks(20)),
                "CraftEntity#setFireTicks");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> entity.setFallDistance(2.0F)),
                "CraftEntity#setFallDistance");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> entity.setPortalCooldown(10)),
                "CraftEntity#setPortalCooldown");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> entity.setTicksLived(10)),
                "CraftEntity#setTicksLived");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> entity.setGlowing(true)),
                "CraftEntity#setGlowing");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> entity.setInvulnerable(true)),
                "CraftEntity#setInvulnerable");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> entity.setInvisible(true)),
                "CraftEntity#setInvisible");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> entity.setSilent(true)),
                "CraftEntity#setSilent");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> entity.setGravity(false)),
                "CraftEntity#setGravity");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> entity.setPose(Pose.STANDING)),
                "CraftEntity#setPose");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, entity::eject),
                "CraftEntity#eject");
        assertRefusal(
                assertThrows(
                        UnsupportedOperationException.class,
                        () -> entity.addPassenger(passenger)),
                "CraftEntity#addPassenger");
        assertRefusal(
                assertThrows(
                        UnsupportedOperationException.class,
                        () -> entity.removePassenger(passenger)),
                "CraftEntity#removePassenger");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> entity.teleport(validLoc)),
                "CraftEntity#teleport");
        assertRefusal(
                assertThrows(
                        UnsupportedOperationException.class,
                        () -> entity.teleport(validLoc, PlayerTeleportEvent.TeleportCause.PLUGIN)),
                "CraftEntity#teleport");
        TeleportFlag testFlag = TeleportFlag.Relative.VELOCITY_X;
        assertRefusal(
                assertThrows(
                        UnsupportedOperationException.class,
                        () -> entity.teleport(
                                validLoc,
                                PlayerTeleportEvent.TeleportCause.PLUGIN,
                                testFlag)),
                "CraftEntity#teleport");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, () -> entity.teleport(entity)),
                "CraftEntity#teleport");
        assertRefusal(
                assertThrows(
                        UnsupportedOperationException.class,
                        () -> entity.teleport(entity, PlayerTeleportEvent.TeleportCause.PLUGIN)),
                "CraftEntity#teleport");
        assertRefusal(
                assertThrows(
                        UnsupportedOperationException.class,
                        () -> entity.teleportAsync(
                                validLoc,
                                PlayerTeleportEvent.TeleportCause.PLUGIN,
                                testFlag)),
                "CraftEntity#teleportAsync");

        assertThrows(IllegalArgumentException.class, () -> entity.teleportAsync(null));
        assertThrows(IllegalArgumentException.class, () -> entity.teleportAsync(new Location(null, 0, 64, 0)));
        assertThrows(
                IllegalArgumentException.class,
                () -> entity.teleportAsync(new Location(world, Double.NaN, 64.0, 0.0)));
        assertTrue(host.teleports.isEmpty());
    }

    @Test
    void removedAndStatelessEntitiesAreExplicit() throws Exception {
        UUID removedId = UUID.randomUUID();
        host.entities.put(
                removedId,
                new EntitySnapshot(
                                removedId,
                                "minecraft:zombie",
                                FakeNmsHost.WORLD_ID,
                                0.0,
                                64.0,
                                0.0,
                                0.0F,
                                0.0F,
                                0.0,
                                0.0,
                                0.0,
                                null,
                                List.of())
                        .withState(new EntityState(99, true, 5, 0, 0.0F, false, 0, "DYING")));
        Entity removed = Entity.of(removedId);
        assertTrue(removed.isRemoved());
        assertFalse(removed.isValid());
        assertEquals("DYING", removed.poseName());
        assertEquals(Pose.DYING, newCraftEntity(removedId).getPose());

        UUID statelessId = UUID.randomUUID();
        host.entities.put(
                statelessId,
                new EntitySnapshot(
                        statelessId,
                        "minecraft:cow",
                        FakeNmsHost.WORLD_ID,
                        0.0,
                        64.0,
                        0.0,
                        0.0F,
                        0.0F,
                        0.0,
                        0.0,
                        0.0,
                        null,
                        List.of()));
        Entity stateless = Entity.of(statelessId);
        assertTrue(stateless.isValid());
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, stateless::getId),
                "Entity#getId");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, stateless::isRemoved),
                "Entity#isRemoved");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, stateless::poseName),
                "Entity#poseName");
        CraftEntity craftStateless = newCraftEntity(statelessId);
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, craftStateless::getEntityId),
                "CraftEntity#getEntityId");
        assertRefusal(
                assertThrows(UnsupportedOperationException.class, craftStateless::getPose),
                "CraftEntity#getPose");
    }

    @Test
    void ignoredEntityNeverLeaksIntoTheOtherAdapter() throws Exception {
        UUID ignoredId = UUID.randomUUID();
        host.entities.put(
                ignoredId,
                new EntitySnapshot(
                                ignoredId,
                                "minecraft:item",
                                FakeNmsHost.WORLD_ID,
                                0.0,
                                64.0,
                                0.0,
                                0.0F,
                                0.0F,
                                0.0,
                                0.0,
                                0.0,
                                null,
                                List.of())
                        .withState(new EntityState(100, false, 0, 0, 0.0F, true, 0, "STANDING")));
        CraftEntity ignored = newCraftEntity(ignoredId);
        assertEquals(100, ignored.getEntityId());
        assertTrue(ignored.isValid());
        host.entities.remove(ignoredId);
        assertFalse(ignored.isValid());
    }

    @Test
    void equipmentMetadataPoseVelocityAndPassengerAsyncWritesWork() throws Exception {
        CraftEntity craft = newCraftEntity(FakeNmsHost.ENTITY_ID);
        Entity handle = craft.getHandle();

        handle.setEquipmentAsync("MAIN_HAND", "minecraft:iron_sword").get(5, TimeUnit.SECONDS);
        handle.setEquipmentAsync("HEAD", "minecraft:iron_helmet").get(5, TimeUnit.SECONDS);
        craft.setCustomNameAsync("Sentinel", true).get(5, TimeUnit.SECONDS);
        craft.setPoseAsync(org.bukkit.entity.Pose.SNEAKING).get(5, TimeUnit.SECONDS);
        craft.setVelocityAsync(new Vector(0.4, 0.5, -0.2)).get(5, TimeUnit.SECONDS);
        craft.setRotationAsync(135.0F, -15.0F).get(5, TimeUnit.SECONDS);
        craft.addPassengerAsync(FakeNmsHost.VEHICLE_ID).get(5, TimeUnit.SECONDS);

        Entity updated = Entity.of(FakeNmsHost.ENTITY_ID);
        assertEquals("minecraft:iron_sword", updated.equipmentSlot("HAND"));
        assertEquals("minecraft:iron_helmet", craft.getEquipmentItemKey("HEAD"));
        assertEquals("Sentinel", craft.getCustomName());
        assertTrue(craft.isCustomNameVisible());
        assertEquals(org.bukkit.entity.Pose.SNEAKING, craft.getPose());
        assertEquals(0.4, updated.getDeltaMovement().x(), 1e-6);
        assertEquals(135.0F, updated.getYRot(), 1e-6);
        assertTrue(updated.passengerIds().contains(FakeNmsHost.VEHICLE_ID));

        craft.removePassengerAsync(FakeNmsHost.VEHICLE_ID).get(5, TimeUnit.SECONDS);
        assertFalse(Entity.of(FakeNmsHost.ENTITY_ID).passengerIds().contains(FakeNmsHost.VEHICLE_ID));

        NmsHostException invalidSlot = assertThrows(
                NmsHostException.class, () -> handle.setEquipmentAsync("TAIL", "minecraft:stick"));
        assertEquals("invalid_equipment_slot", invalidSlot.code());

        UUID missing = UUID.randomUUID();
        NmsHostException unknown = assertThrows(
                NmsHostException.class,
                () -> host.setEquipment(missing, "HAND", "minecraft:stick"));
        assertEquals("unknown_entity", unknown.code());
    }

    static void assertRefusal(UnsupportedOperationException error, String expectedToken) {
        assertTrue(
                error.getMessage() != null && error.getMessage().contains(expectedToken),
                "Expected message containing " + expectedToken + " but got: " + error.getMessage());
    }

    static World worldProxy(UUID worldId) {
        return (World) Proxy.newProxyInstance(
                World.class.getClassLoader(),
                new Class<?>[] {World.class},
                (proxy, method, args) -> {
                    if ("getUID".equals(method.getName())) {
                        return worldId;
                    }
                    throw new UnsupportedOperationException("Test World proxy does not implement " + method.getName());
                });
    }

    static CraftEntity newCraftEntity(UUID entityId) throws Exception {
        return instantiateCraftSubclass(CraftEntity.class, entityId);
    }

    static <T> T instantiateCraftSubclass(Class<T> baseClass, UUID id) throws Exception {
        return instantiateCraftSubclass(baseClass, id, "getUniqueId");
    }

    @SuppressWarnings("unchecked")
    static <T> T instantiateCraftSubclass(Class<T> baseClass, UUID id, String idMethod)
            throws Exception {
        String binaryName = "dev.ferrite.nms.Test"
                + baseClass.getSimpleName()
                + "$"
                + GENERATED_COUNTER.incrementAndGet();
        ClassDesc self = ClassDesc.of(binaryName);
        ClassDesc superDesc = ClassDesc.of(baseClass.getName());
        ClassDesc uuidDesc = ClassDesc.of(UUID.class.getName());

        byte[] bytes = ClassFile.of().build(self, cb -> {
            cb.withSuperclass(superDesc);
            cb.withFlags(ClassFile.ACC_PUBLIC | ClassFile.ACC_FINAL);
            cb.withField("id", uuidDesc, ClassFile.ACC_PRIVATE | ClassFile.ACC_FINAL);
            cb.withMethodBody(
                    ConstantDescs.INIT_NAME,
                    MethodTypeDesc.of(ConstantDescs.CD_void, uuidDesc),
                    ClassFile.ACC_PUBLIC,
                    code -> code.aload(0)
                            .invokespecial(superDesc, ConstantDescs.INIT_NAME, MethodTypeDesc.of(ConstantDescs.CD_void))
                            .aload(0)
                            .aload(1)
                            .putfield(self, "id", uuidDesc)
                            .return_());
            cb.withMethodBody(
                    idMethod,
                    MethodTypeDesc.of(uuidDesc),
                    ClassFile.ACC_PUBLIC,
                    code -> code.aload(0).getfield(self, "id", uuidDesc).areturn());
        });

        ClassLoader loader = new ClassLoader(baseClass.getClassLoader()) {
            Class<?> define() {
                return defineClass(binaryName, bytes, 0, bytes.length);
            }
        };
        Class<?> generated = ((ClassLoader) loader).getClass().getDeclaredMethod("define").invoke(loader) instanceof Class<?> cls
                ? cls
                : null;
        return (T) generated.getConstructor(UUID.class).newInstance(id);
    }

    @Test
    void hostStateReaderParsesPlayerAndEntityStateJsonSnapshots() {
        UUID playerId = UUID.randomUUID();
        String playerJson = """
                {
                  "uuid": "%s",
                  "type": "minecraft:player",
                  "name": "LivePlayer",
                  "world": "minecraft:overworld",
                  "x": 12.5,
                  "y": 64.0,
                  "z": -4.5,
                  "yaw": 90.0,
                  "pitch": 15.0,
                  "vx": 0.0,
                  "vy": 0.0,
                  "vz": 0.0,
                  "game_mode": "SURVIVAL",
                  "health": 14.5,
                  "food_level": 18,
                  "network_id": 42
                }
                """.formatted(playerId);
        var playerMap = net.minecraft.world.entity.HostStateReader.parseJsonObject(playerJson);
        dev.ferrite.nms.host.PlayerSnapshot playerSnap = net.minecraft.world.entity.HostStateReader.toPlayerSnapshot(playerMap, null);
        assertEquals(playerId, playerSnap.id());
        assertEquals("LivePlayer", playerSnap.name());
        assertEquals(12.5, playerSnap.x());
        assertEquals("SURVIVAL", playerSnap.gameMode());
        assertEquals(14.5F, playerSnap.health());
        assertEquals(18, playerSnap.foodLevel());
        assertEquals(42, playerSnap.entityState().networkId());

        UUID pigId = UUID.randomUUID();
        UUID riderId = UUID.randomUUID();
        String entityJson = """
                {
                  "uuid": "%s",
                  "type": "minecraft:pig",
                  "name": "Wilbur",
                  "custom_name": "Wilbur",
                  "world": "minecraft:overworld",
                  "x": 1.5,
                  "y": 65.0,
                  "z": 2.5,
                  "yaw": 180.0,
                  "pitch": 0.0,
                  "vx": 0.1,
                  "vy": 0.0,
                  "vz": -0.2,
                  "network_id": 77,
                  "vehicle": null,
                  "passengers": ["%s"],
                  "equipment": {
                    "mainhand": {"id": "minecraft:diamond_sword", "count": 1},
                    "head": {"id": "minecraft:golden_helmet", "count": 1}
                  }
                }
                """.formatted(pigId, riderId);
        var entityMap = net.minecraft.world.entity.HostStateReader.parseJsonObject(entityJson);
        EntitySnapshot entitySnap = net.minecraft.world.entity.HostStateReader.toEntitySnapshot(entityMap, null);
        assertEquals(pigId, entitySnap.id());
        assertEquals("minecraft:pig", entitySnap.typeKey());
        assertEquals(1.5, entitySnap.x());
        assertEquals(0.1, entitySnap.velocityX());
        assertEquals(List.of(riderId), entitySnap.passengers());
        assertEquals("Wilbur", entitySnap.state().customName());
        assertEquals(77, entitySnap.state().networkId());
        assertEquals("minecraft:diamond_sword", entitySnap.state().equipmentItem("HAND"));
        assertEquals("minecraft:golden_helmet", entitySnap.state().equipmentItem("HEAD"));
        assertNull(entitySnap.state().equipmentItem("CHEST"));
    }
}
