package dev.ferrite.api.server;

import dev.ferrite.api.FerriteRuntime;
import dev.ferrite.api.block.FerriteBlockState;
import dev.ferrite.api.boss.FerriteBossBar;
import dev.ferrite.api.boss.FerriteKeyedBossBar;
import dev.ferrite.api.inventory.FerriteCustomInventory;
import dev.ferrite.api.scoreboard.FerriteCriteria;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import java.util.UUID;
import java.util.logging.Logger;
import net.kyori.adventure.key.Key;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.World;
import org.bukkit.block.data.BlockData;
import org.bukkit.boss.BarColor;
import org.bukkit.boss.BarFlag;
import org.bukkit.boss.BarStyle;
import org.bukkit.boss.BossBar;
import org.bukkit.boss.KeyedBossBar;
import org.bukkit.command.CommandMap;
import org.bukkit.command.ConsoleCommandSender;
import org.bukkit.command.PluginCommand;
import org.bukkit.entity.Player;
import org.bukkit.event.inventory.InventoryType;
import org.bukkit.help.HelpMap;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.InventoryHolder;
import org.bukkit.plugin.Plugin;
import org.bukkit.plugin.PluginManager;
import org.bukkit.plugin.messaging.Messenger;
import org.bukkit.scheduler.BukkitScheduler;
import org.bukkit.scoreboard.Criteria;
import org.bukkit.scoreboard.ScoreboardManager;

/**
 * The first {@link org.bukkit.Server} slice: identity, worlds, players, manager,
 * scheduler, console and command lookup.
 *
 * <p>Every other Server method inherits {@link UnsupportedServerBase} and fails with an
 * {@code UnsupportedOperationException} naming the method and plugin.
 */
public final class FerriteServer extends UnsupportedServerBase {

    private final FerriteRuntime runtime;
    private final Map<NamespacedKey, KeyedBossBar> keyedBossBars = new LinkedHashMap<>();
    private final Map<NamespacedKey, org.bukkit.inventory.Recipe> recipes =
            java.util.Collections.synchronizedMap(new LinkedHashMap<>());
    private final dev.ferrite.api.item.FerriteItemFactory itemFactory =
            new dev.ferrite.api.item.FerriteItemFactory();
    private final FerriteServerTickManager tickManager = new FerriteServerTickManager();
    private final Set<Plugin> pauseBlockingPlugins = java.util.concurrent.ConcurrentHashMap.newKeySet();
    private volatile boolean whitelistEnabled;
    private volatile boolean whitelistEnforced;
    private volatile org.bukkit.GameMode defaultGameMode = org.bukkit.GameMode.SURVIVAL;
    private volatile Component motd = Component.text("A Ferrite Minecraft Server");
    private volatile int viewDistance = 10;
    private volatile int simulationDistance = 10;
    private volatile boolean onlineMode = true;
    private volatile int pauseWhenEmptyTime = 60;
    private volatile World respawnWorld;

    public FerriteServer(FerriteRuntime runtime) {
        this.runtime = runtime;
    }

    @Override
    public String getName() {
        return runtime.serverName();
    }

    @Override
    public String getVersion() {
        return runtime.version();
    }

    @Override
    public String getBukkitVersion() {
        return runtime.bukkitVersion();
    }

    @Override
    public String getMinecraftVersion() {
        return runtime.minecraftVersion();
    }

    @Override
    public Logger getLogger() {
        return runtime.logger();
    }

    @Override
    public PluginManager getPluginManager() {
        return runtime.plugins();
    }

    @Override
    public org.bukkit.plugin.ServicesManager getServicesManager() {
        return runtime.servicesManager();
    }

    @Override
    public org.bukkit.OfflinePlayer getOfflinePlayer(UUID id) {
        Objects.requireNonNull(id, "id");
        Player online = getPlayer(id);
        if (online != null) {
            return online;
        }
        return new dev.ferrite.api.scoreboard.FerriteOfflinePlayer(id, null);
    }

    @Override
    @SuppressWarnings("deprecation")
    public org.bukkit.OfflinePlayer getOfflinePlayer(String name) {
        Objects.requireNonNull(name, "name");
        return dev.ferrite.api.scoreboard.FerriteOfflinePlayer.resolve(name);
    }

    @Override
    public org.bukkit.OfflinePlayer getOfflinePlayerIfCached(String name) {
        if (name == null) {
            return null;
        }
        return getPlayerExact(name);
    }

    @Override
    public BukkitScheduler getScheduler() {
        return runtime.scheduler();
    }

    @Override
    public io.papermc.paper.threadedregions.scheduler.RegionScheduler getRegionScheduler() {
        return runtime.regionScheduler();
    }

    @Override
    public io.papermc.paper.threadedregions.scheduler.GlobalRegionScheduler getGlobalRegionScheduler() {
        return runtime.globalRegionScheduler();
    }

    @Override
    public io.papermc.paper.threadedregions.scheduler.AsyncScheduler getAsyncScheduler() {
        return runtime.asyncScheduler();
    }

