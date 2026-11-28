package dev.ferrite.api.plugin;

import dev.ferrite.api.FerriteRuntime;
import dev.ferrite.api.internal.FerriteContext;
import dev.ferrite.api.internal.Unsupported;
import java.io.File;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import java.util.logging.Level;
import org.bukkit.command.CommandMap;
import org.bukkit.command.PluginCommand;
import org.bukkit.event.Event;
import org.bukkit.event.EventException;
import org.bukkit.event.EventPriority;
import org.bukkit.event.HandlerList;
import org.bukkit.event.Listener;
import org.bukkit.permissions.Permissible;
import org.bukkit.permissions.Permission;
import org.bukkit.plugin.EventExecutor;
import org.bukkit.plugin.InvalidDescriptionException;
import org.bukkit.plugin.InvalidPluginException;
import org.bukkit.plugin.Plugin;
import org.bukkit.plugin.PluginDescriptionFile;
import org.bukkit.plugin.RegisteredListener;
import org.bukkit.plugin.UnknownDependencyException;
import org.bukkit.plugin.java.JavaPlugin;

/**
 * Bukkit plugin-manager semantics for the first slice: plugin identity, listener
 * registration/dispatch and removal on disable, permissions, commands and lifecycle.
 *
 * <p>Loading jars and creating classloaders stay with the host; {@link #loadPlugin} and
 * friends fail explicitly.
 */
public final class FerritePluginManager extends UnsupportedPluginManagerBase {

    private final FerriteRuntime runtime;
    // Exposed as `commandMap` so legacy Spigot reflection (e.g. WorldEdit CommandRegistration)
    // finds the same live CommandMap as Server#getCommandMap().
    @SuppressWarnings("unused")
    private final CommandMap commandMap;
    private final Map<String, JavaPlugin> plugins = new LinkedHashMap<>();
    private final Map<String, Permission> permissions = new LinkedHashMap<>();
    private final Map<String, Set<Permissible>> permissionSubscriptions = new LinkedHashMap<>();
    private final Set<Permissible> defaultTrueSubscriptions =
            Collections.newSetFromMap(new java.util.WeakHashMap<>());
    private final Set<Permissible> defaultFalseSubscriptions =
            Collections.newSetFromMap(new java.util.WeakHashMap<>());

    public FerritePluginManager(FerriteRuntime runtime) {
        this.runtime = runtime;
        this.commandMap = runtime.commandDispatcher().commandMap();
    }

    /** Register a plugin already initialized by the host. */
    public void registerPlugin(JavaPlugin plugin) {
        Objects.requireNonNull(plugin, "plugin");
        PluginDescriptionFile description = plugin.getDescription();
        String key = plugin.getName().toLowerCase(Locale.ROOT);
        JavaPlugin previous = plugins.putIfAbsent(key, plugin);
        if (previous != null) {
            throw new IllegalStateException(
                    "Plugin " + plugin.getName() + " is already registered");
        }
        for (Permission permission : description.getPermissions()) {
            addPermission(permission);
        }
        for (Map.Entry<String, Map<String, Object>> entry : description.getCommands().entrySet()) {
            registerCommand(plugin, entry.getKey(), entry.getValue());
        }
    }

    /** Removes a disabled plugin from the registry so it can be hot-reloaded at runtime. */
    public void unregisterPlugin(JavaPlugin plugin) {
        if (plugin == null) {
            return;
        }
        String key = plugin.getName().toLowerCase(Locale.ROOT);
        plugins.remove(key, plugin);
        dev.ferrite.api.server.FerriteLifecycleManager.dispose(plugin);
        syncEventMaskToHost();
    }

    private void registerCommand(JavaPlugin plugin, String name, Map<String, Object> data) {
        Map<String, Object> values = data == null ? Map.of() : data;
        PluginCommand command = org.bukkit.command.FerriteCommandFactory.create(
                name.toLowerCase(Locale.ROOT), plugin);
        // JavaPlugin is a CommandExecutor/TabCompleter; Bukkit falls back to it when the
        // plugin.yml command does not install a dedicated executor.
        command.setExecutor(plugin);
        command.setTabCompleter(plugin);
        command.setDescription(stringValue(values, "description", ""));
        command.setUsage(stringValue(values, "usage", "/" + name));
        String permission = stringValue(values, "permission", null);
        if (permission != null) {
            command.setPermission(permission);
        }
        Object aliases = values.get("aliases");
        if (aliases instanceof Collection<?> collection) {
            command.setAliases(collection.stream().map(String::valueOf).toList());
        }
        runtime.commandDispatcher().register(command);
    }

