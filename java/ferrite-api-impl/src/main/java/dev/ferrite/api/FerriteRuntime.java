package dev.ferrite.api;

import dev.ferrite.api.command.FerriteCommandDispatcher;
import dev.ferrite.api.command.FerriteConsoleSender;
import dev.ferrite.api.command.FerriteHelpMap;
import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.entity.PlayerState;
import dev.ferrite.api.internal.FerriteContext;
import dev.ferrite.api.internal.FerriteMainThread;
import dev.ferrite.api.internal.QueuedMainThread;
import dev.ferrite.api.permission.DefaultPermissionResolver;
import dev.ferrite.api.permission.PermissionResolver;
import dev.ferrite.api.plugin.FerriteMessenger;
import dev.ferrite.api.plugin.FerritePluginLoader;
import dev.ferrite.api.plugin.FerritePluginManager;
import dev.ferrite.api.scheduler.FerriteScheduler;
import dev.ferrite.api.server.FerriteServer;
import dev.ferrite.api.world.FerriteWorld;
import dev.ferrite.api.world.WorldSpec;
import java.io.File;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.Collection;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.UUID;
import java.util.concurrent.Callable;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.Future;
import java.util.logging.Logger;
import net.kyori.adventure.text.Component;
import org.bukkit.GameMode;
import org.bukkit.World;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.plugin.PluginDescriptionFile;
import org.bukkit.plugin.java.JavaPlugin;

/**
 * Java-side state of one Ferrite server: worlds, players, plugin manager, scheduler and
 * the Bukkit server object.
 *
 * <p>This is the in-process integration point for the host. It deliberately does not name
 * any Rust RPC op: Lane 1 freezes {@code docs/plugin-b-host-contract.md}, and the host
 * adapts its ops to these methods (world/block/player snapshots, event injection,
 * command dispatch, tick). Before that contract exists the facade is exercised by
 * in-memory tests and the typical fixture jar.
 */
public final class FerriteRuntime {

    private final String serverName;
    private final String version;
    private final String bukkitVersion;
    private final String minecraftVersion;
    private final int maxPlayers;
    private final int port;

    private final FerriteMainThread mainThread;
    private final Logger logger;
    private final FerritePluginManager pluginManager;
    private final FerriteScheduler scheduler;
    private final FerriteCommandDispatcher commandDispatcher;
    private final FerriteHelpMap helpMap;
    private final FerriteMessenger messenger;
    private final FerriteConsoleSender consoleSender;
    private final FerriteServer server;
    private final FerritePluginLoader pluginLoader;
    private final PermissionResolver permissionResolver;

    private final Map<String, FerriteWorld> worlds = new LinkedHashMap<>();
    private final Map<UUID, FerritePlayer> players = new LinkedHashMap<>();

    private boolean closed;

    private FerriteRuntime(Builder builder) {
        this.serverName = Objects.requireNonNull(builder.serverName, "serverName");
        this.version = Objects.requireNonNull(builder.version, "version");
        this.bukkitVersion = Objects.requireNonNull(builder.bukkitVersion, "bukkitVersion");
        this.minecraftVersion = Objects.requireNonNull(builder.minecraftVersion, "minecraftVersion");
        this.maxPlayers = builder.maxPlayers;
        this.port = builder.port;
        this.mainThread = builder.mainThread == null
                ? new QueuedMainThread("Ferrite-Main")
                : builder.mainThread;
        this.logger = builder.logger == null ? Logger.getLogger("Ferrite") : builder.logger;

        this.scheduler = new FerriteScheduler(this);
        this.server = new FerriteServer(this);
        this.commandDispatcher = new FerriteCommandDispatcher(this);
        this.helpMap = new FerriteHelpMap(this);
        this.messenger = new FerriteMessenger();
        this.pluginManager = new FerritePluginManager(this);
        this.consoleSender = new FerriteConsoleSender(this);
        this.pluginLoader = new FerritePluginLoader(this);
        this.permissionResolver = builder.permissionResolver == null
                ? new DefaultPermissionResolver(this.pluginManager)
                : builder.permissionResolver;

        List<WorldSpec> specs = new ArrayList<>(builder.worldSpecs);
        if (specs.isEmpty()) {
            specs.add(WorldSpec.of("world", World.Environment.NORMAL));
        }
        for (WorldSpec spec : specs) {
            FerriteWorld world = new FerriteWorld(this, spec);
            this.worlds.put(world.getName().toLowerCase(Locale.ROOT), world);
        }
    }

    public static Builder builder() {
        return new Builder();
    }

    public String serverName() {
        return serverName;
    }

    public String version() {
        return version;
    }

    public String bukkitVersion() {
        return bukkitVersion;
    }

    public String minecraftVersion() {
        return minecraftVersion;
    }

    public int maxPlayers() {
        return maxPlayers;
    }

    public int port() {
        return port;
    }

    public FerriteMainThread mainThread() {
        return mainThread;
    }

