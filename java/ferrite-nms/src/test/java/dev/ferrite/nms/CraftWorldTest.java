// Clean-room Ferrite NMS test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.NmsHostException;
import dev.ferrite.nms.test.FakeNmsHost;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;
import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.block.state.BlockState;
import org.bukkit.Material;
import org.bukkit.craftbukkit.CraftWorld;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class CraftWorldTest {
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
    void blockReadsGoThroughTheWorldHandleAndCheckHeight() throws Exception {
        CraftWorld world = craftWorld();

        BlockState stone = world.getNMSBlockState(0, 64, 0);
        assertEquals("minecraft:stone", stone.getAsString());
        assertEquals(1, stone.getGlobalId());
        assertEquals(Material.STONE, stone.getBukkitMaterial());
        assertTrue(world.getNMSBlockState(new BlockPos(5, 64, 5)).isAir());
        assertTrue(world.isChunkLoaded(0, 0));
        assertFalse(world.isChunkLoaded(4, 4));

        IllegalArgumentException below = assertThrows(
                IllegalArgumentException.class, () -> world.getNMSBlockState(0, -65, 0));
        assertTrue(below.getMessage().contains("valid range [-64, 320)"), below.getMessage());
        assertThrows(IllegalArgumentException.class, () -> world.getNMSBlockState(0, 320, 0));
    }

    @Test
    void entityAndPlayerReadsAreWorldScoped() throws Exception {
        CraftWorld world = craftWorld();

        assertEquals(3, world.getNMSEntities().size());
        assertNotNull(world.getNMSEntity(FakeNmsHost.ENTITY_ID));
        assertEquals(
                "minecraft:armor_stand",
                world.getNMSEntity(FakeNmsHost.ENTITY_ID).getTypeKey());
        assertEquals(
                FakeNmsHost.WORLD_ID,
                world.getNMSEntity(FakeNmsHost.ENTITY_ID).level().getUUID());
        assertNull(world.getNMSEntity(UUID.randomUUID()));

        assertEquals(1, world.getNMSPlayers().size());
        assertEquals("Steve", world.getNMSPlayer(FakeNmsHost.PLAYER_ID).getName());
        assertEquals(
                FakeNmsHost.PLAYER_ID,
                world.getNMSPlayers().get(0).getUUID());
        assertNull(world.getNMSPlayer(UUID.randomUUID()));
    }

    @Test
    void serverLevelRejectsSnapshotsThatBelongToAnotherWorld() throws Exception {
        CraftWorld world = craftWorld();
        ServerLevel level = world.getHandle();
        UUID otherWorld = UUID.randomUUID();
        UUID foreignEntity = UUID.randomUUID();
        host.entities.put(
                foreignEntity,
                new dev.ferrite.nms.host.EntitySnapshot(
                        foreignEntity,
                        "minecraft:pig",
                        otherWorld,
                        0.0,
                        64.0,
                        0.0,
                        0.0F,
                        0.0F,
                        0.0,
                        0.0,
                        0.0,
                        null,
                        java.util.List.of()));

        NmsHostException error =
                assertThrows(NmsHostException.class, () -> level.getEntity(foreignEntity));
        assertEquals("wrong_world", error.code());
        assertTrue(error.getMessage().contains(otherWorld.toString()), error.getMessage());
    }

    @Test
    void asyncBlockWritesUseTheWorldUuidAndCompleteOnTheRegionThread() throws Exception {
        CraftWorld world = craftWorld();
        BlockState state = world.getNMSBlockState(0, 64, 0);

        CompletableFuture<Void> write = world.setBlockAsync(new BlockPos(4, 70, 4), state);
        assertFalse(write.isDone());
        assertEquals(
                new FakeNmsHost.BlockWrite(FakeNmsHost.WORLD_ID, 4, 70, 4, 1),
                host.blockWrites.get(0));

        host.completeBlockWrite();
        write.get(5, TimeUnit.SECONDS);
        assertEquals("ferrite-region-0", host.completionThread.get().getName());

        assertThrows(
                IllegalArgumentException.class,
                () -> world.setBlockAsync(0, 320, 0, state));
    }

    private static CraftWorld craftWorld() throws Exception {
        return EntityTest.instantiateCraftSubclass(
                CraftWorld.class, FakeNmsHost.WORLD_ID, "getUID");
    }
}
