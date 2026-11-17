package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.test.TestPluginFactory;
import dev.ferrite.api.test.TestServer;
import dev.ferrite.api.world.FerriteWorld;
import java.io.StringReader;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import net.kyori.adventure.text.Component;
import org.bukkit.GameMode;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.block.Block;
import org.bukkit.block.BlockFace;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.block.BlockBreakEvent;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.plugin.PluginDescriptionFile;
import org.junit.jupiter.api.Test;

class ApiBreadthClosureTest {

    @Test
    void playerAndBlockBreadthPathsWorkWhileUnmodelledRefusalsStayExplicit() throws Exception {
        try (TestServer testServer = new TestServer()) {
            FerriteRuntime runtime = testServer.runtime();
            testServer.onMain(() -> {
                FerriteWorld world = runtime.world("world");
                FerritePlayer player = runtime.playerJoined(runtime.createPlayer(
                        "BreadthPlayer", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 1.5, 65.0, 2.5));

                assertTrue(player.isValid());
                assertFalse(player.isDead());
                assertEquals(20.0, player.getMaxHealth());
                assertEquals(66.62, player.getEyeLocation().getY(), 1e-6);
                Location copy = new Location(null, 0, 0, 0);
                player.getLocation(copy);
                assertEquals(1.5, copy.getX(), 1e-6);

                player.displayName(Component.text("Alias"));
                assertEquals("Alias", player.getDisplayName());
                player.playerListName(Component.text("TabAlias"));
                assertEquals("TabAlias", player.getPlayerListName());

                NamespacedKey key = new NamespacedKey("ferrite", "rank");
                player.getPersistentDataContainer().set(key, PersistentDataType.INTEGER, 3);
                assertEquals(3, player.getPersistentDataContainer().get(key, PersistentDataType.INTEGER));

                Block block = world.getBlockAt(4, 64, 5);
                block.setType(Material.STONE);
                assertEquals(Material.STONE, block.getBlockData().getMaterial());
                assertFalse(block.isEmpty());
                assertFalse(block.isLiquid());
                assertTrue(block.getRelative(BlockFace.UP).isEmpty());
                assertThrows(UnsupportedOperationException.class, block::getState);

                // Unmodelled player methods remain explicit UnsupportedOperationExceptions.
                assertThrows(UnsupportedOperationException.class, () -> player.sleep(copy, true));
                assertThrows(UnsupportedOperationException.class, player::getConnection);
                assertThrows(UnsupportedOperationException.class, player::loadData);
                return null;
            });
        }
    }

    @Test
    void cancellableEventOrderingRunsOncePerListenerAndRespectsIgnoreCancelled() throws Exception {
        try (TestServer testServer = new TestServer()) {
            FerriteRuntime runtime = testServer.runtime();
            var loaded = TestPluginFactory.createWithLoader();
            var desc = new PluginDescriptionFile(new StringReader("""
                    name: EventOrderPlugin
                    version: 1.0.0
                    main: dev.ferrite.api.test.LoadedTestPlugin
                    """));
            var plugin = testServer.onMain(() -> {
                var attached = runtime.attachPlugin(
                        loaded.plugin(),
                        desc,
                        testServer.tempDir().resolve("event-plugin").toFile(),
                        testServer.tempDir().resolve("event-plugin.jar").toFile(),
                        loaded.classLoader());
                runtime.plugins().enablePlugin(attached);
                return attached;
            });

            List<String> trace = new ArrayList<>();
            runtime.plugins().registerEvents(new Listener() {
                @EventHandler(priority = EventPriority.LOWEST)
                public void lowest(BlockBreakEvent event) {
                    trace.add("lowest");
                    event.setCancelled(true);
                }

                @EventHandler(priority = EventPriority.NORMAL, ignoreCancelled = true)
                public void normalIgnored(BlockBreakEvent event) {
                    trace.add("normalIgnored");
                }

                @EventHandler(priority = EventPriority.MONITOR)
                public void monitor(BlockBreakEvent event) {
                    trace.add("monitor:" + event.isCancelled());
                }
            }, plugin);

            testServer.onMain(() -> {
                FerriteWorld world = runtime.world("world");
                FerritePlayer player = runtime.playerJoined(runtime.createPlayer(
                        "EventPlayer", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 0.5, 65, 0.5));
                BlockBreakEvent event = new BlockBreakEvent(world.getBlockAt(0, 64, 0), player);
                runtime.plugins().callEvent(event);
                assertEquals(List.of("lowest", "monitor:true"), trace);
                return null;
            });
        }
    }
}