    public Logger logger() {
        return logger;
    }

    public FerriteServer server() {
        return server;
    }

    public FerritePluginManager plugins() {
        return pluginManager;
    }

    public FerriteScheduler scheduler() {
        return scheduler;
    }

    public FerriteCommandDispatcher commandDispatcher() {
        return commandDispatcher;
    }

    public FerriteHelpMap helpMap() {
        return helpMap;
    }

    public FerriteMessenger messenger() {
        return messenger;
    }

    public FerriteConsoleSender consoleSender() {
        return consoleSender;
    }

    public FerritePluginLoader pluginLoader() {
        return pluginLoader;
    }

    public PermissionResolver permissionResolver() {
        return permissionResolver;
    }

    public boolean isPrimaryThread() {
        return mainThread.isPrimaryThread();
    }

    public void requirePrimaryThread(String operation) {
        if (!isPrimaryThread()) {
            throw new IllegalStateException(operation
                    + " must run on the Ferrite server main thread; current thread is '"
                    + Thread.currentThread().getName()
                    + "'");
        }
    }

    public <T> T callOnMainThread(Callable<T> action) {
        Objects.requireNonNull(action, "action");
        if (isPrimaryThread()) {
            return callUnchecked(action);
        }
        Future<T> future = mainThread.submit(action);
        try {
            return future.get();
        } catch (InterruptedException exception) {
            Thread.currentThread().interrupt();
            throw new IllegalStateException("interrupted while waiting for the server main thread", exception);
        } catch (ExecutionException exception) {
            throw rethrow(exception.getCause());
        }
    }

    public void runOnMainThread(Runnable action) {
        Objects.requireNonNull(action, "action");
        if (isPrimaryThread()) {
            action.run();
            return;
        }
        mainThread.execute(action);
    }

    /** Advance one server tick; the host calls this from its tick loop. */
    public void tick() {
        requirePrimaryThread("FerriteRuntime#tick");
        ensureOpen();
        scheduler.tick();
    }

    public Collection<FerriteWorld> worlds() {
        return List.copyOf(worlds.values());
    }

    public FerriteWorld world(String name) {
        if (name == null) {
            return null;
        }
        return worlds.get(name.toLowerCase(Locale.ROOT));
    }

    public FerriteWorld world(UUID uniqueId) {
        for (FerriteWorld world : worlds.values()) {
            if (world.getUID().equals(uniqueId)) {
                return world;
            }
        }
        return null;
    }

    public Collection<FerritePlayer> onlinePlayers() {
        return List.copyOf(players.values());
    }

    public List<org.bukkit.entity.Player> onlinePlayersIn(FerriteWorld world) {
        List<org.bukkit.entity.Player> found = new ArrayList<>();
        for (FerritePlayer player : players.values()) {
            if (player.getWorld() == world) {
                found.add(player);
            }
        }
        return found;
    }

    public FerritePlayer player(UUID uniqueId) {
        return players.get(uniqueId);
    }

    public FerritePlayer playerExact(String name) {
        for (FerritePlayer player : players.values()) {
            if (player.getName().equalsIgnoreCase(name)) {
                return player;
            }
        }
        return null;
    }

    public FerritePlayer createPlayer(
            String name,
            UUID uniqueId,
            boolean op,
            GameMode gameMode,
            FerriteWorld world,
            double x,
            double y,
            double z) {
        Objects.requireNonNull(name, "name");
        Objects.requireNonNull(uniqueId, "uniqueId");
        Objects.requireNonNull(gameMode, "gameMode");
        Objects.requireNonNull(world, "world");
        if (players.containsKey(uniqueId)) {
            throw new IllegalStateException("A player with UUID " + uniqueId + " already exists");
        }
        PlayerState state = new PlayerState(name, uniqueId, op, gameMode, world, x, y, z);
        return new FerritePlayer(this, state);
    }

    /**
     * Add an already-created player to the online set and fire {@code PlayerJoinEvent}.
     * The host calls this when a connection finishes login; tests call it directly.
     */
    public FerritePlayer playerJoined(FerritePlayer player) {
        requirePrimaryThread("FerriteRuntime#playerJoined");
        ensureOpen();
        Objects.requireNonNull(player, "player");
        if (player.getServer() != server) {
            throw new IllegalArgumentException("player belongs to another Ferrite runtime");
        }
        if (players.putIfAbsent(player.getUniqueId(), player) != null) {
            throw new IllegalStateException("Player " + player.getName() + " is already online");
        }
        player.state().setOnline(true);
        pluginManager.callEvent(new PlayerJoinEvent(
                player, Component.text(player.getName() + " joined the game")));
        return player;
    }

