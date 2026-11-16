package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.test.TestPlugin;
import dev.ferrite.api.test.TestServer;
import java.util.List;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import org.bukkit.scheduler.BukkitTask;
import org.junit.jupiter.api.Test;

class SchedulerTest {

    @Test
    void syncTasksRunOnlyOnTickAndOnTheMainThread() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("SchedulerPlugin");
            server.enable(plugin);
            List<String> runs = new CopyOnWriteArrayList<>();
            BukkitTask immediate = server.onMain(() -> plugin.getServer().getScheduler()
                    .runTask(plugin, () -> runs.add("immediate:" + server.runtime().isPrimaryThread())));
            BukkitTask delayed = server.onMain(() -> plugin.getServer().getScheduler()
                    .runTaskLater(plugin, () -> runs.add("delayed:" + server.runtime().isPrimaryThread()), 2));

            assertTrue(runs.isEmpty(), "sync tasks must not run before a tick");
            server.tick(1);
            assertEquals(List.of("immediate:true"), runs);
            assertEquals(immediate.getTaskId() + 1, delayed.getTaskId());
            server.tick(1);
            assertEquals(List.of("immediate:true", "delayed:true"), runs);
        }
    }

    @Test
    void repeatingTasksAndCancellationMatchBukkit() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("RepeatPlugin");
            server.enable(plugin);
            List<String> runs = new CopyOnWriteArrayList<>();
            BukkitTask repeating = server.onMain(() -> plugin.getServer().getScheduler()
                    .runTaskTimer(plugin, () -> runs.add("repeat"), 1, 2));

            server.tick(5);
            assertEquals(List.of("repeat", "repeat", "repeat"), runs);

            repeating.cancel();
            assertTrue(repeating.isCancelled());
            server.tick(4);
            assertEquals(3, runs.size());

            BukkitTask cancelledBeforeRun = server.onMain(() -> plugin.getServer().getScheduler()
                    .runTask(plugin, () -> runs.add("never")));
            cancelledBeforeRun.cancel();
            server.tick(1);
            assertEquals(3, runs.size());
            assertFalse(plugin.getServer().getScheduler().getPendingTasks().stream()
                    .anyMatch(task -> task.getTaskId() == cancelledBeforeRun.getTaskId()));
        }
    }

    @Test
    void asyncTasksUseAnotherThreadAndPluginDisableCancelsThem() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("AsyncPlugin");
            server.enable(plugin);
            List<String> threads = new CopyOnWriteArrayList<>();
            CountDownLatch latch = new CountDownLatch(1);
            server.onMain(() -> plugin.getServer().getScheduler().runTaskAsynchronously(plugin, () -> {
                threads.add(Thread.currentThread().getName());
                assertFalse(server.runtime().isPrimaryThread());
                latch.countDown();
            }));
            assertTrue(latch.await(5, TimeUnit.SECONDS), "async task did not run");
            assertEquals(1, threads.size());
            assertFalse(threads.get(0).startsWith("Test-Main"));

            List<String> syncRuns = new CopyOnWriteArrayList<>();
            server.onMain(() -> plugin.getServer().getScheduler()
                    .runTaskTimer(plugin, () -> syncRuns.add("tick"), 1, 1));
            server.tick(2);
            assertEquals(2, syncRuns.size());

            server.disable(plugin);
            assertTrue(server.runtime().scheduler().getPendingTasks().isEmpty());
            server.tick(3);
            assertEquals(2, syncRuns.size(), "disabled plugins must not keep ticking");
        }
    }

    @Test
    void callSyncMethodAndThreadGuards() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("SyncCallPlugin");
            server.enable(plugin);

            Future<String> future = plugin.getServer().getScheduler()
                    .callSyncMethod(plugin, () -> "main=" + server.runtime().isPrimaryThread());
            // Bukkit schedules callSyncMethod for the next server tick.
            server.tick(1);
            assertEquals("main=true", future.get(5, TimeUnit.SECONDS));

            assertThrows(IllegalStateException.class, () -> server.runtime().tick());

            CountDownLatch executed = new CountDownLatch(1);
            server.onMain(() -> plugin.getServer().getScheduler()
                    .getMainThreadExecutor(plugin)
                    .execute(executed::countDown));
            assertTrue(executed.await(5, TimeUnit.SECONDS));
        }
    }
}
