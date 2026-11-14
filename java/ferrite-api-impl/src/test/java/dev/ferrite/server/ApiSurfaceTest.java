package dev.ferrite.server;
import java.util.*;
import org.bukkit.Bukkit;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
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

    @Test void redirectedCommandSenderUsesConsolePermissionsAndDeliversFeedback() {
        var messages = new java.util.ArrayList<net.kyori.adventure.text.Component>();
        var sender = server.createCommandSender(messages::add);
        var console = server.getConsoleSender();
        var permission = new org.bukkit.permissions.Permission("ferrite.test");
        assertEquals(console.isOp(), sender.isOp());
        assertEquals(console.hasPermission("ferrite.test"), sender.hasPermission("ferrite.test"));
        assertEquals(console.hasPermission(permission), sender.hasPermission(permission));
        assertSame(server, sender.getServer());
        sender.sendMessage("plain");
        sender.sendMessage(net.kyori.adventure.text.Component.text("component"));
        sender.sendMessage(PLAYER_ID, "identified");
        sender.sendMessage("\u00a7agreen");
        var chat = net.kyori.adventure.chat.ChatType.CHAT.bind(net.kyori.adventure.text.Component.text("sender"));
        sender.sendMessage(net.kyori.adventure.text.Component.text("bound"), chat);
        sender.sendMessage(net.kyori.adventure.chat.SignedMessage.system("signed", null), chat);
        sender.sendMessage(new net.md_5.bungee.api.chat.BaseComponent[]{new net.md_5.bungee.api.chat.TextComponent("old"), new net.md_5.bungee.api.chat.TextComponent(" api")});
        assertEquals(java.util.List.of(
            net.kyori.adventure.text.Component.text("plain"),
            net.kyori.adventure.text.Component.text("component"),
            net.kyori.adventure.text.Component.text("identified"),
            net.kyori.adventure.text.Component.text("green", net.kyori.adventure.text.format.NamedTextColor.GREEN),
            net.kyori.adventure.text.Component.text("bound"),
            net.kyori.adventure.text.Component.text("signed")
        ), messages.subList(0, 6));
        assertEquals(7, messages.size());
        assertEquals("old api", net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(messages.get(6)));
        assertThrows(NullPointerException.class, () -> server.createCommandSender(null));
    }

    @Test void legacyPermissionMessageMatchesAdventureMessage() {
        var expected = net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer.legacySection().serialize(server.permissionMessage());
        assertEquals(expected, server.getPermissionMessage());
    }

    @Test void serverIconsEncodeValidatedImagesFromMemoryAndFiles() throws Exception {
        var image = new java.awt.image.BufferedImage(64, 64, java.awt.image.BufferedImage.TYPE_INT_ARGB);
        image.setRGB(2, 3, 0xff32a053);
        var icon = server.loadServerIcon(image);
        assertTrue(icon.getData().startsWith("data:image/png;base64,"));
        var bytes = java.util.Base64.getDecoder().decode(icon.getData().substring("data:image/png;base64,".length()));
        var decoded = javax.imageio.ImageIO.read(new java.io.ByteArrayInputStream(bytes));
        assertEquals(image.getRGB(2, 3), decoded.getRGB(2, 3));
        var file = java.nio.file.Files.createTempFile("ferrite-icon-", ".png");
        try {
            javax.imageio.ImageIO.write(image, "png", file.toFile());
            assertEquals(icon.getData(), server.loadServerIcon(file.toFile()).getData());
            java.nio.file.Files.write(file, new byte[]{1, 2, 3});
            assertThrows(IllegalArgumentException.class, () -> server.loadServerIcon(file.toFile()));
        } finally { java.nio.file.Files.deleteIfExists(file); }
        assertThrows(IllegalArgumentException.class, () -> server.loadServerIcon((java.io.File) null));
        assertThrows(IllegalArgumentException.class, () -> server.loadServerIcon((java.awt.image.BufferedImage) null));
        assertThrows(IllegalArgumentException.class, () -> server.loadServerIcon(new java.awt.image.BufferedImage(32, 64, java.awt.image.BufferedImage.TYPE_INT_ARGB)));
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

    @Test void inventoryViewsReportTheVanillaMenuType() {
        var chest = FerriteInventoryView.create(player, server.createInventory(null, 54));
        assertEquals(NamespacedKey.minecraft("generic_9x6"), chest.getMenuType().getKey());
        assertEquals(NamespacedKey.minecraft("generic_9x3"),
            FerriteInventoryView.create(player, server.createInventory(null, 27)).getMenuType().getKey());
        var shulker = server.createInventory(null, InventoryType.SHULKER_BOX);
        assertEquals(27, shulker.getSize());
        var view = FerriteInventoryView.create(player, shulker);
        assertEquals(NamespacedKey.minecraft("shulker_box"), view.getMenuType().getKey());
        assertEquals(InventoryType.SHULKER_BOX, view.getType());
        assertEquals(NamespacedKey.minecraft("cartography_table"),
            FerriteInventoryView.create(player, server.createInventory(null, InventoryType.CARTOGRAPHY)).getMenuType().getKey());
        var unmapped = FerriteInventoryView.create(player, server.createInventory(null, InventoryType.HOPPER));
        var error = assertThrows(UnsupportedOperationException.class, unmapped::getMenuType);
        assertTrue(error.getMessage().contains("HOPPER"), error.getMessage());
    }

    @Test void inventoryDragEventCarriesSlotsAndCursors() {
        var view = FerriteInventoryView.create(player, player.getInventory());
        var oldCursor = new org.bukkit.inventory.ItemStack(Material.STONE, 40);
        var newCursor = new org.bukkit.inventory.ItemStack(Material.AIR);
        var newItems = new java.util.HashMap<Integer, org.bukkit.inventory.ItemStack>();
        newItems.put(9, new org.bukkit.inventory.ItemStack(Material.STONE, 20));
        newItems.put(10, new org.bukkit.inventory.ItemStack(Material.STONE, 20));
        var drag = new org.bukkit.event.inventory.InventoryDragEvent(view, newCursor, oldCursor, false, newItems);
        assertEquals(java.util.Set.of(9, 10), drag.getRawSlots());
        assertSame(player, drag.getWhoClicked());
        assertSame(player.getInventory(), drag.getInventory());
        assertEquals(Material.STONE, drag.getOldCursor().getType());
        assertEquals(40, drag.getOldCursor().getAmount());
        assertEquals(Material.AIR, drag.getCursor().getType());
        assertEquals(20, drag.getNewItems().get(9).getAmount());
        assertEquals(20, drag.getNewItems().get(10).getAmount());
        drag.setCancelled(true);
        assertTrue(drag.isCancelled());
    }

    @Test void helpMapBuildsTopicsFromCommandsAndFactories() {
        var help = server.getHelpMap();
        var probe = new org.bukkit.command.Command("ferrite-help-probe") {
            @Override public boolean execute(org.bukkit.command.CommandSender sender, String label, String[] args) { return true; }
        };
        server.getCommandMap().register("ferrite", probe);
        var topic = help.getHelpTopic("ferrite-help-probe");
        assertNotNull(topic);
        assertEquals("/ferrite-help-probe", topic.getName());
        assertTrue(help.getHelpTopics().stream().anyMatch(candidate -> candidate.getName().equals("/ferrite-help-probe")));
        var custom = new org.bukkit.help.HelpTopic() {
            @Override public String getName() { return "/ferrite-help-probe"; }
            @Override public String getShortText() { return "probe"; }
            @Override public String getFullText(org.bukkit.command.CommandSender sender) { return "probe full text"; }
            @Override public boolean canSee(org.bukkit.command.CommandSender sender) { return true; }
        };
        help.registerHelpTopicFactory(org.bukkit.command.Command.class, new org.bukkit.help.HelpTopicFactory<org.bukkit.command.Command>() {
            @Override public org.bukkit.help.HelpTopic createTopic(org.bukkit.command.Command command) { return custom; }
        });
        assertSame(custom, help.getHelpTopic("ferrite-help-probe"));
    }

    @Test void helpYmlIgnoresPluginsAmendsTopicsAndBuildsIndexes() throws Exception {
        var file = java.nio.file.Files.createTempFile("help", ".yml").toFile();
        java.nio.file.Files.writeString(file.toPath(), String.join("\n",
            "ignore-plugins:", "  - HiddenPlugin",
            "amended-topics:", "  /ferrite-amend-probe:", "    shortText: Short now", "    fullText: Full now",
            "index-topics:", "  Probes:", "    shortText: All probes", "    preamble: Pick one", "    commands:", "      - ferrite-amend-probe", ""));
        var map = new FerriteHelpMap(server, file);
        var command = new org.bukkit.command.Command("ferrite-amend-probe") {
            @Override public boolean execute(org.bukkit.command.CommandSender sender, String label, String[] args) { return true; }
        };
        server.getCommandMap().register("ferrite", command);
        var topic = map.getHelpTopic("ferrite-amend-probe");
        assertEquals("Short now", topic.getShortText());
        assertEquals("Full now", topic.getFullText(server.getConsoleSender()));
        var index = map.getHelpTopic("Probes");
        assertNotNull(index);
        assertTrue(index.getFullText(server.getConsoleSender()).contains("Pick one"));
        assertTrue(map.getHelpTopics().stream().anyMatch(candidate -> candidate.getName().equals("Probes")));
        assertEquals(java.util.List.of("HiddenPlugin"), map.getIgnoredPlugins());
        assertNull(new FerriteHelpMap(server, new java.io.File("missing-help.yml")).getHelpTopic("nothing-here"));
    }

    @Test void serverSurfaceImplementsTheBoundedCallsAndDocumentsTheRest() {
        var stick = new org.bukkit.inventory.ItemStack(Material.STICK, 1);
        assertNotNull(server.getRecipesFor(stick));
        assertTrue(server.getRecipesFor(new org.bukkit.inventory.ItemStack(Material.AIR)).isEmpty());
        var map = server.createMap(world);
        assertTrue(map.getId() > 0);
        assertSame(map, server.getMap(map.getId()));
        assertNull(server.getMap(-12345));
        assertSame(world, server.createWorld(new org.bukkit.WorldCreator(world.getName())));
        var missing = assertThrows(UnsupportedOperationException.class, () -> server.createWorld(new org.bukkit.WorldCreator("ferrite_missing_world")));
        assertTrue(missing.getMessage().contains("createRuntimeWorld"), () -> "Unexpected runtime world failure: " + missing.getMessage());
        var snapshot = assertThrows(IllegalStateException.class, () -> server.getEntityFactory().createEntitySnapshot("{}"));
        assertTrue(snapshot.getMessage().contains("no host is available"));
    }

    /**
     * A stand-in for the Rust host's runtime-world calls. Like the real host it keeps the seed a world was
     * created with and answers a later creation of the same name with that stored seed, and it refuses an
     * unload on request (players inside, failed flush), which the real host reports as {@code false}.
     */
    private static final class WorldHost implements HostBridge.Transport {
        final java.util.List<String> creates = new java.util.ArrayList<>();
        final java.util.List<String> unloads = new java.util.ArrayList<>();
        final java.util.Map<String, String[]> stored = new java.util.HashMap<>();
        volatile boolean refuseUnload;
        @Override public String call(String owner, String subject, String method, String args) {
            if (!owner.equals("Server")) return null;
            var parts = args.split(String.valueOf(HostBridge.ARG), -1);
            if (method.equals("createRuntimeWorld(String,String,long)")) {
                creates.add(args);
                var entry = stored.computeIfAbsent(parts[0], name -> new String[]{java.util.UUID.nameUUIDFromBytes(name.getBytes()).toString(), parts[2]});
                return entry[0] + HostBridge.ITEM + "0" + HostBridge.ITEM + "65" + HostBridge.ITEM + "0" + HostBridge.ITEM + entry[1];
            }
            if (method.equals("unloadRuntimeWorld(String,boolean)")) {
                unloads.add(args);
                return refuseUnload ? "false" : "true";
            }
            return null;
        }
    }

    private static void withWorldHost(java.util.function.Consumer<WorldHost> body) {
        var host = new WorldHost();
        var previous = HostBridge.transport;
        HostBridge.transport = host;
        try { body.accept(host); }
        finally {
            HostBridge.transport = previous;
            for (var created : host.stored.values()) server.unregisterWorld(java.util.UUID.fromString(created[0]));
        }
    }

    private static org.bukkit.plugin.Plugin probePlugin() {
        return (org.bukkit.plugin.Plugin) java.lang.reflect.Proxy.newProxyInstance(ApiSurfaceTest.class.getClassLoader(), new Class<?>[]{org.bukkit.plugin.Plugin.class}, (proxy, method, args) -> switch (method.getName()) {
            case "getName" -> "WorldLifecycleProbe";
            case "isEnabled" -> true;
            case "hashCode" -> 17;
            case "equals" -> proxy == args[0];
            case "toString" -> "WorldLifecycleProbe";
            default -> null;
        });
    }

    @Test void createWorldWithAnUnsetSeedSucceedsAndCarriesTheSeedToTheHost() {
        withWorldHost(host -> {
            var creator = new org.bukkit.WorldCreator("ferrite_random_seed");
            var created = server.createWorld(creator);
            assertNotNull(created);
            assertSame(created, server.getWorld("ferrite_random_seed"));
            assertEquals(1, host.creates.size());
            var sent = host.creates.get(0).split(String.valueOf(HostBridge.ARG), -1);
            assertEquals(Long.toString(creator.seed()), sent[2], "the creator's own (random) seed is what the host is asked to use");
            assertEquals(creator.seed(), created.getSeed());
        });
    }

    @Test void anExplicitSeedRoundTripsThroughUnloadAndReopen() {
        withWorldHost(host -> {
            var first = server.createWorld(new org.bukkit.WorldCreator("ferrite_seeded").seed(-42_424_242L));
            assertEquals(-42_424_242L, first.getSeed());
            assertTrue(server.unloadWorld(first, true));
            assertNull(server.getWorld("ferrite_seeded"));
            // Paper keeps an existing world's seed; the host answers with the stored one, not the requested one.
            var reopened = server.createWorld(new org.bukkit.WorldCreator("ferrite_seeded").seed(7L));
            assertEquals(-42_424_242L, reopened.getSeed());
            assertEquals(first.getUID(), reopened.getUID());
        });
    }

    @Test void unsupportedCreatorOptionsAreRejectedWithExplicitMessagesAndNoHostCall() {
        withWorldHost(host -> {
            var generator = assertThrows(UnsupportedOperationException.class, () -> server.createWorld(new org.bukkit.WorldCreator("ferrite_gen").generator(new org.bukkit.generator.ChunkGenerator() {})));
            assertTrue(generator.getMessage().contains("custom chunk generators"), generator.getMessage());
            var structures = assertThrows(UnsupportedOperationException.class, () -> server.createWorld(new org.bukkit.WorldCreator("ferrite_nostruct").generateStructures(false)));
            assertTrue(structures.getMessage().contains("structure generation"), structures.getMessage());
            var type = assertThrows(UnsupportedOperationException.class, () -> server.createWorld(new org.bukkit.WorldCreator("ferrite_flat").type(org.bukkit.WorldType.FLAT)));
            assertTrue(type.getMessage().contains("FLAT") && type.getMessage().contains("NORMAL"), type.getMessage());
            var environment = assertThrows(UnsupportedOperationException.class, () -> server.createWorld(new org.bukkit.WorldCreator("ferrite_custom").environment(org.bukkit.World.Environment.CUSTOM)));
            assertTrue(environment.getMessage().contains("CUSTOM"), environment.getMessage());
            assertTrue(host.creates.isEmpty(), "a rejected creator never reaches the host");
            for (var name : java.util.List.of("ferrite_gen", "ferrite_nostruct", "ferrite_flat", "ferrite_custom")) assertNull(server.getWorld(name), name);
        });
    }

    /**
     * Installs a handler that cancels every {@link org.bukkit.event.world.WorldUnloadEvent} and returns the undo.
     * The plugin manager only accepts handlers from plugins it loaded, and loading one here would collide with
     * BootstrapTest in the same JVM, so the handler goes straight into the manager's list.
     */
    private static Runnable vetoWorldUnloads() throws ReflectiveOperationException {
        var handlersField = FerritePluginManager.class.getDeclaredField("handlers");
        handlersField.setAccessible(true);
        var handlerType = java.util.Arrays.stream(FerritePluginManager.class.getDeclaredClasses()).filter(type -> type.getSimpleName().equals("Handler")).findFirst().orElseThrow();
        var constructor = handlerType.getDeclaredConstructors()[0];
        constructor.setAccessible(true);
        java.util.function.Consumer<Object> veto = event -> ((org.bukkit.event.Cancellable) event).setCancelled(true);
        var invoker = java.lang.invoke.MethodHandles.lookup().findVirtual(java.util.function.Consumer.class, "accept", java.lang.invoke.MethodType.methodType(void.class, Object.class)).bindTo(veto);
        var handler = constructor.newInstance(org.bukkit.event.world.WorldUnloadEvent.class, org.bukkit.event.EventPriority.NORMAL, false, probePlugin(), invoker);
        @SuppressWarnings("unchecked") var handlers = (java.util.List<Object>) handlersField.get(server.plugins);
        synchronized (server.plugins) { handlers.add(handler); }
        return () -> { synchronized (server.plugins) { handlers.remove(handler); } };
    }

    @Test void aCancelledWorldUnloadEventLeavesTheWorldIntact() throws Exception {
        var undo = vetoWorldUnloads();
        try {
            withWorldHost(host -> {
                var target = server.createWorld(new org.bukkit.WorldCreator("ferrite_cancelled").seed(5L));
                assertFalse(server.unloadWorld(target, true));
                assertTrue(host.unloads.isEmpty(), "a vetoed unload never asks the host to stop the world");
                assertSame(target, server.getWorld(target.getUID()), "the world is still registered");
                assertTrue(server.getWorlds().contains(target));
                undo.run();
                assertTrue(server.unloadWorld(target, true), "without the veto the same world unloads");
                assertNull(server.getWorld(target.getUID()));
            });
        } finally { undo.run(); }
    }

    @Test void aHostRefusalSuchAsAPlayerInsideOrAFailedFlushKeepsTheWorldRegistered() {
        withWorldHost(host -> {
            var target = server.createWorld(new org.bukkit.WorldCreator("ferrite_refused").seed(5L));
            host.refuseUnload = true;
            assertFalse(server.unloadWorld(target, true), "Bukkit reports a refused unload as false, not as an exception");
            assertEquals(1, host.unloads.size());
            assertSame(target, server.getWorld(target.getUID()), "a refused unload does not unregister the world");
            host.refuseUnload = false;
            assertTrue(server.unloadWorld(target, true));
            assertNull(server.getWorld(target.getUID()));
        });
    }

    @Test void unloadingWithoutSavingTellsTheHostToSkipPersistence() {
        withWorldHost(host -> {
            var target = server.createWorld(new org.bukkit.WorldCreator("ferrite_nosave").seed(5L));
            assertTrue(server.unloadWorld(target, false));
            assertEquals(java.util.List.of(target.getUID() + String.valueOf(HostBridge.ARG) + "false"), host.unloads);
            assertNull(server.getWorld(target.getUID()), "the world is fully unregistered afterwards");
            assertNull(server.getWorld("ferrite_nosave"));
        });
    }

    /** Runs {@code body} with a fake host that answers world data queries through {@link HostBridge#transport}. */
    private static void withWorldData(HostBridge.Transport host, Runnable body) {
        var previous = HostBridge.transport;
        HostBridge.transport = host;
        try { body.run(); } finally { HostBridge.transport = previous; }
    }

    @Test void heightmapSpecificHighestBlockQueriesReachTheHost() {
        var asked = new java.util.ArrayList<String>();
        withWorldData((owner, subject, method, args) -> {
            if (!owner.equals("World") || !method.equals("getHighestBlockYAt(int,int,HeightMap)")) return null;
            asked.add(args);
            var parts = args.split(String.valueOf(HostBridge.ARG), -1);
            if (parts[0].equals("9")) return HostBridge.NULL;
            return switch (parts[2]) {
                case "WORLD_SURFACE" -> "68";
                case "MOTION_BLOCKING" -> "67";
                case "MOTION_BLOCKING_NO_LEAVES" -> "65";
                case "OCEAN_FLOOR" -> "64";
                default -> null;
            };
        }, () -> {
            assertEquals(68, world.getHighestBlockYAt(1, 1, org.bukkit.HeightMap.WORLD_SURFACE));
            assertEquals(67, world.getHighestBlockYAt(1, 1, org.bukkit.HeightMap.MOTION_BLOCKING));
            assertEquals(65, world.getHighestBlockYAt(1, 1, org.bukkit.HeightMap.MOTION_BLOCKING_NO_LEAVES));
            assertEquals(64, world.getHighestBlockYAt(new org.bukkit.Location(world, 1.5, 70, 1.5), org.bukkit.HeightMap.OCEAN_FLOOR));
            assertTrue(asked.contains("1" + HostBridge.ARG + "1" + HostBridge.ARG + "OCEAN_FLOOR"), "the Location overload sends block coordinates: " + asked);
            var missing = assertThrows(UnsupportedOperationException.class, () -> world.getHighestBlockYAt(9, 9, org.bukkit.HeightMap.MOTION_BLOCKING));
            assertTrue(missing.getMessage().contains("MOTION_BLOCKING"), missing.getMessage());
        });
    }

    @Test void blockLightLevelComesFromTheHost() {
        withWorldData((owner, subject, method, args) -> owner.equals("World") && method.equals("getLightLevel(int,int,int)")
                ? (args.startsWith("1" + HostBridge.ARG + "70") ? "13" : HostBridge.NULL) : null, () -> {
            assertEquals((byte) 13, world.getBlockAt(1, 70, 2).getLightLevel());
            var unknown = assertThrows(UnsupportedOperationException.class, () -> world.getBlockAt(5, 5, 5).getLightLevel());
            assertTrue(unknown.getMessage().contains("light"), unknown.getMessage());
        });
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
        var skeletons = server.getTag("entity_types", org.bukkit.NamespacedKey.minecraft("skeletons"), org.bukkit.entity.EntityType.class);
        assertNotNull(skeletons);
        assertTrue(skeletons.isTagged(org.bukkit.entity.EntityType.SKELETON));
        assertFalse(skeletons.isTagged(org.bukkit.entity.EntityType.COW));
        var water = server.getTag("fluids", org.bukkit.NamespacedKey.minecraft("water"), org.bukkit.Fluid.class);
        assertNotNull(water);
        assertTrue(water.isTagged(org.bukkit.Fluid.WATER));
        assertFalse(water.isTagged(org.bukkit.Fluid.LAVA));
        assertTrue(org.bukkit.Tag.FLUIDS_WATER.isTagged(org.bukkit.Fluid.WATER));
        assertNotNull(org.bukkit.Tag.ITEMS_ARROWS);
        assertNotNull(org.bukkit.Tag.BEDS);
        assertNotNull(org.bukkit.Tag.ITEMS_COALS);
        var missingTags = new java.util.ArrayList<String>();
        for (var field : org.bukkit.Tag.class.getFields()) if (field.getType() == org.bukkit.Tag.class) {
            try { if (field.get(null) == null) missingTags.add(field.getName()); }
            catch (IllegalAccessException e) { throw new AssertionError(e); }
        }
        assertTrue(missingTags.isEmpty(), "Bukkit Tag constants missing: " + missingTags);
        assertTrue(org.bukkit.Tag.GAME_EVENT_VIBRATIONS.isTagged(org.bukkit.GameEvent.STEP));
        assertEquals(16, org.bukkit.GameEvent.STEP.getRange());
        assertEquals(1, org.bukkit.GameEvent.STEP.getVibrationLevel());
        assertEquals(10, org.bukkit.GameEvent.JUKEBOX_PLAY.getRange());
        assertEquals(0, org.bukkit.GameEvent.JUKEBOX_PLAY.getVibrationLevel());
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

    private static org.bukkit.plugin.Plugin fakePlugin(String name) {
        return (org.bukkit.plugin.Plugin) java.lang.reflect.Proxy.newProxyInstance(ApiSurfaceTest.class.getClassLoader(), new Class<?>[] { org.bukkit.plugin.Plugin.class }, (proxy, method, args) -> switch (method.getName()) {
            case "getName" -> name;
            case "hashCode" -> System.identityHashCode(proxy);
            case "equals" -> proxy == args[0];
            case "toString" -> "Plugin[" + name + "]";
            default -> null;
        });
    }

    @Test void playerEntityStateMetadataAndTags() {
        var id = java.util.UUID.fromString("00000000-0000-0000-0000-0000000000bb");
        var p = FerritePlayer.create(server, id, "Tagged", false, 20.0, 20, 0, new org.bukkit.Location(world, 1.5, 70.0, -2.5, 90f, 10f));
        server.registerPlayer(p);
        try {
            var first = fakePlugin("First"); var second = fakePlugin("Second");
            assertFalse(p.hasMetadata("rank"));
            p.setMetadata("rank", new org.bukkit.metadata.FixedMetadataValue(first, "admin"));
            p.setMetadata("rank", new org.bukkit.metadata.FixedMetadataValue(second, 3));
            p.setMetadata("rank", new org.bukkit.metadata.FixedMetadataValue(first, "owner"));
            assertTrue(p.hasMetadata("rank"));
            assertEquals(2, p.getMetadata("rank").size(), "one value per owning plugin");
            assertTrue(p.getMetadata("rank").stream().anyMatch(v -> "owner".equals(v.asString())));
            p.removeMetadata("rank", first); p.removeMetadata("rank", second);
            assertFalse(p.hasMetadata("rank"));
            assertTrue(p.getMetadata("rank").isEmpty());

            assertTrue(p.addScoreboardTag("vip"));
            assertFalse(p.addScoreboardTag("vip"));
            assertEquals(Set.of("vip"), p.getScoreboardTags());
            assertTrue(p.removeScoreboardTag("vip"));
            assertEquals("Tagged", p.getScoreboardEntryName());

            assertEquals(org.bukkit.entity.EntityType.PLAYER, p.getType());
            assertEquals(1.5, p.getX()); assertEquals(70.0, p.getY()); assertEquals(-2.5, p.getZ());
            assertEquals(90f, p.getYaw()); assertEquals(10f, p.getPitch());
            var reused = new org.bukkit.Location(null, 0, 0, 0);
            assertSame(reused, p.getLocation(reused));
            assertEquals(p.getLocation(), reused);
            assertEquals(71.62, p.getEyeLocation().getY(), 1e-9);
            assertTrue(p.isValid()); assertTrue(p.isConnected()); assertFalse(p.isDead());
            assertEquals(world.getSpawnLocation(), p.getCompassTarget());
            assertNull(p.getWorldBorder());

            p.setHealth(15.0); p.heal(3.0); assertEquals(18.0, p.getHealth());
            p.heal(10.0); assertEquals(20.0, p.getHealth(), "healing stops at max health");
            assertThrows(IllegalArgumentException.class, () -> p.heal(-1));
        } finally {
            server.unregisterPlayer(id);
        }
    }

    @Test void maxHealthIsKeptOnTheAttributeAndPassedToTheHost() {
        var id = java.util.UUID.fromString("00000000-0000-0000-0000-0000000000ce");
        var p = FerritePlayer.create(server, id, "Hearts", false, 20.0, 20, 0, new org.bukkit.Location(world, 0, 65, 0));
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method + "|" + args); return ""; };
        try {
            assertEquals(20.0, p.getMaxHealth(), 1e-9);
            p.setMaxHealth(40.0);
            assertEquals(40.0, p.getMaxHealth(), 1e-9);
            assertEquals(40.0, p.getAttribute(org.bukkit.attribute.Attribute.MAX_HEALTH).getBaseValue(), 1e-9);
            assertTrue(calls.stream().anyMatch(c -> c.startsWith("setMaxHealth(double)|") && c.contains("40")), calls.toString());
            assertThrows(IllegalArgumentException.class, () -> p.setMaxHealth(0));
        } finally { HostBridge.transport = null; }
    }

    @Test void permissionAttachmentsGrantAndRevoke() {
        var id = java.util.UUID.fromString("00000000-0000-0000-0000-0000000000cf");
        var p = FerritePlayer.create(server, id, "Perms", false, 20.0, 20, 0, new org.bukkit.Location(world, 0, 65, 0));
        var plugin = (org.bukkit.plugin.Plugin) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.plugin.Plugin.class},
            (proxy, method, args) -> method.getName().equals("isEnabled") ? Boolean.TRUE : method.getReturnType() == boolean.class ? (Object) Boolean.FALSE : null);
        assertFalse(p.hasPermission("ferrite.test.fly"));
        assertFalse(p.isPermissionSet("ferrite.test.fly"));
        var attachment = p.addAttachment(plugin, "ferrite.test.fly", true);
        assertTrue(p.hasPermission("ferrite.test.fly"));
        assertTrue(p.isPermissionSet("ferrite.test.fly"));
        assertTrue(p.getEffectivePermissions().stream().anyMatch(i -> i.getPermission().equals("ferrite.test.fly") && i.getValue()));
        p.removeAttachment(attachment);
        assertFalse(p.hasPermission("ferrite.test.fly"));
    }

    @Test void experienceCanBeSetFromATotalNumberOfPoints() {
        var id = java.util.UUID.fromString("00000000-0000-0000-0000-0000000000cd");
        var p = FerritePlayer.create(server, id, "Total", false, 20.0, 20, 0, new org.bukkit.Location(world, 0, 65, 0));
        p.setExperienceLevelAndProgress(10);
        assertEquals(1, p.getLevel());
        assertEquals(3f / 9f, p.getExp(), 1e-6);
        assertEquals(10, p.getTotalExperience());
        // Levels 0..14 cost 7+9+...+35 = 315 points; 20 more is part of level 15.
        p.setExperienceLevelAndProgress(335);
        assertEquals(15, p.getLevel());
        assertEquals(20f / 37f, p.getExp(), 1e-6);
        p.setExperienceLevelAndProgress(0);
        assertEquals(0, p.getLevel());
        assertThrows(IllegalArgumentException.class, () -> p.setExperienceLevelAndProgress(-1));
    }

    @Test void playerExperienceFollowsVanillaCurve() {
        var id = java.util.UUID.fromString("00000000-0000-0000-0000-0000000000cc");
        var p = FerritePlayer.create(server, id, "Learner", false, 20.0, 20, 0, new org.bukkit.Location(world, 0, 65, 0));
        assertEquals(7, p.getExpToLevel());
        assertEquals(37, FerritePlayer.experienceToLevel(15));
        assertEquals(112, FerritePlayer.experienceToLevel(30));
        // 7 points finish level 0 and 3 of level 1's 9 remain as progress.
        p.giveExp(10);
        assertEquals(1, p.getLevel());
        assertEquals(3f / 9f, p.getExp(), 1e-6);
        assertEquals(10, p.getTotalExperience());
        assertEquals(10, p.calculateTotalExperiencePoints());
        p.giveExp(-5);
        assertEquals(0, p.getLevel());
        assertEquals(5f / 7f, p.getExp(), 1e-6);
        p.giveExpLevels(30);
        assertEquals(30, p.getLevel());
        assertEquals(112, p.getExperiencePointsNeededForNextLevel());
        p.giveExpLevels(-100);
        assertEquals(0, p.getLevel());
    }

    @Test void playerListHeaderFooterAndActionBarKeepState() {
        var id = java.util.UUID.fromString("00000000-0000-0000-0000-0000000000dd");
        var p = FerritePlayer.create(server, id, "Lister", false, 20.0, 20, 0, new org.bukkit.Location(world, 0, 65, 0));
        assertNull(p.getPlayerListHeader());
        assertEquals(net.kyori.adventure.text.Component.empty(), p.playerListFooter());
        p.setPlayerListHeaderFooter("\u00a7aWelcome", "Footer");
        assertEquals("\u00a7aWelcome", p.getPlayerListHeader());
        assertEquals("Footer", p.getPlayerListFooter());
        p.setPlayerListFooter(null);
        assertNull(p.getPlayerListFooter(), "a null footer clears it");
        assertEquals("\u00a7aWelcome", p.getPlayerListHeader(), "setting the footer keeps the header");
        p.setPlayerListHeaderFooter(new net.md_5.bungee.api.chat.TextComponent("Bungee"), null);
        assertEquals("Bungee", p.getPlayerListHeader());
        assertDoesNotThrow(() -> { p.sendActionBar("hi"); p.sendActionBar('&', "&ahi"); p.sendActionBar(net.kyori.adventure.text.Component.text("hi")); p.clearTitle(); p.resetTitle(); });
    }
}