    /** Fire {@code PlayerQuitEvent} while the player is still online, then remove them. */
    public FerritePlayer playerQuit(UUID uniqueId) {
        requirePrimaryThread("FerriteRuntime#playerQuit");
        FerritePlayer player = players.get(uniqueId);
        if (player == null) {
            return null;
        }
        pluginManager.callEvent(new PlayerQuitEvent(
                player, Component.text(player.getName() + " left the game")));
        player.closeInventory(org.bukkit.event.inventory.InventoryCloseEvent.Reason.DISCONNECT);
        players.remove(uniqueId);
        player.state().setOnline(false);
        return player;
    }

    /**
     * Attach a plugin instance whose class was loaded by the host.
     *
     * <p>The host owns the classloader and jar discovery. The facade only calls
     * {@link JavaPlugin#init}, registers the descriptor's permissions/commands, and calls
     * {@code onLoad}. Enabling is a separate phase so the host can respect
     * startup/postworld ordering.
     */
    public JavaPlugin attachPlugin(
            JavaPlugin plugin,
            PluginDescriptionFile description,
            File jarFile,
            File dataFolder,
            ClassLoader classLoader) {
        requirePrimaryThread("FerriteRuntime#attachPlugin");
        ensureOpen();
        Objects.requireNonNull(plugin, "plugin");
        Objects.requireNonNull(description, "description");
        Objects.requireNonNull(jarFile, "jarFile");
        Objects.requireNonNull(dataFolder, "dataFolder");
        Objects.requireNonNull(classLoader, "classLoader");
        try {
            Files.createDirectories(dataFolder.toPath());
        } catch (IOException exception) {
            throw new UncheckedIOException(
                    "could not create plugin data folder " + dataFolder, exception);
        }
        // Paper's init signature takes the data folder before the jar file.
        plugin.init(pluginLoader, server, description, dataFolder, jarFile, classLoader);
        pluginManager.registerPlugin(plugin);
        FerriteContext.withPlugin(plugin.getName(), plugin::onLoad);
        return plugin;
    }

    public void enablePlugin(JavaPlugin plugin) {
        requirePrimaryThread("FerriteRuntime#enablePlugin");
        ensureOpen();
        pluginManager.enablePlugin(plugin);
    }

    public void disablePlugin(JavaPlugin plugin) {
        requirePrimaryThread("FerriteRuntime#disablePlugin");
        pluginManager.disablePlugin(plugin);
    }

    public void shutdown() {
        if (closed) {
            return;
        }
        closed = true;
        try {
            pluginManager.disablePlugins();
        } finally {
            scheduler.shutdown();
            mainThread.shutdown();
        }
    }

    private void ensureOpen() {
        if (closed) {
            throw new IllegalStateException("The Ferrite runtime is shutting down");
        }
    }

    private static <T> T callUnchecked(Callable<T> action) {
        try {
            return action.call();
        } catch (RuntimeException | Error exception) {
            throw exception;
        } catch (Exception exception) {
            throw new IllegalStateException(exception);
        }
    }

    private static RuntimeException rethrow(Throwable throwable) {
        if (throwable instanceof RuntimeException runtimeException) {
            return runtimeException;
        }
        if (throwable instanceof Error error) {
            throw error;
        }
        return new IllegalStateException(throwable);
    }

    public static final class Builder {

        private String serverName = "Ferrite";
        private String version = "0.1.0-SNAPSHOT";
        private String bukkitVersion = "26.3.build.227-stable";
        private String minecraftVersion = "26.3";
        private int maxPlayers = 20;
        private int port = 25565;
        private Logger logger;
        private FerriteMainThread mainThread;
        private PermissionResolver permissionResolver;
        private final List<WorldSpec> worldSpecs = new ArrayList<>();

        private Builder() {
        }

        public Builder serverName(String value) {
            this.serverName = Objects.requireNonNull(value, "serverName");
            return this;
        }

        public Builder version(String value) {
            this.version = Objects.requireNonNull(value, "version");
            return this;
        }

        public Builder bukkitVersion(String value) {
            this.bukkitVersion = Objects.requireNonNull(value, "bukkitVersion");
            return this;
        }

        public Builder minecraftVersion(String value) {
            this.minecraftVersion = Objects.requireNonNull(value, "minecraftVersion");
            return this;
        }

        public Builder maxPlayers(int value) {
            if (value <= 0) {
                throw new IllegalArgumentException("maxPlayers must be positive");
            }
            this.maxPlayers = value;
            return this;
        }

        public Builder port(int value) {
            this.port = value;
            return this;
        }

        public Builder logger(Logger value) {
            this.logger = Objects.requireNonNull(value, "logger");
            return this;
        }

        public Builder mainThread(FerriteMainThread value) {
            this.mainThread = Objects.requireNonNull(value, "mainThread");
            return this;
        }

        public Builder permissionResolver(PermissionResolver value) {
            this.permissionResolver = Objects.requireNonNull(value, "permissionResolver");
            return this;
        }

        public Builder world(WorldSpec spec) {
            this.worldSpecs.add(Objects.requireNonNull(spec, "spec"));
            return this;
        }

        public FerriteRuntime build() {
            return new FerriteRuntime(this);
        }
    }

}
