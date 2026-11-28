package dev.ferrite.api.server;

import io.papermc.paper.command.brigadier.Commands;
import io.papermc.paper.plugin.lifecycle.event.LifecycleEvent;
import io.papermc.paper.plugin.lifecycle.event.LifecycleEventManager;
import io.papermc.paper.plugin.lifecycle.event.handler.LifecycleEventHandler;
import io.papermc.paper.plugin.lifecycle.event.handler.configuration.LifecycleEventHandlerConfiguration;
import io.papermc.paper.plugin.lifecycle.event.registrar.ReloadableRegistrarEvent;
import io.papermc.paper.plugin.lifecycle.event.types.LifecycleEventType;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Comparator;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.atomic.AtomicLong;
import java.util.function.BooleanSupplier;
import java.util.logging.Level;
import org.bukkit.plugin.Plugin;

/**
 * A plugin's Paper lifecycle event manager ({@code JavaPlugin#getLifecycleManager()}).
 *
 * <p>The only plugin-owned lifecycle event this host can fire is {@code LifecycleEvents.COMMANDS}:
 * it fires once with cause {@code INITIAL} after every plugin has been enabled and again with
 * {@code RELOAD} when the server asks. The other event types the API declares belong to
 * {@code BootstrapContext} (tags, datapack discovery); the host does not run
 * {@code PluginBootstrap}, so nothing can ever fire them and they are refused instead of
 * pretending to be registered.
 *
 * <p>Handlers run ordered by monitor flag (monitors last), then priority (ascending), then
 * registration order, across all plugins. A handler that throws is logged and the next one still
 * runs. Registration is only legal while {@code allowed} is true (Paper allows it from
 * construction until the end of {@code onEnable}). A plugin's handlers and the commands they
 * registered are removed when it is disabled.
 */
public final class FerriteLifecycleManager implements LifecycleEventManager<Plugin> {
    /** The one event name a plugin-owned manager can fire. */
    public static final String COMMANDS = "commands";

    private static final AtomicLong SEQUENCE = new AtomicLong();
    // Identity keys: the manager is created inside JavaPlugin's constructor, before the
    // plugin's own hashCode (which reads its meta) works.
    private static final Map<Plugin, FerriteLifecycleManager> MANAGERS =
            Collections.synchronizedMap(new IdentityHashMap<>());

    private static List<FerriteLifecycleManager> managers() {
        synchronized (MANAGERS) {
            return new ArrayList<>(MANAGERS.values());
        }
    }

    record Registration(
            String type,
            int priority,
            boolean monitor,
            long order,
            LifecycleEventHandler<? super ReloadableRegistrarEvent<Commands>> handler) {
    }

    private final Plugin plugin;
    private final BooleanSupplier allowed;
    private final List<Registration> registrations = new CopyOnWriteArrayList<>();
    private final FerriteCommands commands;
    private boolean initialFired;

    public FerriteLifecycleManager(Plugin plugin, BooleanSupplier allowed) {
        this.plugin = Objects.requireNonNull(plugin, "plugin");
        this.allowed = Objects.requireNonNull(allowed, "allowed");
        this.commands = new FerriteCommands(plugin);
        MANAGERS.put(plugin, this);
    }

    /** A handler configuration built by {@link FerriteLifecycleTypes}. */
    @SuppressWarnings("rawtypes")
    interface Configuration {
        LifecycleEventType eventType();

        LifecycleEventHandler eventHandler();

        int eventPriority();

        boolean isMonitorHandler();
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    @Override
    public void registerEventHandler(
            LifecycleEventHandlerConfiguration<? super Plugin> configuration) {
        Objects.requireNonNull(configuration, "configuration");
        if (!((Object) configuration instanceof Configuration config)) {
            throw new IllegalArgumentException(
                    "Ferrite: unknown lifecycle handler configuration "
                            + configuration.getClass().getName());
        }
        if (!allowed.getAsBoolean()) {
            throw new IllegalStateException(
                    "Cannot register lifecycle event handlers for " + plugin.getName()
                            + " after it has finished enabling");
        }
        String name = config.eventType().name();
        if (!COMMANDS.equals(name)) {
            throw new UnsupportedOperationException(
                    "Ferrite: lifecycle event '" + name
                            + "' can never fire: it belongs to the plugin bootstrap phase, which"
                            + " the host does not run (only 'commands' is supported)");
        }
        registrations.add(new Registration(
                name,
                config.eventPriority(),
                config.isMonitorHandler(),
                SEQUENCE.incrementAndGet(),
                config.eventHandler()));
    }

    /** Number of handlers this plugin currently has registered. */
    public int handlerCount() {
        return registrations.size();
    }

    /** Drops this plugin's handlers and the commands they registered (plugin disabled). */
    public void dispose() {
        registrations.clear();
        commands.clear();
        MANAGERS.remove(plugin, this);
    }

    /** {@link #dispose()} for a plugin that has a manager. */
    public static void dispose(Plugin plugin) {
        FerriteLifecycleManager manager = MANAGERS.get(plugin);
        if (manager != null) {
            manager.dispose();
        }
    }

    /** Drops every plugin's handlers: the plugin manager is being cleared. */
    public static void disposeAll() {
        for (FerriteLifecycleManager manager : managers()) {
            manager.dispose();
        }
    }

    /**
     * Fires the {@code COMMANDS} event for every enabled plugin that has not seen the cause yet.
     * A reload first forgets the commands the previous round registered, like Paper's reload of
     * the command dispatcher.
     */
    public static void fireCommands(ReloadableRegistrarEvent.Cause cause) {
        Objects.requireNonNull(cause, "cause");
        List<Map.Entry<FerriteLifecycleManager, Registration>> queue = new ArrayList<>();
        for (FerriteLifecycleManager manager : managers()) {
            if (!manager.plugin.isEnabled()) {
                continue;
            }
            if (cause == ReloadableRegistrarEvent.Cause.INITIAL) {
                if (manager.initialFired) {
                    continue;
                }
                manager.initialFired = true;
            } else {
                manager.commands.clear();
            }
            for (Registration registration : manager.registrations) {
                queue.add(Map.entry(manager, registration));
            }
        }
        queue.sort(Comparator
                .<Map.Entry<FerriteLifecycleManager, Registration>, Boolean>comparing(
                        entry -> entry.getValue().monitor())
                .thenComparingInt(entry -> entry.getValue().priority())
                .thenComparingLong(entry -> entry.getValue().order()));
        for (Map.Entry<FerriteLifecycleManager, Registration> entry : queue) {
            FerriteLifecycleManager manager = entry.getKey();
            CommandsEvent event = new CommandsEvent(manager.commands, cause);
            try {
                entry.getValue().handler().run(event);
            } catch (Throwable error) {
                manager.plugin.getLogger().log(
                        Level.SEVERE,
                        "Could not run lifecycle event handler for '" + COMMANDS + "'",
                        error);
            }
        }
    }

    private record CommandsEvent(Commands registrar, Cause cause)
            implements ReloadableRegistrarEvent<Commands>, LifecycleEvent {
    }
}
