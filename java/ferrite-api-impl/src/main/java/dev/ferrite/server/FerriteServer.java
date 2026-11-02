package dev.ferrite.server;

public class FerriteServer extends ServerStubs {
    public record Settings(String motd, int maxPlayers, int port, int viewDistance, boolean whitelist, boolean onlineMode, boolean allowFlight, boolean allowNether, boolean allowEnd, boolean generateStructures, int maxWorldSize, int simulationDistance, int spawnRadius, int idleTimeout, int pauseWhenEmptyTime, int connectionThrottle) {
        public Settings(String motd, int maxPlayers, int port, int viewDistance, boolean whitelist, boolean onlineMode) { this(motd, maxPlayers, port, viewDistance, whitelist, onlineMode, false, true, true, true, 29999984, 10, 10, 0, -1, -1); }
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
            default -> throw new UnsupportedOperationException("Ferrite: not implemented: OfflinePlayer#" + method.getName());
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
    @Override public org.bukkit.inventory.Inventory createInventory(org.bukkit.inventory.InventoryHolder holder, int size) { return FerriteInventory.create(holder, org.bukkit.event.inventory.InventoryType.CHEST, size); }
    @Override public org.bukkit.inventory.Inventory createInventory(org.bukkit.inventory.InventoryHolder holder, int size, String title) { java.util.Objects.requireNonNull(title); return createInventory(holder, size); }
    @Override public org.bukkit.inventory.Inventory createInventory(org.bukkit.inventory.InventoryHolder holder, int size, net.kyori.adventure.text.Component title) { java.util.Objects.requireNonNull(title); return createInventory(holder, size); }
    @Override public org.bukkit.inventory.Inventory createInventory(org.bukkit.inventory.InventoryHolder holder, org.bukkit.event.inventory.InventoryType type) { return FerriteInventory.create(holder, type, type.getDefaultSize()); }
    @Override public org.bukkit.inventory.Inventory createInventory(org.bukkit.inventory.InventoryHolder holder, org.bukkit.event.inventory.InventoryType type, String title) { java.util.Objects.requireNonNull(title); return createInventory(holder, type); }
    @Override public org.bukkit.inventory.Inventory createInventory(org.bukkit.inventory.InventoryHolder holder, org.bukkit.event.inventory.InventoryType type, net.kyori.adventure.text.Component title) { java.util.Objects.requireNonNull(title); return createInventory(holder, type); }
    private final org.bukkit.scoreboard.ScoreboardManager scoreboards = new FerriteScoreboards();
    @Override public org.bukkit.scoreboard.ScoreboardManager getScoreboardManager() { return scoreboards; }
    private final FerriteIpBanList ipBans = new FerriteIpBanList();
    void seedIpBan(String ip) { ipBans.seed(ip); }
    @SuppressWarnings("unchecked") @Override public <T extends org.bukkit.BanList<?>> T getBanList(org.bukkit.BanList.Type type) { if (type != org.bukkit.BanList.Type.IP) throw new UnsupportedOperationException("Ferrite: profile bans not implemented"); return (T) ipBans; }
    @SuppressWarnings("unchecked") @Override public <B extends org.bukkit.BanList<E>, E> B getBanList(io.papermc.paper.ban.BanListType<B> type) { if (type != io.papermc.paper.ban.BanListType.IP) throw new UnsupportedOperationException("Ferrite: profile bans not implemented"); return (B) ipBans; }
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
            throw new UnsupportedOperationException("Ferrite: not implemented: ConsoleCommandSender#" + method.getName());
        });
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
    @Override public boolean removeBossBar(org.bukkit.NamespacedKey key) { java.util.Objects.requireNonNull(key, "key"); return bossBars.remove(key) != null; }
    @Override public java.util.Iterator<org.bukkit.inventory.Recipe> recipeIterator() { return FerriteRecipes.entries().stream().map(recipe -> (org.bukkit.inventory.Recipe) recipe).toList().iterator(); }
    @Override public org.bukkit.inventory.Recipe getRecipe(org.bukkit.NamespacedKey key) { java.util.Objects.requireNonNull(key, "key"); for (var entry : FerriteRecipes.entries()) if (entry.key().equals(key)) return entry; return null; }
    @Override public boolean hasRecipe(org.bukkit.NamespacedKey key) { return getRecipe(key) != null; }
    @Override public java.util.List<org.bukkit.inventory.Recipe> getRecipesFor(org.bukkit.inventory.ItemStack result) {
        java.util.Objects.requireNonNull(result, "result");
        if (result.getType() == org.bukkit.Material.AIR) return java.util.List.of();
        var matches = new java.util.ArrayList<org.bukkit.inventory.Recipe>();
        for (var entry : FerriteRecipes.entries()) if (entry.result().getType() == result.getType()) matches.add(entry);
        return java.util.List.copyOf(matches);
    }
    @SuppressWarnings("unchecked")
    @Override public <T extends org.bukkit.Keyed> java.lang.Iterable<org.bukkit.Tag<T>> getTags(String registry, Class<T> type) {
        java.util.Objects.requireNonNull(registry, "registry");
        java.util.Objects.requireNonNull(type, "type");
        if (type != org.bukkit.Material.class) throw new UnsupportedOperationException("Ferrite: getTags is backed only for Material, not " + type.getName());
        return (java.lang.Iterable<org.bukkit.Tag<T>>) (java.lang.Iterable<?>) FerriteTags.tags(registry);
    }
    private final FerriteRegistryAccess registries = new FerriteRegistryAccess();
    @Override public <T extends org.bukkit.Keyed> org.bukkit.Registry<T> getRegistry(Class<T> type) { return registries.getRegistry(type); }
    private final org.bukkit.inventory.ItemFactory itemFactory = new FerriteItemFactory();
    @Override public org.bukkit.inventory.ItemFactory getItemFactory() { return itemFactory; }
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
}
