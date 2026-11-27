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

            // 8. Folia-style GlobalRegionScheduler / RegionScheduler / EntityScheduler & BossBar.
            AtomicBoolean globalRan = new AtomicBoolean(false);
            AtomicBoolean entityRan = new AtomicBoolean(false);
            server.runtime().server().getGlobalRegionScheduler().execute(plugin, () -> globalRan.set(true));
            player.getScheduler().execute(plugin, () -> entityRan.set(true), null, 0L);
            server.tick(1);
            assertTrue(globalRan.get());
            assertTrue(entityRan.get());

            var bar = server.runtime().server().createBossBar(
                    "Wither Boss", org.bukkit.boss.BarColor.PURPLE, org.bukkit.boss.BarStyle.SOLID);
            bar.setProgress(0.75);
            bar.addPlayer(player);
            assertEquals(0.75, bar.getProgress(), 1e-6);
            assertTrue(bar.getPlayers().contains(player));
            bar.removeAll();

            // 9. Folia RegionScheduler delayed/repeating/cancel + Player metadata/tags/attributes/respawn.
            AtomicInteger regionTicks = new AtomicInteger(0);
            var repeating = server.runtime().server().getRegionScheduler().runAtFixedRate(
                    plugin, world, 0, 0, task -> {
                        if (regionTicks.incrementAndGet() >= 2) {
                            task.cancel();
                        }
                    }, 1L, 1L);
            server.tick(4);
            assertEquals(2, regionTicks.get());
            assertTrue(repeating.isCancelled());

            player.setMetadata("vanished", new org.bukkit.metadata.FixedMetadataValue(plugin, true));
            assertTrue(player.hasMetadata("vanished"));
            assertTrue(player.getMetadata("vanished").get(0).asBoolean());
            player.removeMetadata("vanished", plugin);
            assertFalse(player.hasMetadata("vanished"));

            assertTrue(player.addScoreboardTag("vip"));
            assertTrue(player.getScoreboardTags().contains("vip"));
            assertTrue(player.removeScoreboardTag("vip"));

            var maxHp = player.getAttribute(org.bukkit.attribute.Attribute.MAX_HEALTH);
            assertNotNull(maxHp);
            assertEquals(20.0, maxHp.getValue(), 1e-6);
            maxHp.addModifier(new org.bukkit.attribute.AttributeModifier(
                    new NamespacedKey(plugin, "bonus_hp"),
                    10.0,
                    org.bukkit.attribute.AttributeModifier.Operation.ADD_NUMBER));
            assertEquals(30.0, maxHp.getValue(), 1e-6);

            Location bed = new Location(world, 10.0, 65.0, -4.0);
            player.setRespawnLocation(bed, true);
            assertEquals(10.0, player.getRespawnLocation().getX(), 1e-6);
            assertEquals("vanilla", player.getClientBrandName());
        }
    }

    @Test
    void multiPluginEcosystemInteractionWorksEndToEnd() throws Exception {
        try (TestServer server = new TestServer()) {
            // Simulate 5 collaborating plugins: LuckPerms, Vault, EssentialsX, WorldGuard, DeluxeMenus.
            TestPlugin luckPerms = server.attachPlugin("LuckPerms");
            TestPlugin vault = server.attachPlugin("Vault");
            TestPlugin essentials = server.attachPlugin("EssentialsX");
            TestPlugin worldGuard = server.attachPlugin("WorldGuard");
            TestPlugin deluxeMenus = server.attachPlugin("DeluxeMenus");
            server.enable(luckPerms);
            server.enable(vault);
            server.enable(essentials);
            server.enable(worldGuard);
            server.enable(deluxeMenus);

            FerriteWorld world = server.runtime().world("world");
            FerritePlayer alice = server.onMain(() -> server.runtime().createPlayer(
                    "Alice", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 0.5, 65.0, 0.5));
            FerritePlayer bob = server.onMain(() -> server.runtime().createPlayer(
                    "Bob", UUID.randomUUID(), false, GameMode.SURVIVAL, world, 4.5, 65.0, 4.5));
            server.onMain(() -> {
                server.runtime().playerJoined(alice);
                server.runtime().playerJoined(bob);
            });

            // 1. LuckPerms grants dynamic permission attachment to Alice.
            var attachment = alice.addAttachment(luckPerms);
            attachment.setPermission("essentials.pay", true);
            assertTrue(alice.hasPermission("essentials.pay"));
            assertFalse(bob.hasPermission("essentials.pay"));

            // 2. Vault registers shared EconomyService used by EssentialsX and DeluxeMenus.
            var balances = new java.util.HashMap<UUID, Double>();
            balances.put(alice.getUniqueId(), 500.0);
            balances.put(bob.getUniqueId(), 100.0);
            EconomyService eco = id -> balances.getOrDefault(id, 0.0);
            server.runtime().server().getServicesManager().register(
                    EconomyService.class, eco, vault, ServicePriority.High);
            var resolvedEco = server.runtime().server().getServicesManager()
                    .getRegistration(EconomyService.class);
            assertNotNull(resolvedEco);
            assertEquals(500.0, resolvedEco.getProvider().balance(alice.getUniqueId()), 1e-6);

            // 3. EssentialsX awards XP (vanilla level progression), plays world sound, and spawns particles.
            alice.giveExp(55, false); // Level 0->1 (7), 1->2 (9), 2->3 (11), 3->4 (13), 4->5 (15) = 55 XP -> level 5!
            assertEquals(5, alice.getLevel());
            assertEquals(55, alice.getTotalExperience());
            world.playSound(
                    alice.getLocation(),
                    "minecraft:entity.player.levelup",
                    org.bukkit.SoundCategory.PLAYERS,
                    1.0f,
                    1.0f);
            world.spawnParticle(
                    org.bukkit.Particle.HAPPY_VILLAGER,
                    alice.getLocation(),
                    8,
                    0.5,
                    0.5,
                    0.5,
                    0.0);
            assertFalse(alice.state().playedSounds().isEmpty());
            assertFalse(bob.state().playedSounds().isEmpty());
            assertFalse(alice.state().spawnedParticles().isEmpty());

            // 4. WorldGuard toggles PvP, Difficulty, and Server MOTD/GameMode/Tick tracking.
            world.setPVP(false);
            assertFalse(world.getPVP());
            world.setDifficulty(org.bukkit.Difficulty.HARD);
            assertEquals(org.bukkit.Difficulty.HARD, world.getDifficulty());
            var srv = server.runtime().server();
            srv.motd(net.kyori.adventure.text.Component.text("Ferrite Survival 26.3"));
            assertTrue(srv.getMotd().contains("Ferrite Survival 26.3"));
            srv.setDefaultGameMode(GameMode.ADVENTURE);
            assertEquals(GameMode.ADVENTURE, srv.getDefaultGameMode());
            srv.setMaxPlayers(250);
            assertEquals(250, srv.getMaxPlayers());
            assertTrue(srv.getAllowNether());
            assertTrue(srv.getAllowEnd());
            int beforeTick = srv.getCurrentTick();
            server.tick(3);
            assertEquals(beforeTick + 3, srv.getCurrentTick());

            // 5. DeluxeMenus builds a rich GUI item with ItemMeta (unbreakable, flags, enchants, PDC).
            Inventory shop = srv.createInventory(alice, 27, "VIP Shop");
            ItemStack sword = new ItemStack(Material.DIAMOND_SWORD, 1);
            var meta = sword.getItemMeta();
            assertNotNull(meta);
            meta.setUnbreakable(true);
            meta.addItemFlags(org.bukkit.inventory.ItemFlag.HIDE_UNBREAKABLE);
            meta.getPersistentDataContainer().set(
                    new NamespacedKey(deluxeMenus, "price"),
                    PersistentDataType.INTEGER,
                    250);
            sword.setItemMeta(meta);
            server.onMain(() -> shop.setItem(13, sword));
            ItemStack stored = shop.getItem(13);
            assertNotNull(stored);
            assertTrue(stored.getItemMeta().isUnbreakable());
            assertTrue(stored.getItemMeta().hasItemFlag(org.bukkit.inventory.ItemFlag.HIDE_UNBREAKABLE));
            assertEquals(
                    250,
                    stored.getItemMeta()
                            .getPersistentDataContainer()
                            .get(new NamespacedKey(deluxeMenus, "price"), PersistentDataType.INTEGER));

            // 6. Extended Block, EnderChest, workstation menus, and Server ban/config APIs.
            var baseBlock = world.getBlockAt(10, 64, 10);
            baseBlock.setType(Material.REDSTONE_BLOCK, false);
            var above = baseBlock.getRelative(org.bukkit.block.BlockFace.UP);
            above.setType(Material.TORCH);
            assertEquals(org.bukkit.block.BlockFace.UP, baseBlock.getFace(above));
            assertTrue(baseBlock.isBlockPowered());
            assertTrue(above.isBlockIndirectlyPowered());
            assertEquals(15, above.getLightFromBlocks());
            assertTrue(baseBlock.isSolid());
            assertTrue(above.isPassable());
            assertEquals(1, baseBlock.getDrops().size());
            assertNotNull(baseBlock.getChunk());

            server.onMain(() -> {
                alice.getEnderChest().setItem(0, new ItemStack(Material.ENDER_PEARL, 16));
                assertEquals(16, alice.getEnderChest().getItem(0).getAmount());
                assertNotNull(alice.openAnvil(null, true));
                assertNotNull(alice.openLoom(null, true));
                assertNotNull(alice.openGrindstone(null, true));
                assertNotNull(alice.openCartographyTable(null, true));
                assertNotNull(alice.openSmithingTable(null, true));
                assertNotNull(alice.openStonecutter(null, true));
            });

            srv.banIP("192.0.2.42");
            assertTrue(srv.getIPBans().contains("192.0.2.42"));
            srv.unbanIP("192.0.2.42");
            assertFalse(srv.getIPBans().contains("192.0.2.42"));
            srv.setSpawnRadius(8);
            assertEquals(8, srv.getSpawnRadius());
            srv.setIdleTimeout(15);
            assertEquals(15, srv.getIdleTimeout());
            assertTrue(srv.reloadCommandAliases());

            // 7. Bukkit/Paper lifecycle & gameplay event dispatch + cancellation (Teleport, Food, Exp, Damage, Death, Weather, CreatureSpawn).
            java.util.List<String> fired = new java.util.ArrayList<>();
            java.util.concurrent.atomic.AtomicBoolean cancelTeleport = new java.util.concurrent.atomic.AtomicBoolean(true);
            java.util.concurrent.atomic.AtomicBoolean cancelWeather = new java.util.concurrent.atomic.AtomicBoolean(true);
            java.util.concurrent.atomic.AtomicBoolean cancelSpawn = new java.util.concurrent.atomic.AtomicBoolean(true);
            srv.getPluginManager().registerEvents(new org.bukkit.event.Listener() {
                @org.bukkit.event.EventHandler
                public void onTeleport(org.bukkit.event.player.PlayerTeleportEvent e) {
                    fired.add("teleport");
                    if (cancelTeleport.get()) {
                        e.setCancelled(true);
                    }
                }

                @org.bukkit.event.EventHandler
                public void onFood(org.bukkit.event.entity.FoodLevelChangeEvent e) {
                    fired.add("food:" + e.getFoodLevel());
                    if (e.getFoodLevel() == 5) {
                        e.setFoodLevel(12);
                    }
                }

                @org.bukkit.event.EventHandler
                public void onExp(org.bukkit.event.player.PlayerExpChangeEvent e) {
                    fired.add("exp:" + e.getAmount());
                    e.setAmount(e.getAmount() * 2);
                }

                @org.bukkit.event.EventHandler
                public void onDamage(org.bukkit.event.entity.EntityDamageEvent e) {
                    fired.add("damage:" + (int) e.getDamage());
                }

                @org.bukkit.event.EventHandler
                public void onDeath(org.bukkit.event.entity.PlayerDeathEvent e) {
                    fired.add("death");
                }

                @org.bukkit.event.EventHandler
                public void onWeather(org.bukkit.event.weather.WeatherChangeEvent e) {
                    fired.add("weather:" + e.toWeatherState());
                    if (cancelWeather.get()) {
                        e.setCancelled(true);
                    }
                }

                @org.bukkit.event.EventHandler
                public void onSpawn(org.bukkit.event.entity.CreatureSpawnEvent e) {
                    fired.add("spawn:" + e.getEntityType().name());
                    if (cancelSpawn.get()) {
                        e.setCancelled(true);
                    }
                }
            }, essentials);

            server.onMain(() -> {
                assertFalse(alice.teleport(new org.bukkit.Location(world, 99, 70, 99)));
                cancelTeleport.set(false);
                assertTrue(alice.teleport(new org.bukkit.Location(world, 99, 70, 99)));
                assertEquals(99.0, alice.getLocation().getX(), 1e-6);

                alice.setFoodLevel(5);
                assertEquals(12, alice.getFoodLevel());

                int expBefore = alice.getTotalExperience();
                alice.giveExp(10);
                assertEquals(expBefore + 20, alice.getTotalExperience());

                alice.setHealth(10.0);
                alice.damage(10.0);
                assertEquals(0.0, alice.getHealth(), 1e-6);

                world.setStorm(true);
                assertFalse(world.hasStorm());
                cancelWeather.set(false);
                world.setStorm(true);
                assertTrue(world.hasStorm());

                int beforeEntities = world.getEntities().size();
                world.spawnEntity(new org.bukkit.Location(world, 1, 65, 1), org.bukkit.entity.EntityType.ZOMBIE);
                assertEquals(beforeEntities, world.getEntities().size());
                cancelSpawn.set(false);
                world.spawnEntity(new org.bukkit.Location(world, 1, 65, 1), org.bukkit.entity.EntityType.ZOMBIE);
                assertEquals(beforeEntities + 1, world.getEntities().size());
            });
            assertTrue(fired.contains("teleport"));
            assertTrue(fired.contains("food:5"));
            assertTrue(fired.contains("exp:10"));
            assertTrue(fired.contains("damage:10"));
            assertTrue(fired.contains("death"));
            assertTrue(fired.contains("weather:true"));
            assertTrue(fired.contains("spawn:ZOMBIE"));
        }
    }
}