    @Override
    public boolean isOwnedByCurrentRegion(World world, io.papermc.paper.math.Position position) {
        Objects.requireNonNull(world, "world");
        Objects.requireNonNull(position, "position");
        return runtime.isPrimaryThread();
    }

    @Override
    public boolean isOwnedByCurrentRegion(
            World world, io.papermc.paper.math.Position position, int squareRadiusChunks) {
        Objects.requireNonNull(world, "world");
        Objects.requireNonNull(position, "position");
        return runtime.isPrimaryThread();
    }

    @Override
    public boolean isOwnedByCurrentRegion(org.bukkit.Location location) {
        Objects.requireNonNull(location, "location");
        return runtime.isPrimaryThread();
    }

    @Override
    public boolean isOwnedByCurrentRegion(org.bukkit.Location location, int squareRadiusChunks) {
        Objects.requireNonNull(location, "location");
        return runtime.isPrimaryThread();
    }

    @Override
    public boolean isOwnedByCurrentRegion(World world, int chunkX, int chunkZ) {
        Objects.requireNonNull(world, "world");
        return runtime.isPrimaryThread();
    }

    @Override
    public boolean isOwnedByCurrentRegion(World world, int chunkX, int chunkZ, int squareRadiusChunks) {
        Objects.requireNonNull(world, "world");
        return runtime.isPrimaryThread();
    }

    @Override
    public boolean isOwnedByCurrentRegion(
            World world, int minChunkX, int minChunkZ, int maxChunkX, int maxChunkZ) {
        Objects.requireNonNull(world, "world");
        return runtime.isPrimaryThread();
    }

    @Override
    public boolean isOwnedByCurrentRegion(org.bukkit.entity.Entity entity) {
        Objects.requireNonNull(entity, "entity");
        return runtime.isPrimaryThread();
    }

    @Override
    public boolean isGlobalTickThread() {
        return runtime.isPrimaryThread();
    }

    @Override
    public Iterable<? extends net.kyori.adventure.audience.Audience> audiences() {
        List<net.kyori.adventure.audience.Audience> audiences = new ArrayList<>();
        audiences.add(runtime.consoleSender());
        audiences.addAll(runtime.onlinePlayers());
        return List.copyOf(audiences);
    }

    @Override
    public List<World> getWorlds() {
        return new ArrayList<World>(runtime.worlds());
    }

    @Override
    public World getWorld(String name) {
        return runtime.world(name);
    }

    @Override
    public World getWorld(UUID uniqueId) {
        return runtime.world(uniqueId);
    }

    @Override
    public BlockData createBlockData(Material material) {
        Objects.requireNonNull(material, "material");
        if (!material.isBlock()) {
            throw new IllegalArgumentException("Material " + material + " is not a block");
        }
        String key = material.getKey().asString();
        return (BlockData) Proxy.newProxyInstance(
                BlockData.class.getClassLoader(),
                new Class<?>[] {BlockData.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "getMaterial" -> material;
                    case "getAsString" -> key;
                    case "matches" -> args != null
                            && args.length == 1
                            && args[0] instanceof BlockData other
                            && other.getMaterial() == material;
                    case "clone" -> proxy;
                    case "createBlockState" -> new FerriteBlockState(material, (BlockData) proxy);
                    case "equals" -> args != null
                            && args.length == 1
                            && args[0] instanceof BlockData other
                            && other.getMaterial() == material
                            && key.equals(other.getAsString());
                    case "hashCode" -> material.hashCode();
                    case "toString" -> "FerriteBlockData{" + key + "}";
                    // Only material/asString/equality are modelled; block state properties are not.
                    default -> throw new UnsupportedOperationException(
                            "Ferrite BlockData does not support " + method.getName());
                });
    }

    @Override
    public World getWorld(Key key) {
        String value = key.value();
        int colon = value.indexOf(':');
        return runtime.world(colon >= 0 ? value.substring(colon + 1) : value);
    }

    @Override
    public Player getPlayer(String name) {
        if (name == null) {
            return null;
        }
        String lower = name.toLowerCase(Locale.ROOT);
        for (Player player : runtime.onlinePlayers()) {
            if (player.getName().toLowerCase(Locale.ROOT).startsWith(lower)) {
                return player;
            }
        }
        return null;
    }

    @Override
    public Player getPlayerExact(String name) {
        return runtime.playerExact(name);
    }

    @Override
    public Player getPlayer(UUID uniqueId) {
        return runtime.player(uniqueId);
    }

    @Override
    public UUID getPlayerUniqueId(String name) {
        Player player = runtime.playerExact(name);
        return player == null ? null : player.getUniqueId();
    }

    @Override
    public Collection<? extends Player> getOnlinePlayers() {
        return List.copyOf(runtime.onlinePlayers());
    }

    @Override
    public int getMaxPlayers() {
        return runtime.maxPlayers();
    }

    @Override
    public int getPort() {
        return runtime.port();
    }

    @Override
    public boolean isPrimaryThread() {
        return runtime.isPrimaryThread();
    }

    @Override
    public ConsoleCommandSender getConsoleSender() {
        return runtime.consoleSender();
    }

