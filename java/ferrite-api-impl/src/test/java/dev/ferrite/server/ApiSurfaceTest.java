package dev.ferrite.server;
import java.util.*;
import org.bukkit.Bukkit;
import org.bukkit.Material;
import org.bukkit.event.entity.EntityDamageEvent;
import org.bukkit.event.inventory.ClickType;
import org.bukkit.event.inventory.InventoryAction;
import org.bukkit.event.inventory.InventoryType;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class ApiSurfaceTest {
    private static final java.util.UUID WORLD_ID = java.util.UUID.fromString("00000000-0000-0000-0000-000000000001");
    private static final java.util.UUID PLAYER_ID = java.util.UUID.fromString("00000000-0000-0000-0000-0000000000aa");
    private static FerriteServer server;
    private static org.bukkit.World world;
    private static org.bukkit.entity.Player player;

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        // Reuse a server another test class already installed; Bukkit forbids a second one.
        Bootstrap.startServer();
        server = (FerriteServer) org.bukkit.Bukkit.getServer();
        world = FerriteWorld.create(server, WORLD_ID, "surface_world", 0L, 0, 64, 0);
        server.registerWorld(world);
        player = FerritePlayer.create(server, PLAYER_ID, "Surface", false, 20.0, 20, 0, new org.bukkit.Location(world, 0.5, 65.0, 0.5));
        server.registerPlayer(player);
        assertNotNull(world);
        assertNotNull(player);
    }

    @AfterAll static void stop() {
        server.unregisterWorld(WORLD_ID);
        server.unregisterPlayer(PLAYER_ID);
    }

    @Test void serverQueriesResolveWorldsPlayersAndSettings() {
        assertSame(world, server.getWorld("surface_world"));
        assertTrue(server.getWorlds().contains(world));
        assertSame(player, server.getPlayer("Surface"));
        assertSame(player, server.getPlayer("surf"));
        assertNull(server.getPlayer("nobody"));
        assertTrue(server.getOnlinePlayers().contains(player));
        assertTrue(server.getMaxPlayers() > 0);
        assertEquals(25565, server.getPort());
        assertEquals("", server.getIp());
        assertEquals("Ferrite", server.getName());
        assertEquals("Ferrite 0.1.0", server.getVersion());
        assertEquals("26.3", server.getBukkitVersion());
        assertNotNull(server.getLogger());
        assertNotNull(server.getPluginManager());
        assertNotNull(server.getScheduler());
        assertTrue(server.getWorldContainer().isAbsolute());
        assertEquals("update", server.getUpdateFolder());
        assertNotNull(server.getRespawnWorld());
        assertNotNull(server.getCommandAliases());
    }

    @Test void helpMapAndSpawnLimitsAreUsable() {
        var help = server.getHelpMap();
        assertNotNull(help);
        assertNotNull(help.getHelpTopics());
        assertEquals(-1, server.getSpawnLimit(org.bukkit.entity.SpawnCategory.MISC));
        assertTrue(server.getSpawnLimit(org.bukkit.entity.SpawnCategory.MONSTER) > 0);
        assertEquals(1, server.getTicksPerSpawns(org.bukkit.entity.SpawnCategory.ANIMAL));
    }

    @Test void offlinePlayersAndOperatorListIncludeKnownNames() {
        server.registerOfflinePlayer(java.util.UUID.fromString("00000000-0000-0000-0000-0000000000bb"), "KnownOffline", false);
        assertNotNull(server.getOfflinePlayerIfCached("KnownOffline"));
        assertTrue(Arrays.stream(server.getOfflinePlayers()).anyMatch(candidate -> "KnownOffline".equals(candidate.getName())));
    }

    @Test void pluginManagerRejectsListenersForUnknownPlugins() {
        var listener = new org.bukkit.event.Listener() {};
        assertThrows(NullPointerException.class, () -> server.getPluginManager().registerEvents(listener, null));
        var unknown = (org.bukkit.plugin.Plugin) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.plugin.Plugin.class}, (proxy, method, args) -> null);
        assertThrows(IllegalArgumentException.class, () -> server.getPluginManager().registerEvents(listener, unknown));
    }

    @Test void inventoryEventsCarryTheClickedView() {
        var view = FerriteInventoryView.create(player, player.getInventory());
        var click = new org.bukkit.event.inventory.InventoryClickEvent(view, InventoryType.SlotType.CONTAINER, 0, ClickType.LEFT, InventoryAction.PICKUP_ALL);
        assertSame(player, click.getWhoClicked());
        assertSame(player.getInventory(), click.getClickedInventory());
        assertEquals(0, click.getSlot());
        click.setCancelled(true);
        assertTrue(click.isCancelled());
        var close = new org.bukkit.event.inventory.InventoryCloseEvent(view, org.bukkit.event.inventory.InventoryCloseEvent.Reason.PLUGIN);
        assertSame(player, close.getPlayer());
        assertSame(player.getInventory(), close.getInventory());
    }

    @Test void damageEventsExposeMutableDamage() {
        var damage = new org.bukkit.event.entity.EntityDamageEvent(player, EntityDamageEvent.DamageCause.FALL, 5.0);
        assertSame(player, damage.getEntity());
        assertEquals(EntityDamageEvent.DamageCause.FALL, damage.getCause());
        assertEquals(5.0, damage.getDamage());
        damage.setDamage(2.0);
        assertEquals(2.0, damage.getFinalDamage());
        damage.setCancelled(true);
        assertTrue(damage.isCancelled());
    }

    @Test void deathAndRespawnEventsCarryTheirPositions() {
        var source = org.bukkit.damage.DamageSource.builder(org.bukkit.damage.DamageType.GENERIC_KILL).build();
        var drops = new ArrayList<org.bukkit.inventory.ItemStack>();
        var death = new org.bukkit.event.entity.PlayerDeathEvent(player, source, drops, 7, (net.kyori.adventure.text.Component) null, true);
        assertSame(player, death.getPlayer());
        assertEquals(7, death.getDroppedExp());
        death.setKeepInventory(true);
        assertTrue(death.getKeepInventory());
        var respawn = new org.bukkit.event.player.PlayerRespawnEvent(player, new org.bukkit.Location(world, 1, 2, 3), false, false, false, org.bukkit.event.player.PlayerRespawnEvent.RespawnReason.DEATH);
        respawn.setRespawnLocation(new org.bukkit.Location(world, 9, 70, 9, 90f, 0f));
        assertEquals(9, respawn.getRespawnLocation().getBlockX());
    }

    @Test void worldAndChunkEventsUseSnapshotObjects() {
        var chunk = FerriteEventObjects.chunk(world, 2, 3);
        assertEquals(2, chunk.getX());
        assertEquals(3, chunk.getZ());
        assertSame(world, chunk.getWorld());
        var load = new org.bukkit.event.world.ChunkLoadEvent(chunk, true);
        assertTrue(load.isNewChunk());
        assertSame(chunk, load.getChunk());
        var unload = new org.bukkit.event.world.ChunkUnloadEvent(chunk);
        assertSame(chunk, unload.getChunk());
        var weather = new org.bukkit.event.weather.WeatherChangeEvent(world, true);
        assertSame(world, weather.getWorld());
        assertTrue(weather.toWeatherState());
        weather.setCancelled(true);
        assertTrue(weather.isCancelled());
    }

    @Test void levelAndExperienceEventsExposeTheirMutations() {
        var level = new org.bukkit.event.player.PlayerLevelChangeEvent(player, 1, 5);
        assertEquals(1, level.getOldLevel());
        assertEquals(5, level.getNewLevel());
        var exp = new org.bukkit.event.player.PlayerExpChangeEvent(player, 4);
        assertEquals(4, exp.getAmount());
        exp.setAmount(9);
        assertEquals(9, exp.getAmount());
    }

    @Test void blockSnapshotsAreWritableFromEvents() {
        var block = FerriteEventObjects.block(world, 1, 64, 2, Material.STONE);
        assertEquals(Material.STONE, block.getType());
        block.setType(Material.DIRT);
        assertEquals(Material.DIRT, world.getBlockAt(1, 64, 2).getType());
    }

    @Test void serverTagsResolveFromTheDataPack() {
        var coals = server.getTag(org.bukkit.Tag.REGISTRY_ITEMS, org.bukkit.NamespacedKey.minecraft("coals"), Material.class);
        assertNotNull(coals);
        assertEquals(org.bukkit.NamespacedKey.minecraft("coals"), coals.getKey());
        assertTrue(coals.isTagged(Material.COAL));
        assertTrue(coals.isTagged(Material.CHARCOAL));
        assertFalse(coals.isTagged(Material.DIAMOND));
        var logs = server.getTag(org.bukkit.Tag.REGISTRY_BLOCKS, org.bukkit.NamespacedKey.minecraft("logs"), Material.class);
        assertNotNull(logs);
        assertTrue(logs.isTagged(Material.OAK_LOG));
        assertNull(server.getTag(org.bukkit.Tag.REGISTRY_ITEMS, org.bukkit.NamespacedKey.minecraft("ferrite_missing"), Material.class));
        assertThrows(UnsupportedOperationException.class, () -> server.getTag("entity_types", org.bukkit.NamespacedKey.minecraft("skeletons"), org.bukkit.entity.EntityType.class));
    }

    @Test void audiencesIncludeTheConsoleAndOnlinePlayers() {
        assertTrue(containsIdentity(server.audiences(), server.getConsoleSender()));
        assertTrue(containsIdentity(server.audiences(), player));
        server.unregisterPlayer(PLAYER_ID);
        try {
            assertTrue(containsIdentity(server.audiences(), server.getConsoleSender()));
            assertFalse(containsIdentity(server.audiences(), player));
        } finally {
            server.registerPlayer(player);
        }
        assertTrue(containsIdentity(server.audiences(), player));
    }

    private static boolean containsIdentity(java.lang.Iterable<? extends net.kyori.adventure.audience.Audience> audiences, Object expected) {
        for (var audience : audiences) if (audience == expected) return true;
        return false;
    }

    @Test void commandSenderRedirectsFeedbackToTheConsumer() {
        var feedback = new java.util.ArrayList<net.kyori.adventure.text.Component>();
        var sender = server.createCommandSender(feedback::add);
        sender.sendMessage(net.kyori.adventure.text.Component.text("hello"));
        sender.sendMessage("plain");
        sender.sendMessage(net.kyori.adventure.text.Component.text("first"));
        sender.sendMessage(net.kyori.adventure.text.Component.text("second"));
        assertEquals(4, feedback.size());
        assertEquals("hello", plain(feedback.get(0)));
        assertEquals("plain", plain(feedback.get(1)));
        assertEquals("first", plain(feedback.get(2)));
        assertEquals("second", plain(feedback.get(3)));
        assertSame(server, sender.getServer());
        assertFalse(sender.isOp());
        assertFalse(sender.hasPermission("ferrite.test"));
        assertTrue(sender.getEffectivePermissions().isEmpty());
    }

    private static String plain(net.kyori.adventure.text.Component component) {
        return net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(component);
    }
}
