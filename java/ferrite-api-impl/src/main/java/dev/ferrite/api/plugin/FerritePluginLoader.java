package dev.ferrite.api.plugin;

import dev.ferrite.api.FerriteRuntime;
import dev.ferrite.api.internal.FerriteContext;
import dev.ferrite.api.internal.Unsupported;
import java.io.File;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.Map;
import java.util.Set;
import java.util.regex.Pattern;
import org.bukkit.event.Event;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventException;
import org.bukkit.event.HandlerList;
import org.bukkit.event.Listener;
import org.bukkit.plugin.EventExecutor;
import org.bukkit.plugin.InvalidDescriptionException;
import org.bukkit.plugin.InvalidPluginException;
import org.bukkit.plugin.Plugin;
import org.bukkit.plugin.PluginDescriptionFile;
import org.bukkit.plugin.PluginLoader;
import org.bukkit.plugin.RegisteredListener;
import org.bukkit.plugin.UnknownDependencyException;

/**
 * {@link PluginLoader} for the facade.
 *
 * <p>Jar discovery and classloading belong to the Ferrite host (Lane 1). This loader
 * exists because {@link org.bukkit.plugin.java.JavaPlugin#getPluginLoader()} and
 * {@link org.bukkit.plugin.PluginManager#registerEvents} need one, and because listener
 * scanning is Bukkit semantics owned by this lane.
 */
public final class FerritePluginLoader implements PluginLoader {

    private static final Pattern[] FILTERS = {Pattern.compile("\\.jar$", Pattern.CASE_INSENSITIVE)};

    private final FerriteRuntime runtime;

    public FerritePluginLoader(FerriteRuntime runtime) {
        this.runtime = runtime;
    }

    @Override
    public Plugin loadPlugin(File file)
            throws InvalidPluginException, UnknownDependencyException {
        // Jar class loading belongs to the Ferrite host's plugin bootstrapper.
        throw Unsupported.exception("PluginLoader#loadPlugin");
    }

    @Override
    public PluginDescriptionFile getPluginDescription(File file) throws InvalidDescriptionException {
        // Reading plugin.yml belongs to the Ferrite host's plugin bootstrapper.
        throw Unsupported.exception("PluginLoader#getPluginDescription");
    }

    @Override
    public Pattern[] getPluginFileFilters() {
        return FILTERS.clone();
    }

    @Override
    public Map<Class<? extends Event>, Set<RegisteredListener>> createRegisteredListeners(
            Listener listener, Plugin plugin) {
        Map<Class<? extends Event>, Set<RegisteredListener>> result = new LinkedHashMap<>();
        for (Method method : listener.getClass().getMethods()) {
            EventHandler annotation = method.getAnnotation(EventHandler.class);
            if (annotation == null || method.isBridge() || method.isSynthetic()) {
                continue;
            }
            if (Modifier.isStatic(method.getModifiers())) {
                throw new IllegalArgumentException(
                        "Listener method " + method + " must not be static");
            }
            Class<?>[] parameters = method.getParameterTypes();
            if (parameters.length != 1 || !Event.class.isAssignableFrom(parameters[0])) {
                throw new IllegalArgumentException("Listener method " + method
                        + " must have exactly one Event parameter");
            }
            @SuppressWarnings("unchecked")
            Class<? extends Event> eventClass = (Class<? extends Event>) parameters[0];
            EventExecutor executor = EventExecutor.create(method, eventClass);
            RegisteredListener registered = new RegisteredListener(
                    listener,
                    new ContextualExecutor(executor, plugin.getName()),
                    annotation.priority(),
                    plugin,
                    annotation.ignoreCancelled());
            result.computeIfAbsent(eventClass, ignored -> new LinkedHashSet<>()).add(registered);
        }
        return result;
    }

    @Override
    public void enablePlugin(Plugin plugin) {
        runtime.plugins().enablePlugin(plugin);
    }

    @Override
    public void disablePlugin(Plugin plugin) {
        runtime.plugins().disablePlugin(plugin);
    }

    /** Resolve the event class's Bukkit handler list. */
    static HandlerList handlerList(Class<? extends Event> eventClass) {
        try {
            Method getHandlerList = eventClass.getMethod("getHandlerList");
            if (!Modifier.isStatic(getHandlerList.getModifiers())) {
                throw new IllegalArgumentException(
                        eventClass.getName() + ".getHandlerList() is not static");
            }
            return (HandlerList) getHandlerList.invoke(null);
        } catch (NoSuchMethodException exception) {
            throw new IllegalArgumentException(
                    eventClass.getName() + " does not expose a static getHandlerList()", exception);
        } catch (ReflectiveOperationException exception) {
            throw new IllegalStateException(
                    "could not read the handler list of " + eventClass.getName(), exception);
        }
    }

    private static final class ContextualExecutor implements EventExecutor {

        private final EventExecutor delegate;
        private final String pluginName;

        private ContextualExecutor(EventExecutor delegate, String pluginName) {
            this.delegate = delegate;
            this.pluginName = pluginName;
        }

        @Override
        public void execute(Listener listener, Event event) throws EventException {
            String previous = FerriteContext.push(pluginName);
            try {
                delegate.execute(listener, event);
            } finally {
                FerriteContext.pop(previous);
            }
        }
    }
}
