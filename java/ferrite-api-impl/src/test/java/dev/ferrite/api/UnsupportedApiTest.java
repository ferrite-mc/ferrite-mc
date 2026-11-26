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

            assertNotNull(world.getWorldBorder());
            UnsupportedOperationException biomeWrite = assertThrows(UnsupportedOperationException.class,
                    () -> FerriteContext.withPlugin("UnsupportedPlugin",
                            () -> world.setBiome(0, 64, 0, org.bukkit.block.Biome.PLAINS)));
            assertTrue(biomeWrite.getMessage().contains("World#setBiome"));
            assertTrue(biomeWrite.getMessage().contains("plugin 'UnsupportedPlugin'"));

            assertNotNull(server.runtime().server().getRegionScheduler());
            assertNotNull(server.runtime().server().getGlobalRegionScheduler());
            assertNotNull(server.runtime().server().getAsyncScheduler());
            assertNotNull(player.getScheduler());

            assertNotNull(player.spigot());
            player.setAllowFlight(true);
            player.setFlying(true);
            assertTrue(player.isFlying());
            player.setLevel(7);
            assertEquals(7, player.getLevel());
            assertNotNull(server.runtime().server().getServicesManager());
            assertNotNull(server.runtime().server().getOfflinePlayer("OfflineHero"));
            assertTrue(player.addPotionEffect(new org.bukkit.potion.PotionEffect(
                    org.bukkit.potion.PotionEffectType.SPEED, 200, 1)));
            assertTrue(player.hasPotionEffect(org.bukkit.potion.PotionEffectType.SPEED));
            player.removePotionEffect(org.bukkit.potion.PotionEffectType.SPEED);
            assertEquals(0, player.getActivePotionEffects().size());
            UnsupportedOperationException expCooldown = assertThrows(
                    UnsupportedOperationException.class, () -> player.setExpCooldown(5));
            assertTrue(expCooldown.getMessage().contains("setExpCooldown"));

            // Paper plugin commands go through the lifecycle-event API in 26.3. The first
            // slice deliberately refuses that channel instead of silently dropping the
            // handler; a plugin must use plugin.yml commands. `LifecycleEvents.*` cannot
            // initialize without Paper's lifecycle service provider, so exercise the
            // configuration entry point that Paper's typed overload delegates to.
            UnsupportedOperationException lifecycle = assertThrows(UnsupportedOperationException.class,
                    () -> plugin.getLifecycleManager().registerEventHandler(null));
            assertTrue(lifecycle.getMessage().contains("lifecycle event registration"),
                    lifecycle.getMessage());

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