    private static String stringValue(Map<String, Object> values, String key, String fallback) {
        Object value = values.get(key);
        return value == null ? fallback : String.valueOf(value);
    }

    @Override
    public Plugin getPlugin(String name) {
        return name == null ? null : plugins.get(name.toLowerCase(Locale.ROOT));
    }

    @Override
    public Plugin[] getPlugins() {
        return plugins.values().toArray(Plugin[]::new);
    }

    @Override
    public boolean isPluginEnabled(String name) {
        Plugin plugin = getPlugin(name);
        return plugin != null && plugin.isEnabled();
    }

    @Override
    public boolean isPluginEnabled(Plugin plugin) {
        return plugin != null && plugin.isEnabled();
    }

    @Override
    public Plugin loadPlugin(File file)
            throws InvalidPluginException, InvalidDescriptionException, UnknownDependencyException {
        // Class loading and dependency resolution belong to the Ferrite host bootstrapper.
        throw Unsupported.exception("PluginManager#loadPlugin");
    }

    @Override
    public Plugin[] loadPlugins(File directory) {
        // Class loading and dependency resolution belong to the Ferrite host bootstrapper.
        throw Unsupported.exception("PluginManager#loadPlugins");
    }

    @Override
    public Plugin[] loadPlugins(File[] files) {
        // Class loading and dependency resolution belong to the Ferrite host bootstrapper.
        throw Unsupported.exception("PluginManager#loadPlugins");
    }

    @Override
    public void disablePlugins() {
        List<JavaPlugin> loaded = new ArrayList<>(plugins.values());
        Collections.reverse(loaded);
        for (JavaPlugin plugin : loaded) {
            disablePlugin(plugin);
        }
    }

    @Override
    public void clearPlugins() {
        disablePlugins();
        plugins.clear();
        permissions.clear();
        syncEventMaskToHost();
    }

    /**
     * Publishes a bitmask of contract events with at least one active Bukkit listener so Rust
     * short-circuits unheard events before JSON serialization or JNI crossing.
     */
    @SuppressWarnings("deprecation")
    public void syncEventMaskToHost() {
        // Bits 0 (player_join), 1 (player_quit), 10 (plugin_message) stay enabled for roster/channels.
        long mask = (1L << 0) | (1L << 1) | (1L << 10);
        if (hasListeners(org.bukkit.event.block.BlockBreakEvent.getHandlerList())) {
            mask |= 1L << 2;
        }
        if (hasListeners(org.bukkit.event.block.BlockPlaceEvent.getHandlerList())) {
            mask |= 1L << 3;
        }
        if (hasListeners(org.bukkit.event.player.PlayerInteractEvent.getHandlerList())) {
            mask |= 1L << 4;
        }
        if (hasListeners(org.bukkit.event.player.PlayerMoveEvent.getHandlerList())) {
            mask |= 1L << 5;
        }
        if (hasListeners(org.bukkit.event.player.AsyncPlayerChatEvent.getHandlerList())) {
            mask |= 1L << 6;
        }
        if (hasListeners(org.bukkit.event.player.PlayerCommandPreprocessEvent.getHandlerList())) {
            mask |= 1L << 7;
        }
        if (hasListeners(org.bukkit.event.entity.EntityDamageEvent.getHandlerList())) {
            mask |= 1L << 8;
        }
        if (hasListeners(org.bukkit.event.player.PlayerRespawnEvent.getHandlerList())) {
            mask |= 1L << 9;
        }
        if (hasListeners(org.bukkit.event.inventory.InventoryClickEvent.getHandlerList())) {
            mask |= 1L << 11;
        }
        if (hasListeners(org.bukkit.event.inventory.InventoryCloseEvent.getHandlerList())) {
            mask |= 1L << 12;
        }
        if (hasListeners(org.bukkit.event.entity.PlayerDeathEvent.getHandlerList())) {
            mask |= 1L << 13;
        }
        if (hasListeners(org.bukkit.event.world.ChunkLoadEvent.getHandlerList())) {
            mask |= 1L << 14;
        }
        if (hasListeners(org.bukkit.event.world.ChunkUnloadEvent.getHandlerList())) {
            mask |= 1L << 15;
        }
        if (hasListeners(org.bukkit.event.world.WorldLoadEvent.getHandlerList())) {
            mask |= 1L << 16;
        }
        if (hasListeners(org.bukkit.event.block.BlockExplodeEvent.getHandlerList())) {
            mask |= 1L << 17;
        }
        if (hasListeners(org.bukkit.event.weather.WeatherChangeEvent.getHandlerList())) {
            mask |= 1L << 18;
        }
        if (hasListeners(org.bukkit.event.player.PlayerLevelChangeEvent.getHandlerList())) {
            mask |= 1L << 19;
        }
        if (hasListeners(org.bukkit.event.player.PlayerExpChangeEvent.getHandlerList())) {
            mask |= 1L << 20;
        }
        if (hasListeners(org.bukkit.event.entity.EntityDeathEvent.getHandlerList())) {
            mask |= 1L << 21;
        }
        if (hasListeners(org.bukkit.event.player.PlayerResourcePackStatusEvent.getHandlerList())) {
            mask |= 1L << 22;
        }
        if (hasListeners(org.bukkit.event.inventory.InventoryDragEvent.getHandlerList())) {
            mask |= 1L << 23;
        }
        if (hasListeners(org.bukkit.event.entity.CreatureSpawnEvent.getHandlerList())) {
            mask |= 1L << 24;
        }
        if (hasListeners(org.bukkit.event.block.BlockSpreadEvent.getHandlerList())) {
            mask |= 1L << 25;
        }
        if (hasListeners(org.bukkit.event.entity.EntityChangeBlockEvent.getHandlerList())) {
            mask |= 1L << 26;
        }
        try {
            dev.ferrite.host.Bootstrap.eventMaskSync(mask);
        } catch (Throwable ignored) {
            // Pure-Java unit tests have no native Rust HostBridge attached.
        }
    }

