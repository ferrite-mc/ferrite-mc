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

    public FerriteServicePluginLoader() {
    }

    @Override
    public Plugin loadPlugin(File file)
            throws InvalidPluginException, UnknownDependencyException {
        throw new UnsupportedOperationException(
                "PluginLoader#loadPlugin is owned by the Ferrite host");
    }

    @Override
    public PluginDescriptionFile getPluginDescription(File file) throws InvalidDescriptionException {
        throw new UnsupportedOperationException(
                "PluginLoader#getPluginDescription is owned by the Ferrite host");
    }

    @Override
    public Pattern[] getPluginFileFilters() {
        return FILTERS.clone();
    }

    @Override
    public Map<Class<? extends Event>, Set<RegisteredListener>> createRegisteredListeners(
            Listener listener, Plugin plugin) {
        throw new UnsupportedOperationException(
                "PluginLoader#createRegisteredListeners is provided by FerritePluginLoader");
    }

    @Override
    public void enablePlugin(Plugin plugin) {
        throw new UnsupportedOperationException(
                "PluginLoader#enablePlugin is provided by FerritePluginManager");
    }

    @Override
    public void disablePlugin(Plugin plugin) {
        throw new UnsupportedOperationException(
                "PluginLoader#disablePlugin is provided by FerritePluginManager");
    }
}
