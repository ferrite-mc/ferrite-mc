package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.test.TestPlugin;
import dev.ferrite.api.test.TestServer;
import dev.ferrite.api.world.FerriteWorld;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;
import org.bukkit.GameMode;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.OfflinePlayer;
import org.bukkit.Statistic;
import org.bukkit.block.Block;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.block.BlockBreakEvent;
import org.bukkit.event.inventory.InventoryClickEvent;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.ItemStack;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.plugin.ServicePriority;
import org.bukkit.util.RayTraceResult;
import org.bukkit.util.Vector;
import org.junit.jupiter.api.Test;

/**
 * End-to-end compatibility harness exercising API patterns used by major Paper/Spigot plugins
 * (Vault, LuckPerms, EssentialsX, WorldGuard, DeluxeMenus, WorldEdit).
 */
class RealWorldPluginCompatTest {

    public interface EconomyService {
        double balance(UUID player);
    }

    @Test
    void majorPluginPatternsWorkTogetherCleanly() throws Exception {
        try (TestServer server = new TestServer()) {
            TestPlugin plugin = server.attachPlugin("SuiteCompat");
            server.enable(plugin);
            FerriteWorld world = server.runtime().world("world");
            FerritePlayer player = server.onMain(() -> server.runtime().createPlayer(
                    "Steve", UUID.randomUUID(), true, GameMode.SURVIVAL, world, 0.5, 65.0, 0.5));
            server.onMain(() -> server.runtime().playerJoined(player));

            // 1. Vault-style ServicesManager provider registration & lookup.
            EconomyService eco = id -> 1250.5;
            server.runtime().server().getServicesManager().register(
                    EconomyService.class, eco, plugin, ServicePriority.Normal);
            var reg = server.runtime().server().getServicesManager().getRegistration(EconomyService.class);
            assertNotNull(reg);
            assertEquals(1250.5, reg.getProvider().balance(player.getUniqueId()), 1e-6);

            // 2. WorldGuard-style synchronous BlockBreakEvent cancellation + DeluxeMenus GUI click cancellation.
            AtomicBoolean breakCancelled = new AtomicBoolean(false);
            AtomicInteger guiClicks = new AtomicInteger(0);
            server.onMain(() -> server.runtime().plugins().registerEvents(new Listener() {
                @EventHandler
                public void onBreak(BlockBreakEvent event) {
                    event.setCancelled(true);
                    breakCancelled.set(true);
                }

                @EventHandler
                public void onClick(InventoryClickEvent event) {
                    event.setCancelled(true);
                    guiClicks.incrementAndGet();
                }
            }, plugin));

            Block target = world.getBlockAt(2, 65, 2);
            target.setType(Material.DIAMOND_BLOCK);
            BlockBreakEvent breakEv = new BlockBreakEvent(target, player);
            server.onMain(breakEv::callEvent);
            assertTrue(breakEv.isCancelled());
            assertTrue(breakCancelled.get());

            // 3. World highest block, rayTraceBlocks, and breakNaturally.
            assertEquals(65, world.getHighestBlockYAt(2, 2));
            RayTraceResult hit = world.rayTraceBlocks(
                    new Location(world, 2.5, 68.0, 2.5), new Vector(0, -1, 0), 10.0);
            assertNotNull(hit);
            assertNotNull(hit.getHitBlock());
            assertEquals(Material.DIAMOND_BLOCK, hit.getHitBlock().getType());
            assertTrue(target.breakNaturally());
            assertTrue(target.getType().isAir());

            // 4. DeluxeMenus custom chest GUI & workbench opening.
            Inventory menu = server.runtime().server().createInventory(player, 27, "Shop");
            server.onMain(() -> menu.setItem(13, new ItemStack(Material.EMERALD, 1)));
            var view = server.onMain(() -> player.openInventory(menu));
            assertNotNull(view);
            assertNotNull(server.onMain(() -> player.openWorkbench(null, true)));

            // 5. OfflinePlayer op/whitelist/statistics/PDC + Server admin collections.
            OfflinePlayer offline = server.runtime().server().getOfflinePlayer("AlexOffline");
            offline.setOp(true);
            offline.setWhitelisted(true);
            offline.incrementStatistic(Statistic.MINE_BLOCK, Material.STONE, 42);
            assertEquals(42, offline.getStatistic(Statistic.MINE_BLOCK, Material.STONE));
            assertFalse(server.runtime().server().getOperators().isEmpty());
            assertFalse(server.runtime().server().getWhitelistedPlayers().isEmpty());
            server.runtime().server().setWhitelist(true);
            assertTrue(server.runtime().server().hasWhitelist());

            // 6. Chunk PDC & force-loaded tickets.
            var chunk = world.getChunkAt(1, 1);
            chunk.setForceLoaded(true);
            assertTrue(world.isChunkForceLoaded(1, 1));
            NamespacedKey key = new NamespacedKey(plugin, "owner");
            chunk.getPersistentDataContainer().set(key, PersistentDataType.STRING, "Steve");
            assertEquals("Steve", chunk.getPersistentDataContainer().get(key, PersistentDataType.STRING));

            // 7. Player address & world time/weather/broadcast.
            assertNotNull(player.getAddress());
            world.setTime(1000L);
            assertEquals(1000L, world.getFullTime());
            assertEquals(1, server.runtime().server().broadcastMessage("Hello Ferrite!"));
        }
    }
}
