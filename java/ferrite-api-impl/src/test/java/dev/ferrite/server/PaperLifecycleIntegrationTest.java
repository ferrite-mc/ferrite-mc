package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import io.papermc.paper.threadedregions.scheduler.ScheduledTask;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.logging.Handler;
import java.util.logging.Level;
import java.util.logging.LogRecord;
import org.bukkit.Bukkit;
import org.bukkit.plugin.IllegalPluginAccessException;
import org.bukkit.plugin.Plugin;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

/**
 * Real plugin jars (compiled in the test) loaded through the host's plugin manager: Paper bootstrapper commands, the
 * enable/disable events, listener priority, command registration and what a disable or shutdown must clean up.
 */
class PaperLifecycleIntegrationTest {
    private static FerriteServer server;
    @TempDir Path dir;

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        server = (FerriteServer) Bukkit.getServer();
        // BootstrapTest shuts the shared server's async threads down at its end.
        server.asyncScheduler.restart();
    }

    private final List<UUID> worlds = new ArrayList<>();
    private UUID world(String name) {
        var id = UUID.randomUUID();
        Bootstrap.registerWorld(id.toString(), name, 0, 0, 64, 0);
        worlds.add(id);
        return id;
    }

    @AfterEach void clean() {
        worlds.forEach(id -> Bootstrap.unregisterWorld(id.toString()));
        worlds.clear();
        server.plugins.disablePlugins();
        LifeLog.EVENTS.clear();
    }

    private static final String HEADER = "package life;\nimport dev.ferrite.server.LifeLog;\nimport org.bukkit.event.*;\nimport org.bukkit.plugin.java.JavaPlugin;\n";

    /** LifeA: priorities, the enable/disable bookkeeping, a command through the plugin.yml. */
    private static final String PLUGIN_A = HEADER + """
        public class LifeA extends JavaPlugin implements Listener {
            @Override public void onEnable() {
                LifeLog.add("A:enable");
                var pm = getServer().getPluginManager();
                for (var priority : EventPriority.values()) {
                    pm.registerEvent(LifeLog.Ping.class, this, priority, (l, e) -> {
                        LifeLog.add("A:" + priority);
                        if (priority == EventPriority.LOW) ((Cancellable) e).setCancelled(true);
                    }, this, false);
                }
                pm.registerEvent(LifeLog.Ping.class, this, EventPriority.NORMAL, (l, e) -> LifeLog.add("A:NORMAL-ignoring"), this, true);
                pm.registerEvent(LifeLog.Ping.class, this, EventPriority.HIGH, (l, e) -> LifeLog.add("A:HIGH-ignoring"), this, true);
            }
            @Override public void onDisable() {
                LifeLog.add("A:disable");
                try { getServer().getPluginManager().registerEvents(new Listener() {}, this); LifeLog.add("A:registered-in-disable"); }
                catch (org.bukkit.plugin.IllegalPluginAccessException e) { LifeLog.add("A:refused-in-disable"); }
                try { getServer().getScheduler().runTask(this, () -> {}); LifeLog.add("A:scheduled-in-disable"); }
                catch (org.bukkit.plugin.IllegalPluginAccessException e) { LifeLog.add("A:schedule-refused-in-disable"); }
            }
        }
        """;
    /** LifeB depends on LifeA and watches its enable/disable events. */
    private static final String PLUGIN_B = HEADER + """
        public class LifeB extends JavaPlugin implements Listener {
            @Override public void onEnable() {
                LifeLog.add("B:enable");
                getServer().getPluginManager().registerEvents(this, this);
            }
            @org.bukkit.event.EventHandler public void on(org.bukkit.event.server.PluginDisableEvent e) {
                LifeLog.add("B:sees-disable:" + e.getPlugin().getName() + ":enabled=" + e.getPlugin().isEnabled());
            }
            @org.bukkit.event.EventHandler public void on(org.bukkit.event.server.PluginEnableEvent e) {
                LifeLog.add("B:sees-enable:" + e.getPlugin().getName() + ":enabled=" + e.getPlugin().isEnabled());
            }
            @Override public void onDisable() { LifeLog.add("B:disable"); }
        }
        """;
    /** Paper plugin with a bootstrapper that registers a COMMANDS handler before the plugin class exists. */
    private static final String BOOT = HEADER + """
        public class LifeBoot implements io.papermc.paper.plugin.bootstrap.PluginBootstrap {
            @Override public void bootstrap(io.papermc.paper.plugin.bootstrap.BootstrapContext context) {
                LifeLog.add("boot:bootstrap");
                context.getLifecycleManager().registerEventHandler(io.papermc.paper.plugin.lifecycle.event.types.LifecycleEvents.COMMANDS, event -> {
                    LifeLog.add("boot:commands:" + event.cause());
                    event.registrar().register("bootcmd", "from the bootstrapper", java.util.List.of("bc"), (stack, args) -> LifeLog.add("bootcmd:run"));
                });
            }
        }
        """;
    private static final String BOOT_PLUGIN = HEADER + """
        public class LifeBootPlugin extends JavaPlugin {
            @Override public void onEnable() {
                LifeLog.add("bootplugin:enable");
                getLifecycleManager().registerEventHandler(io.papermc.paper.plugin.lifecycle.event.types.LifecycleEvents.COMMANDS, event -> {
                    LifeLog.add("plugin:commands:" + event.cause());
                    event.registrar().register("plugincmd", "from the plugin", java.util.List.of(), (stack, args) -> LifeLog.add("plugincmd:run"));
                });
            }
        }
        """;

    private Path jar(String name, Map<String, String> sources, Map<String, String> resources) throws Exception {
        return PluginFixture.jar(dir, name, sources, resources);
    }

    private Path pluginA() throws Exception {
        return jar("LifeA", Map.of("life.LifeA", PLUGIN_A), Map.of("plugin.yml", "name: LifeA\nversion: '1.0'\nmain: life.LifeA\napi-version: '1.21'\n"));
    }
    private Path pluginB() throws Exception {
        return jar("LifeB", Map.of("life.LifeB", PLUGIN_B), Map.of("plugin.yml", "name: LifeB\nversion: '1.0'\nmain: life.LifeB\ndepend: [LifeA]\napi-version: '1.21'\n"));
    }
    private Path bootPlugin() throws Exception {
        return jar("LifeBoot", Map.of("life.LifeBoot", BOOT, "life.LifeBootPlugin", BOOT_PLUGIN),
            Map.of("paper-plugin.yml", "name: LifeBoot\nversion: '1.0'\nmain: life.LifeBootPlugin\nbootstrapper: life.LifeBoot\napi-version: '26.1'\n"));
    }
    private Plugin load(Path... jars) {
        server.plugins.loadOrdered(java.util.Arrays.stream(jars).map(Path::toString).toArray(String[]::new), null);
        return server.plugins.getPlugins().length == 0 ? null : server.plugins.getPlugins()[0];
    }
    private static long count(String line) { return LifeLog.EVENTS.stream().filter(line::equals).count(); }

    // --- enable / disable events -----------------------------------------------------------------------------------

    @Test void pluginEnableAndDisableEventsAreDeliveredWhileTheSubjectIsStillEnabledOnDisable() throws Exception {
        var a = pluginA();
        load(a, pluginB());
        var pluginA = server.plugins.getPlugin("LifeA");
        // B registered its listener in onEnable, so it also hears its own enable event, like on Bukkit.
        assertEquals(List.of("A:enable", "B:enable", "B:sees-enable:LifeB:enabled=true"), LifeLog.EVENTS);
        LifeLog.EVENTS.clear();
        server.plugins.disablePlugin(pluginA);
        // B hears the disable of A before A is switched off, then A's own onDisable runs.
        assertEquals(List.of("B:sees-disable:LifeA:enabled=true", "A:disable", "A:refused-in-disable", "A:schedule-refused-in-disable"), LifeLog.EVENTS);
        assertFalse(pluginA.isEnabled());
        server.plugins.enablePlugin(pluginA);
        assertTrue(LifeLog.EVENTS.contains("B:sees-enable:LifeA:enabled=true"), LifeLog.EVENTS.toString());
    }

    @Test void disablingEverythingDisablesInReverseOrderAndAnnouncesEachPlugin() throws Exception {
        load(pluginA(), pluginB());
        LifeLog.EVENTS.clear();
        server.plugins.disablePlugins();
        int bDisable = LifeLog.EVENTS.indexOf("B:disable"), aDisable = LifeLog.EVENTS.indexOf("A:disable");
        assertTrue(bDisable >= 0 && aDisable > bDisable, "dependants go first: " + LifeLog.EVENTS);
    }

    @Test void registeringListenersWhileNotEnabledIsRefusedAndAReenableDoesNotDuplicateHandlers() throws Exception {
        load(pluginA());
        var plugin = server.plugins.getPlugin("LifeA");
        int enabledHandlers = server.plugins.handlerCount(plugin);
        assertTrue(enabledHandlers > 0);
        server.plugins.disablePlugin(plugin);
        assertEquals(0, server.plugins.handlerCount(plugin), "a disabled plugin's listeners are gone");
        var error = assertThrows(IllegalPluginAccessException.class, () -> server.plugins.registerEvents(new org.bukkit.event.Listener() {}, plugin));
        assertTrue(error.getMessage().startsWith("Plugin attempted to register") && error.getMessage().endsWith("while not enabled"), error.getMessage());
        server.plugins.enablePlugin(plugin);
        assertEquals(enabledHandlers, server.plugins.handlerCount(plugin), "enabling again registers each handler once");
    }

    // --- priority --------------------------------------------------------------------------------------------------

    @Test void eventHandlersRunLowestToMonitorAndIgnoreCancelledSkipsAfterACancel() throws Exception {
        load(pluginA());
        LifeLog.EVENTS.clear();
        var ping = new LifeLog.Ping();
        server.plugins.callEvent(ping);
        // LOW cancels: NORMAL (ignoreCancelled=false) and HIGH still run, the two ignoreCancelled handlers do not.
        assertEquals(List.of("A:LOWEST", "A:LOW", "A:NORMAL", "A:HIGH", "A:HIGHEST", "A:MONITOR"), LifeLog.EVENTS);
        assertTrue(ping.isCancelled());
    }

    // --- Paper bootstrapper commands --------------------------------------------------------------------------------

    @Test void aBootstrapperCommandsHandlerIsHandedToThePluginFiresInitialAndReloadAndGoesOnDisable() throws Exception {
        load(bootPlugin());
        var plugin = server.plugins.getPlugin("LifeBoot");
        assertEquals(1, count("boot:commands:INITIAL"));
        assertEquals(1, count("plugin:commands:INITIAL"));
        assertEquals("boot:bootstrap", LifeLog.EVENTS.get(0));
        assertTrue(LifeLog.EVENTS.indexOf("boot:commands:INITIAL") > LifeLog.EVENTS.indexOf("bootplugin:enable"), "commands fire after the plugin is enabled");
        assertNotNull(server.getCommandMap().getCommand("bootcmd"));
        assertNotNull(server.getCommandMap().getCommand("bc"));
        assertNotNull(server.getCommandMap().getCommand("plugincmd"));
        assertEquals(1, Bootstrap.runPluginCommand("console", "bc", new String[0]));
        assertEquals(1, count("bootcmd:run"));
        LifeLog.EVENTS.clear();
        // Server#reloadData asks the host to reload, then fires this; the host call is not available to unit tests.
        FerriteLifecycleManager.fireCommands(io.papermc.paper.plugin.lifecycle.event.registrar.ReloadableRegistrarEvent.Cause.RELOAD);
        assertEquals(1, count("boot:commands:RELOAD"));
        assertEquals(1, count("plugin:commands:RELOAD"));
        assertNotNull(server.getCommandMap().getCommand("bootcmd"), "a reload registers the same commands again, not twice");
        server.plugins.disablePlugin(plugin);
        assertNull(server.getCommandMap().getCommand("bootcmd"));
        assertNull(server.getCommandMap().getCommand("plugincmd"));
    }

    // --- schedulers ------------------------------------------------------------------------------------------------

    @Test void schedulingWhileDisabledIsRefusedButAlreadyCancelledTasksStayCancelled() throws Exception {
        load(pluginA());
        var plugin = server.plugins.getPlugin("LifeA");
        var task = Bukkit.getScheduler().runTaskTimer(plugin, () -> LifeLog.add("sync"), 1, 1);
        server.plugins.disablePlugin(plugin);
        assertTrue(task.isCancelled());
        assertThrows(IllegalPluginAccessException.class, () -> Bukkit.getScheduler().runTask(plugin, () -> {}));
        assertThrows(IllegalPluginAccessException.class, () -> Bukkit.getScheduler().runTaskAsynchronously(plugin, () -> {}));
    }

    @Test void disablingAPluginCancelsItsBukkitGlobalRegionEntityAndAsyncTasks() throws Exception {
        load(pluginA());
        var plugin = server.plugins.getPlugin("LifeA");
        var world = world("life");
        var bukkitWorld = server.getWorld(world);
        var bukkit = Bukkit.getScheduler().runTaskTimer(plugin, () -> LifeLog.add("bukkit"), 1, 1);
        var global = server.getGlobalRegionScheduler().runAtFixedRate(plugin, t -> LifeLog.add("global"), 1, 1);
        var region = server.getRegionScheduler().runAtFixedRate(plugin, bukkitWorld, 0, 0, t -> LifeLog.add("region"), 1, 1);
        var entity = server.entityScheduler(UUID.randomUUID(), () -> true).runAtFixedRate(plugin, t -> LifeLog.add("entity"), null, 1, 1);
        var async = server.getAsyncScheduler().runAtFixedRate(plugin, t -> LifeLog.add("async"), 5, 5, java.util.concurrent.TimeUnit.MILLISECONDS);
        for (int i = 0; i < 3; i++) server.scheduler.tick();
        assertTrue(count("bukkit") >= 1 && count("global") >= 1, LifeLog.EVENTS.toString());

        server.plugins.disablePlugin(plugin);
        assertTrue(bukkit.isCancelled());
        for (ScheduledTask task : new ScheduledTask[]{global, region, entity, async})
            assertTrue(task.isCancelled(), task.getClass().getSimpleName() + " " + task.getExecutionState());
        var before = LifeLog.EVENTS.size();
        for (int i = 0; i < 3; i++) server.scheduler.tick();
        Thread.sleep(40);
        assertEquals(before, LifeLog.EVENTS.size(), "no cancelled task runs again: " + LifeLog.EVENTS);
        assertTrue(server.getScheduler().getPendingTasks().stream().noneMatch(t -> t.getOwner() == plugin));
    }

    @Test void aFailingRepeatingTaskIsLoggedAndKeepsItsSchedule() throws Exception {
        load(pluginA());
        var plugin = server.plugins.getPlugin("LifeA");
        var severe = new ArrayList<String>();
        var handler = new Handler() {
            @Override public void publish(LogRecord record) { if (record.getLevel() == Level.SEVERE) severe.add(record.getMessage()); }
            @Override public void flush() {}
            @Override public void close() {}
        };
        plugin.getLogger().addHandler(handler);
        try {
            var world = world("life2");
            var runs = new AtomicInteger();
            var region = server.getRegionScheduler().runAtFixedRate(plugin, server.getWorld(world), 0, 0, t -> { runs.incrementAndGet(); throw new IllegalStateException("boom"); }, 1, 1);
            var id = (long) region.getClass().getDeclaredField("id").get(region);
            server.regionScheduler.execute(id);
            server.regionScheduler.execute(id);
            assertEquals(2, runs.get(), "the second run happens: an exception does not end a repeating task");
            assertFalse(region.isCancelled());
            assertTrue(severe.stream().anyMatch(m -> m.contains("generated an exception while executing a region task")), severe.toString());

            var asyncRuns = new AtomicInteger();
            server.getAsyncScheduler().runAtFixedRate(plugin, t -> { asyncRuns.incrementAndGet(); throw new IllegalStateException("async boom"); }, 5, 5, java.util.concurrent.TimeUnit.MILLISECONDS);
            long deadline = System.currentTimeMillis() + 3000;
            while (asyncRuns.get() < 3 && System.currentTimeMillis() < deadline) Thread.sleep(5);
            assertTrue(asyncRuns.get() >= 3, "the async task kept running after the first failure: " + asyncRuns);
            assertTrue(severe.stream().anyMatch(m -> m.contains("async task")), severe.toString());
        } finally { plugin.getLogger().removeHandler(handler); }
    }

    @Test void aRunningAsyncTaskShowsUpAsAWorkerUntilItEnds() throws Exception {
        load(pluginA());
        var plugin = server.plugins.getPlugin("LifeA");
        var started = new java.util.concurrent.CountDownLatch(1);
        var release = new java.util.concurrent.CountDownLatch(1);
        var task = Bukkit.getScheduler().runTaskAsynchronously(plugin, () -> { started.countDown(); try { release.await(); } catch (InterruptedException ignored) {} });
        assertTrue(started.await(3, java.util.concurrent.TimeUnit.SECONDS));
        var workers = Bukkit.getScheduler().getActiveWorkers();
        assertTrue(workers.stream().anyMatch(w -> w.getOwner() == plugin && w.getTaskId() == task.getTaskId()), workers.toString());
        assertTrue(workers.stream().filter(w -> w.getTaskId() == task.getTaskId()).allMatch(w -> w.getThread().isAlive()));
        release.countDown();
        long deadline = System.currentTimeMillis() + 3000;
        while (Bukkit.getScheduler().getActiveWorkers().stream().anyMatch(w -> w.getTaskId() == task.getTaskId()) && System.currentTimeMillis() < deadline) Thread.sleep(5);
        assertTrue(Bukkit.getScheduler().getActiveWorkers().stream().noneMatch(w -> w.getTaskId() == task.getTaskId()));
    }

    @Test void servicesAClosingPluginRegisteredAreRemovedWithIt() throws Exception {
        load(pluginA());
        var plugin = server.plugins.getPlugin("LifeA");
        server.getServicesManager().register(Runnable.class, () -> {}, plugin, org.bukkit.plugin.ServicePriority.Normal);
        assertNotNull(server.getServicesManager().load(Runnable.class));
        server.plugins.disablePlugin(plugin);
        assertNull(server.getServicesManager().load(Runnable.class));
    }

    @Test void shutdownCancelsEveryPluginTaskAndForgetsTheCommands() throws Exception {
        load(bootPlugin());
        var plugin = server.plugins.getPlugin("LifeBoot");
        var task = Bukkit.getScheduler().runTaskTimer(plugin, () -> {}, 5, 5);
        server.plugins.disablePlugins();
        assertTrue(task.isCancelled());
        assertNull(server.getCommandMap().getCommand("bootcmd"));
        assertEquals(0, server.plugins.getPlugins().length);
    }
}
