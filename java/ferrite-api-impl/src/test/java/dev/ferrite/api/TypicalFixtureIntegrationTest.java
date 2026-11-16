package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assertions.fail;

import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.test.PluginJarLoader;
import dev.ferrite.api.test.TestServer;
import dev.ferrite.api.world.FerriteWorld;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.UUID;
import org.bukkit.GameMode;
import org.bukkit.Material;
import org.bukkit.block.BlockFace;
import org.bukkit.event.block.Action;
import org.bukkit.event.block.BlockBreakEvent;
import org.bukkit.event.player.PlayerInteractEvent;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.plugin.java.JavaPlugin;
import org.junit.jupiter.api.Test;

/**
 * Loads the real {@code typical-plugin-1.0.0.jar} with a URLClassLoader, runs the whole
 * first vertical slice, and asserts on the fixture's own log.
 *
 * <p>This is not the Lane 1 embedded-JVM integration: that test starts the JVM through
 * {@code crates/ferrite-plugin-host} and is blocked until the host contract and the Java
 * host module exist. This test still proves the facade and the fixture jar agree on the
 * Bukkit surface.
 */
class TypicalFixtureIntegrationTest {

    @Test
    void typicalFixtureRunsTheWholeSlice() throws Exception {
        String jarProperty = System.getProperty("ferrite.fixture.typical.jar");
        assertNotNull(jarProperty, "ferrite.fixture.typical.jar must be set by Gradle");
        Path jar = Path.of(jarProperty);
        assertTrue(Files.isRegularFile(jar), "fixture jar does not exist: " + jar);

        try (TestServer server = new TestServer()) {
            FerriteWorld world = server.runtime().world("world");
            world.blockStore().setType(0, 64, 0, Material.STONE);
            world.blockStore().setType(4, 64, 4, Material.DIAMOND_BLOCK);

            Path dataFolder = server.tempDir().resolve("typical-data");
            try (PluginJarLoader.Loaded loaded = PluginJarLoader.load(server.runtime(), jar, dataFolder)) {
                JavaPlugin plugin = loaded.plugin();
                assertTrue(plugin.isEnabled());
                assertTrue(plugin.getLogger() != null);
                List<String> log = readLog(dataFolder);

                assertHas(log, "lifecycle:onLoad");
                assertHasPrefix(log, "lifecycle:onEnable:thread=Test-Main");
                assertHas(log, "config:greeting=hello from config.yml");
                assertHas(log, "config:exists=true");
                assertHas(log, "command:registered=ferrite-hello:permission=ferrite.typical.hello:aliases=[fhello]");
                assertHas(log, "item:plain-unpolluted=true");
                assertHas(log, "item:type=DIAMOND_SWORD:amount=2");
                assertHas(log, "item:name=Ferrite Blade");
                assertHas(log, "item:lore=[first line, second line]");
                assertHas(log, "item:damage=12");
                assertHas(log, "item:cmd=42");
                assertHas(log, "item:unbreakable=true");
                assertHas(log, "item:flags=[HIDE_UNBREAKABLE]");
                assertHas(log, "item:model=typicalfixture:ferrite_blade_model");
                assertHas(log, "item:tooltip=typicalfixture:ferrite_tooltip");
                assertHas(log, "item:enchantable=15");
                assertHas(log, "item:food=6");
                assertHas(log, "item:nested-pdc=123");
                assertHas(log, "item:max-stack=8");
                assertHas(log, "item:pdc=pdc-value");
                assertHas(log, "item:pdc-view=pdc-value");
                assertHas(log, "item:clone-equal=true:similar=true");
                assertHasPrefix(log, "world:name=world:env=NORMAL");
                assertHas(log, "world:block-before=STONE");
                assertHas(log, "world:block-after=GOLD_BLOCK");
                assertHasPrefix(log, "unsupported:world-border=");
                assertHasPrefix(log, "unsupported:region-scheduler=");
                assertTrue(log.stream().anyMatch(line -> line.contains("unsupported:world-border=")
                        && line.contains("World#getWorldBorder") && line.contains("TypicalFixture")));
                assertTrue(log.stream().anyMatch(line -> line.contains("unsupported:region-scheduler=")
                        && line.contains("Server#getRegionScheduler") && line.contains("TypicalFixture")));
                assertTrue(log.stream().anyMatch(line -> line.contains("unsupported:item-component=")
                        && line.contains("DIAMOND_SWORD")
                        && line.contains("minecraft:glider")
                        && line.contains("TypicalFixture")));
                assertTrue(log.stream().anyMatch(line -> line.contains("unsupported:pdc-type=")
                        && line.contains("PersistentDataContainer#set")
                        && line.contains("TypicalFixture")));

                FerritePlayer player = server.onMain(() -> {
                    FerritePlayer created = server.runtime().createPlayer(
                            "Alice", UUID.nameUUIDFromBytes("alice".getBytes(StandardCharsets.UTF_8)),
                            false, GameMode.SURVIVAL, world, 0.5, 65.0, 0.5);
                    server.runtime().playerJoined(created);
                    return created;
                });

                log = readLog(dataFolder);
                int lowest = indexOf(log, "join:lowest:Alice");
                int monitor = indexOf(log, "join:monitor:Alice:world=world");
                assertTrue(lowest >= 0, "missing lowest-priority join: " + log);
                assertTrue(monitor > lowest, "LOWEST must run before MONITOR: " + log);
                assertHas(log, "inventory:main-hand=Inventory Blade:lore=[inv lore]:damage=4:pdc=77");
                assertHas(log, "inventory:copy-isolated=true");
                assertHas(log, "inventory:add-remove=addOverflow=0:removeOverflow=0:remaining=3");

                server.onMain(() -> server.runtime().commandDispatcher()
                        .dispatch(player, "/ferrite-hello Bob"));
                assertTrue(player.state().messages().stream()
                        .anyMatch(message -> message.contains("do not have permission")),
                        "non-op player must be denied: " + player.state().messages());
                assertFalse(player.isOp());
                log = readLog(dataFolder);
                assertHasPrefix(log, "preprocess:");

                player.setOp(true);
                server.onMain(() -> server.runtime().commandDispatcher()
                        .dispatch(player, "/ferrite-hello Bob"));
                assertTrue(player.state().messages().contains("Hello, Bob! hello from config.yml"),
                        "command response missing: " + player.state().messages());
                log = readLog(dataFolder);
                assertHas(log, "command:executed:label=ferrite-hello:sender=Alice:args=Bob");

                BlockBreakEvent breakEvent = server.onMain(() -> {
                    BlockBreakEvent created = new BlockBreakEvent(world.getBlockAt(4, 64, 4), player);
                    created.callEvent();
                    return created;
                });
                assertTrue(breakEvent.isCancelled());
                log = readLog(dataFolder);
                assertHas(log, "break:cancelled:DIAMOND_BLOCK");

                PlayerInteractEvent interactEvent = server.onMain(() -> {
                    PlayerInteractEvent created = new PlayerInteractEvent(
                            player, Action.RIGHT_CLICK_BLOCK, null,
                            world.getBlockAt(4, 64, 4), BlockFace.NORTH, EquipmentSlot.HAND);
                    created.callEvent();
                    return created;
                });
                assertTrue(interactEvent.isCancelled());
                log = readLog(dataFolder);
                assertHas(log, "interact:cancelled");

                server.tick(6);
                log = readLog(dataFolder);
                assertCountPrefix(log, "scheduler:sync-repeat:", 3);
                assertCountPrefix(log, "scheduler:sync-delayed", 1);
                awaitLine(dataFolder, line -> line.startsWith("scheduler:async:thread="));
                log = readLog(dataFolder);
                String asyncLine = log.stream()
                        .filter(line -> line.startsWith("scheduler:async:thread="))
                        .findFirst()
                        .orElseThrow();
                assertFalse(asyncLine.contains("Test-Main"), "async task must not run on the main thread: " + asyncLine);

                server.onMain(() -> server.runtime().disablePlugin(plugin));
                assertFalse(plugin.isEnabled());
                assertTrue(server.runtime().scheduler().getPendingTasks().isEmpty());
                log = readLog(dataFolder);
                assertHas(log, "lifecycle:onDisable:syncRuns=3");

                int before = log.size();
                server.tick(4);
                assertTrue(Files.readAllLines(dataFolder.resolve("typical.log")).size() == before,
                        "no fixture task may run after disable");
                server.onMain(() -> {
                    FerritePlayer bob = server.runtime().createPlayer(
                            "Bob", UUID.nameUUIDFromBytes("bob".getBytes(StandardCharsets.UTF_8)),
                            true, GameMode.SURVIVAL, world, 0.5, 65.0, 0.5);
                    server.runtime().playerJoined(bob);
                });
                log = readLog(dataFolder);
                assertTrue(log.stream().noneMatch(line -> line.contains("join:lowest:Bob")),
                        "disabled listener must not fire: " + log);
            }
        }
    }

