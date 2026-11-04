package dev.ferrite.server;
import java.io.*;
import java.net.*;
import org.bukkit.plugin.PluginDescriptionFile;
import org.bukkit.plugin.java.JavaPlugin;
import io.papermc.paper.plugin.provider.classloader.*;
final class FerriteClassLoader extends URLClassLoader implements ConfiguredPluginClassLoader {
    private final FerriteServer server;
    private final File file;
    private final PluginDescriptionFile description;
    private record Lookup(FerriteClassLoader loader, String name) {}
    private static final ThreadLocal<java.util.Set<Lookup>> LOOKUP = ThreadLocal.withInitial(java.util.HashSet::new);
    private final java.util.List<String> dependencies;
    private final java.util.Map<String, FerriteClassLoader> loaders;
    private JavaPlugin plugin;
    FerriteClassLoader(FerriteServer server, File file, PluginDescriptionFile description, java.util.List<String> dependencies, java.util.Map<String, FerriteClassLoader> loaders) throws MalformedURLException {
        super(new URL[]{file.toURI().toURL()}, FerriteClassLoader.class.getClassLoader());
        this.server = server; this.file = file; this.description = description; this.dependencies = java.util.List.copyOf(dependencies); this.loaders = loaders;
    }
    @Override protected Class<?> findClass(String name) throws ClassNotFoundException {
        java.util.Set<Lookup> visited = LOOKUP.get();
        Lookup lookup = new Lookup(this, name);
        if (!visited.add(lookup)) throw new ClassNotFoundException(name);
        try {
            try { return super.findClass(name); }
            catch (ClassNotFoundException original) {
                for (String key : dependencies) {
                    FerriteClassLoader dependency = loaders.get(key);
                    if (dependency != null) try { return dependency.loadClass(name); } catch (ClassNotFoundException ignored) { }
                }
                throw original;
            }
        } finally { visited.remove(lookup); if (visited.isEmpty()) LOOKUP.remove(); }
    }
    /** Adds the plugin's resolved {@code libraries:} jars to its own classpath. */
    void addLibraries(java.util.List<URL> libraries) { for (URL library : libraries) addURL(library); }
    public PluginDescriptionFile getConfiguration() { return description; }
    public Class<?> loadClass(String name, boolean resolve, boolean checkGlobal, boolean checkLibraries) throws ClassNotFoundException { return loadClass(name, resolve); }
    public void init(JavaPlugin plugin) {
        this.plugin = plugin;
        plugin.init(server, description, new File(file.getParentFile(), description.getName()), file, this, description, java.util.logging.Logger.getLogger(description.getName()));
    }
    public JavaPlugin getPlugin() { return plugin; }
    public PluginClassLoaderGroup getGroup() { return null; }
}
