package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.internal.FerriteContext;
import dev.ferrite.api.test.TestPlugin;
import dev.ferrite.api.test.TestServer;
import dev.ferrite.api.world.FerriteWorld;
import java.io.File;
import java.util.UUID;
import org.bukkit.GameMode;
import org.junit.jupiter.api.Test;

class UnsupportedApiTest {

    @Test
    void serverWorldPlayerAndSchedulerRefusalsNameTheMethodAndPlugin() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("UnsupportedPlugin");
            server.enable(plugin);
            FerriteWorld world = server.runtime().world("world");
            FerritePlayer player = server.onMain(() -> server.runtime().createPlayer(
                    "Probe", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 0.5, 65, 0.5));

            UnsupportedOperationException border = assertThrows(UnsupportedOperationException.class,
                    () -> FerriteContext.withPlugin("UnsupportedPlugin", world::getWorldBorder));
            assertTrue(border.getMessage().contains("World#getWorldBorder"));
            assertTrue(border.getMessage().contains("plugin 'UnsupportedPlugin'"));

            UnsupportedOperationException region = assertThrows(UnsupportedOperationException.class,
                    () -> server.runtime().server().getRegionScheduler());
            assertTrue(region.getMessage().contains("Server#getRegionScheduler"));

            UnsupportedOperationException spigot = assertThrows(UnsupportedOperationException.class,
                    player::spigot);
            assertTrue(spigot.getMessage().contains("Player#spigot"));

            UnsupportedOperationException load = assertThrows(UnsupportedOperationException.class,
                    () -> server.runtime().plugins().loadPlugin(new File("missing.jar")));
            assertTrue(load.getMessage().contains("PluginManager#loadPlugin"));
            assertTrue(load.getMessage().contains("owned by the Ferrite host")
                    || load.getMessage().contains("PluginManager#loadPlugin"));

            assertEquals(null, server.runtime().server().getPluginCommand("missing"));
            assertNotNull(server.runtime().server().getWorld("world"));
            assertEquals(world, server.runtime().server().getWorld(world.getUID()));
        }
    }

    @Test
    void worldGameRuleWritesAndInventoryMenusAreRefused() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("RefusalPlugin");
            server.enable(plugin);
            FerriteWorld world = server.runtime().world("world");

            assertEquals("true", world.getGameRuleValue("doDaylightCycle"));
            UnsupportedOperationException rules = assertThrows(UnsupportedOperationException.class,
                    () -> world.setGameRuleValue("doDaylightCycle", "false"));
            assertTrue(rules.getMessage().contains("World#setGameRuleValue"));

            FerritePlayer player = server.onMain(() -> server.runtime().createPlayer(
                    "Inventor", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 0.5, 65, 0.5));
            assertTrue(player.getInventory().getViewers().isEmpty());
            assertEquals(41, player.getInventory().getSize());
            UnsupportedOperationException viewers =
                    assertThrows(UnsupportedOperationException.class,
                            () -> player.getInventory().close());
            assertTrue(viewers.getMessage().contains("PlayerInventory#close"));
        }
    }
}
