package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.test.TestPlugin;
import dev.ferrite.api.test.TestServer;
import dev.ferrite.api.world.FerriteWorld;
import io.papermc.paper.threadedregions.scheduler.ScheduledTask;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import org.bukkit.GameMode;
import org.junit.jupiter.api.Test;

class FoliaSchedulerTest {

    @Test
    void regionAndGlobalSchedulersRunAndReportStates() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("FoliaPlugin");
            server.enable(plugin);
            FerriteWorld world = server.runtime().world("world");
            List<String> events = new CopyOnWriteArrayList<>();

            ScheduledTask regionTask = server.onMain(() -> plugin.getServer().getRegionScheduler()
                    .runDelayed(plugin, world, 1, -2, task -> events.add(
                            "region:" + plugin.getServer().isOwnedByCurrentRegion(world, 1, -2)),
                            2L));
            ScheduledTask globalRepeat = server.onMain(() -> plugin.getServer().getGlobalRegionScheduler()
                    .runAtFixedRate(plugin, task -> events.add(
                            "global:" + plugin.getServer().isGlobalTickThread()),
                            1L, 2L));

            assertEquals(ScheduledTask.ExecutionState.IDLE, regionTask.getExecutionState());
            assertFalse(regionTask.isRepeatingTask());
            assertTrue(globalRepeat.isRepeatingTask());

            server.tick(1);
            assertEquals(List.of("global:true"), events);
            server.tick(1);
            assertEquals(List.of("global:true", "region:true"), events);
            assertEquals(ScheduledTask.ExecutionState.FINISHED, regionTask.getExecutionState());
            assertEquals(ScheduledTask.CancelledState.ALREADY_EXECUTED, regionTask.cancel());

            assertEquals(ScheduledTask.CancelledState.NEXT_RUNS_CANCELLED, globalRepeat.cancel());
            assertEquals(ScheduledTask.ExecutionState.CANCELLED, globalRepeat.getExecutionState());
            server.tick(4);
            assertEquals(2, events.size());
        }
    }

    @Test
    void asyncSchedulerRunsOffMainThreadAndCancelsCleanly() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("FoliaAsyncPlugin");
            server.enable(plugin);
            CountDownLatch latch = new CountDownLatch(1);
            List<String> threads = new CopyOnWriteArrayList<>();

            ScheduledTask task = plugin.getServer().getAsyncScheduler()
                    .runNow(plugin, scheduled -> {
                        threads.add(Thread.currentThread().getName());
                        assertFalse(server.runtime().isPrimaryThread());
                        latch.countDown();
                    });
            assertNotNull(task);
            assertTrue(latch.await(5, TimeUnit.SECONDS));
            assertFalse(threads.get(0).startsWith("Test-Main"));
            long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(2);
            while (task.getExecutionState() != ScheduledTask.ExecutionState.FINISHED
                    && System.nanoTime() < deadline) {
                Thread.sleep(5);
            }
            assertEquals(ScheduledTask.ExecutionState.FINISHED, task.getExecutionState());

            AtomicInteger ticks = new AtomicInteger();
            plugin.getServer().getAsyncScheduler().runAtFixedRate(
                    plugin, scheduled -> ticks.incrementAndGet(), 5, 5, TimeUnit.MILLISECONDS);
            server.disable(plugin);
            int frozen = ticks.get();
            Thread.sleep(40);
            assertEquals(frozen, ticks.get());
        }
    }

    @Test
    void entitySchedulerInvokesRetiredWhenPlayerLeaves() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("FoliaEntityPlugin");
            server.enable(plugin);
            FerriteWorld world = server.runtime().world("world");
            FerritePlayer player = server.onMain(() -> {
                FerritePlayer created = server.runtime().createPlayer(
                        "FoliaWalker",
                        UUID.randomUUID(),
                        false,
                        GameMode.SURVIVAL,
                        world,
                        0.5,
                        65.0,
                        0.5);
                return server.runtime().playerJoined(created);
            });

            List<String> events = new CopyOnWriteArrayList<>();
            ScheduledTask delayed = server.onMain(() -> player.getScheduler().runDelayed(
                    plugin,
                    task -> events.add("ran"),
                    () -> events.add("retired"),
                    2L));
            assertNotNull(delayed);

            server.onMain(() -> server.runtime().playerQuit(player.getUniqueId()));
            assertFalse(player.isValid());
            assertNull(server.onMain(() -> player.getScheduler().run(
                    plugin, task -> events.add("after-quit"), null)));

            server.tick(3);
            assertEquals(List.of("retired"), events);
        }
    }
}