    private static boolean hasListeners(HandlerList list) {
        return list != null && list.getRegisteredListeners().length > 0;
    }

    @Override
    public void callEvent(Event event) {
        Objects.requireNonNull(event, "event");
        if (event.isAsynchronous()) {
            if (runtime.isPrimaryThread()) {
                throw new IllegalStateException("Asynchronous event " + event.getEventName()
                        + " cannot be fired from the primary thread");
            }
        } else {
            runtime.requirePrimaryThread("Event " + event.getEventName());
        }
        for (RegisteredListener listener : event.getHandlers().getRegisteredListeners()) {
            try {
                listener.callEvent(event);
            } catch (Throwable throwable) {
                // RegisteredListener normally wraps listener failures in EventException,
                // but a plain RuntimeException can escape when an executor throws it.
                // Both must be reported with the event and plugin by Bukkit semantics.
                Throwable cause = throwable instanceof EventException && throwable.getCause() != null
                        ? throwable.getCause()
                        : throwable;
                throw new IllegalStateException(
                        "Could not pass event " + event.getEventName() + " to plugin "
                                + listener.getPlugin().getName(),
                        cause);
            }
        }
    }

    @Override
    public void registerEvents(Listener listener, Plugin plugin) {
        Objects.requireNonNull(listener, "listener");
        Objects.requireNonNull(plugin, "plugin");
        requireEnabled(plugin, "registerEvents");
        Map<Class<? extends Event>, Set<RegisteredListener>> listeners =
                runtime.pluginLoader().createRegisteredListeners(listener, plugin);
        for (Map.Entry<Class<? extends Event>, Set<RegisteredListener>> entry
                : listeners.entrySet()) {
            HandlerList handlerList = FerritePluginLoader.handlerList(entry.getKey());
            for (RegisteredListener registered : entry.getValue()) {
                handlerList.register(registered);
            }
        }
        syncEventMaskToHost();
    }

    @Override
    public void registerEvent(
            Class<? extends Event> event,
            Listener listener,
            EventPriority priority,
            EventExecutor executor,
            Plugin plugin) {
        registerEvent(event, listener, priority, executor, plugin, false);
    }

    @Override
    public void registerEvent(
            Class<? extends Event> event,
            Listener listener,
            EventPriority priority,
            EventExecutor executor,
            Plugin plugin,
            boolean ignoreCancelled) {
        Objects.requireNonNull(event, "event");
        Objects.requireNonNull(listener, "listener");
        Objects.requireNonNull(priority, "priority");
        Objects.requireNonNull(executor, "executor");
        Objects.requireNonNull(plugin, "plugin");
        requireEnabled(plugin, "registerEvent");
        FerritePluginLoader.handlerList(event)
                .register(new RegisteredListener(listener, executor, priority, plugin, ignoreCancelled));
        syncEventMaskToHost();
    }

    private void requireEnabled(Plugin plugin, String operation) {
        if (!plugin.isEnabled()) {
            throw new IllegalStateException("Plugin " + plugin.getName()
                    + " attempted to use PluginManager#" + operation
                    + " while it is not enabled");
        }
    }

