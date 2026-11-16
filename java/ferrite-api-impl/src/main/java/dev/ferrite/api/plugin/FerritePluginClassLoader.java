package dev.ferrite.api.plugin;

import io.papermc.paper.plugin.configuration.PluginMeta;
import io.papermc.paper.plugin.provider.classloader.ConfiguredPluginClassLoader;
import io.papermc.paper.plugin.provider.classloader.PluginClassLoaderGroup;
import java.io.IOException;
import java.net.URL;
import java.net.URLClassLoader;
import org.bukkit.plugin.java.JavaPlugin;

/**
 * Classloader the Paper API accepts as a plugin loader.
 *
 * <p>{@link JavaPlugin}'s constructor requires its defining classloader to implement
 * {@link ConfiguredPluginClassLoader}; a plain {@link URLClassLoader} is rejected. The host
 * still owns jar discovery and supplies the URLs, while this class supplies the small Paper
 * contract (identity, metadata, plugin registration) and keeps plugin resources child-first
 * so a plugin's own {@code config.yml} wins over the server classpath.
 */
public final class FerritePluginClassLoader extends URLClassLoader
        implements ConfiguredPluginClassLoader {

    private final PluginMeta configuration;
    private JavaPlugin plugin;

    public FerritePluginClassLoader(URL[] urls, ClassLoader parent, PluginMeta configuration) {
        super(urls, parent);
        this.configuration = configuration;
    }

    /** Define a plugin class from bytes, used by the test harness for generated subclasses. */
    public Class<?> definePluginClass(String name, byte[] bytes) {
        return defineClass(name, bytes, 0, bytes.length);
    }

    @Override
    public PluginMeta getConfiguration() {
        if (configuration != null) {
            return configuration;
        }
        return plugin == null ? null : plugin.getPluginMeta();
    }

    @Override
    public Class<?> loadClass(
            String name, boolean resolve, boolean checkGlobal, boolean checkLibraries)
            throws ClassNotFoundException {
        // Ferrite has no Paper plugin groups (shared libraries between plugins). Resolve
        // through this loader's normal parent-first delegation.
        return super.loadClass(name, resolve);
    }

    @Override
    public void init(JavaPlugin plugin) {
        this.plugin = plugin;
    }

    @Override
    public JavaPlugin getPlugin() {
        return plugin;
    }

    @Override
    public PluginClassLoaderGroup getGroup() {
        return null;
    }

    @Override
    public URL getResource(String name) {
        URL own = findResource(name);
        return own != null ? own : super.getResource(name);
    }

    @Override
    public void close() throws IOException {
        super.close();
    }
}
