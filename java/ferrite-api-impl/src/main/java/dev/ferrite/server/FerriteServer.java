package dev.ferrite.server;

public class FerriteServer extends ServerStubs {
    public record Settings(String motd, int maxPlayers, int port, int viewDistance, boolean whitelist, boolean onlineMode, boolean allowFlight, boolean allowNether, boolean allowEnd, boolean generateStructures, int maxWorldSize, int simulationDistance, int spawnRadius, int idleTimeout, int pauseWhenEmptyTime, int connectionThrottle) {
        /** The host has one distance setting and sends it as both view and simulation distance. */
        public Settings(String motd, int maxPlayers, int port, int viewDistance, boolean whitelist, boolean onlineMode) { this(motd, maxPlayers, port, viewDistance, whitelist, onlineMode, false, true, true, true, 29999984, viewDistance, 10, 0, -1, -1); }
    }
    private final Settings settings;
    private volatile String motd;
    private volatile int maxPlayers;
    private volatile org.bukkit.GameMode defaultGameMode = org.bukkit.GameMode.SURVIVAL;
    private volatile boolean whitelistEnforced;
    private volatile int spawnRadius;
    private volatile int idleTimeout;
    private volatile int pauseWhenEmptyTime;
    private volatile boolean stopping;
    private volatile String shutdownMessage;
    volatile String pluginsFolder;
    public FerriteServer() { this(new Settings("Ferrite", 20, 25565, 10, false, true)); }
    protected FerriteServer(Settings settings) { this.settings = settings; this.whitelistEnabled = settings.whitelist(); this.whitelistEnforced = settings.whitelist(); this.motd = settings.motd(); this.maxPlayers = settings.maxPlayers(); this.spawnRadius = settings.spawnRadius(); this.idleTimeout = settings.idleTimeout(); this.pauseWhenEmptyTime = settings.pauseWhenEmptyTime(); this.shutdownMessage = System.getProperty("ferrite.shutdownMessage"); facade.paperPluginManager = plugins; }
    /** Marks the server as shutting down so plugins polling {@link #isStopping()} stop work. */
    void beginShutdown() { stopping = true; }
    @Override public String getMotd() { return motd; }
    @Override public void setMotd(String value) { motd = java.util.Objects.requireNonNull(value, "motd"); }
    @Override public net.kyori.adventure.text.Component motd() { return net.kyori.adventure.text.Component.text(motd); }
    @Override public void motd(net.kyori.adventure.text.Component value) { motd = net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(java.util.Objects.requireNonNull(value, "motd")); }
    @Override public int getMaxPlayers() { return maxPlayers; }
    @Override public void setMaxPlayers(int value) { maxPlayers = value; }
    @Override public int getPort() { return settings.port(); }
    @Override public int getViewDistance() { return settings.viewDistance(); }
    @Override public boolean getOnlineMode() { return settings.onlineMode(); }
    @Override public boolean hasWhitelist() { return whitelistEnabled; }
    private final java.util.concurrent.ConcurrentMap<java.util.UUID, org.bukkit.World> worlds = new java.util.concurrent.ConcurrentHashMap<>();
    public void registerWorld(org.bukkit.World world) { worlds.put(world.getUID(), world); }
    public void unregisterWorld(java.util.UUID id) { worlds.remove(id); }
    @Override public java.util.List<org.bukkit.World> getWorlds() { return java.util.List.copyOf(worlds.values()); }
    @Override public org.bukkit.World getWorld(java.util.UUID id) { return worlds.get(id); }
    @Override public org.bukkit.World getWorld(String name) { return worlds.values().stream().filter(world -> world.getName().equalsIgnoreCase(name)).findFirst().orElse(null); }
    @Override public org.bukkit.World getWorld(net.kyori.adventure.key.Key key) { return worlds.values().stream().filter(world -> world.key().equals(key)).findFirst().orElse(null); }
    private final java.util.concurrent.ConcurrentMap<java.util.UUID, org.bukkit.entity.Player> players = new java.util.concurrent.ConcurrentHashMap<>();
    public void registerPlayer(org.bukkit.entity.Player player) { players.put(player.getUniqueId(), player); }
    public void unregisterPlayer(java.util.UUID id) { players.remove(id); }
    @Override public org.bukkit.entity.Player getPlayer(java.util.UUID id) { return players.get(id); }
    @Override public org.bukkit.entity.Player getPlayerExact(String name) {
        java.util.Objects.requireNonNull(name, "name");
        return players.values().stream().filter(p -> p.getName().equalsIgnoreCase(name)).findFirst().orElse(null);
    }
    @Override public org.bukkit.entity.Player getPlayer(String name) {
        java.util.Objects.requireNonNull(name, "name");
        var exact = getPlayerExact(name);
        if (exact != null) return exact;
        String prefix = name.toLowerCase(java.util.Locale.ROOT);
        return players.values().stream().filter(p -> p.getName().toLowerCase(java.util.Locale.ROOT).startsWith(prefix)).min(java.util.Comparator.comparingInt(p -> p.getName().length())).orElse(null);
    }
    private final org.bukkit.plugin.messaging.Messenger messenger = new org.bukkit.plugin.messaging.StandardMessenger();
    @Override public org.bukkit.plugin.messaging.Messenger getMessenger() { return messenger; }
    @Override public java.util.Set<String> getListeningPluginChannels() { return messenger.getIncomingChannels(); }
    private final java.util.concurrent.ConcurrentMap<java.util.UUID, org.bukkit.OfflinePlayer> offlinePlayers = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.concurrent.ConcurrentMap<java.util.UUID, String> offlineNames = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.concurrent.ConcurrentMap<String, java.util.UUID> offlineIds = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.Set<java.util.UUID> operators = java.util.concurrent.ConcurrentHashMap.newKeySet();
    /** Remember a player seen while offline so plugins can resolve its name and id. */
    public void registerOfflinePlayer(java.util.UUID id, String name, boolean operator) {
        java.util.Objects.requireNonNull(id, "id");
        if (name != null) { offlineNames.put(id, name); offlineIds.put(name.toLowerCase(java.util.Locale.ROOT), id); }
        if (operator) operators.add(id); else operators.remove(id);
    }
    private org.bukkit.OfflinePlayer offline(java.util.UUID id, String name) {
        return offlinePlayers.computeIfAbsent(id, key -> (org.bukkit.OfflinePlayer) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.OfflinePlayer.class}, (proxy, method, args) -> switch (method.getName()) {
            case "getUniqueId" -> key;
            case "getName" -> offlineNames.getOrDefault(key, name);
            case "getPlayer" -> players.get(key);
            case "isOnline" -> players.containsKey(key);
            case "isOp" -> operators.contains(key);
            case "setOp" -> { boolean value = (boolean) args[0]; if (!Native.playerOperator(key.toString(), value)) throw new IllegalStateException("No player command handler"); if (value) operators.add(key); else operators.remove(key); yield null; }
            case "hashCode" -> key.hashCode();
            case "equals" -> args[0] instanceof org.bukkit.OfflinePlayer other && key.equals(other.getUniqueId());
            case "toString" -> "OfflinePlayer[" + key + "]";
            default -> HostBridge.dispatch("OfflinePlayer", key, method, args);
        }));
    }
    @Override public org.bukkit.OfflinePlayer getOfflinePlayer(java.util.UUID id) { var player = players.get(id); return player != null ? player : offline(id, offlineNames.get(id)); }
    @Override public org.bukkit.OfflinePlayer getOfflinePlayer(String name) {
        java.util.Objects.requireNonNull(name, "name");
        var player = getPlayerExact(name);
        if (player != null) return player;
        var known = offlineIds.get(name.toLowerCase(java.util.Locale.ROOT));
        if (known != null) return offline(known, name);
        return offline(java.util.UUID.nameUUIDFromBytes(("OfflinePlayer:" + name).getBytes(java.nio.charset.StandardCharsets.UTF_8)), name);
    }
    @Override public java.util.Set<org.bukkit.OfflinePlayer> getOperators() {
        var result = new java.util.HashSet<org.bukkit.OfflinePlayer>();
        for (var id : operators) result.add(getOfflinePlayer(id));
        for (var player : players.values()) if (player.isOp()) result.add(player);
        return java.util.Set.copyOf(result);
    }
    /** Vanilla objective criteria: only these are written by plugins, everything else is maintained by the game. */
    @Override public org.bukkit.scoreboard.Criteria getScoreboardCriteria(String name) {
        java.util.Objects.requireNonNull(name, "name");
        boolean writable = java.util.Set.of("dummy", "trigger", "deathCount", "playerKillCount", "totalKillCount").contains(name);
        var render = name.equals("health") ? org.bukkit.scoreboard.RenderType.HEARTS : org.bukkit.scoreboard.RenderType.INTEGER;
        return new org.bukkit.scoreboard.Criteria() {
            @Override public String getName() { return name; }
            @Override public boolean isReadOnly() { return !writable; }
            @Override public org.bukkit.scoreboard.RenderType getDefaultRenderType() { return render; }
            @Override public boolean equals(Object other) { return other instanceof org.bukkit.scoreboard.Criteria c && c.getName().equals(name); }
            @Override public int hashCode() { return name.hashCode(); }
            @Override public String toString() { return "Criteria[" + name + "]"; }
        };
    }
    @Override public org.bukkit.inventory.Inventory createInventory(org.bukkit.inventory.InventoryHolder holder, int size) { return FerriteInventory.create(holder, org.bukkit.event.inventory.InventoryType.CHEST, size); }
    @Override public org.bukkit.inventory.Inventory createInventory(org.bukkit.inventory.InventoryHolder holder, int size, String title) { java.util.Objects.requireNonNull(title); return FerriteInventory.create(holder, org.bukkit.event.inventory.InventoryType.CHEST, size, title); }
    @Override public org.bukkit.inventory.Inventory createInventory(org.bukkit.inventory.InventoryHolder holder, int size, net.kyori.adventure.text.Component title) { java.util.Objects.requireNonNull(title); return FerriteInventory.create(holder, org.bukkit.event.inventory.InventoryType.CHEST, size, net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(title)); }
    @Override public org.bukkit.inventory.Inventory createInventory(org.bukkit.inventory.InventoryHolder holder, org.bukkit.event.inventory.InventoryType type) { return FerriteInventory.create(holder, type, type.getDefaultSize()); }
    @Override public org.bukkit.inventory.Inventory createInventory(org.bukkit.inventory.InventoryHolder holder, org.bukkit.event.inventory.InventoryType type, String title) { java.util.Objects.requireNonNull(title); return FerriteInventory.create(holder, type, type.getDefaultSize(), title); }
    @Override public org.bukkit.inventory.Inventory createInventory(org.bukkit.inventory.InventoryHolder holder, org.bukkit.event.inventory.InventoryType type, net.kyori.adventure.text.Component title) { java.util.Objects.requireNonNull(title); return FerriteInventory.create(holder, type, type.getDefaultSize(), net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(title)); }
    private final org.bukkit.scoreboard.ScoreboardManager scoreboards = new FerriteScoreboards();
    @Override public org.bukkit.scoreboard.ScoreboardManager getScoreboardManager() { return scoreboards; }
    private final FerriteIpBanList ipBans = new FerriteIpBanList();
    private final FerriteProfileBanList profileBans = new FerriteProfileBanList();
    void seedIpBan(String ip) { ipBans.seed(ip); }
    @SuppressWarnings("unchecked") @Override public <T extends org.bukkit.BanList<?>> T getBanList(org.bukkit.BanList.Type type) { return (T) (type == org.bukkit.BanList.Type.IP ? (org.bukkit.BanList<?>) ipBans : profileBans); }
    @SuppressWarnings("unchecked") @Override public <B extends org.bukkit.BanList<E>, E> B getBanList(io.papermc.paper.ban.BanListType<B> type) { return (B) (type == io.papermc.paper.ban.BanListType.IP ? (org.bukkit.BanList<?>) ipBans : profileBans); }
    private final FerritePotionBrewer potionBrewer = new FerritePotionBrewer();
    @Override public org.bukkit.potion.PotionBrewer getPotionBrewer() { return potionBrewer; }
    private final FerriteServerConfig serverConfig = new FerriteServerConfig();
    @Override public io.papermc.paper.configuration.ServerConfiguration getServerConfig() { return serverConfig; }
    private final FerriteMobGoals mobGoals = new FerriteMobGoals();
    @Override public com.destroystokyo.paper.entity.ai.MobGoals getMobGoals() { return mobGoals; }
    private final FerriteDatapackManager datapackManager = new FerriteDatapackManager();
    @Override public io.papermc.paper.datapack.DatapackManager getDatapackManager() { return datapackManager; }
    private final FerriteStructureManager structureManager = new FerriteStructureManager();
    @Override public org.bukkit.structure.StructureManager getStructureManager() { return structureManager; }
    private final FerriteEntityFactory entityFactory = new FerriteEntityFactory();
    @Override public org.bukkit.entity.EntityFactory getEntityFactory() { return entityFactory; }
    @Override public void banIP(String address) { ipBans.addBan(address, null, (java.util.Date) null, null); }
    @Override public void banIP(java.net.InetAddress address) { ipBans.addBan(address, null, (java.util.Date) null, null); }
    @Override public void unbanIP(String address) { ipBans.pardon(address); }
    @Override public void unbanIP(java.net.InetAddress address) { ipBans.pardon(address); }
    @Override public java.util.Set<String> getIPBans() { return ipBans.addresses(); }
    private volatile boolean whitelistEnabled;
    @Override public void setWhitelist(boolean value) { if (!Native.serverWhitelist(value)) throw new IllegalStateException("No server command handler"); whitelistEnabled = value; }
    private final Thread primaryThread = Thread.currentThread();
    private final org.bukkit.plugin.ServicesManager services = new org.bukkit.plugin.SimpleServicesManager();
    @Override public org.bukkit.plugin.ServicesManager getServicesManager() { return services; }
    @Override public net.kyori.adventure.text.Component permissionMessage() { return net.kyori.adventure.text.Component.text("You do not have permission to use this command."); }
    @Override public String getPermissionMessage() { return net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer.legacySection().serialize(permissionMessage()); }
    @Override public org.bukkit.util.CachedServerIcon loadServerIcon(java.io.File file) {
        if (file == null) throw new IllegalArgumentException("file must not be null");
        try {
            var image = javax.imageio.ImageIO.read(file);
            if (image == null) throw new IllegalArgumentException("unsupported server icon image");
            return loadServerIcon(image);
        } catch (java.io.IOException error) {
            throw new IllegalArgumentException("cannot read server icon", error);
        }
    }
    @Override public org.bukkit.util.CachedServerIcon loadServerIcon(java.awt.image.BufferedImage image) {
        if (image == null || image.getWidth() != 64 || image.getHeight() != 64)
            throw new IllegalArgumentException("server icon must be a 64x64 image");
        try {
            var bytes = new java.io.ByteArrayOutputStream();
            if (!javax.imageio.ImageIO.write(image, "png", bytes)) throw new IllegalArgumentException("PNG encoder unavailable");
            String data = "data:image/png;base64," + java.util.Base64.getEncoder().encodeToString(bytes.toByteArray());
            return () -> data;
        } catch (java.io.IOException error) {
            throw new IllegalArgumentException("cannot encode server icon", error);
        }
    }
    @Override public boolean isPrimaryThread() { return Thread.currentThread() == primaryThread || Thread.holdsLock(Bootstrap.LEGACY_LOCK); }
    private final java.util.Map<String, org.bukkit.command.Command> commands = new java.util.HashMap<>();
    private final org.bukkit.command.SimpleCommandMap commandMap = new org.bukkit.command.SimpleCommandMap(this, commands);
    @Override public org.bukkit.command.CommandMap getCommandMap() { return commandMap; }
    void registerCommands(org.bukkit.plugin.java.JavaPlugin plugin) throws ReflectiveOperationException {
        var constructor = org.bukkit.command.PluginCommand.class.getDeclaredConstructor(String.class, org.bukkit.plugin.Plugin.class);
        constructor.setAccessible(true);
        if (plugin.getDescription().getCommands() == null) return;
        for (String name : plugin.getDescription().getCommands().keySet()) {
            var command = constructor.newInstance(name, plugin);
            var properties = plugin.getDescription().getCommands().get(name);
            Object permission = properties.get("permission");
            if (permission instanceof String value) command.setPermission(value);
            commands.putIfAbsent(name.toLowerCase(java.util.Locale.ROOT), command);
            commands.put(plugin.getName().toLowerCase(java.util.Locale.ROOT) + ":" + name.toLowerCase(java.util.Locale.ROOT), command);
            Object aliases = properties.get("aliases");
            java.util.List<String> names = new java.util.ArrayList<>();
            if (aliases instanceof String alias) names.add(alias);
            else if (aliases instanceof java.util.List<?> list) for (Object alias : list) if (alias instanceof String value) names.add(value);
            command.setAliases(names);
            for (String alias : names) {
                String key = alias.toLowerCase(java.util.Locale.ROOT);
                commands.putIfAbsent(key, command);
                commands.put(plugin.getName().toLowerCase(java.util.Locale.ROOT) + ":" + key, command);
            }
        }
    }
    /**
     * Registers a lifecycle-made plugin command: {@code plugin:label} always, and each free name (label, aliases) as
     * well. Returns the plain names that were free; every map key added goes to {@code keysOut} for later removal.
     */
    java.util.Set<String> registerPluginCommand(org.bukkit.plugin.Plugin plugin, String label, String description, java.util.List<String> aliases,
            org.bukkit.command.CommandExecutor executor, org.bukkit.command.TabCompleter completer, String permission, java.util.List<String> keysOut) {
        try {
            var constructor = org.bukkit.command.PluginCommand.class.getDeclaredConstructor(String.class, org.bukkit.plugin.Plugin.class);
            constructor.setAccessible(true);
            var command = constructor.newInstance(label, plugin);
            command.setExecutor(executor);
            command.setTabCompleter(completer);
            if (permission != null && !permission.isEmpty()) command.setPermission(permission);
            if (description != null) command.setDescription(description);
            command.setAliases(aliases);
            var accepted = new java.util.LinkedHashSet<String>();
            var prefix = plugin.getName().toLowerCase(java.util.Locale.ROOT) + ":";
            var names = new java.util.ArrayList<String>();
            names.add(label);
            names.addAll(aliases);
            synchronized (commands) {
                for (var name : names) {
                    var key = name.toLowerCase(java.util.Locale.ROOT);
                    if (commands.putIfAbsent(key, command) == null) { accepted.add(key); keysOut.add(key); }
                    commands.put(prefix + key, command);
                    keysOut.add(prefix + key);
                }
            }
            return accepted;
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("Cannot create a plugin command", error);
        }
    }
    void removeCommandKeys(java.util.List<String> keys) { synchronized (commands) { for (var key : keys) commands.remove(key); } }
    void unregisterCommands(org.bukkit.plugin.Plugin plugin) { commands.values().removeIf(command -> command instanceof org.bukkit.command.PluginCommand pc && pc.getPlugin() == plugin); }
    @Override public org.bukkit.command.PluginCommand getPluginCommand(String name) { var command = commands.get(name.toLowerCase(java.util.Locale.ROOT)); return command instanceof org.bukkit.command.PluginCommand pc ? pc : null; }
    @Override public boolean dispatchCommand(org.bukkit.command.CommandSender sender, String line) {
        return commandMap.dispatch(sender, line);
    }
    private final org.bukkit.command.ConsoleCommandSender console = createConsoleSender();
    @Override public org.bukkit.command.ConsoleCommandSender getConsoleSender() { return console; }
    private org.bukkit.command.ConsoleCommandSender createConsoleSender() {
        return (org.bukkit.command.ConsoleCommandSender) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.command.ConsoleCommandSender.class}, (proxy, method, args) -> {
            if (method.getName().equals("sendMessage") && args != null) { for (Object arg : args) if (arg instanceof String message) getLogger().info(message); return null; }
            if (method.getName().equals("equals")) return proxy == args[0];
            if (method.getName().equals("hashCode")) return System.identityHashCode(proxy);
            if (method.getName().equals("toString")) return "ConsoleCommandSender";
            if (method.getName().equals("getName")) return "CONSOLE";
            if (method.getName().equals("getServer")) return this;
            if (method.getName().equals("isOp") || method.getName().equals("hasPermission")) return true;
            return HostBridge.dispatch("ConsoleCommandSender", "console", method, args);
        });
    }
    @Override public org.bukkit.command.CommandSender createCommandSender(java.util.function.Consumer<? super net.kyori.adventure.text.Component> feedback) {
        java.util.Objects.requireNonNull(feedback, "feedback");
        return (org.bukkit.command.CommandSender) java.lang.reflect.Proxy.newProxyInstance(
            getClass().getClassLoader(), new Class<?>[]{org.bukkit.command.CommandSender.class},
            (proxy, method, args) -> {
                switch (method.getName()) {
                    case "sendMessage", "sendRawMessage" -> {
                        if (args != null) for (var arg : args) redirectFeedback(feedback, arg);
                        return null;
                    }
                    case "equals" -> { return proxy == args[0]; }
                    case "hashCode" -> { return System.identityHashCode(proxy); }
                    case "toString" -> { return "RedirectedCommandSender"; }
                    default -> {
                        try { return method.invoke(console, args); }
                        catch (java.lang.reflect.InvocationTargetException error) { throw error.getCause(); }
                    }
                }
            });
    }
    private static void redirectFeedback(java.util.function.Consumer<? super net.kyori.adventure.text.Component> feedback, Object value) {
        if (value instanceof net.kyori.adventure.text.Component component) feedback.accept(component);
        else if (value instanceof net.kyori.adventure.text.ComponentLike like) feedback.accept(like.asComponent());
        else if (value instanceof net.kyori.adventure.chat.SignedMessage message) feedback.accept(message.unsignedContent() != null ? message.unsignedContent() : net.kyori.adventure.text.Component.text(message.message()));
        else if (value instanceof net.md_5.bungee.api.chat.BaseComponent[] components) feedback.accept(net.kyori.adventure.text.serializer.gson.GsonComponentSerializer.gson().deserialize(net.md_5.bungee.chat.ComponentSerializer.toString(components)));
        else if (value instanceof net.md_5.bungee.api.chat.BaseComponent component) feedback.accept(net.kyori.adventure.text.serializer.gson.GsonComponentSerializer.gson().deserialize(net.md_5.bungee.chat.ComponentSerializer.toString(component)));
        else if (value instanceof String text) feedback.accept(net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer.legacySection().deserialize(text));
        else if (value instanceof Object[] values) for (var item : values) redirectFeedback(feedback, item);
        else if (!(value instanceof java.util.UUID) && !(value instanceof net.kyori.adventure.identity.Identity) && !(value instanceof net.kyori.adventure.chat.ChatType.Bound))
            throw new IllegalArgumentException("Unsupported command feedback: " + value);
    }
    final FerriteEntitySchedulers entitySchedulers = new FerriteEntitySchedulers();
    public io.papermc.paper.threadedregions.scheduler.EntityScheduler entityScheduler(java.util.UUID id, java.util.function.BooleanSupplier alive) { return entitySchedulers.forEntity(id, alive); }
    final FerriteAsyncScheduler asyncScheduler = new FerriteAsyncScheduler();
    @Override public io.papermc.paper.threadedregions.scheduler.AsyncScheduler getAsyncScheduler() { return asyncScheduler; }
    final FerriteRegionScheduler regionScheduler = new FerriteRegionScheduler();
    @Override public io.papermc.paper.threadedregions.scheduler.RegionScheduler getRegionScheduler() { return regionScheduler; }
    @Override public boolean isOwnedByCurrentRegion(org.bukkit.World world, int x, int z) { return Native.isRegionThread(world.getUID().toString(), x, z); }
    @Override public boolean isOwnedByCurrentRegion(org.bukkit.World world, io.papermc.paper.math.Position pos) { return isOwnedByCurrentRegion(world, pos.blockX() >> 4, pos.blockZ() >> 4); }
    @Override public boolean isOwnedByCurrentRegion(org.bukkit.World world, io.papermc.paper.math.Position pos, int radius) { return isOwnedByCurrentRegion(world, pos.blockX() >> 4, pos.blockZ() >> 4, radius); }
    @Override public boolean isOwnedByCurrentRegion(org.bukkit.Location location) { return isOwnedByCurrentRegion(java.util.Objects.requireNonNull(location.getWorld()), location.getBlockX() >> 4, location.getBlockZ() >> 4); }
    @Override public boolean isOwnedByCurrentRegion(org.bukkit.Location location, int radius) { return isOwnedByCurrentRegion(java.util.Objects.requireNonNull(location.getWorld()), location.getBlockX() >> 4, location.getBlockZ() >> 4, radius); }
    @Override public boolean isOwnedByCurrentRegion(org.bukkit.World world, int x, int z, int radius) { if (radius < 0) throw new IllegalArgumentException("radius"); long minX = (long) x - radius, minZ = (long) z - radius, maxX = (long) x + radius, maxZ = (long) z + radius; if (minX < Integer.MIN_VALUE || minZ < Integer.MIN_VALUE || maxX > Integer.MAX_VALUE || maxZ > Integer.MAX_VALUE) return false; return isOwnedByCurrentRegion(world, (int) minX, (int) minZ, (int) maxX, (int) maxZ); }
    @Override public boolean isOwnedByCurrentRegion(org.bukkit.World world, int minX, int minZ, int maxX, int maxZ) { if (minX > maxX || minZ > maxZ) throw new IllegalArgumentException("invalid range"); for (long x = minX; x <= maxX; x++) for (long z = minZ; z <= maxZ; z++) if (!isOwnedByCurrentRegion(world, (int) x, (int) z)) return false; return true; }
    @Override public boolean isOwnedByCurrentRegion(org.bukkit.entity.Entity entity) { return entitySchedulers.isOwned(entity.getUniqueId()); }
    final FerriteScheduler scheduler = new FerriteScheduler();
    private final FerriteGlobalScheduler globalScheduler = new FerriteGlobalScheduler(scheduler);
    @Override public io.papermc.paper.threadedregions.scheduler.GlobalRegionScheduler getGlobalRegionScheduler() { return globalScheduler; }
    @Override public org.bukkit.scheduler.BukkitScheduler getScheduler() { return scheduler; }
    final FerritePluginManager plugins = new FerritePluginManager(this);
    final org.bukkit.plugin.SimplePluginManager facade = new org.bukkit.plugin.SimplePluginManager(this, commandMap);
    @Override public org.bukkit.plugin.PluginManager getPluginManager() { return facade; }
    @Override public String getName() { return "Ferrite"; }
    @Override public String getVersion() { return "Ferrite 0.1.0"; }
    @Override public String getBukkitVersion() { return "26.3"; }
    @Override public String getMinecraftVersion() { return "26.3"; }
    @Override public java.util.logging.Logger getLogger() { return java.util.logging.Logger.getLogger("Ferrite"); }
    @Override public java.util.Collection<? extends org.bukkit.entity.Player> getOnlinePlayers() { return java.util.List.copyOf(players.values()); }
    /** The console and every online player, the audiences a server-wide message reaches. */
    @Override public java.lang.Iterable<? extends net.kyori.adventure.audience.Audience> audiences() {
        var audiences = new java.util.ArrayList<net.kyori.adventure.audience.Audience>(players.size() + 1);
        audiences.add(console);
        audiences.addAll(players.values());
        return java.util.List.copyOf(audiences);
    }
    @Override public org.bukkit.GameMode getDefaultGameMode() { return defaultGameMode; }
    @Override public void setDefaultGameMode(org.bukkit.GameMode mode) { defaultGameMode = java.util.Objects.requireNonNull(mode, "mode"); }
    @Override public boolean getAllowFlight() { return settings.allowFlight(); }
    @Override public boolean getAllowNether() { return settings.allowNether(); }
    @Override public boolean getAllowEnd() { return settings.allowEnd(); }
    @Override public boolean getGenerateStructures() { return settings.generateStructures(); }
    @Override public int getMaxWorldSize() { return settings.maxWorldSize(); }
    @Override public int getSimulationDistance() { return settings.simulationDistance(); }
    @Override public int getSpawnRadius() { return spawnRadius; }
    @Override public void setSpawnRadius(int value) { if (value < 0) throw new IllegalArgumentException("spawn radius must not be negative"); spawnRadius = value; }
    @Override public int getIdleTimeout() { return idleTimeout; }
    @Override public void setIdleTimeout(int value) { if (value < 0) throw new IllegalArgumentException("idle timeout must not be negative"); idleTimeout = value; }
    @Override public int getPauseWhenEmptyTime() { return pauseWhenEmptyTime; }
    @Override public void setPauseWhenEmptyTime(int value) { if (value < -1) throw new IllegalArgumentException("pause time must be -1 or more"); pauseWhenEmptyTime = value; }
    @Override public long getConnectionThrottle() { return settings.connectionThrottle(); }
    @Override public boolean isWhitelistEnforced() { return whitelistEnforced; }
    @Override public void setWhitelistEnforced(boolean value) { whitelistEnforced = value; }
    @Override public boolean isStopping() { return stopping; }
    @Override public String getShutdownMessage() { return shutdownMessage; }
    @Override public net.kyori.adventure.text.Component shutdownMessage() { return shutdownMessage == null ? null : net.kyori.adventure.text.Component.text(shutdownMessage); }
    /** Ticks the host scheduler has run, from the shared counter the scheduler tasks use. */
    @Override public int getCurrentTick() { return (int) Native.currentTick(); }
    @Override public int broadcast(net.kyori.adventure.text.Component message, String permission) {
        java.util.Objects.requireNonNull(message, "message");
        int sent = 0;
        for (var player : players.values()) {
            if (permission != null && !player.hasPermission(permission)) continue;
            player.sendMessage(message);
            sent++;
        }
        return sent;
    }
    @Override public java.util.List<org.bukkit.entity.Player> matchPlayer(String name) {
        java.util.Objects.requireNonNull(name, "name");
        String prefix = name.toLowerCase(java.util.Locale.ROOT);
        return players.values().stream().filter(player -> player.getName().toLowerCase(java.util.Locale.ROOT).startsWith(prefix)).toList();
    }
    @Override public java.util.UUID getPlayerUniqueId(String name) {
        var player = getPlayerExact(java.util.Objects.requireNonNull(name, "name"));
        return player == null ? null : player.getUniqueId();
    }
    @Override public org.bukkit.OfflinePlayer getOfflinePlayerIfCached(String name) {
        java.util.Objects.requireNonNull(name, "name");
        var player = getPlayerExact(name);
        if (player != null) return player;
        var known = offlineIds.get(name.toLowerCase(java.util.Locale.ROOT));
        return known == null ? null : offline(known, offlineNames.get(known));
    }
    /** Only players are bridged; other entity kinds are not tracked by the host yet. */
    @Override public org.bukkit.entity.Entity getEntity(java.util.UUID id) { return players.get(java.util.Objects.requireNonNull(id, "id")); }
    @Override public java.io.File getPluginsFolder() { return new java.io.File(pluginsFolder == null ? "plugins" : pluginsFolder); }
    @Override public String getIp() { return Bootstrap.serverAddress(); }
    /** Hardcore is not a host setting yet. */
    @Override public boolean isHardcore() { return false; }
    @Override public boolean isLoggingIPs() { return true; }
    /** Online mode requires Mojang-signed profiles, so it also enforces secure profiles. */
    @Override public boolean isEnforcingSecureProfiles() { return settings.onlineMode(); }
    @Override public boolean isAcceptingTransfers() { return false; }
    @Override public boolean getHideOnlinePlayers() { return false; }
    /** Every join is forced onto the default game mode by the host's login packet. */
    @Override public boolean forcesDefaultGameMode() { return false; }
    @Override public java.io.File getWorldContainer() { return new java.io.File(".").getAbsoluteFile().toPath().normalize().toFile(); }
    @Override public java.nio.file.Path getLevelDirectory() { return getWorldContainer().toPath(); }
    @Override public String getUpdateFolder() { return "update"; }
    @Override public java.io.File getUpdateFolderFile() { return new java.io.File(getWorldContainer(), getUpdateFolder()); }
    @Override public org.bukkit.Warning.WarningState getWarningState() { return org.bukkit.Warning.WarningState.DEFAULT; }
    @Override public boolean isPaused() { return false; }
    @Override public boolean isTickingWorlds() { return !stopping; }
    @Override public boolean isGlobalTickThread() { return isPrimaryThread(); }
    /** The host persists player data on quit and shutdown; there is no separate flush. */
    @Override public void savePlayers() {}
    @Override public int getMaxChainedNeighborUpdates() { return 1_000_000; }
    @Override public java.util.List<String> getInitialEnabledPacks() { return java.util.List.of("vanilla"); }
    @Override public java.util.List<String> getInitialDisabledPacks() { return java.util.List.of(); }
    @Override public String getResourcePack() { return null; }
    @Override public String getResourcePackHash() { return null; }
    @Override public String getResourcePackPrompt() { return null; }
    @Override public boolean isResourcePackRequired() { return false; }
    @Override public org.bukkit.packs.ResourcePack getServerResourcePack() { return null; }
    private final java.util.concurrent.ConcurrentMap<org.bukkit.NamespacedKey, org.bukkit.boss.KeyedBossBar> bossBars = new java.util.concurrent.ConcurrentHashMap<>();
    private final java.util.concurrent.atomic.AtomicLong nextBossBar = new java.util.concurrent.atomic.AtomicLong(1);
    @Override public org.bukkit.boss.BossBar createBossBar(String title, org.bukkit.boss.BarColor color, org.bukkit.boss.BarStyle style, org.bukkit.boss.BarFlag... flags) { return createBossBar(new org.bukkit.NamespacedKey("ferrite", "bossbar_" + nextBossBar.getAndIncrement()), title, color, style, flags); }
    @Override public org.bukkit.boss.KeyedBossBar createBossBar(org.bukkit.NamespacedKey key, String title, org.bukkit.boss.BarColor color, org.bukkit.boss.BarStyle style, org.bukkit.boss.BarFlag... flags) {
        java.util.Objects.requireNonNull(key, "key");
        var bar = new FerriteBossBar(key, title, color, style, flags);
        if (bossBars.putIfAbsent(key, bar) != null) throw new IllegalArgumentException("Boss bar already exists: " + key);
        return bar;
    }
    @Override public java.util.Iterator<org.bukkit.boss.KeyedBossBar> getBossBars() { return java.util.List.copyOf(bossBars.values()).iterator(); }
    @Override public org.bukkit.boss.KeyedBossBar getBossBar(org.bukkit.NamespacedKey key) { java.util.Objects.requireNonNull(key, "key"); return bossBars.get(key); }
    @Override public boolean removeBossBar(org.bukkit.NamespacedKey key) {
        java.util.Objects.requireNonNull(key, "key");
        var removed = bossBars.remove(key);
        // Hide it for every viewer before dropping the registry entry so clients do not keep a stale bar.
        if (removed instanceof FerriteBossBar bar) bar.removeAll();
        return removed != null;
    }
    @Override public java.util.Iterator<org.bukkit.inventory.Recipe> recipeIterator() { return FerriteRecipes.all().iterator(); }
    @Override public org.bukkit.inventory.Recipe getRecipe(org.bukkit.NamespacedKey key) { java.util.Objects.requireNonNull(key, "key"); return FerriteRecipes.get(key); }
    @Override public boolean hasRecipe(org.bukkit.NamespacedKey key) { return getRecipe(key) != null; }
    @Override public org.bukkit.generator.ChunkGenerator.ChunkData createChunkData(org.bukkit.World world) {
        java.util.Objects.requireNonNull(world, "world");
        return new FerriteChunkData(world.getMinHeight(), world.getMaxHeight());
    }
    @Override public org.bukkit.inventory.Recipe getCraftingRecipe(org.bukkit.inventory.ItemStack[] craftingMatrix, org.bukkit.World world) { return FerriteCrafting.recipe(craftingMatrix); }
    @Override public org.bukkit.inventory.ItemCraftResult craftItemResult(org.bukkit.inventory.ItemStack[] craftingMatrix, org.bukkit.World world) { return FerriteCrafting.craft(craftingMatrix); }
    @Override public org.bukkit.inventory.ItemCraftResult craftItemResult(org.bukkit.inventory.ItemStack[] craftingMatrix, org.bukkit.World world, org.bukkit.entity.Player player) { return FerriteCrafting.craft(craftingMatrix); }
    @Override public java.util.List<org.bukkit.inventory.Recipe> getRecipesFor(org.bukkit.inventory.ItemStack result) {
        java.util.Objects.requireNonNull(result, "result");
        if (result.getType() == org.bukkit.Material.AIR) return java.util.List.of();
        var matches = new java.util.ArrayList<org.bukkit.inventory.Recipe>();
        for (var recipe : FerriteRecipes.all()) if (recipe.getResult().getType() == result.getType()) matches.add(recipe);
        return java.util.List.copyOf(matches);
    }
    @Override public boolean addRecipe(org.bukkit.inventory.Recipe recipe) { java.util.Objects.requireNonNull(recipe, "recipe"); return FerriteRecipes.add(recipe); }
    @Override public boolean removeRecipe(org.bukkit.NamespacedKey key) { java.util.Objects.requireNonNull(key, "key"); return FerriteRecipes.remove(key); }
    @Override public void clearRecipes() { FerriteRecipes.clear(); }
    @Override public void resetRecipes() { FerriteRecipes.reset(); }
    @Override public void updateRecipes() { }
    @Override public org.bukkit.loot.LootTable getLootTable(org.bukkit.NamespacedKey key) { return FerriteLootTable.get(java.util.Objects.requireNonNull(key, "key")); }
    @Override public org.bukkit.advancement.Advancement getAdvancement(org.bukkit.NamespacedKey key) { return FerriteAdvancements.get(java.util.Objects.requireNonNull(key, "key")); }
    @Override public java.util.Iterator<org.bukkit.advancement.Advancement> advancementIterator() { return FerriteAdvancements.iterator(); }
    @Override public org.bukkit.ServerLinks getServerLinks() { return serverLinks; }
    final FerriteServerLinks serverLinks = new FerriteServerLinks();
    @Override public org.bukkit.ServerTickManager getServerTickManager() { return tickManager; }
    final FerriteTickManager tickManager = new FerriteTickManager();
    /** Resolves one tag from the same generated block/item tables {@link #getTags} exposes. */
    @SuppressWarnings("unchecked")
    @Override public <T extends org.bukkit.Keyed> org.bukkit.Tag<T> getTag(String registry, org.bukkit.NamespacedKey tag, Class<T> type) {
        java.util.Objects.requireNonNull(registry, "registry");
        java.util.Objects.requireNonNull(tag, "tag");
        java.util.Objects.requireNonNull(type, "type");
        for (var candidate : FerriteTags.tags(registry, type)) if (candidate.getKey().equals(tag)) return candidate;
        return null;
    }
    @SuppressWarnings("unchecked")
    @Override public <T extends org.bukkit.Keyed> java.lang.Iterable<org.bukkit.Tag<T>> getTags(String registry, Class<T> type) {
        java.util.Objects.requireNonNull(registry, "registry");
        java.util.Objects.requireNonNull(type, "type");
        return FerriteTags.tags(registry, type);
    }
    private final FerriteRegistryAccess registries = new FerriteRegistryAccess();
    @Override public <T extends org.bukkit.Keyed> org.bukkit.Registry<T> getRegistry(Class<T> type) { return registries.getRegistry(type); }
    private final org.bukkit.inventory.ItemFactory itemFactory = new FerriteItemFactory();
    @Override public org.bukkit.inventory.ItemFactory getItemFactory() { return itemFactory; }
    /** The host reloads asynchronously; plugins' {@code COMMANDS} handlers run again with cause RELOAD once the request is sent. */
    @Override public void reloadData() {
        super.reloadData();
        FerriteLifecycleManager.fireCommands(io.papermc.paper.plugin.lifecycle.event.registrar.ReloadableRegistrarEvent.Cause.RELOAD);
    }
    @Override public void reloadPermissions() { facade.permissions.values().forEach(plugins::recalculatePermissionDefaults); }
    // ---- plugin-facing runtime surface kept from the fb4 bridge -----------------------------
    private final FerriteUnsafe unsafe = new FerriteUnsafe();
    @Override public org.bukkit.UnsafeValues getUnsafe() { return unsafe; }
    @Override public org.bukkit.block.data.BlockData createBlockData(String data) { return FerriteBlockData.create(data); }
    @Override public org.bukkit.block.data.BlockData createBlockData(org.bukkit.Material material) { return FerriteBlockData.create(material); }
    @Override public org.bukkit.block.data.BlockData createBlockData(org.bukkit.Material material, String data) {
        java.util.Objects.requireNonNull(material, "material");
        if (data != null && !data.isEmpty()) throw new UnsupportedOperationException("Ferrite: block-state properties are owned by the Rust server: " + data);
        return FerriteBlockData.create(material);
    }
    @Override public org.bukkit.block.data.BlockData createBlockData(org.bukkit.Material material, java.util.function.Consumer<? super org.bukkit.block.data.BlockData> consumer) {
        var data = FerriteBlockData.create(material);
        consumer.accept(data);
        return data;
    }
    private static final double[] NOMINAL_TPS = {20.0, 20.0, 20.0};
    /** Nominal until per-region tick metrics are exposed to the bridge (see docs/BACKLOG.md). */
    @Override public double[] getTPS() { return NOMINAL_TPS.clone(); }
    @Override public double getAverageTickTime() { return 50.0; }
    @Override public long[] getTickTimes() { return new long[100]; }
    @Override public java.util.Set<org.bukkit.OfflinePlayer> getWhitelistedPlayers() { return java.util.Set.of(); }
    @Override public java.util.Set<org.bukkit.OfflinePlayer> getBannedPlayers() { return java.util.Set.of(); }
    @Override public String getWorldType() { return "normal"; }
    /** Commands registered by plugins; the plain names map to their declared aliases. */
    @Override public java.util.Map<String, String[]> getCommandAliases() {
        var aliases = new java.util.LinkedHashMap<String, String[]>();
        for (var command : commands.values()) {
            if (!(command instanceof org.bukkit.command.PluginCommand)) continue;
            aliases.putIfAbsent(command.getName(), command.getAliases().toArray(String[]::new));
        }
        return java.util.Map.copyOf(aliases);
    }
    private volatile org.bukkit.World respawnWorld;
    @Override public org.bukkit.World getRespawnWorld() { var world = respawnWorld; return world != null ? world : getWorlds().stream().findFirst().orElse(null); }
    @Override public void setRespawnWorld(org.bukkit.World world) { respawnWorld = world; }
    private volatile org.bukkit.help.HelpMap helpMap;
    /** A help map over the plugin command map; topics are generated from registered commands. */
    @Override public org.bukkit.help.HelpMap getHelpMap() {
        var map = helpMap;
        if (map == null) synchronized (this) { map = helpMap; if (map == null) helpMap = map = new FerriteHelpMap(this); }
        return map;
    }
    /** Every player name the host has seen, even while disconnected. */
    @Override public org.bukkit.OfflinePlayer[] getOfflinePlayers() {
        return offlineIds.values().stream().distinct().map(this::getOfflinePlayer).toArray(org.bukkit.OfflinePlayer[]::new);
    }
    @Override public int getSpawnLimit(org.bukkit.entity.SpawnCategory category) {
        java.util.Objects.requireNonNull(category, "category");
        return switch (category) {
            case MONSTER -> 70;
            case ANIMAL -> 10;
            case WATER_ANIMAL -> 5;
            case WATER_AMBIENT -> 20;
            case WATER_UNDERGROUND_CREATURE -> 5;
            case AMBIENT -> 15;
            case AXOLOTL -> 5;
            case MISC -> -1;
        };
    }
    @Override public int getTicksPerSpawns(org.bukkit.entity.SpawnCategory category) { java.util.Objects.requireNonNull(category, "category"); return 1; }
    @Override public org.bukkit.WorldBorder createWorldBorder() { return new FerriteWorldBorder(); }
    @Override public org.bukkit.profile.PlayerProfile createPlayerProfile(String name) { return new FerriteProfile(null, name); }
    @Override public org.bukkit.profile.PlayerProfile createPlayerProfile(java.util.UUID id) { return new FerriteProfile(id, null); }
    @Override public org.bukkit.profile.PlayerProfile createPlayerProfile(java.util.UUID id, String name) { return new FerriteProfile(id, name); }
    @Override public com.destroystokyo.paper.profile.PlayerProfile createProfile(String name) { return new FerriteProfile(null, name); }
    @Override public com.destroystokyo.paper.profile.PlayerProfile createProfile(java.util.UUID id) { return new FerriteProfile(id, null); }
    @Override public com.destroystokyo.paper.profile.PlayerProfile createProfile(java.util.UUID id, String name) { return new FerriteProfile(id, name); }
    @Override public com.destroystokyo.paper.profile.PlayerProfile createProfileExact(java.util.UUID id, String name) { return new FerriteProfile(id, name); }
    private final java.util.concurrent.ConcurrentMap<Integer, org.bukkit.map.MapView> maps = new java.util.concurrent.ConcurrentHashMap<>();
    @Override public org.bukkit.inventory.Merchant createMerchant() { return new FerriteMerchant(); }
    @Override public org.bukkit.inventory.Merchant createMerchant(String title) { return new FerriteMerchant(title); }
    @Override public org.bukkit.inventory.Merchant createMerchant(net.kyori.adventure.text.Component title) { return new FerriteMerchant(title); }
    @Override public org.bukkit.map.MapView createMap(org.bukkit.World world) {
        var view = new FerriteMapView(java.util.Objects.requireNonNull(world, "world"));
        maps.put(view.getId(), view);
        return view;
    }
    @Override public org.bukkit.map.MapView getMap(int id) { return maps.get(id); }
    @Override public org.bukkit.inventory.ItemStack createExplorerMap(org.bukkit.World world, org.bukkit.Location origin, org.bukkit.StructureType type, int radius, boolean findUnexplored) {
        var wanted = new java.util.ArrayList<org.bukkit.generator.structure.Structure>();
        for (var structure : org.bukkit.Registry.STRUCTURE) {
            var path = structure.getKey().getKey();
            if (path.equals(type.getName()) || path.startsWith(type.getName() + "_")) wanted.add(structure);
        }
        return explorerMap(world, origin, wanted, radius, findUnexplored);
    }
    @Override public org.bukkit.inventory.ItemStack createExplorerMap(org.bukkit.World world, org.bukkit.Location origin, org.bukkit.generator.structure.StructureType type, org.bukkit.map.MapCursor.Type cursor, int radius, boolean findUnexplored) {
        var wanted = new java.util.ArrayList<org.bukkit.generator.structure.Structure>();
        for (var structure : org.bukkit.Registry.STRUCTURE) if (type.equals(structure.getStructureType())) wanted.add(structure);
        return explorerMap(world, origin, wanted, radius, findUnexplored);
    }
    @Override public org.bukkit.inventory.ItemStack createExplorerMap(org.bukkit.World world, org.bukkit.Location origin, org.bukkit.generator.structure.Structure structure, org.bukkit.map.MapCursor.Type cursor, int radius, boolean findUnexplored) {
        return explorerMap(world, origin, java.util.List.of(structure), radius, findUnexplored);
    }
    /** A filled map centred on the nearest of the structures, or null when the host finds none. The terrain is not rendered and no marker cursor is drawn. */
    private org.bukkit.inventory.ItemStack explorerMap(org.bukkit.World world, org.bukkit.Location origin, java.util.List<org.bukkit.generator.structure.Structure> wanted, int radius, boolean findUnexplored) {
        java.util.Objects.requireNonNull(world, "world");
        java.util.Objects.requireNonNull(origin, "origin");
        org.bukkit.Location best = null;
        for (var structure : wanted) {
            var found = world.locateNearestStructure(origin, structure, radius, findUnexplored);
            if (found != null && (best == null || found.getLocation().distanceSquared(origin) < best.distanceSquared(origin))) best = found.getLocation();
        }
        if (best == null) return null;
        var view = createMap(world);
        view.setCenterX(best.getBlockX());
        view.setCenterZ(best.getBlockZ());
        view.setScale(org.bukkit.map.MapView.Scale.FAR);
        view.setTrackingPosition(true);
        var item = new org.bukkit.inventory.ItemStack(org.bukkit.Material.FILLED_MAP);
        var meta = (org.bukkit.inventory.meta.MapMeta) item.getItemMeta();
        meta.setMapView(view);
        item.setItemMeta(meta);
        return item;
    }
    @Override public void sendPluginMessage(org.bukkit.plugin.Plugin source, String channel, byte[] message) {
        for (var player : players.values()) player.sendPluginMessage(source, channel, message);
    }
    @Override public org.bukkit.World createWorld(org.bukkit.WorldCreator creator) {
        java.util.Objects.requireNonNull(creator, "creator");
        var existing = getWorld(creator.name());
        if (existing == null) existing = getWorld(creator.key());
        if (existing != null) return existing;
        if (creator.generator() != null) {
            throw new UnsupportedOperationException("Ferrite: custom chunk generators are not supported (WorldCreator#generator must be null)");
        }
        if (!creator.generateStructures()) {
            throw new UnsupportedOperationException("Ferrite: disabling structure generation is not supported (WorldCreator#generateStructures must be true)");
        }
        if (creator.type() != org.bukkit.WorldType.NORMAL) {
            throw new UnsupportedOperationException("Ferrite: world type " + creator.type() + " is not supported (only WorldType.NORMAL)");
        }
        if (creator.environment() != org.bukkit.World.Environment.NORMAL && creator.environment() != org.bukkit.World.Environment.NETHER && creator.environment() != org.bukkit.World.Environment.THE_END) {
            throw new UnsupportedOperationException("Ferrite: world environment " + creator.environment() + " is not supported (NORMAL, NETHER or THE_END)");
        }
        // Paper's WorldCreator(String) already picked a random seed, so every creator carries one. The host
        // applies it only when it creates the world; an existing world answers with the seed stored on disk.
        String reply = Bootstrap.<String>awaitLoop(() -> (String) HostBridge.call("Server", "server", "createRuntimeWorld(String,String,long)", String.class, creator.name(), creator.environment().name(), creator.seed()));
        if (reply == null || reply.isBlank()) throw new IllegalStateException("Ferrite: runtime world creation failed");
        var parts = reply.split(String.valueOf(HostBridge.ITEM), -1);
        if (parts.length != 5) throw new IllegalStateException("Ferrite: invalid runtime world response");
        var world = FerriteWorld.create(this, java.util.UUID.fromString(parts[0]), creator.name(), 0L,
                Integer.parseInt(parts[1]), Integer.parseInt(parts[2]), Integer.parseInt(parts[3]));
        FerriteWorld.setSeed(world, Long.parseLong(parts[4]));
        registerWorld(world);
        synchronized (Bootstrap.LEGACY_LOCK) { plugins.callEvent(new org.bukkit.event.world.WorldLoadEvent(world)); }
        return world;
    }
    @Override public boolean unloadWorld(String name, boolean save) { var world = getWorld(name); return world != null && unloadWorld(world, save); }
    @Override public boolean unloadWorld(org.bukkit.World world, boolean save) {
        java.util.Objects.requireNonNull(world, "world");
        if (getWorld(world.getUID()) != world || getWorld("world") == world) return false;
        synchronized (Bootstrap.LEGACY_LOCK) {
            var event = new org.bukkit.event.world.WorldUnloadEvent(world);
            plugins.callEvent(event);
            if (event.isCancelled()) return false;
        }
        var reply = Bootstrap.<Object>awaitLoop(() -> HostBridge.call("Server", "server", "unloadRuntimeWorld(String,boolean)", Boolean.class, world.getUID().toString(), save));
        if (!Boolean.TRUE.equals(reply)) return false;
        unregisterWorld(world.getUID());
        return true;
    }
    @Override public void allowPausing(org.bukkit.plugin.Plugin plugin, boolean value) { java.util.Objects.requireNonNull(plugin, "plugin"); }
    @Override public org.bukkit.util.CachedServerIcon getServerIcon() { return null; }
    @Override public void restart() { HostBridge.callVoid("Server", "server", "shutdown()"); }
    @Override public java.util.List<org.bukkit.entity.Entity> selectEntities(org.bukkit.command.CommandSender sender, String selector) {
        java.util.Objects.requireNonNull(sender, "sender");
        java.util.Objects.requireNonNull(selector, "selector");
        var out = new java.util.ArrayList<org.bukkit.entity.Entity>();
        switch (selector) {
            case "@a", "@e" -> out.addAll(players.values());
            case "@p" -> { var from = sender instanceof org.bukkit.entity.Entity e ? e.getLocation() : null;
                players.values().stream().filter(p -> from == null || p.getWorld().equals(from.getWorld()))
                        .min(java.util.Comparator.comparingDouble(p -> from == null ? 0 : p.getLocation().distanceSquared(from))).ifPresent(out::add); }
            case "@r" -> { var all = new java.util.ArrayList<>(players.values()); if (!all.isEmpty()) out.add(all.get(java.util.concurrent.ThreadLocalRandom.current().nextInt(all.size()))); }
            case "@s" -> { if (sender instanceof org.bukkit.entity.Entity e) out.add(e); }
            default -> {
                if (selector.startsWith("@")) throw new IllegalArgumentException("Unsupported selector: " + selector);
                var named = getPlayerExact(selector);
                if (named != null) out.add(named);
            }
        }
        return out;
    }
    @Override public boolean addRecipes(Iterable<org.bukkit.inventory.Recipe> recipes) { boolean all = true; for (var recipe : recipes) all &= addRecipe(recipe); return all; }
    @Override public boolean removeRecipes(Iterable<org.bukkit.NamespacedKey> keys) { boolean all = true; for (var key : keys) all &= removeRecipe(key); return all; }
    @Override public boolean reloadCommandAliases() { return true; }
    @Override public boolean suggestPlayerNamesWhenNullTabCompletions() { return true; }
    @Override public void updateResources() { }
    private final org.bukkit.Server.Spigot spigot = new org.bukkit.Server.Spigot() {
        @Override public org.bukkit.configuration.file.YamlConfiguration getConfig() { return new org.bukkit.configuration.file.YamlConfiguration(); }
        @Override public org.bukkit.configuration.file.YamlConfiguration getBukkitConfig() { return new org.bukkit.configuration.file.YamlConfiguration(); }
        @Override public org.bukkit.configuration.file.YamlConfiguration getSpigotConfig() { return new org.bukkit.configuration.file.YamlConfiguration(); }
        @Override public org.bukkit.configuration.file.YamlConfiguration getPaperConfig() { return new org.bukkit.configuration.file.YamlConfiguration(); }
        @Override public void broadcast(net.md_5.bungee.api.chat.BaseComponent component) { broadcastMessage(net.md_5.bungee.api.chat.TextComponent.toLegacyText(component)); }
        @Override public void broadcast(net.md_5.bungee.api.chat.BaseComponent... components) { broadcastMessage(net.md_5.bungee.api.chat.TextComponent.toLegacyText(components)); }
        @Override public void restart() { FerriteServer.this.restart(); }
    };
    @Override public org.bukkit.Server.Spigot spigot() { return spigot; }
}
