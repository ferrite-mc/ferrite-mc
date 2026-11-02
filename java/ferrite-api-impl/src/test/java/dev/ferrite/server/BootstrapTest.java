package dev.ferrite.server;
import java.util.*;
import java.util.logging.*;
import org.bukkit.Bukkit;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;
class BootstrapTest {
    static final class ProbeEvent extends org.bukkit.event.Event {
        private static final org.bukkit.event.HandlerList HANDLERS = new org.bukkit.event.HandlerList();
        public org.bukkit.event.HandlerList getHandlers() { return HANDLERS; }
        public static org.bukkit.event.HandlerList getHandlerList() { return HANDLERS; }
    }
    static final class CancelEvent extends org.bukkit.event.Event implements org.bukkit.event.Cancellable {
        private static final org.bukkit.event.HandlerList HANDLERS = new org.bukkit.event.HandlerList();
        boolean cancelled;
        public boolean isCancelled() { return cancelled; }
        public void setCancelled(boolean value) { cancelled = value; }
        public org.bukkit.event.HandlerList getHandlers() { return HANDLERS; }
        public static org.bukkit.event.HandlerList getHandlerList() { return HANDLERS; }
    }
    static class ParentListener implements org.bukkit.event.Listener {
        final java.util.concurrent.atomic.AtomicInteger called = new java.util.concurrent.atomic.AtomicInteger();
        @org.bukkit.event.EventHandler public void inherited(CancelEvent event) { called.incrementAndGet(); }
    }
    static final class ChildListener extends ParentListener {}
    @Test void paperOnlyLoadsAndFailureRollsBack() throws Exception {
        var jar = java.nio.file.Files.createTempFile("ferrite-paper", ".jar");
        try (var input = new java.util.jar.JarFile(System.getProperty("test.dependency.jar")); var output = new java.util.jar.JarOutputStream(java.nio.file.Files.newOutputStream(jar))) {
            var entries = input.entries();
            while (entries.hasMoreElements()) {
                var entry = entries.nextElement();
                if (entry.getName().equals("plugin.yml")) continue;
                output.putNextEntry(new java.util.jar.JarEntry(entry.getName()));
                try (var in = input.getInputStream(entry)) { in.transferTo(output); }
                output.closeEntry();
            }
            output.putNextEntry(new java.util.jar.JarEntry("paper-plugin.yml"));
            output.write("name: PaperOnly\nversion: '1.0'\nmain: dev.ferrite.dependency.DependencyPlugin\napi-version: '26.3'\n".getBytes(java.nio.charset.StandardCharsets.UTF_8));
            output.closeEntry();
        }
        var manager = new FerriteServer().plugins;
        manager.loadOrdered(new String[]{jar.toString()}, null);
        assertTrue(manager.isPluginEnabled("PaperOnly"));
        manager.disablePlugins();
        var failing = new FerriteServer().plugins;
        assertThrows(IllegalStateException.class, () -> failing.loadOrdered(new String[]{jar.toString(), jar + ".missing"}, null));
        assertEquals(0, failing.getPlugins().length);
    }
    @Test void loadsRunsAndDisablesPlugin() {
        List<String> log = new ArrayList<>();
        Logger.getLogger("").addHandler(new Handler() {
            public void publish(LogRecord record) { log.add(record.getMessage()); }
            public void flush() {}
            public void close() {}
        });
        System.setProperty("ferrite.native", "false");
        Bootstrap.start(new String[]{System.getProperty("test.plugin.jar"), System.getProperty("test.dependency.jar"), System.getProperty("test.legacy.jar")});
        assertEquals("Ferrite", Bukkit.getServer().getName());
        assertInstanceOf(org.bukkit.craftbukkit.CraftServer.class, Bukkit.getServer());
        assertEquals(org.bukkit.GameMode.SURVIVAL, Bukkit.getServer().getDefaultGameMode());
        Bukkit.getServer().setDefaultGameMode(org.bukkit.GameMode.ADVENTURE);
        assertEquals(org.bukkit.GameMode.ADVENTURE, Bukkit.getServer().getDefaultGameMode());
        assertFalse(Bukkit.getServer().getAllowFlight());
        assertTrue(Bukkit.getServer().getAllowNether());
        assertTrue(Bukkit.getServer().getAllowEnd());
        assertTrue(Bukkit.getServer().getGenerateStructures());
        assertEquals(29999984, Bukkit.getServer().getMaxWorldSize());
        assertEquals(10, Bukkit.getServer().getSimulationDistance());
        assertEquals(10, Bukkit.getServer().getSpawnRadius());
        Bukkit.getServer().setSpawnRadius(16);
        assertEquals(16, Bukkit.getServer().getSpawnRadius());
        assertEquals(0, Bukkit.getServer().getIdleTimeout());
        Bukkit.getServer().setIdleTimeout(30);
        assertEquals(30, Bukkit.getServer().getIdleTimeout());
        assertEquals(-1, Bukkit.getServer().getPauseWhenEmptyTime());
        Bukkit.getServer().setPauseWhenEmptyTime(60);
        assertEquals(60, Bukkit.getServer().getPauseWhenEmptyTime());
        assertEquals(-1, Bukkit.getServer().getConnectionThrottle());
        assertFalse(Bukkit.getServer().isWhitelistEnforced());
        Bukkit.getServer().setWhitelistEnforced(true);
        assertTrue(Bukkit.getServer().isWhitelistEnforced());
        assertEquals("Ferrite", Bukkit.getServer().getMotd());
        Bukkit.getServer().setMotd("Changed");
        assertEquals("Changed", Bukkit.getServer().getMotd());
        assertEquals("Changed", net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(Bukkit.getServer().motd()));
        assertEquals(20, Bukkit.getServer().getMaxPlayers());
        Bukkit.getServer().setMaxPlayers(42);
        assertEquals(42, Bukkit.getServer().getMaxPlayers());
        assertFalse(Bukkit.getServer().isStopping());
        assertNull(Bukkit.getServer().getShutdownMessage());
        assertTrue(Bukkit.getServer().getCurrentTick() >= 0);
        assertNotNull(Bukkit.getServer().getPluginsFolder());
        assertFalse(Bukkit.getPluginManager().useTimings());
        assertTrue(log.stream().anyMatch(line -> line.startsWith("surface ")), log.toString());
        assertTrue(log.stream().anyMatch(line -> line.startsWith("surface-motd Changed") || line.startsWith("surface-motd Ferrite")), log.toString());
        var surfaceApi = log.stream().filter(line -> line.startsWith("surface-api ")).findFirst().orElseThrow();
        for (String expected : List.of("hardcore=false", "logips=true", "secure=true", "transfers=false", "hide=false", "force-default=false", "container=true", "level=true", "update=update/update", "warnings=DEFAULT", "paused=false", "ticking=true", "global=true", "chained=1000000", "enabled=[vanilla]", "disabled=[]", "pack=true", "serverpack=true", "pack-required=false", "bosses=false", "boss=true", "remove-boss=false", "throttle=-1", "registry=true")) assertTrue(surfaceApi.contains(expected), surfaceApi);
        var surfaceFactory = log.stream().filter(line -> line.startsWith("surface-itemfactory ")).findFirst().orElseThrow();
        for (String expected : List.of("meta=true", "air=true", "applicable=true", "not-applicable=false", "copied=true", "leather=10511680", "egg=true", "name=block.minecraft.stone")) assertTrue(surfaceFactory.contains(expected), surfaceFactory);
        assertTrue(log.stream().anyMatch(line -> line.startsWith("surface-common dispatched=true")), log.toString());
        assertTrue(log.contains("pong"));
        assertNotNull(Bukkit.getServicesManager());
        assertInstanceOf(org.bukkit.plugin.SimplePluginManager.class, Bukkit.getPluginManager());
        var permission = new org.bukkit.permissions.Permission("ferrite.example", org.bukkit.permissions.PermissionDefault.TRUE);
        Bukkit.getPluginManager().addPermission(permission);
        assertSame(permission, Bukkit.getPluginManager().getPermission("FERRITE.EXAMPLE"));
        assertTrue(Bukkit.getPluginManager().getDefaultPermissions(false).contains(permission));
        Bukkit.getPluginManager().removePermission(permission);
        assertNull(Bukkit.getPluginManager().getPermission("ferrite.example"));
        assertTrue(Bukkit.getConsoleSender().equals(Bukkit.getConsoleSender()));
        assertDoesNotThrow(() -> Bukkit.getConsoleSender().hashCode());
        assertTrue(Bukkit.getOnlineMode());
        Bukkit.banIP("192.0.2.1");
        assertTrue(Bukkit.getIPBans().contains("192.0.2.1"));
        assertTrue(Bukkit.getBanList(org.bukkit.BanList.Type.IP).isBanned("192.0.2.1"));
        Bukkit.unbanIP("192.0.2.1");
        assertFalse(Bukkit.getBanList(org.bukkit.BanList.Type.IP).isBanned("192.0.2.1"));
        assertNotNull(Bukkit.getMessenger());
        assertNotNull(Bukkit.getCommandMap());
        var scoreboard = Bukkit.getScoreboardManager().getMainScoreboard();
        var team = scoreboard.registerNewTeam("example");
        team.addEntry("player");
        assertSame(team, scoreboard.getEntryTeam("player"));
        assertTrue(scoreboard.getEntries().contains("player"));
        team.unregister();
        assertNull(scoreboard.getTeam("example"));
        assertNull(org.bukkit.Registry.MENU.get(org.bukkit.NamespacedKey.minecraft("not_a_menu")));
        assertNotNull(org.bukkit.Registry.ATTRIBUTE.get(org.bukkit.NamespacedKey.minecraft("armor")));
        assertNotNull(org.bukkit.Registry.SOUNDS.get(org.bukkit.NamespacedKey.minecraft("entity.player.levelup")));
        assertNotNull(org.bukkit.Registry.BIOME.get(org.bukkit.NamespacedKey.minecraft("plains")));
        assertNotNull(org.bukkit.Registry.ENTITY_TYPE.get(org.bukkit.NamespacedKey.minecraft("zombie")));
        var sharpness = org.bukkit.Registry.ENCHANTMENT.get(org.bukkit.NamespacedKey.minecraft("sharpness"));
        assertNotNull(sharpness);
        assertSame(sharpness, io.papermc.paper.registry.RegistryAccess.registryAccess().getRegistry(io.papermc.paper.registry.RegistryKey.ENCHANTMENT).get(org.bukkit.NamespacedKey.minecraft("sharpness")));
        assertNotNull(org.bukkit.Registry.MOB_EFFECT.get(org.bukkit.NamespacedKey.minecraft("speed")));
        assertNull(org.bukkit.Registry.ATTRIBUTE.get(org.bukkit.NamespacedKey.minecraft("not_an_attribute")));
        assertTrue(org.bukkit.Material.STONE.isBlock());
        assertFalse(org.bukkit.Material.STONE.isAir());
        var stoneKey = org.bukkit.NamespacedKey.minecraft("stone");
        assertNotNull(Bukkit.getRegistry(org.bukkit.block.BlockType.class).get(stoneKey));
        assertSame(org.bukkit.Material.STONE, Bukkit.getRegistry(org.bukkit.block.BlockType.class).get(stoneKey).asMaterial());
        assertSame(org.bukkit.Material.STONE, Bukkit.getRegistry(org.bukkit.inventory.ItemType.class).get(stoneKey).asMaterial());
        assertSame(Bukkit.getServer().getUnsafe(), Bukkit.getUnsafe());
        assertEquals(777, Bukkit.getUnsafe().getProtocolVersion());
        assertEquals(5023, Bukkit.getUnsafe().getDataVersion());
        assertEquals("world", Bukkit.getUnsafe().getMainLevelName());
        assertTrue(Bukkit.getUnsafe().isSupportedApiVersion("26.3"));
        assertFalse(Bukkit.getUnsafe().isSupportedApiVersion("1.12"));
        var stoneData = Bukkit.createBlockData(org.bukkit.Material.STONE);
        assertEquals(org.bukkit.Material.STONE, stoneData.getMaterial());
        assertEquals("minecraft:stone", stoneData.getAsString());
        assertTrue(stoneData.matches(Bukkit.createBlockData("minecraft:stone")));
        assertThrows(IllegalArgumentException.class, () -> Bukkit.createBlockData("minecraft:not_a_block"));
        assertEquals(20.0, Bukkit.getTPS()[0], 0.001);
        assertEquals(50.0, Bukkit.getAverageTickTime(), 0.001);
        assertEquals(100, Bukkit.getTickTimes().length);
        assertFalse(Bukkit.isStopping());
        assertEquals(org.bukkit.GameMode.ADVENTURE, Bukkit.getDefaultGameMode());
        assertEquals("", Bukkit.getIp());
        assertEquals(29999984, Bukkit.getMaxWorldSize());
        assertTrue(org.bukkit.Material.AIR.isAir());
        var inventory = Bukkit.createInventory(null, 9, "test");
        assertEquals(9, inventory.getSize());
        var item = new org.bukkit.inventory.ItemStack(org.bukkit.Material.STONE, 4);
        var meta = item.getItemMeta();
        meta.setDisplayName("Example");
        meta.setLore(java.util.List.of("first"));
        meta.setCustomModelData(12);
        var key = new org.bukkit.NamespacedKey("ferrite", "example");
        meta.getPersistentDataContainer().set(key, org.bukkit.persistence.PersistentDataType.STRING, "value");
        assertTrue(item.setItemMeta(meta));
        assertEquals("Example", item.getItemMeta().getDisplayName());
        assertEquals(java.util.List.of("first"), item.getItemMeta().getLore());
        assertEquals(12, item.getItemMeta().getCustomModelData());
        assertEquals("value", item.getItemMeta().getPersistentDataContainer().get(key, org.bukkit.persistence.PersistentDataType.STRING));
        inventory.setItem(0, item);
        assertEquals(4, inventory.getItem(0).getAmount());
        assertEquals(1, inventory.firstEmpty());
        inventory.clear();
        assertNull(inventory.getItem(0));
        assertSame(Bukkit.getPluginCommand("ft"), Bukkit.getCommandMap().getCommand("ft"));
        var dispatched = new java.util.concurrent.atomic.AtomicBoolean();
        Bukkit.getCommandMap().register("probe", new org.bukkit.command.Command("plain") {
            public boolean execute(org.bukkit.command.CommandSender sender, String label, String[] args) { dispatched.set(true); return true; }
        });
        assertTrue(Bukkit.dispatchCommand(Bukkit.getConsoleSender(), "plain"));
        assertTrue(dispatched.get());
        assertEquals(org.bukkit.NamespacedKey.minecraft("stone"), org.bukkit.craftbukkit.util.CraftNamespacedKey.fromString("minecraft:stone"));
        assertEquals(org.bukkit.Material.STONE, org.bukkit.craftbukkit.inventory.CraftItemStack.asCraftCopy(new org.bukkit.inventory.ItemStack(org.bukkit.Material.STONE)).getType());
        assertEquals(org.bukkit.Material.STONE, org.bukkit.craftbukkit.block.data.CraftBlockData.create(Bukkit.createBlockData("minecraft:stone")).getMaterial());
        for (String shim : java.util.List.of("org.bukkit.craftbukkit.CraftWorld", "org.bukkit.craftbukkit.CraftOfflinePlayer", "org.bukkit.craftbukkit.entity.CraftPlayer", "org.bukkit.craftbukkit.block.CraftBlock", "org.bukkit.craftbukkit.inventory.CraftInventory", "org.bukkit.craftbukkit.inventory.CraftInventoryPlayer")) {
            assertDoesNotThrow(() -> Class.forName(shim), shim);
        }
        assertTrue(Bukkit.isPrimaryThread());
        assertTrue(Bukkit.getPluginManager().isPluginEnabled("FerriteTest"));
        assertTrue(((FerriteServer) Bukkit.getServer()).plugins.isLegacy(Bukkit.getPluginManager().getPlugin("FerriteLegacyTest")));
        assertThrows(ClassNotFoundException.class, () -> Bukkit.getPluginManager().getPlugin("FerriteLegacyTest").getClass().getClassLoader().loadClass("dev.ferrite.dependency.Helper"));
        assertSame(Bukkit.getPluginManager().getPlugin("FerriteDependency"), Bukkit.getPluginManager().getPlugin("FerriteAlias"));
        var dependency = Bukkit.getPluginManager().getPlugin("FerriteDependency");
        assertTrue(dependency.isEnabled());
        Bukkit.getPluginManager().disablePlugin(dependency);
        assertFalse(dependency.isEnabled());
        Bukkit.getPluginManager().enablePlugin(dependency);
        assertTrue(dependency.isEnabled());
        var ghost = (org.bukkit.plugin.Plugin) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.plugin.Plugin.class}, (proxy, method, args) -> switch (method.getName()) {
            case "getName" -> "Ghost";
            case "isEnabled" -> false;
            case "hashCode" -> 0;
            case "equals" -> proxy == args[0];
            case "toString" -> "Ghost";
            default -> null;
        });
        assertThrows(IllegalArgumentException.class, () -> Bukkit.getPluginManager().disablePlugin(ghost));
        assertThrows(IllegalArgumentException.class, () -> Bukkit.getPluginManager().enablePlugin(ghost));
        var order = new ArrayList<String>();
        var owner = Bukkit.getPluginManager().getPlugin("FerriteDependency");
        org.bukkit.event.Listener registered = new org.bukkit.event.Listener() {};
        Bukkit.getPluginManager().registerEvent(CancelEvent.class, registered, org.bukkit.event.EventPriority.HIGH, (listener, event) -> order.add("ignored"), owner, true);
        Bukkit.getPluginManager().registerEvent(CancelEvent.class, registered, org.bukkit.event.EventPriority.LOW, (listener, event) -> { order.add("first"); ((CancelEvent) event).setCancelled(true); }, owner, false);
        Bukkit.getPluginManager().registerEvent(CancelEvent.class, registered, org.bukkit.event.EventPriority.MONITOR, (listener, event) -> order.add("last"), owner, false);
        var child = new ChildListener();
        Bukkit.getPluginManager().registerEvents(child, owner);
        Bukkit.getPluginManager().registerEvent(CancelEvent.class, registered, org.bukkit.event.EventPriority.LOWEST, (listener, event) -> { throw new IllegalStateException("expected"); }, owner, false);
        Bukkit.getPluginManager().callEvent(new CancelEvent());
        assertEquals(1, child.called.get());
        assertEquals(List.of("first", "last"), order);
        var firstEntered = new java.util.concurrent.CountDownLatch(1);
        var release = new java.util.concurrent.CountDownLatch(1);
        var secondEntered = new java.util.concurrent.CountDownLatch(1);
        var count = new java.util.concurrent.atomic.AtomicInteger();
        org.bukkit.event.Listener listener = new org.bukkit.event.Listener() {
            @org.bukkit.event.EventHandler public void handle(ProbeEvent event) {
                if (count.incrementAndGet() == 1) { firstEntered.countDown(); try { release.await(); } catch (InterruptedException e) { Thread.currentThread().interrupt(); } }
                else secondEntered.countDown();
            }
        };
        Bukkit.getPluginManager().registerEvents(listener, Bukkit.getPluginManager().getPlugin("FerriteDependency"));
        Thread first = new Thread(() -> Bukkit.getPluginManager().callEvent(new ProbeEvent()));
        Thread second = new Thread(() -> Bukkit.getPluginManager().callEvent(new ProbeEvent()));
        first.start();
        try {
            assertTrue(firstEntered.await(5, java.util.concurrent.TimeUnit.SECONDS));
            second.start();
            assertFalse(secondEntered.await(200, java.util.concurrent.TimeUnit.MILLISECONDS));
        } catch (InterruptedException e) { throw new AssertionError(e); }
        finally { release.countDown(); try { first.join(); second.join(); } catch (InterruptedException e) { Thread.currentThread().interrupt(); } }
        assertEquals(2, count.get());
        assertTrue(log.contains("dependency visible"));
        var bukkitAsync = new java.util.concurrent.CountDownLatch(1);
        Bukkit.getScheduler().runTaskAsynchronously(Bukkit.getPluginManager().getPlugin("FerriteTest"), bukkitAsync::countDown);
        try { assertTrue(bukkitAsync.await(5, java.util.concurrent.TimeUnit.SECONDS)); } catch (InterruptedException e) { throw new AssertionError(e); }
        var asyncRan = new java.util.concurrent.CountDownLatch(1);
        Bukkit.getAsyncScheduler().runNow(Bukkit.getPluginManager().getPlugin("FerriteTest"), task -> asyncRan.countDown());
        try { assertTrue(asyncRan.await(5, java.util.concurrent.TimeUnit.SECONDS)); } catch (InterruptedException e) { throw new AssertionError(e); }
        assertTrue(log.contains("FerriteTest event"));
        UUID snapshotId = UUID.randomUUID();
        Bootstrap.registerPlayer(snapshotId.toString(), "snapshot", true);
        var snapshotPlayer = Bukkit.getPlayer(snapshotId);
        assertInstanceOf(org.bukkit.craftbukkit.entity.CraftHumanEntity.class, snapshotPlayer);
        assertEquals("snapshot", snapshotPlayer.getName());
        assertTrue(snapshotPlayer.isOnline());
        assertTrue(snapshotPlayer.isOp());
        assertTrue(snapshotPlayer.hasPermission("ferrite.test"));
        var shim = (org.bukkit.craftbukkit.entity.CraftHumanEntity) snapshotPlayer;
        var oldPermissible = shim.perm;
        try {
            org.bukkit.craftbukkit.entity.CraftHumanEntity.class.getDeclaredField("perm").set(snapshotPlayer, new org.bukkit.permissions.PermissibleBase(snapshotPlayer) {
                @Override public boolean hasPermission(String name) { return name.equals("injected.permission"); }
            });
        } catch (ReflectiveOperationException e) { throw new AssertionError(e); }
        assertTrue(snapshotPlayer.hasPermission("injected.permission"));
        assertFalse(snapshotPlayer.hasPermission("ferrite.test"));
        shim.perm = oldPermissible;
        snapshotPlayer.setHealth(12.0);
        assertEquals(12.0, snapshotPlayer.getHealth());
        snapshotPlayer.setFoodLevel(8);
        assertEquals(8, snapshotPlayer.getFoodLevel());
        snapshotPlayer.setGameMode(org.bukkit.GameMode.CREATIVE);
        assertEquals(org.bukkit.GameMode.CREATIVE, snapshotPlayer.getGameMode());
        snapshotPlayer.getPersistentDataContainer().set(key, org.bukkit.persistence.PersistentDataType.STRING, "player-value");
        assertEquals("player-value", snapshotPlayer.getPersistentDataContainer().get(key, org.bukkit.persistence.PersistentDataType.STRING));
        UUID playerId = UUID.randomUUID();
        org.bukkit.entity.Player player = (org.bukkit.entity.Player) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.entity.Player.class}, (proxy, method, args) -> {
            if (method.getName().equals("getUniqueId")) return playerId;
            if (method.getName().equals("hasPermission") || method.getName().equals("isOp")) return false;
            if (method.getName().equals("sendMessage")) return null;
            if (method.getName().equals("getName")) return "player";
            throw new UnsupportedOperationException(method.getName());
        });
        ((FerriteServer) Bukkit.getServer()).registerPlayer(player);
        var ferrite = (FerriteServer) Bukkit.getServer();
        UUID worldId = UUID.randomUUID();
        var world = (org.bukkit.World) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.World.class}, (proxy, method, args) -> {
            if (method.getName().equals("getUID")) return worldId;
            if (method.getName().equals("getName")) return "world";
            throw new UnsupportedOperationException(method.getName());
        });
        ferrite.registerWorld(world);
        assertSame(world, Bukkit.getWorld(worldId));
        assertSame(world, Bukkit.getWorld("world"));
        assertEquals(1, Bukkit.getWorlds().size());
        assertSame(player, Bukkit.getOfflinePlayer(playerId));
        UUID knownId = UUID.fromString("00000000-0000-0000-0000-00000000002c");
        Bootstrap.registerOfflinePlayer(knownId.toString(), "KnownOffline", true);
        var known = Bukkit.getOfflinePlayer("knownOFFLINE");
        assertEquals(knownId, known.getUniqueId());
        assertEquals("KnownOffline", known.getName());
        assertTrue(known.isOp());
        assertEquals(knownId, Bukkit.getOfflinePlayer(knownId).getUniqueId());
        assertTrue(Bukkit.getOperators().contains(known));
        var offline = Bukkit.getOfflinePlayer("Absent");
        assertEquals("Absent", offline.getName());
        assertFalse(offline.isOnline());
        offline.setOp(true);
        assertTrue(Bukkit.getOperators().contains(offline));
        assertSame(player, Bukkit.getPlayerExact("PLAYER"));
        assertSame(player, Bukkit.getPlayer("pla"));
        assertNull(Bukkit.getPlayerExact("pla"));
        assertSame(player, Bukkit.getServer().getEntity(playerId));
        var matches = Bukkit.getServer().matchPlayer("pla");
        assertEquals(1, matches.size());
        assertSame(player, matches.get(0));
        assertEquals(playerId, Bukkit.getServer().getPlayerUniqueId("PLAYER"));
        assertNull(Bukkit.getServer().getPlayerUniqueId("nobody"));
        assertNotNull(Bukkit.getServer().getOfflinePlayerIfCached("knownOFFLINE"));
        assertNull(Bukkit.getServer().getOfflinePlayerIfCached("NobodyHere"));
        Bukkit.getPluginManager().addPermission(new org.bukkit.permissions.Permission("ferrite.broadcast.test", org.bukkit.permissions.PermissionDefault.FALSE));
        assertEquals(0, Bukkit.getServer().broadcast(net.kyori.adventure.text.Component.text("hello"), "ferrite.broadcast.test"));
        assertEquals(Bukkit.getOnlinePlayers().size(), Bukkit.getServer().broadcast(net.kyori.adventure.text.Component.text("hello"), null));
        assertFalse(Bootstrap.dispatchCommand("unknown", "ferrite-test hello"));
        assertFalse(Native.dispatchCommand((org.bukkit.command.CommandSender) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.command.ConsoleCommandSender.class}, (proxy, method, args) -> null), "bridge-command"));
        Bootstrap.dispatchCommand(playerId.toString(), "ferrite-test hello");
        assertFalse(log.contains("hello hello"));
        assertNotNull(Bukkit.getPluginCommand("ft"));
        assertEquals(List.of("hello"), Bukkit.getPluginCommand("ft").tabComplete(Bukkit.getConsoleSender(), "ft", new String[]{"h"}));
        assertTrue(Bootstrap.dispatchCommand("console", "ft hello"));
        assertTrue(log.contains("hello hello"));
        var survivor = new java.util.concurrent.atomic.AtomicInteger();
        Bukkit.getScheduler().runTask(owner, () -> { throw new IllegalStateException("expected task failure"); });
        Bukkit.getScheduler().runTask(owner, survivor::incrementAndGet);
        var delayed = new java.util.concurrent.atomic.AtomicInteger();
        Bukkit.getScheduler().runTaskLater(Bukkit.getPluginManager().getPlugin("FerriteTest"), delayed::incrementAndGet, 2);
        Bootstrap.tick();
        assertEquals(0, delayed.get());
        assertEquals(1, survivor.get());
        Bootstrap.tick(); Bootstrap.tick();
        assertEquals(1, delayed.get());
        assertTrue(log.contains("FerriteTest region"));
        assertTrue(log.contains("FerriteTest owned"));
        assertTrue(log.contains("FerriteTest entity"));
        assertEquals(6, log.stream().filter("FerriteTest tick"::equals).count());
        assertEquals(6, log.stream().filter("FerriteTest global"::equals).count());
        // Plugin disable cancels the plugin's async tasks too.
        var asyncAfterDisable = new java.util.concurrent.atomic.AtomicInteger();
        var asyncOwner = Bukkit.getPluginManager().getPlugin("FerriteDependency");
        Bukkit.getScheduler().runTaskTimerAsynchronously(asyncOwner, asyncAfterDisable::incrementAndGet, 1, 1);
        try { Thread.sleep(100); } catch (InterruptedException e) { Thread.currentThread().interrupt(); }
        Bootstrap.shutdown();
        try { Thread.sleep(100); } catch (InterruptedException e) { Thread.currentThread().interrupt(); }
        int settledAsync = asyncAfterDisable.get();
        try { Thread.sleep(250); } catch (InterruptedException e) { Thread.currentThread().interrupt(); }
        assertEquals(settledAsync, asyncAfterDisable.get(), "an async task ran after plugin disable");
        Bootstrap.shutdown();
        assertTrue(Bukkit.getServer().isStopping());
        assertFalse(Bukkit.getServer().isTickingWorlds());
        assertFalse(Bukkit.getPluginManager().isPluginEnabled("FerriteTest"));
        assertTrue(log.contains("FerriteTest disabled"));
        Bootstrap.tick();
        assertTrue(log.contains("FerriteTest region"));
        assertTrue(log.contains("FerriteTest owned"));
        assertTrue(log.contains("FerriteTest entity"));
        assertEquals(6, log.stream().filter("FerriteTest tick"::equals).count());
        assertEquals(6, log.stream().filter("FerriteTest global"::equals).count());
        Bukkit.getPluginManager().clearPlugins();
        assertEquals(0, Bukkit.getPluginManager().getPlugins().length);
    }
}
