package dev.ferrite.server;
public final class Bootstrap {
    static final Object LEGACY_LOCK = new Object();
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
        if (org.bukkit.Bukkit.getServer() != null) { server = (FerriteServer) org.bukkit.Bukkit.getServer(); return; }
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
        synchronized (LEGACY_LOCK) { return resolved != null && server.dispatchCommand(resolved, line); }
    }
    public static void tick() { synchronized (LEGACY_LOCK) { server.scheduler.tick(); Native.tickFallback(); } }
    public static void runGlobalTask(long id) { synchronized (LEGACY_LOCK) { server.scheduler.executeId(id); } }
    public static void runEntityTask(long id) { server.entitySchedulers.execute(id); }
    public static void runRegionTask(long id) { server.regionScheduler.execute(id); }
    public static void shutdown() { synchronized (LEGACY_LOCK) { server.beginShutdown(); server.getPluginManager().disablePlugins(); server.asyncScheduler.shutdown(); } }
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
    /** Fires Bukkit's {@code PlayerDeathEvent}; drops and experience stay host-owned. */
    public static void firePlayerDeath(String id, String name, String cause, int droppedExp, String message) {
        var player = ensurePlayer(id, name);
        var event = new org.bukkit.event.entity.PlayerDeathEvent(player, damageSource(), new java.util.ArrayList<org.bukkit.inventory.ItemStack>(), droppedExp, message == null ? null : net.kyori.adventure.text.Component.text(message), true);
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(event); }
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
        return event.isCancelled() ? 0.0 : event.getFinalDamage();
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
        var view = FerriteInventoryView.create(player, player.getInventory());
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
    /** Fires Bukkit's {@code InventoryCloseEvent} when the host closes a player's menu. */
    public static void fireInventoryClose(String id, String name) {
        var player = ensurePlayer(id, name);
        var view = FerriteInventoryView.create(player, player.getInventory());
        synchronized (LEGACY_LOCK) { server.plugins.callEvent(new org.bukkit.event.inventory.InventoryCloseEvent(view, org.bukkit.event.inventory.InventoryCloseEvent.Reason.PLUGIN)); }
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
        synchronized (LEGACY_LOCK) {
            if (!command.testPermission(resolved)) return 2;
            return command.execute(resolved, label, args) ? 1 : 0;
        }
    }
}