    @Override
    public PluginCommand getPluginCommand(String name) {
        return runtime.commandDispatcher().get(name);
    }

    @Override
    public HelpMap getHelpMap() {
        return runtime.helpMap();
    }

    @Override
    public CommandMap getCommandMap() {
        return runtime.commandDispatcher().commandMap();
    }

    @Override
    public Messenger getMessenger() {
        return runtime.messenger();
    }

    @Override
    public void sendPluginMessage(Plugin source, String channel, byte[] message) {
        runtime.messenger().sendPluginMessage(source, null, channel, message);
    }

    @Override
    public Set<String> getListeningPluginChannels() {
        return runtime.messenger().getIncomingChannels();
    }

    @Override
    public Component permissionMessage() {
        return Component.text(
                "I'm sorry, but you do not have permission to perform this command. "
                        + "Please contact the server administrators if you believe that this is in error.");
    }

    @Override
    public boolean dispatchCommand(
            org.bukkit.command.CommandSender sender, String commandLine)
            throws org.bukkit.command.CommandException {
        Objects.requireNonNull(sender, "sender");
        Objects.requireNonNull(commandLine, "commandLine");
        return runtime.commandDispatcher().dispatch(sender, commandLine);
    }

    @Override
    public double[] getTPS() {
        return new double[] {20.0, 20.0, 20.0};
    }

    @Override
    public double getAverageTickTime() {
        return 0.5;
    }

