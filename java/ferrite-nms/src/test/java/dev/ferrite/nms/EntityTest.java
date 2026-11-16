// Clean-room Ferrite NMS test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.NmsHostException;
import dev.ferrite.nms.test.FakeNmsHost;
import java.util.UUID;
import java.util.concurrent.TimeUnit;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.Entity;
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
}
