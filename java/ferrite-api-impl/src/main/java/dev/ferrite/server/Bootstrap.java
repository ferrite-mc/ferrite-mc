package dev.ferrite.server;
public final class Bootstrap {
    static final Object LEGACY_LOCK = new Object();
    private static final java.util.concurrent.locks.ReentrantLock COMMAND_LOCK = new java.util.concurrent.locks.ReentrantLock();
    /**
     * Runs a host call that waits on the game loop. The loop's tick takes {@link #LEGACY_LOCK}, so a thread
     * holding it (a plugin command) would deadlock the loop it waits on: release it for the duration.
     */
    @SuppressWarnings("unchecked")
    static <T> T awaitLoop(java.util.function.Supplier<T> call) {
        if (!Thread.holdsLock(LEGACY_LOCK)) return call.get();
        var result = new Object[1];
        var failure = new Throwable[1];
        var done = new boolean[1];
        var worker = new Thread(() -> {
            try { result[0] = call.get(); } catch (Throwable t) { failure[0] = t; }
            synchronized (LEGACY_LOCK) { done[0] = true; LEGACY_LOCK.notifyAll(); }
        }, "ferrite-host-call");
        worker.setDaemon(true);
        worker.start();
        boolean interrupted = false;
        while (!done[0]) {
            try { LEGACY_LOCK.wait(); } catch (InterruptedException e) { interrupted = true; }
        }
        if (interrupted) Thread.currentThread().interrupt();
        if (failure[0] instanceof RuntimeException e) throw e;
        if (failure[0] instanceof Error e) throw e;
        if (failure[0] != null) throw new IllegalStateException(failure[0]);
        return (T) result[0];
    }
    private static FerriteServer server;
    private static final java.util.List<String> initialIpBans = new java.util.ArrayList<>();
    private static FerriteServer.Settings settings = new FerriteServer.Settings("Ferrite", 20, 25565, 10, false, true);
    public static void configure(String motd, int maxPlayers, int port, int viewDistance, boolean whitelist, boolean onlineMode) {
        initialIpBans.clear();
        settings = new FerriteServer.Settings(motd, maxPlayers, port, viewDistance, whitelist, onlineMode);
    }
    public static void configureIpBan(String ip) { initialIpBans.add(ip); }
    private static volatile String address = "";
    private static volatile String dataDirectory;
    /** The host part of the listener bind address, for {@code Server#getIp}. */
    public static void configureAddress(String value) { address = value == null ? "" : value; }
    /** The extracted vanilla data pack the recipe and tag tables are read from. */
    public static void configureDataDir(String value) { dataDirectory = value == null || value.isEmpty() ? null : value; }
    static String serverAddress() { return address; }
    static java.io.File dataFile(String child) { var dir = dataDirectory; return dir == null ? null : new java.io.File(dir, child); }
    private Bootstrap() {}
    public static void start(String[] paths) { start(paths, null); }
    public static void start(String[] paths, boolean[] regionAware) { startServer(); enablePlugins(paths, regionAware); }
    /**
     * Creates the Bukkit server without enabling plugins, so the host can publish worlds and
     * players before {@code onEnable} runs. Pair it with {@link #enablePlugins}.
     */
    public static void startServer() {
        // Initialize GameRule before plugins can access GameRules: Paper's legacy aliases are circular.
        org.bukkit.GameRule.values();
        if (org.bukkit.Bukkit.getServer() != null) { server = (FerriteServer) org.bukkit.Bukkit.getServer(); Native.link(); return; }
        Native.link();
        if (!"false".equals(System.getProperty("ferrite.native"))) java.util.logging.Logger.getLogger("").addHandler(new java.util.logging.Handler() {
            public void publish(java.util.logging.LogRecord record) { Native.log(record.getMessage()); }
            public void flush() {} public void close() {}
        });
        server = new org.bukkit.craftbukkit.CraftServer(settings);
        initialIpBans.forEach(server::seedIpBan);
        org.bukkit.Bukkit.setServer(server);
    }
    public static void enablePlugins(String[] paths, boolean[] regionAware) {
        synchronized (LEGACY_LOCK) { server.plugins.loadOrdered(paths, regionAware); }
    }
    public static void registerWorld(String id, String name, long time, int x, int y, int z) {
        var world = FerriteWorld.create(server, java.util.UUID.fromString(id), name, time, x, y, z);
        server.registerWorld(world);
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(new org.bukkit.event.world.WorldLoadEvent(world)); }
    }
    public static boolean unregisterWorld(String id) {
        var uuid = java.util.UUID.fromString(id);
        var world = server.getWorld(uuid);
        if (world == null) return false;
        synchronized (LEGACY_LOCK) {
            var event = new org.bukkit.event.world.WorldUnloadEvent(world);
            server.plugins.callEvent(event);
            if (event.isCancelled()) return false;
            server.unregisterWorld(uuid);
            return true;
        }
    }
    public static void setHeightSnapshot(String world, int x, int z, int y) { FerriteWorld.setHeight(server.getWorld(java.util.UUID.fromString(world)), x, z, y); }
    public static void setBlockSnapshot(String world, int x, int y, int z, String material) { FerriteWorld.setSnapshot(server.getWorld(java.util.UUID.fromString(world)), x, y, z, material); }
    public static void registerPlayer(String id, String name, boolean operator) { registerPlayer(id, name, operator, 20.0, 20, 0, null, 0, 0, 0, 0, 0); }
    public static void registerPlayer(String id, String name, boolean operator, double health, int food, int mode, String world, double x, double y, double z, float yaw, float pitch) {
        var uuid = java.util.UUID.fromString(id);
        var location = world == null ? null : new org.bukkit.Location(java.util.Objects.requireNonNull(server.getWorld(java.util.UUID.fromString(world))), x, y, z, yaw, pitch);
        // Keep the object identity: plugins hold Player references across refreshes.
        if (server.getPlayer(uuid) instanceof FerritePlayer current) { current.updateSnapshot(health, food, mode, location, operator); return; }
        server.registerPlayer(FerritePlayer.create(server, uuid, name, operator, health, food, mode, location));
    }
    /** Moves the host-side snapshot with the player; called on every accepted movement packet. */
    public static void updatePlayerLocation(String id, String world, double x, double y, double z, float yaw, float pitch) {
        if (!(server.getPlayer(java.util.UUID.fromString(id)) instanceof FerritePlayer player)) return;
        var resolved = server.getWorld(java.util.UUID.fromString(world));
        if (resolved != null) player.updateLocation(new org.bukkit.Location(resolved, x, y, z, yaw, pitch));
    }
    public static void setInventorySnapshot(String player, int slot, String material, int amount) { ((FerritePlayer) java.util.Objects.requireNonNull(server.getPlayer(java.util.UUID.fromString(player)))).inventorySnapshot(slot, material, amount); }
    public static void registerOfflinePlayer(String id, String name, boolean operator) { server.registerOfflinePlayer(java.util.UUID.fromString(id), name, operator); }
    public static boolean isPluginEnabled(String name) { return server.getPluginManager().isPluginEnabled(name); }
    public static boolean dispatchCommand(String sender, String line) {
        org.bukkit.command.CommandSender resolved;
        if ("console".equals(sender)) resolved = server.getConsoleSender();
        else {
            try { resolved = server.getPlayer(java.util.UUID.fromString(sender)); }
            catch (IllegalArgumentException | NullPointerException e) { return false; }
        }
        COMMAND_LOCK.lock();
        try {
            synchronized (LEGACY_LOCK) { return resolved != null && server.dispatchCommand(resolved, line); }
        } finally {
            COMMAND_LOCK.unlock();
        }
    }
    public static void tick() { synchronized (LEGACY_LOCK) { server.scheduler.tick(); Native.tickFallback(); } }
    public static void runGlobalTask(long id) { synchronized (LEGACY_LOCK) { server.scheduler.executeId(id); } }
    public static void runEntityTask(long id) { server.entitySchedulers.execute(id); }
    public static void runRegionTask(long id) { server.regionScheduler.execute(id); }
    public static void shutdown() { synchronized (LEGACY_LOCK) { server.beginShutdown(); server.getPluginManager().disablePlugins(); server.asyncScheduler.shutdown(); } Native.close(); }
    /** Resolves the live player object, registering a snapshot when the server has none yet. */
    private static org.bukkit.entity.Player ensurePlayer(String id, String name) {
        var uuid = java.util.UUID.fromString(id);
        var player = server.getPlayer(uuid);
        if (player == null) { player = FerritePlayer.create(server, uuid, name, false, 20.0, 20, 1, null); server.registerPlayer(player); }
        return player;
    }
    /** Fires Bukkit's {@code PlayerJoinEvent}. */
    public static void firePlayerJoin(String id, String name) {
        var player = ensurePlayer(id, name);
        // Like vanilla, a joining player sees the main scoreboard until a plugin gives them another.
        try { player.setScoreboard(server.getScoreboardManager().getMainScoreboard()); } catch (RuntimeException noHost) { /* nothing to show without a host */ }
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(new org.bukkit.event.player.PlayerJoinEvent(player, net.kyori.adventure.text.Component.empty())); }
    }
    /** Fires Bukkit's {@code PlayerQuitEvent} and forgets the disconnected player. */
    public static void firePlayerQuit(String id) {
        var player = server.getPlayer(java.util.UUID.fromString(id));
        if (player == null) return;
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(new org.bukkit.event.player.PlayerQuitEvent(player, net.kyori.adventure.text.Component.empty(), org.bukkit.event.player.PlayerQuitEvent.QuitReason.DISCONNECTED)); }
        server.unregisterPlayer(player.getUniqueId());
    }
    /** Fires Paper's {@code AsyncChatEvent} then Bukkit's {@code AsyncPlayerChatEvent}; null means cancelled. */
    public static String fireChat(String id, String name, String message) {
        var player = ensurePlayer(id, name);
        if (player instanceof PlayerMisc active) active.touch();
        if (player.isConversing()) {
            player.acceptConversationInput(message);
            return null;
        }
        String result = message;
        synchronized (LEGACY_LOCK) {
            if (server.plugins.hasListeners(io.papermc.paper.event.player.AsyncChatEvent.class)) {
                java.util.Set<net.kyori.adventure.audience.Audience> viewers = new java.util.LinkedHashSet<>(server.getOnlinePlayers());
                var text = net.kyori.adventure.text.Component.text(message);
                var event = new io.papermc.paper.event.player.AsyncChatEvent(true, player, viewers, io.papermc.paper.chat.ChatRenderer.defaultRenderer(), text, text, null);
                server.plugins.callEvent(event);
                if (event.isCancelled()) return null;
                result = net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(event.message());
            }
            if (server.plugins.hasListeners(org.bukkit.event.player.AsyncPlayerChatEvent.class)) {
                var event = new org.bukkit.event.player.AsyncPlayerChatEvent(true, player, result, new java.util.LinkedHashSet<>(server.getOnlinePlayers()));
                server.plugins.callEvent(event);
                if (event.isCancelled()) return null;
                result = event.getMessage();
            }
        }
        return result;
    }
    /** Fires Bukkit's {@code PlayerMoveEvent}; false means a listener cancelled the move. */
    public static boolean firePlayerMove(String id, String name, double fromX, double fromY, double fromZ, float fromYaw, float fromPitch, double toX, double toY, double toZ, float toYaw, float toPitch) {
        if (ensurePlayer(id, name) instanceof PlayerMisc active) active.touch();
        var event = new org.bukkit.event.player.PlayerMoveEvent(ensurePlayer(id, name), new org.bukkit.Location(null, fromX, fromY, fromZ, fromYaw, fromPitch), new org.bukkit.Location(null, toX, toY, toZ, toYaw, toPitch));
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(event); }
        return !event.isCancelled();
    }
    /** The world block event proxies report; the Rust host currently runs one overworld. */
    static synchronized org.bukkit.World defaultWorld() {
        if (defaultWorld == null) {
            var worlds = server.getWorlds();
            if (!worlds.isEmpty()) defaultWorld = worlds.get(0);
            else {
                defaultWorld = FerriteWorld.create(server, java.util.UUID.fromString("00000000-0000-0000-0000-000000000001"), "world", 0L, 0, 64, 0);
                server.registerWorld(defaultWorld);
            }
        }
        return defaultWorld;
    }
    private static org.bukkit.World defaultWorld;
    /** Bukkit material for the namespaced block id the host sends with an event. */
    private static org.bukkit.Material material(String key) {
        var material = org.bukkit.Material.matchMaterial(key);
        if (material == null) throw new IllegalArgumentException("Unknown block material " + key);
        return material;
    }
    /** Fires Bukkit's {@code PlayerInteractEvent} for a block click; false means cancelled. */
    public static boolean firePlayerInteract(String id, String name, int action, int x, int y, int z, int face, String type) {
        var event = new org.bukkit.event.player.PlayerInteractEvent(ensurePlayer(id, name), action == 0 ? org.bukkit.event.block.Action.LEFT_CLICK_BLOCK : org.bukkit.event.block.Action.RIGHT_CLICK_BLOCK, null, FerriteEventObjects.block(x, y, z, material(type)), FerriteEventObjects.face(face), org.bukkit.inventory.EquipmentSlot.HAND);
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(event); }
        return !event.isCancelled() && event.useInteractedBlock() != org.bukkit.event.Event.Result.DENY;
    }
    /** Fires Bukkit's {@code BlockBreakEvent}; false means a listener cancelled the break. */
    public static boolean fireBlockBreak(String id, String name, int x, int y, int z, String type) {
        var event = new org.bukkit.event.block.BlockBreakEvent(FerriteEventObjects.block(x, y, z, material(type)), ensurePlayer(id, name));
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(event); }
        return !event.isCancelled();
    }
    /**
     * Fires Bukkit's {@code BlockExplodeEvent} for the blocks a plugin explosion would destroy.
     * Returns null when the world is unknown, "" when a listener cancelled the explosion, and the
     * surviving block positions (encoded like {@link HostBridge#ITEM}) otherwise.
     */
    public static String fireBlockExplode(String world, String center, String blocks, float yield) {
        var resolved = server.getWorld(java.util.UUID.fromString(world));
        if (resolved == null) return null;
        var centerFields = center.split(",");
        if (centerFields.length != 3) throw new IllegalArgumentException("Ferrite: invalid explosion centre " + center);
        var centerBlock = resolved.getBlockAt(Integer.parseInt(centerFields[0]), Integer.parseInt(centerFields[1]), Integer.parseInt(centerFields[2]));
        var list = new java.util.ArrayList<org.bukkit.block.Block>();
        if (!blocks.isEmpty()) {
            for (var line : blocks.split(String.valueOf(HostBridge.ITEM))) {
                var fields = line.split(",");
                if (fields.length != 3) continue;
                list.add(resolved.getBlockAt(Integer.parseInt(fields[0]), Integer.parseInt(fields[1]), Integer.parseInt(fields[2])));
            }
        }
        var event = new org.bukkit.event.block.BlockExplodeEvent(centerBlock, FerriteEventObjects.state(centerBlock, centerBlock.getType()), list, yield, org.bukkit.ExplosionResult.DESTROY);
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(event); }
        if (event.isCancelled()) return "";
        var kept = new StringBuilder();
        for (var block : event.blockList()) {
            if (kept.length() > 0) kept.append(HostBridge.ITEM);
            kept.append(block.getX()).append(',').append(block.getY()).append(',').append(block.getZ());
        }
        return kept.toString();
    }
    /** Fires Bukkit's {@code BlockPlaceEvent}; false means a listener cancelled the placement. */
    public static boolean fireBlockPlace(String id, String name, int x, int y, int z, int face, String placed, String replaced) {
        var block = FerriteEventObjects.block(x, y, z, material(placed));
        var blockFace = FerriteEventObjects.face(face);
        var against = FerriteEventObjects.block(x - blockFace.getModX(), y - blockFace.getModY(), z - blockFace.getModZ(), block.getType());
        var event = new org.bukkit.event.block.BlockPlaceEvent(block, FerriteEventObjects.state(block, material(replaced)), against, null, ensurePlayer(id, name), true, org.bukkit.inventory.EquipmentSlot.HAND);
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(event); }
        return !event.isCancelled() && event.canBuild();
    }
    /** Fires Bukkit's {@code PlayerCommandPreprocessEvent}; false means a listener cancelled it. */
    public static boolean firePlayerCommand(String id, String name, String message) {
        var event = new org.bukkit.event.player.PlayerCommandPreprocessEvent(ensurePlayer(id, name), message);
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(event); }
        return !event.isCancelled();
    }
    /** Fires Bukkit's {@code ChunkLoadEvent} for a chunk the host just streamed. */
    public static void fireChunkLoad(String world, int x, int z, boolean newChunk) {
        var resolved = server.getWorld(java.util.UUID.fromString(world));
        if (resolved == null) return;
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(new org.bukkit.event.world.ChunkLoadEvent(FerriteEventObjects.chunk(resolved, x, z), newChunk)); }
    }
    /** Fires Bukkit's {@code ChunkUnloadEvent} for a chunk the host dropped. */
    public static void fireChunkUnload(String world, int x, int z) {
        var resolved = server.getWorld(java.util.UUID.fromString(world));
        if (resolved == null) return;
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(new org.bukkit.event.world.ChunkUnloadEvent(FerriteEventObjects.chunk(resolved, x, z))); }
    }
    /** Fires Bukkit's {@code PlayerDeathEvent}; returns the drops and experience a listener left. */
    public static String firePlayerDeath(String id, String name, String cause, int droppedExp, boolean keepInventory, String message, String encodedDrops) {
        var player = ensurePlayer(id, name);
        var drops = new java.util.ArrayList<org.bukkit.inventory.ItemStack>();
        if (encodedDrops != null && !encodedDrops.isEmpty()) {
            for (var line : encodedDrops.split("\n")) {
                if (line.isEmpty()) continue;
                int tab = line.indexOf('\t');
                if (tab <= 0) throw new IllegalArgumentException("Ferrite: invalid death drop: " + line);
                drops.add(new org.bukkit.inventory.ItemStack(material(line.substring(0, tab)), Integer.parseInt(line.substring(tab + 1))));
            }
        }
        var event = new org.bukkit.event.entity.PlayerDeathEvent(player, damageSource(), drops, droppedExp, message == null ? null : net.kyori.adventure.text.Component.text(message), keepInventory);
        boolean initialKeepLevel = event.getKeepLevel();
        int initialNewExp = event.getNewExp();
        int initialNewTotalExp = event.getNewTotalExp();
        int initialNewLevel = event.getNewLevel();
        boolean initialShouldDropExperience = event.shouldDropExperience();
        boolean initialShowDeathMessages = event.getShowDeathMessages();
        var initialDeathMessage = event.deathMessage();
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(event); }
        if (event.getKeepLevel() != initialKeepLevel) throw unsupportedDeath("keepLevel");
        if (event.getNewExp() != initialNewExp) throw unsupportedDeath("newExp");
        if (event.getNewTotalExp() != initialNewTotalExp) throw unsupportedDeath("newTotalExp");
        if (event.getNewLevel() != initialNewLevel) throw unsupportedDeath("newLevel");
        if (event.shouldDropExperience() != initialShouldDropExperience) throw unsupportedDeath("shouldDropExperience");
        if (event.getShowDeathMessages() != initialShowDeathMessages) throw unsupportedDeath("showDeathMessages");
        if (!java.util.Objects.equals(event.deathMessage(), initialDeathMessage)) throw unsupportedDeath("deathMessage");
        if (event.getDroppedExp() < 0) throw unsupportedDeath("droppedExp");
        var out = new StringBuilder();
        out.append(event.getDroppedExp()).append('\t').append(event.getKeepInventory() ? 1 : 0).append('\n');
        for (var item : event.getDrops()) {
            if (item == null) throw unsupportedDeath("null drop");
            if (item.getType().isAir() || item.getAmount() <= 0) continue;
            if (item.hasItemMeta()) throw unsupportedDeath("drop metadata");
            if (item.getAmount() > 255) throw unsupportedDeath("drop amount");
            out.append(item.getType().getKey()).append('\t').append(item.getAmount()).append('\n');
        }
        return out.toString();
    }
    /** A death-event mutation the host cannot represent. */
    private static UnsupportedOperationException unsupportedDeath(String field) {
        return new UnsupportedOperationException("Ferrite: PlayerDeathEvent " + field + " changes are not supported");
    }
    /** Fires Bukkit's {@code PlayerRespawnEvent}; returns the (possibly moved) spawn as "x y z yaw pitch". */
    public static String firePlayerRespawn(String id, String name, double x, double y, double z, float yaw, float pitch, String reason) {
        var player = ensurePlayer(id, name);
        var location = new org.bukkit.Location(player.getWorld(), x, y, z, yaw, pitch);
        org.bukkit.event.player.PlayerRespawnEvent.RespawnReason why;
        try { why = org.bukkit.event.player.PlayerRespawnEvent.RespawnReason.valueOf(reason); }
        catch (IllegalArgumentException e) { why = org.bukkit.event.player.PlayerRespawnEvent.RespawnReason.PLUGIN; }
        var event = new org.bukkit.event.player.PlayerRespawnEvent(player, location, false, false, false, why);
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(event); }
        var spawn = event.getRespawnLocation();
        return spawn.getX() + " " + spawn.getY() + " " + spawn.getZ() + " " + spawn.getYaw() + " " + spawn.getPitch();
    }
    /** Fires Bukkit's {@code EntityDamageEvent}; returns the final damage after listeners. */
    public static double fireEntityDamage(String id, String name, String cause, double amount) {
        var entity = entity(id, name);
        var event = new org.bukkit.event.entity.EntityDamageEvent(entity, damageCause(cause), amount);
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(event); }
        double finalDamage = event.isCancelled() ? 0.0 : event.getFinalDamage();
        if (!Double.isFinite(finalDamage) || finalDamage < 0.0 || finalDamage > Float.MAX_VALUE) throw new UnsupportedOperationException("Ferrite: EntityDamageEvent damage " + finalDamage + " is not supported");
        if (finalDamage > 0 && entity instanceof PlayerMisc tracked) tracked.recordDamage(damageSource(), finalDamage);
        return finalDamage;
    }
    /** Fires Bukkit's {@code EntityDeathEvent} for a non-player entity the host removed. */
    public static void fireEntityDeath(String id, String type, double x, double y, double z, int droppedExp) {
        var entityType = org.bukkit.entity.EntityType.fromName(type);
        if (entityType == null) return;
        var entity = FerriteEventObjects.living(java.util.UUID.fromString(id), entityType, new org.bukkit.Location(defaultWorld(), x, y, z), 0.0);
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(new org.bukkit.event.entity.EntityDeathEvent(entity, damageSource(), new java.util.ArrayList<org.bukkit.inventory.ItemStack>(), droppedExp)); }
    }
    /** Fires Bukkit's {@code WeatherChangeEvent}; false cancels the change. */
    public static boolean fireWeatherChange(String world, boolean toWeatherState) {
        var resolved = server.getWorld(java.util.UUID.fromString(world));
        if (resolved == null) return true;
        var event = new org.bukkit.event.weather.WeatherChangeEvent(resolved, toWeatherState);
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(event); }
        return !event.isCancelled();
    }
    /** Fires Bukkit's {@code WorldLoadEvent} when the host publishes a world. */
    public static void fireWorldLoad(String world) {
        var resolved = server.getWorld(java.util.UUID.fromString(world));
        if (resolved == null) return;
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(new org.bukkit.event.world.WorldLoadEvent(resolved)); }
    }
    /** Fires Bukkit's {@code PlayerLevelChangeEvent}; returns the level a listener selected. */
    public static int firePlayerLevelChange(String id, String name, int oldLevel, int newLevel) {
        var event = new org.bukkit.event.player.PlayerLevelChangeEvent(ensurePlayer(id, name), oldLevel, newLevel);
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(event); }
        return event.getNewLevel();
    }
    /** Fires Bukkit's {@code PlayerExpChangeEvent}; returns the amount a listener selected. */
    public static int firePlayerExpChange(String id, String name, int amount) {
        var event = new org.bukkit.event.player.PlayerExpChangeEvent(ensurePlayer(id, name), amount);
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(event); }
        return event.getAmount();
    }
    /** Fires Paper's {@code InventoryClickEvent}; false means a listener cancelled the click. */
    public static boolean fireInventoryClick(String id, String name, int slot, String click, String action) {
        var player = ensurePlayer(id, name);
        var view = openView(player);
        org.bukkit.event.inventory.ClickType clickType;
        try { clickType = org.bukkit.event.inventory.ClickType.valueOf(click); }
        catch (IllegalArgumentException e) { clickType = org.bukkit.event.inventory.ClickType.UNKNOWN; }
        org.bukkit.event.inventory.InventoryAction inventoryAction;
        try { inventoryAction = org.bukkit.event.inventory.InventoryAction.valueOf(action); }
        catch (IllegalArgumentException e) { inventoryAction = org.bukkit.event.inventory.InventoryAction.UNKNOWN; }
        var event = new org.bukkit.event.inventory.InventoryClickEvent(view, org.bukkit.event.inventory.InventoryType.SlotType.CONTAINER, slot, clickType, inventoryAction);
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(event); }
        return !event.isCancelled();
    }
    /**
     * Fires Bukkit's {@code InventoryDragEvent} once the host applied a drag; false means a
     * listener cancelled it and the host restores the menu from the start of the drag.
     *
     * <p>The host sends the cursor fields as {@code material<U+001D>amount} and each affected
     * slot as {@code slot<U+001D>material<U+001D>amount}, entries separated by U+001E.
     */
    public static boolean fireInventoryDrag(String id, String name, String oldCursor, String newCursor, String items) {
        var player = ensurePlayer(id, name);
        var view = openView(player);
        var changed = new java.util.HashMap<Integer, org.bukkit.inventory.ItemStack>();
        for (String field : items.split("\u001e", -1)) {
            if (field.isEmpty()) continue;
            var parts = field.split("\u001d", -1);
            if (parts.length != 3) throw new UnsupportedOperationException("Ferrite: malformed drag slot: " + field);
            changed.put(Integer.parseInt(parts[0]), dragItem(parts[1], parts[2]));
        }
        var event = new org.bukkit.event.inventory.InventoryDragEvent(view, cursorItem(newCursor), cursorItem(oldCursor), false, changed);
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(event); }
        return !event.isCancelled();
    }
    /** A drag cursor: {@code material<U+001D>amount}, or null when the cursor is empty. */
    private static org.bukkit.inventory.ItemStack cursorItem(String field) {
        if (field.isEmpty()) return null;
        var parts = field.split("\u001d", -1);
        return dragItem(parts[0], parts.length > 1 ? parts[1] : "");
    }
    /** One item the host named in a drag event. */
    private static org.bukkit.inventory.ItemStack dragItem(String material, String amount) {
        if (material.isEmpty()) return null;
        var type = org.bukkit.Material.matchMaterial(material);
        if (type == null) throw new UnsupportedOperationException("Ferrite: unknown material in drag event: " + material);
        return new org.bukkit.inventory.ItemStack(type, amount.isEmpty() ? 1 : Integer.parseInt(amount));
    }
    /** Fires Bukkit's {@code InventoryCloseEvent} when the host closes a player's menu. */
    public static void fireInventoryClose(String id, String name) {
        var player = ensurePlayer(id, name);
        var view = openView(player);
        try { synchronized (LEGACY_LOCK) { server.plugins.callEvent(new org.bukkit.event.inventory.InventoryCloseEvent(view, org.bukkit.event.inventory.InventoryCloseEvent.Reason.PLUGIN)); } }
        finally { if (player instanceof FerritePlayer ferrite) ferrite.clearPluginView(); }
    }
    /** Fires {@code PlayerResourcePackStatusEvent} for the client's answer to a pushed pack. */
    public static void fireResourcePackStatus(String id, String name, String pack, int action) {
        var player = ensurePlayer(id, name);
        var statuses = org.bukkit.event.player.PlayerResourcePackStatusEvent.Status.values();
        if (action < 0 || action >= statuses.length) return;
        synchronized (LEGACY_LOCK) {
            server.plugins.callEvent(new org.bukkit.event.player.PlayerResourcePackStatusEvent(player, java.util.UUID.fromString(pack), statuses[action]));
        }
    }
    /** What the player is looking at: the inventory a plugin opened for them, or their own. */
    private static org.bukkit.inventory.InventoryView openView(org.bukkit.entity.Player player) {
        if (player instanceof FerritePlayer ferrite && ferrite.pluginView() != null) return ferrite.pluginView();
        return FerriteInventoryView.create(player, player.getInventory());
    }
    /** A live player, or a stand-in entity for the mob ids the host sends. */
    private static org.bukkit.entity.Entity entity(String id, String name) {
        var uuid = java.util.UUID.fromString(id);
        var player = server.getPlayer(uuid);
        return player != null ? player : ensurePlayer(id, name);
    }
    /** The generic damage source for host events that only carry a cause name. */
    private static org.bukkit.damage.DamageSource damageSource() {
        return org.bukkit.damage.DamageSource.builder(org.bukkit.damage.DamageType.GENERIC_KILL).build();
    }
    /** Bukkit's damage cause for the host's lowercase names, defaulting to CUSTOM. */
    private static org.bukkit.event.entity.EntityDamageEvent.DamageCause damageCause(String name) {
        for (var cause : org.bukkit.event.entity.EntityDamageEvent.DamageCause.values()) {
            if (cause.name().equalsIgnoreCase(name) || cause.name().replace('_', '-').equalsIgnoreCase(name)) return cause;
        }
        return org.bukkit.event.entity.EntityDamageEvent.DamageCause.CUSTOM;
    }
    /** Lines of {@code name\taliases\tpermission\tusage} for every enabled plugin command. */
    public static String pluginCommands() {
        StringBuilder out = new StringBuilder();
        for (org.bukkit.plugin.Plugin plugin : server.plugins.getPlugins()) {
            if (!plugin.isEnabled()) continue;
            for (var made : FerriteLifecycleManager.registeredCommands(plugin))
                out.append(made.label()).append('\t').append(String.join(",", made.aliases())).append('\t').append(made.permission()).append('\t').append('\n');
            var commands = plugin.getDescription().getCommands();
            if (commands == null) continue;
            for (var entry : commands.entrySet()) {
                var properties = entry.getValue() == null ? java.util.Collections.<String, Object>emptyMap() : entry.getValue();
                Object aliases = properties.get("aliases");
                String aliasList = "";
                if (aliases instanceof java.util.Collection<?> list) aliasList = list.stream().map(String::valueOf).collect(java.util.stream.Collectors.joining(","));
                else if (aliases instanceof String alias) aliasList = alias;
                out.append(entry.getKey()).append('\t').append(aliasList).append('\t').append(properties.getOrDefault("permission", "")).append('\t').append(properties.getOrDefault("usage", "")).append('\n');
            }
        }
        return out.toString();
    }
    /** Runs a declared plugin command; 0 = unknown, 1 = handled, 2 = no permission. */
    public static int runPluginCommand(String sender, String label, String[] args) {
        org.bukkit.command.CommandSender resolved;
        if ("console".equals(sender)) resolved = server.getConsoleSender();
        else {
            try { resolved = server.getPlayer(java.util.UUID.fromString(sender)); }
            catch (IllegalArgumentException e) { return 0; }
        }
        if (resolved == null) return 0;
        var command = server.getPluginCommand(label);
        if (command == null) return 0;
        COMMAND_LOCK.lock();
        try {
            synchronized (LEGACY_LOCK) {
                if (!command.testPermission(resolved)) return 2;
                return command.execute(resolved, label, args) ? 1 : 0;
            }
        } finally {
            COMMAND_LOCK.unlock();
        }
    }
}
