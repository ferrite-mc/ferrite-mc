package dev.ferrite.api.plugin;

import java.io.File;
import java.util.Map;
import java.util.Set;
import java.util.regex.Pattern;
import org.bukkit.event.Event;
import org.bukkit.event.Listener;
import org.bukkit.plugin.InvalidDescriptionException;
import org.bukkit.plugin.InvalidPluginException;
import org.bukkit.plugin.Plugin;
import org.bukkit.plugin.PluginDescriptionFile;
import org.bukkit.plugin.PluginLoader;
import org.bukkit.plugin.RegisteredListener;
import org.bukkit.plugin.UnknownDependencyException;

/**
 * Service-provider {@link PluginLoader} required by {@code JavaPlugin.init}.
 *
 * <p>Paper's API stores a default loader on every JavaPlugin and expects a server-provided
 * service for it. Ferrite's real loader is {@link FerritePluginLoader} (wired into the
 * PluginManager by the runtime); this instance is only the API's placeholder, so every
 * operation fails explicitly.
 */
public final class FerriteServicePluginLoader implements PluginLoader {

    private static final Pattern[] FILTERS = {Pattern.compile("\\.jar$", Pattern.CASE_INSENSITIVE)};

    /**
     * The real loader, installed when a {@link FerriteRuntime} starts. JavaPlugin discovers this
     * service before the runtime hands plugins their loader, so a static slot is the only bridge
     * the service-provider constructor allows.
     */
    private static volatile FerritePluginLoader delegate;

    public FerriteServicePluginLoader() {
    }

    /** Makes the runtime's loader answer the service-provider methods. */
    public static void install(FerritePluginLoader loader) {
        delegate = loader;
    }

    private static FerritePluginLoader delegate(String operation) {
        FerritePluginLoader current = delegate;
        if (current == null) {
            throw new UnsupportedOperationException(
                    "PluginLoader#" + operation + " is owned by the Ferrite host, which is not running");
        }
        return current;
    }

    @Override
    public Plugin loadPlugin(File file)
            throws InvalidPluginException, UnknownDependencyException {
        return delegate("loadPlugin").loadPlugin(file);
    }

    @Override
    public PluginDescriptionFile getPluginDescription(File file) throws InvalidDescriptionException {
        return delegate("getPluginDescription").getPluginDescription(file);
    }

    @Override
    public Pattern[] getPluginFileFilters() {
        return FILTERS.clone();
    }

    @Override
    public Map<Class<? extends Event>, Set<RegisteredListener>> createRegisteredListeners(
            Listener listener, Plugin plugin) {
        return delegate("createRegisteredListeners").createRegisteredListeners(listener, plugin);
    }

    @Override
    public void enablePlugin(Plugin plugin) {
        delegate("enablePlugin").enablePlugin(plugin);
    }

    @Override
    public void disablePlugin(Plugin plugin) {
        delegate("disablePlugin").disablePlugin(plugin);
    }
}