    private static List<String> readLog(Path dataFolder) throws Exception {
        Path log = dataFolder.resolve("typical.log");
        if (!Files.exists(log)) {
            return List.of();
        }
        return Files.readAllLines(log);
    }

    private static void awaitLine(Path dataFolder, java.util.function.Predicate<String> predicate)
            throws Exception {
        long deadline = System.nanoTime() + 5_000_000_000L;
        while (System.nanoTime() < deadline) {
            if (readLog(dataFolder).stream().anyMatch(predicate)) {
                return;
            }
            Thread.sleep(25);
        }
        fail("timed out waiting for fixture log line: " + dataFolder.resolve("typical.log"));
    }

    private static int indexOf(List<String> log, String prefix) {
        for (int index = 0; index < log.size(); index++) {
            if (log.get(index).startsWith(prefix)) {
                return index;
            }
        }
        return -1;
    }

    private static void assertHas(List<String> log, String expected) {
        assertTrue(log.contains(expected), "missing log line <" + expected + "> in " + log);
    }

    private static void assertHasPrefix(List<String> log, String prefix) {
        assertTrue(log.stream().anyMatch(line -> line.startsWith(prefix)),
                "missing log line with prefix <" + prefix + "> in " + log);
    }

    private static void assertCountPrefix(List<String> log, String prefix, int expected) {
        long count = log.stream().filter(line -> line.startsWith(prefix)).count();
        assertTrue(count == expected,
                "expected " + expected + " lines with prefix <" + prefix + "> but found " + count + " in " + log);
    }
}
