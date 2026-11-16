package dev.ferrite.api.test;

import dev.ferrite.api.FerriteRuntime;
import dev.ferrite.api.plugin.FerritePluginClassLoader;
import java.io.IOException;
import java.io.InputStream;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.file.Path;
import java.util.jar.JarEntry;
import java.util.jar.JarFile;
import org.bukkit.plugin.PluginDescriptionFile;
import org.bukkit.plugin.java.JavaPlugin;

/**
 * Test-only plugin loader: reads {@code plugin.yml}, builds a classloader over the fixture
 * jar and lets the facade attach the plugin.
 *
 * <p>Jar loading and classloaders belong to the Ferrite host (Lane 1); this harness exists
 * only so the facade tests can exercise a real fixture jar before the host contract lands.
 * Resources are child-first so the plugin's own {@code config.yml} wins over test resources.
 */
public final class PluginJarLoader {

    private PluginJarLoader() {
    }

    public static Loaded load(FerriteRuntime runtime, Path jarPath, Path dataFolder) throws Exception {
        PluginDescriptionFile description;
        try (JarFile jar = new JarFile(jarPath.toFile())) {
            JarEntry entry = jar.getJarEntry("plugin.yml");
            if (entry == null) {
                throw new IllegalStateException(jarPath + " has no plugin.yml");
            }
            try (InputStream input = jar.getInputStream(entry)) {
                description = new PluginDescriptionFile(input);
            }
        }
        URL[] urls = {jarPath.toUri().toURL()};
        FerritePluginClassLoader classLoader = new FerritePluginClassLoader(
                urls, PluginJarLoader.class.getClassLoader(), description);
        Class<?> mainClass;
        try {
            mainClass = Class.forName(description.getMain(), true, classLoader);
        } catch (ClassNotFoundException exception) {
            classLoader.close();
            throw new IllegalStateException("main class " + description.getMain() + " is missing", exception);
        }
        if (!JavaPlugin.class.isAssignableFrom(mainClass)) {
            classLoader.close();
            throw new IllegalStateException(description.getMain() + " does not extend JavaPlugin");
        }
        JavaPlugin plugin = (JavaPlugin) mainClass.getDeclaredConstructor().newInstance();
        try {
            runtime.callOnMainThread(() -> {
                runtime.attachPlugin(
                        plugin, description, jarPath.toFile(), dataFolder.toFile(), classLoader);
                runtime.enablePlugin(plugin);
                return null;
            });
        } catch (RuntimeException exception) {
            classLoader.close();
            throw exception;
        }
        return new Loaded(plugin, classLoader, dataFolder);
    }

    public record Loaded(JavaPlugin plugin, URLClassLoader classLoader, Path dataFolder)
            implements AutoCloseable {

        @Override
        public void close() throws IOException {
            classLoader.close();
        }
    }

}
