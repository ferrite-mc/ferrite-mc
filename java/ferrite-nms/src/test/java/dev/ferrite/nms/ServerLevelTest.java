// Clean-room Ferrite NMS test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.NmsHostException;
import dev.ferrite.nms.test.FakeNmsHost;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.TimeUnit;
import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import org.bukkit.Material;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class ServerLevelTest {
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
    void readsTheBoundWorldSnapshot() {
        ServerLevel level = ServerLevel.of(FakeNmsHost.WORLD_ID);
        assertEquals(FakeNmsHost.WORLD_ID, level.getUUID());
        assertEquals("world", level.getName());
        assertEquals("minecraft:overworld", level.dimensionKey());
        assertEquals("NORMAL", level.environment());
        assertEquals(1234L, level.getSeed());
        assertEquals(6000L, level.getDayTime());
        assertEquals(new BlockPos(0, 64, 0), level.getSpawnPos());
        assertEquals(-64, level.getMinY());
        assertEquals(320, level.getMaxY());
        assertTrue(level.owns(FakeNmsHost.WORLD_ID));
        assertFalse(level.owns(UUID.randomUUID()));
        assertTrue(level.getChunkSource().isChunkLoaded(0, 0));
        assertFalse(level.getChunkSource().isChunkLoaded(4, 4));
    }

    @Test
    void readsBlocksFromThisWorldOnly() {
        ServerLevel level = ServerLevel.of(FakeNmsHost.WORLD_ID);
        assertEquals("minecraft:stone", level.getBlockState(0, 64, 0).getAsString());
        assertEquals(1, level.getBlockState(new BlockPos(0, 64, 0)).getGlobalId());
        assertTrue(level.getBlockState(5, 64, 5).isAir());
        assertEquals(Material.STONE, level.getBlockState(0, 64, 0).getBukkitMaterial());
    }

    @Test
    void anUnknownWorldFailsExplicitlyInsteadOfReturningAnEmptyWorld() {
        NmsHostException error = assertThrows(NmsHostException.class,
                () -> ServerLevel.of(UUID.randomUUID()));
        assertEquals("unknown_world", error.code());
    }

    @Test
    void synchronousBlockWritesAreRefusedWithTheAsyncAlternative() {
        ServerLevel level = ServerLevel.of(FakeNmsHost.WORLD_ID);
        UnsupportedOperationException error = assertThrows(UnsupportedOperationException.class,
                () -> level.setBlock(level.getSpawnPos(), level.getBlockState(0, 64, 0)));
        assertTrue(error.getMessage().contains("ServerLevel#setBlock"), error.getMessage());
        assertTrue(error.getMessage().contains("setBlockAsync"), error.getMessage());
    }

    @Test
    void asyncWritesAreRoutedAndDoNotBlockTheCaller() throws Exception {
        ServerLevel level = ServerLevel.of(FakeNmsHost.WORLD_ID);
        var state = level.getBlockState(0, 64, 0);
        CompletableFuture<Void> future = level.setBlockAsync(new BlockPos(1, 2, 3), state);

        assertFalse(future.isDone(), "the fake host completes writes only when the region runs");
        assertEquals(1, host.blockWrites.size());
        assertEquals(new FakeNmsHost.BlockWrite(FakeNmsHost.WORLD_ID, 1, 2, 3, 1), host.blockWrites.get(0));

        host.completeBlockWrite();
        future.get(5, TimeUnit.SECONDS);
        assertTrue(future.isDone());
        assertEquals("ferrite-region-0", host.completionThread.get().getName());
        assertNotEquals(Thread.currentThread().getName(), host.completionThread.get().getName());
    }

    @Test
    void unresolvedBlockStatesResolveThroughTheHostRegistry() {
        ServerLevel level = ServerLevel.of(FakeNmsHost.WORLD_ID);
        var data = new BlockStateForTest(Material.OAK_STAIRS,
                "minecraft:oak_stairs[facing=north,half=top,shape=straight,waterlogged=true]").state();
        level.setBlockAsync(new BlockPos(4, 70, 4), data);
        assertEquals(5463, host.blockWrites.get(0).globalStateId());
    }

    @Test
    void hostWriteFailuresReachTheFutureWithoutBeingSwallowed() throws Exception {
        ServerLevel level = ServerLevel.of(FakeNmsHost.WORLD_ID);
        CompletableFuture<Void> future = level.setBlockAsync(level.getSpawnPos(), level.getBlockState(0, 64, 0));
        NmsHostException failure = new NmsHostException("region_unavailable", "owner region stopped");
        host.failBlockWrite(failure);

        ExecutionException error = assertThrows(ExecutionException.class,
                () -> future.get(5, TimeUnit.SECONDS));
        assertEquals(failure, error.getCause());
    }

    /** Builds an unresolved state without Paper's BlockData factory. */
    private record BlockStateForTest(Material material, String text) {
        net.minecraft.world.level.block.state.BlockState state() {
            return new net.minecraft.world.level.block.state.BlockState(
                    new net.minecraft.world.level.block.Block(material), text, -1);
        }
    }
}