    @Override
    public int broadcastMessage(String message) {
        int count = 0;
        for (Player player : runtime.onlinePlayers()) {
            player.sendMessage(message);
            count++;
        }
        runtime.consoleSender().sendMessage(message);
        try {
            Map<String, Object> req = new LinkedHashMap<>();
            req.put("message", message == null ? "" : message);
            dev.ferrite.host.Bootstrap.serverBroadcast(new com.google.gson.Gson().toJson(req));
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
        return count;
    }

    @Override
    public Inventory createInventory(InventoryHolder owner, InventoryType type) {
        return FerriteCustomInventory.createTyped(runtime, owner, type, null);
    }

    @Override
    public Inventory createInventory(InventoryHolder owner, InventoryType type, Component title) {
        return FerriteCustomInventory.createTyped(runtime, owner, type, title);
    }

    @Override
    public Inventory createInventory(InventoryHolder owner, InventoryType type, String title) {
        Component comp = title == null
                ? null
                : LegacyComponentSerializer.legacySection().deserialize(title);
        return FerriteCustomInventory.createTyped(runtime, owner, type, comp);
    }

    @Override
    public Inventory createInventory(InventoryHolder owner, int size) {
        return FerriteCustomInventory.createChest(runtime, owner, size, null);
    }

    @Override
    public Inventory createInventory(InventoryHolder owner, int size, Component title) {
        return FerriteCustomInventory.createChest(runtime, owner, size, title);
    }

    @Override
    public Inventory createInventory(InventoryHolder owner, int size, String title) {
        Component comp = title == null
                ? null
                : LegacyComponentSerializer.legacySection().deserialize(title);
        return FerriteCustomInventory.createChest(runtime, owner, size, comp);
    }

    @Override
    public ScoreboardManager getScoreboardManager() {
        return runtime.scoreboardManager();
    }

    @Override
    public Criteria getScoreboardCriteria(String name) {
        return FerriteCriteria.getOrCreate(name);
    }

    @Override
    public BossBar createBossBar(String title, BarColor color, BarStyle style, BarFlag... flags) {
        return new FerriteBossBar(title, color, style, flags);
    }

    @Override
    public KeyedBossBar createBossBar(
            NamespacedKey key, String title, BarColor color, BarStyle style, BarFlag... flags) {
        FerriteKeyedBossBar bossBar = new FerriteKeyedBossBar(key, title, color, style, flags);
        synchronized (keyedBossBars) {
            keyedBossBars.put(key, bossBar);
        }
        return bossBar;
    }

    @Override
    public Iterator<KeyedBossBar> getBossBars() {
        synchronized (keyedBossBars) {
            return List.copyOf(keyedBossBars.values()).iterator();
        }
    }

    @Override
    public KeyedBossBar getBossBar(NamespacedKey key) {
        if (key == null) {
            return null;
        }
        synchronized (keyedBossBars) {
            return keyedBossBars.get(key);
        }
    }

    @Override
    public boolean removeBossBar(NamespacedKey key) {
        if (key == null) {
            return false;
        }
        synchronized (keyedBossBars) {
            return keyedBossBars.remove(key) != null;
        }
    }

    @Override
    public Set<org.bukkit.OfflinePlayer> getOperators() {
        Set<org.bukkit.OfflinePlayer> ops = new java.util.LinkedHashSet<>();
        for (Player p : runtime.onlinePlayers()) {
            if (p.isOp()) {
                ops.add(p);
            }
        }
        ops.addAll(dev.ferrite.api.scoreboard.FerriteOfflinePlayer.offlineOperators());
        return ops;
    }

    @Override
    public Set<org.bukkit.OfflinePlayer> getWhitelistedPlayers() {
        return dev.ferrite.api.scoreboard.FerriteOfflinePlayer.whitelistedPlayers();
    }

    @Override
    public boolean hasWhitelist() {
        return whitelistEnabled;
    }

    @Override
    public void setWhitelist(boolean value) {
        this.whitelistEnabled = value;
    }

    @Override
    public void reloadWhitelist() {
        // State is kept in-memory/live; no-op reload succeeds cleanly.
    }

    @Override
    public void setMaxPlayers(int maxPlayers) {
        runtime.setMaxPlayers(maxPlayers);
    }

    @Override
    public int getViewDistance() {
        return viewDistance;
    }

    @Override
    public int getSimulationDistance() {
        return simulationDistance;
    }

    @Override
    public boolean getAllowEnd() {
        return true;
    }

    @Override
    public boolean getAllowNether() {
        return true;
    }

    @Override
    public boolean getOnlineMode() {
        return onlineMode;
    }

    @Override
    public org.bukkit.GameMode getDefaultGameMode() {
        return defaultGameMode;
    }

    @Override
    public void setDefaultGameMode(org.bukkit.GameMode mode) {
        this.defaultGameMode = Objects.requireNonNull(mode, "mode");
    }

    @Override
    public Component motd() {
        return motd;
    }

    @Override
    public void motd(Component motd) {
        this.motd = Objects.requireNonNull(motd, "motd");
    }

    @Override
    @SuppressWarnings("deprecation")
    public String getMotd() {
        return LegacyComponentSerializer.legacySection().serialize(motd);
    }

    @Override
    public int getCurrentTick() {
        return runtime.currentTick();
    }

    private final Set<String> ipBans = java.util.concurrent.ConcurrentHashMap.newKeySet();
    private final FerriteBanLists.FerriteIpBanList ipBanList =
            new FerriteBanLists.FerriteIpBanList(ipBans);
    private final FerriteBanLists.FerriteProfileBanList profileBanList =
            new FerriteBanLists.FerriteProfileBanList();
    private volatile int spawnRadius = 16;
    private volatile int idleTimeout = 0;

    @Override
    @SuppressWarnings("deprecation")
    public Set<String> getIPBans() {
        return java.util.Collections.unmodifiableSet(new java.util.LinkedHashSet<>(ipBans));
    }

    @Override
    @SuppressWarnings("deprecation")
    public void banIP(String address) {
        Objects.requireNonNull(address, "address");
        if (!address.isBlank()) {
            ipBanList.addBan(address.trim(), null, (java.util.Date) null, null);
        }
    }

    @Override
    @SuppressWarnings("deprecation")
    public void unbanIP(String address) {
        Objects.requireNonNull(address, "address");
        ipBanList.pardon(address.trim());
    }

    @Override
    @SuppressWarnings("deprecation")
    public void banIP(java.net.InetAddress address) {
        Objects.requireNonNull(address, "address");
        banIP(address.getHostAddress());
    }

    @Override
    @SuppressWarnings("deprecation")
    public void unbanIP(java.net.InetAddress address) {
        Objects.requireNonNull(address, "address");
        unbanIP(address.getHostAddress());
    }

    @Override
    public Set<org.bukkit.OfflinePlayer> getBannedPlayers() {
        Set<org.bukkit.OfflinePlayer> banned = new java.util.LinkedHashSet<>();
        for (org.bukkit.BanEntry<com.destroystokyo.paper.profile.PlayerProfile> entry :
                profileBanList.<org.bukkit.BanEntry<com.destroystokyo.paper.profile.PlayerProfile>>getEntries()) {
            var target = entry.getBanTarget();
            if (target != null && target.getId() != null) {
                banned.add(getOfflinePlayer(target.getId()));
            } else if (entry.getTarget() != null) {
                banned.add(getOfflinePlayer(entry.getTarget()));
            }
        }
        return java.util.Collections.unmodifiableSet(banned);
    }

    @Override
    public boolean getAllowFlight() {
        return true;
    }

    @Override
    public boolean isHardcore() {
        return false;
    }

    @Override
    public int getSpawnRadius() {
        return spawnRadius;
    }

    @Override
    public void setSpawnRadius(int value) {
        this.spawnRadius = Math.max(0, value);
    }

    @Override
    public int getIdleTimeout() {
        return idleTimeout;
    }

    @Override
    public void setIdleTimeout(int threshold) {
        this.idleTimeout = Math.max(0, threshold);
    }

    @Override
    public Map<String, String[]> getCommandAliases() {
        return java.util.Collections.emptyMap();
    }

    @Override
    public boolean reloadCommandAliases() {
        return true;
    }

    @Override
    public org.bukkit.inventory.ItemFactory getItemFactory() {
        return itemFactory;
    }

    @Override
    public java.io.File getPluginsFolder() {
        return new java.io.File("plugins");
    }

    @Override
    public String getUpdateFolder() {
        return "update";
    }

    @Override
    public java.io.File getUpdateFolderFile() {
        return new java.io.File(getPluginsFolder(), getUpdateFolder());
    }

    @Override
    public java.io.File getWorldContainer() {
        return new java.io.File(".");
    }

    @Override
    public java.nio.file.Path getLevelDirectory() {
        return java.nio.file.Path.of(".").toAbsolutePath().normalize();
    }

    @Override
    public String getIp() {
        return "";
    }

    @Override
    public boolean isLoggingIPs() {
        return true;
    }

    @Override
    public String getWorldType() {
        return "DEFAULT";
    }

    @Override
    public boolean getGenerateStructures() {
        return true;
    }

    @Override
    public int getMaxWorldSize() {
        return 29999984;
    }

    @Override
    public List<String> getInitialEnabledPacks() {
        return List.of("vanilla");
    }

    @Override
    public List<String> getInitialDisabledPacks() {
        return List.of();
    }

    @Override
    public org.bukkit.ServerTickManager getServerTickManager() {
        return tickManager;
    }

    @Override
    public boolean isWhitelistEnforced() {
        return whitelistEnforced;
    }

    @Override
    public void setWhitelistEnforced(boolean value) {
        this.whitelistEnforced = value;
    }

    @Override
    public long getConnectionThrottle() {
        return 4000L;
    }

    @Override
    public int getTicksPerSpawns(org.bukkit.entity.SpawnCategory spawnCategory) {
        Objects.requireNonNull(spawnCategory, "spawnCategory");
        return spawnCategory == org.bukkit.entity.SpawnCategory.ANIMAL ? 400 : 1;
    }

    @Override
    public int getSpawnLimit(org.bukkit.entity.SpawnCategory spawnCategory) {
        Objects.requireNonNull(spawnCategory, "spawnCategory");
        return switch (spawnCategory) {
            case MONSTER -> 70;
            case ANIMAL -> 10;
            case WATER_ANIMAL, WATER_UNDERGROUND_CREATURE, AXOLOTL -> 5;
            case WATER_AMBIENT -> 20;
            case AMBIENT -> 15;
            case MISC -> 0;
        };
    }

    @Override
    @SuppressWarnings("deprecation")
    public List<Player> matchPlayer(String name) {
        Objects.requireNonNull(name, "name");
        String lower = name.toLowerCase(Locale.ROOT);
        List<Player> matched = new ArrayList<>();
        for (Player player : runtime.onlinePlayers()) {
            String playerLower = player.getName().toLowerCase(Locale.ROOT);
            if (playerLower.equals(lower)) {
                return List.of(player);
            }
            if (playerLower.contains(lower)) {
                matched.add(player);
            }
        }
        return matched;
    }

    @Override
    public boolean isTickingWorlds() {
        return false;
    }

    @Override
    public World getRespawnWorld() {
        if (respawnWorld != null) {
            return respawnWorld;
        }
        List<World> worlds = getWorlds();
        return worlds.isEmpty() ? null : worlds.get(0);
    }

    @Override
    public void setRespawnWorld(World world) {
        this.respawnWorld = Objects.requireNonNull(world, "world");
    }

    @Override
    public org.bukkit.WorldBorder createWorldBorder() {
        return new dev.ferrite.api.world.FerriteWorldBorder();
    }

    @Override
    public boolean isEnforcingSecureProfiles() {
        return true;
    }

    @Override
    public boolean isAcceptingTransfers() {
        return false;
    }

    @Override
    public boolean getHideOnlinePlayers() {
        return false;
    }

    @Override
    public boolean forcesDefaultGameMode() {
        return false;
    }

    @Override
    public int broadcast(Component message, String permission) {
        Objects.requireNonNull(message, "message");
        int count = 0;
        for (Player player : runtime.onlinePlayers()) {
            if (permission == null || player.hasPermission(permission)) {
                player.sendMessage(message);
                count++;
            }
        }
        if (permission == null || runtime.consoleSender().hasPermission(permission)) {
            runtime.consoleSender().sendMessage(message);
        }
        return count;
    }

    @Override
    public org.bukkit.OfflinePlayer[] getOfflinePlayers() {
        Map<UUID, org.bukkit.OfflinePlayer> all = new LinkedHashMap<>();
        for (Player p : runtime.onlinePlayers()) {
            all.put(p.getUniqueId(), p);
        }
        for (org.bukkit.OfflinePlayer op : getOperators()) {
            all.putIfAbsent(op.getUniqueId(), op);
        }
        for (org.bukkit.OfflinePlayer wl : getWhitelistedPlayers()) {
            all.putIfAbsent(wl.getUniqueId(), wl);
        }
        return all.values().toArray(org.bukkit.OfflinePlayer[]::new);
    }

    @Override
    public int getMaxChainedNeighborUpdates() {
        return 1_000_000;
    }

    @Override
    @SuppressWarnings("deprecation")
    public void setMotd(String motd) {
        this.motd = motd == null
                ? Component.empty()
                : LegacyComponentSerializer.legacySection().deserialize(motd);
    }

    @Override
    public Component shutdownMessage() {
        return Component.text("Server closed");
    }

    @Override
    @SuppressWarnings("deprecation")
    public String getShutdownMessage() {
        return "Server closed";
    }

    @Override
    public org.bukkit.Warning.WarningState getWarningState() {
        return org.bukkit.Warning.WarningState.DEFAULT;
    }

    @Override
    public int getPauseWhenEmptyTime() {
        return pauseWhenEmptyTime;
    }

    @Override
    public void setPauseWhenEmptyTime(int seconds) {
        this.pauseWhenEmptyTime = Math.max(0, seconds);
    }

    @Override
    public org.bukkit.entity.Entity getEntity(UUID uuid) {
        Objects.requireNonNull(uuid, "uuid");
        Player online = getPlayer(uuid);
        if (online != null) {
            return online;
        }
        for (World world : getWorlds()) {
            org.bukkit.entity.Entity found = world.getEntity(uuid);
            if (found != null) {
                return found;
            }
        }
        return null;
    }

    @Override
    public long[] getTickTimes() {
        return new long[100];
    }

    @Override
    public boolean suggestPlayerNamesWhenNullTabCompletions() {
        return true;
    }

    @Override
    @SuppressWarnings("deprecation")
    public String getPermissionMessage() {
        return LegacyComponentSerializer.legacySection().serialize(permissionMessage());
    }

    @Override
    public boolean isStopping() {
        return runtime.isClosed();
    }

    @Override
    public boolean isPaused() {
        return false;
    }

    @Override
    public void allowPausing(Plugin plugin, boolean value) {
        Objects.requireNonNull(plugin, "plugin");
        if (value) {
            pauseBlockingPlugins.remove(plugin);
        } else {
            pauseBlockingPlugins.add(plugin);
        }
    }

    @Override
    public BlockData createBlockData(
            Material material, java.util.function.Consumer<? super BlockData> consumer) {
        BlockData data = createBlockData(material);
        if (consumer != null) {
            consumer.accept(data);
        }
        return data;
    }

    @Override
    public BlockData createBlockData(String data) throws IllegalArgumentException {
        if (data == null || data.isBlank()) {
            throw new IllegalArgumentException("BlockData string cannot be null or blank");
        }
        String trimmed = data.trim();
        int bracket = trimmed.indexOf('[');
        String matPart = bracket >= 0 ? trimmed.substring(0, bracket).trim() : trimmed;
        Material material = Material.matchMaterial(matPart);
        if (material == null || !material.isBlock()) {
            throw new IllegalArgumentException("Unknown block material in '" + data + "'");
        }
        return createBlockData(material);
    }

    @Override
    public BlockData createBlockData(Material material, String data) throws IllegalArgumentException {
        Objects.requireNonNull(material, "material");
        return createBlockData(material);
    }

    @Override
    public com.destroystokyo.paper.profile.PlayerProfile createProfile(UUID id) {
        return new dev.ferrite.api.entity.FerritePlayerProfile(id, null);
    }

    @Override
    public com.destroystokyo.paper.profile.PlayerProfile createProfile(String name) {
        return new dev.ferrite.api.entity.FerritePlayerProfile(null, name);
    }

    @Override
    public com.destroystokyo.paper.profile.PlayerProfile createProfile(UUID id, String name) {
        return new dev.ferrite.api.entity.FerritePlayerProfile(id, name);
    }

    @Override
    public com.destroystokyo.paper.profile.PlayerProfile createProfileExact(UUID id, String name) {
        return new dev.ferrite.api.entity.FerritePlayerProfile(id, name);
    }

    @Override
    public org.bukkit.profile.PlayerProfile createPlayerProfile(UUID uniqueId, String name) {
        return createProfile(uniqueId, name);
    }

    @Override
    public org.bukkit.profile.PlayerProfile createPlayerProfile(UUID uniqueId) {
        return createProfile(uniqueId);
    }

    @Override
    public org.bukkit.profile.PlayerProfile createPlayerProfile(String name) {
        return createProfile(name);
    }

    @Override
    public org.bukkit.command.CommandSender createCommandSender(
            java.util.function.Consumer<? super Component> feedback) {
        Objects.requireNonNull(feedback, "feedback");
        return new FerriteFeedbackCommandSender(this, feedback);
    }

    @Override
    @SuppressWarnings({"unchecked", "deprecation"})
    public <T extends org.bukkit.BanList<?>> T getBanList(org.bukkit.BanList.Type type) {
        Objects.requireNonNull(type, "type");
        return switch (type) {
            case IP -> (T) ipBanList;
            case NAME, PROFILE -> (T) profileBanList;
        };
    }

    @Override
    @SuppressWarnings("unchecked")
    public <B extends org.bukkit.BanList<E>, E> B getBanList(io.papermc.paper.ban.BanListType<B> type) {
        Objects.requireNonNull(type, "type");
        if (type == io.papermc.paper.ban.BanListType.IP) {
            return (B) ipBanList;
        }
        return (B) profileBanList;
    }

    @Override
    public boolean addRecipe(org.bukkit.inventory.Recipe recipe) {
        if (recipe == null) {
            return false;
        }
        if (recipe instanceof org.bukkit.Keyed keyed) {
            return recipes.putIfAbsent(keyed.getKey(), recipe) == null;
        }
        return false;
    }

    @Override
    public boolean addRecipes(java.lang.Iterable<org.bukkit.inventory.Recipe> recipesToAdd) {
        Objects.requireNonNull(recipesToAdd, "recipes");
        boolean any = false;
        for (org.bukkit.inventory.Recipe recipe : recipesToAdd) {
            any |= addRecipe(recipe);
        }
        return any;
    }

    @Override
    public List<org.bukkit.inventory.Recipe> getRecipesFor(org.bukkit.inventory.ItemStack result) {
        Objects.requireNonNull(result, "result");
        List<org.bukkit.inventory.Recipe> matching = new ArrayList<>();
        synchronized (recipes) {
            for (org.bukkit.inventory.Recipe recipe : recipes.values()) {
                if (recipe.getResult().isSimilar(result)) {
                    matching.add(recipe);
                }
            }
        }
        return matching;
    }

    @Override
    public org.bukkit.inventory.Recipe getRecipe(NamespacedKey recipeKey) {
        return recipeKey == null ? null : recipes.get(recipeKey);
    }

    @Override
    public boolean hasRecipe(NamespacedKey recipeKey) {
        return recipeKey != null && recipes.containsKey(recipeKey);
    }

    @Override
    public boolean removeRecipe(NamespacedKey recipeKey) {
        return recipeKey != null && recipes.remove(recipeKey) != null;
    }

    @Override
    public boolean removeRecipes(java.lang.Iterable<NamespacedKey> recipeKeys) {
        Objects.requireNonNull(recipeKeys, "recipeKeys");
        boolean changed = false;
        for (NamespacedKey key : recipeKeys) {
            changed |= removeRecipe(key);
        }
        return changed;
    }

    @Override
    public Iterator<org.bukkit.inventory.Recipe> recipeIterator() {
        synchronized (recipes) {
            return List.copyOf(recipes.values()).iterator();
        }
    }

    @Override
    public void clearRecipes() {
        recipes.clear();
    }

    @Override
    public void resetRecipes() {
        // Facade stores runtime-added recipes only; reset clears custom recipes cleanly.
        recipes.clear();
    }

    @Override
    public <T extends org.bukkit.Keyed> org.bukkit.Tag<T> getTag(
            String registry, NamespacedKey tag, Class<T> clazz) {
        Objects.requireNonNull(registry, "registry");
        Objects.requireNonNull(tag, "tag");
        Objects.requireNonNull(clazz, "clazz");
        dev.ferrite.api.registry.VanillaData data = dev.ferrite.api.registry.VanillaData.get();
        String keyStr = tag.asString();
        if (!data.tagNames(registry).contains(keyStr)) {
            return null;
        }
        Set<String> ids = data.tagValues(registry, keyStr);
        Set<T> values = new java.util.LinkedHashSet<>();
        for (String id : ids) {
            if (clazz == Material.class) {
                Material m = Material.matchMaterial(id);
                if (m != null) {
                    values.add(clazz.cast(m));
                }
            } else if (clazz == org.bukkit.entity.EntityType.class) {
                NamespacedKey nk = NamespacedKey.fromString(id);
                org.bukkit.entity.EntityType et = nk == null
                        ? null
                        : org.bukkit.Registry.ENTITY_TYPE.get(nk);
                if (et != null) {
                    values.add(clazz.cast(et));
                }
            }
        }
        final Set<T> frozen = Set.copyOf(values);
        return new org.bukkit.Tag<>() {
            @Override
            public boolean isTagged(T item) {
                return item != null && frozen.contains(item);
            }

            @Override
            public Set<T> getValues() {
                return frozen;
            }

            @Override
            public NamespacedKey getKey() {
                return tag;
            }
        };
    }

    @Override
    public <T extends org.bukkit.Keyed> Iterable<org.bukkit.Tag<T>> getTags(
            String registry, Class<T> clazz) {
        Objects.requireNonNull(registry, "registry");
        Objects.requireNonNull(clazz, "clazz");
        List<org.bukkit.Tag<T>> tags = new ArrayList<>();
        for (String tagId : dev.ferrite.api.registry.VanillaData.get().tagNames(registry)) {
            NamespacedKey key = NamespacedKey.fromString(tagId);
            if (key != null) {
                org.bukkit.Tag<T> tag = getTag(registry, key, clazz);
                if (tag != null) {
                    tags.add(tag);
                }
            }
        }
        return List.copyOf(tags);
    }

    @Override
    public <T extends org.bukkit.Keyed> org.bukkit.Registry<T> getRegistry(Class<T> tClass) {
        Objects.requireNonNull(tClass, "tClass");
        return io.papermc.paper.registry.RegistryAccess.registryAccess().getRegistry(tClass);
    }

    private final org.bukkit.Server.Spigot serverSpigot = new org.bukkit.Server.Spigot() {
        @Override
        @SuppressWarnings("deprecation")
        public void broadcast(net.md_5.bungee.api.chat.BaseComponent component) {
            if (component != null) {
                broadcastMessage(component.toLegacyText());
            }
        }

        @Override
        @SuppressWarnings("deprecation")
        public void broadcast(net.md_5.bungee.api.chat.BaseComponent... components) {
            if (components != null && components.length > 0) {
                broadcastMessage(net.md_5.bungee.api.chat.TextComponent.toLegacyText(components));
            }
        }
    };

    @Override
    public org.bukkit.Server.Spigot spigot() {
        return serverSpigot;
    }

    private static final class FerriteFeedbackCommandSender
            implements org.bukkit.command.CommandSender {
        private final FerriteServer server;
        private final java.util.function.Consumer<? super Component> feedback;
        private boolean op = true;
        private final List<org.bukkit.permissions.PermissionAttachment> attachments = new ArrayList<>();
        private final Map<String, org.bukkit.permissions.PermissionAttachmentInfo> perms =
                new LinkedHashMap<>();
        private final CommandSenderSpigot spigot = new CommandSenderSpigot();

        FerriteFeedbackCommandSender(
                FerriteServer server, java.util.function.Consumer<? super Component> feedback) {
            this.server = server;
            this.feedback = feedback;
        }

        @Override
        public void sendMessage(String message) {
            feedback.accept(
                    message == null
                            ? Component.empty()
                            : LegacyComponentSerializer.legacySection().deserialize(message));
        }

        @Override
        public void sendMessage(String... messages) {
            if (messages != null) {
                for (String m : messages) {
                    sendMessage(m);
                }
            }
        }

        @Override
        public void sendMessage(UUID sender, String message) {
            sendMessage(message);
        }

        @Override
        public void sendMessage(UUID sender, String... messages) {
            sendMessage(messages);
        }

        @Override
        public void sendMessage(Component message) {
            feedback.accept(message == null ? Component.empty() : message);
        }

        @Override
        public org.bukkit.Server getServer() {
            return server;
        }

        @Override
        public String getName() {
            return "Feedback";
        }

        @Override
        public Spigot spigot() {
            return spigot;
        }

        @Override
        public Component name() {
            return Component.text("Feedback");
        }

        @Override
        public boolean isPermissionSet(String name) {
            return name != null && perms.containsKey(name.toLowerCase(Locale.ROOT));
        }

        @Override
        public boolean isPermissionSet(org.bukkit.permissions.Permission perm) {
            return perm != null && isPermissionSet(perm.getName());
        }

        @Override
        public boolean hasPermission(String name) {
            if (name == null) {
                return false;
            }
            var info = perms.get(name.toLowerCase(Locale.ROOT));
            return info != null ? info.getValue() : op;
        }

        @Override
        public boolean hasPermission(org.bukkit.permissions.Permission perm) {
            return perm != null && hasPermission(perm.getName());
        }

        @Override
        public org.bukkit.permissions.PermissionAttachment addAttachment(Plugin plugin, String name, boolean value) {
            var att = addAttachment(plugin);
            att.setPermission(name, value);
            return att;
        }

        @Override
        public org.bukkit.permissions.PermissionAttachment addAttachment(Plugin plugin) {
            Objects.requireNonNull(plugin, "plugin");
            var att = new org.bukkit.permissions.PermissionAttachment(plugin, this);
            attachments.add(att);
            recalculatePermissions();
            return att;
        }

        @Override
        public org.bukkit.permissions.PermissionAttachment addAttachment(
                Plugin plugin, String name, boolean value, int ticks) {
            return addAttachment(plugin, name, value);
        }

        @Override
        public org.bukkit.permissions.PermissionAttachment addAttachment(Plugin plugin, int ticks) {
            return addAttachment(plugin);
        }

        @Override
        public void removeAttachment(org.bukkit.permissions.PermissionAttachment attachment) {
            attachments.remove(attachment);
            recalculatePermissions();
        }

        @Override
        public void recalculatePermissions() {
            perms.clear();
            for (var att : attachments) {
                for (var e : att.getPermissions().entrySet()) {
                    perms.put(
                            e.getKey().toLowerCase(Locale.ROOT),
                            new org.bukkit.permissions.PermissionAttachmentInfo(
                                    this, e.getKey(), att, e.getValue()));
                }
            }
        }

        @Override
        public Set<org.bukkit.permissions.PermissionAttachmentInfo> getEffectivePermissions() {
            return Set.copyOf(perms.values());
        }

        @Override
        public boolean isOp() {
            return op;
        }

        @Override
        public void setOp(boolean value) {
            this.op = value;
        }

        private final class CommandSenderSpigot extends org.bukkit.command.CommandSender.Spigot {
            @Override
            @SuppressWarnings("deprecation")
            public void sendMessage(net.md_5.bungee.api.chat.BaseComponent component) {
                if (component != null) {
                    FerriteFeedbackCommandSender.this.sendMessage(component.toLegacyText());
                }
            }

            @Override
            @SuppressWarnings("deprecation")
            public void sendMessage(net.md_5.bungee.api.chat.BaseComponent... components) {
                if (components != null && components.length > 0) {
                    FerriteFeedbackCommandSender.this.sendMessage(
                            net.md_5.bungee.api.chat.TextComponent.toLegacyText(components));
                }
            }
        }
    }
}