    @Override
    public void enablePlugin(Plugin plugin) {
        Objects.requireNonNull(plugin, "plugin");
        if (!(plugin instanceof JavaPlugin javaPlugin)) {
            throw new IllegalArgumentException("Ferrite plugins must extend JavaPlugin, got "
                    + plugin.getClass().getName());
        }
        if (!plugins.containsValue(javaPlugin)) {
            throw new IllegalArgumentException("Plugin " + plugin.getName()
                    + " is not registered with the Ferrite host");
        }
        if (javaPlugin.isEnabled()) {
            return;
        }
        FerriteContext.withPlugin(javaPlugin.getName(), () -> javaPlugin.setEnabled(true));
    }

    @Override
    public void disablePlugin(Plugin plugin) {
        Objects.requireNonNull(plugin, "plugin");
        if (!(plugin instanceof JavaPlugin javaPlugin) || !javaPlugin.isEnabled()) {
            return;
        }
        try {
            FerriteContext.withPlugin(javaPlugin.getName(), () -> javaPlugin.setEnabled(false));
        } finally {
            HandlerList.unregisterAll(javaPlugin);
            runtime.scheduler().cancelTasks(javaPlugin);
            runtime.servicesManager().unregisterAll(javaPlugin);
            runtime.messenger().unregisterIncomingPluginChannel(javaPlugin);
            runtime.messenger().unregisterOutgoingPluginChannel(javaPlugin);
            // A disabled plugin keeps no lifecycle handlers or commands, like Paper's manager.
            dev.ferrite.api.server.FerriteLifecycleManager.dispose(javaPlugin);
            syncEventMaskToHost();
        }
    }

    @Override
    public Permission getPermission(String name) {
        return name == null ? null : permissions.get(name.toLowerCase(Locale.ROOT));
    }

    @Override
    public void addPermission(Permission permission) {
        Objects.requireNonNull(permission, "permission");
        permissions.putIfAbsent(permission.getName().toLowerCase(Locale.ROOT), permission);
    }

    @Override
    public void addPermissions(List<Permission> permissions) {
        for (Permission permission : permissions) {
            addPermission(permission);
        }
    }

    @Override
    public void removePermission(Permission permission) {
        if (permission != null) {
            removePermission(permission.getName());
        }
    }

    @Override
    public void removePermission(String name) {
        if (name != null) {
            permissions.remove(name.toLowerCase(Locale.ROOT));
        }
    }

    @Override
    public void clearPermissions() {
        permissions.clear();
    }

    @Override
    public Set<Permission> getDefaultPermissions(boolean op) {
        Set<Permission> result = new LinkedHashSet<>();
        for (Permission permission : permissions.values()) {
            if (permission.getDefault().getValue(op)) {
                result.add(permission);
            }
        }
        return result;
    }

    @Override
    public void recalculatePermissionDefaults(Permission permission) {
        // The facade has no permission attachments yet; defaults are evaluated on demand.
    }

    @Override
    public void subscribeToPermission(String permission, Permissible permissible) {
        permissionSubscriptions
                .computeIfAbsent(permission.toLowerCase(Locale.ROOT), ignored -> new LinkedHashSet<>())
                .add(permissible);
    }

    @Override
    public void unsubscribeFromPermission(String permission, Permissible permissible) {
        Set<Permissible> subscribers = permissionSubscriptions.get(permission.toLowerCase(Locale.ROOT));
        if (subscribers != null) {
            subscribers.remove(permissible);
        }
    }

    @Override
    public Set<Permissible> getPermissionSubscriptions(String permission) {
        Set<Permissible> subscribers = permissionSubscriptions.get(permission.toLowerCase(Locale.ROOT));
        return subscribers == null ? Set.of() : Set.copyOf(subscribers);
    }

    @Override
    public void subscribeToDefaultPerms(boolean op, Permissible permissible) {
        (op ? defaultTrueSubscriptions : defaultFalseSubscriptions).add(permissible);
    }

    @Override
    public void unsubscribeFromDefaultPerms(boolean op, Permissible permissible) {
        (op ? defaultTrueSubscriptions : defaultFalseSubscriptions).remove(permissible);
    }

    @Override
    public Set<Permissible> getDefaultPermSubscriptions(boolean op) {
        return Set.copyOf(op ? defaultTrueSubscriptions : defaultFalseSubscriptions);
    }

    @Override
    public Set<Permission> getPermissions() {
        return Set.copyOf(permissions.values());
    }

    @Override
    public boolean useTimings() {
        return false;
    }

    /** Log a plugin failure without stopping other plugins or the server tick. */
    public void logPluginFailure(Plugin plugin, String operation, Throwable throwable) {
        runtime.logger().log(Level.SEVERE,
                "Plugin " + plugin.getName() + " failed during " + operation, throwable);
    }
}
