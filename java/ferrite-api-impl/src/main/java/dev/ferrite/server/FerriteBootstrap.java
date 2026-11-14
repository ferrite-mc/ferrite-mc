package dev.ferrite.server;

import io.papermc.paper.datapack.DatapackRegistrar;
import io.papermc.paper.datapack.DiscoveredDatapack;
import io.papermc.paper.plugin.bootstrap.BootstrapContext;
import io.papermc.paper.plugin.bootstrap.PluginBootstrap;
import io.papermc.paper.plugin.configuration.PluginMeta;
import io.papermc.paper.plugin.lifecycle.event.LifecycleEvent;
import io.papermc.paper.plugin.lifecycle.event.LifecycleEventManager;
import io.papermc.paper.plugin.lifecycle.event.handler.LifecycleEventHandler;
import io.papermc.paper.plugin.lifecycle.event.handler.configuration.LifecycleEventHandlerConfiguration;
import io.papermc.paper.plugin.lifecycle.event.registrar.RegistrarEvent;
import java.io.IOException;
import java.net.URI;
import java.nio.file.*;
import java.util.*;
import java.util.function.Consumer;

/**
 * Paper's plugin bootstrap phase ({@code bootstrapper} in paper-plugin.yml): the bootstrapper runs before the plugin
 * class is created and may register {@code DATAPACK_DISCOVERY} handlers. A discovered pack is copied into the world's
 * {@code datapacks} folder, where the host already reads packs from (and where {@code /datapack} manages them).
 *
 * <p>{@code TAGS} handlers are refused: tags are fixed data in the host, so a handler could never change anything.
 */
final class FerriteBootstrap {
    static final String DATAPACK_DISCOVERY = "datapack_discovery";
    private final PluginMeta meta;
    private final Path source;
    private final Path dataDirectory;
    private final Path worldPath;
    private final List<FerriteLifecycleManager.Configuration> handlers = new ArrayList<>();

    FerriteBootstrap(PluginMeta meta, Path source, Path dataDirectory, Path worldPath) {
        this.meta = meta; this.source = source; this.dataDirectory = dataDirectory; this.worldPath = worldPath;
    }

    /** Instantiates and runs the bootstrapper, then fires {@code DATAPACK_DISCOVERY} for what it registered. */
    static void run(String bootstrapperClass, ClassLoader loader, PluginMeta meta, Path source, Path dataDirectory, Path worldPath) throws ReflectiveOperationException {
        var bootstrap = new FerriteBootstrap(meta, source, dataDirectory, worldPath);
        var instance = Class.forName(bootstrapperClass, true, loader).asSubclass(PluginBootstrap.class).getDeclaredConstructor().newInstance();
        instance.bootstrap(bootstrap.context());
        bootstrap.fire();
    }

    BootstrapContext context() {
        var manager = new Manager();
        return new BootstrapContext() {
            @Override public LifecycleEventManager<BootstrapContext> getLifecycleManager() { return manager; }
            @Override public PluginMeta getConfiguration() { return meta; }
            @Override public PluginMeta getPluginMeta() { return meta; }
            @Override public Path getDataDirectory() { return dataDirectory; }
            @Override public net.kyori.adventure.text.logger.slf4j.ComponentLogger getLogger() { return net.kyori.adventure.text.logger.slf4j.ComponentLogger.logger(meta.getLoggerPrefix() == null ? meta.getName() : meta.getLoggerPrefix()); }
            @Override public Path getPluginSource() { return source; }
        };
    }

    private final class Manager implements LifecycleEventManager<BootstrapContext> {
        @SuppressWarnings({"unchecked", "rawtypes"})
        @Override public void registerEventHandler(LifecycleEventHandlerConfiguration<? super BootstrapContext> configuration) {
            Objects.requireNonNull(configuration, "configuration");
            if (!((Object) configuration instanceof FerriteLifecycleManager.Configuration config))
                throw new IllegalArgumentException("Ferrite: unknown lifecycle handler configuration " + configuration.getClass().getName());
            var name = config.eventType().name();
            if (!DATAPACK_DISCOVERY.equals(name))
                throw new UnsupportedOperationException("Ferrite: lifecycle event '" + name + "' cannot be used: registry tags are fixed vanilla data in the host (only 'datapack_discovery' is supported during bootstrap)");
            handlers.add(config);
        }
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    void fire() {
        if (handlers.isEmpty()) return;
        var registrar = new Registrar();
        var ordered = new ArrayList<>(handlers);
        ordered.sort(Comparator.<FerriteLifecycleManager.Configuration, Boolean>comparing(FerriteLifecycleManager.Configuration::isMonitorHandler).thenComparingInt(FerriteLifecycleManager.Configuration::eventPriority));
        var event = new Event(registrar);
        for (var handler : ordered) {
            try { ((LifecycleEventHandler) handler.eventHandler()).run(event); }
            catch (Throwable error) { java.util.logging.Logger.getLogger(meta.getName()).log(java.util.logging.Level.SEVERE, "Could not run lifecycle event handler for '" + DATAPACK_DISCOVERY + "'", error); }
        }
    }

    private record Event(DatapackRegistrar registrar) implements RegistrarEvent<DatapackRegistrar>, LifecycleEvent {}

    private record Discovered(String name, net.kyori.adventure.text.Component title, net.kyori.adventure.text.Component description) implements DiscoveredDatapack {
        @Override public String getName() { return name; }
        @Override public net.kyori.adventure.text.Component getTitle() { return title; }
        @Override public net.kyori.adventure.text.Component getDescription() { return description; }
        @Override public boolean isRequired() { return false; }
        @Override public io.papermc.paper.datapack.Datapack.Compatibility getCompatibility() { return io.papermc.paper.datapack.Datapack.Compatibility.COMPATIBLE; }
        @Override public Set<org.bukkit.FeatureFlag> getRequiredFeatures() { return Set.of(); }
        @Override public io.papermc.paper.datapack.DatapackSource getSource() { return io.papermc.paper.datapack.DatapackSource.PLUGIN; }
    }

    private final class Registrar implements DatapackRegistrar {
        private final Map<String, DiscoveredDatapack> discovered = new LinkedHashMap<>();

        private Path folder() {
            if (worldPath == null) throw new IllegalStateException("Ferrite: this world is not saved to disk, so a datapack cannot be discovered");
            return worldPath.resolve("datapacks");
        }
        @Override public boolean hasPackDiscovered(String name) { return discovered.containsKey(name); }
        @Override public DiscoveredDatapack getDiscoveredPack(String name) { return discovered.get(name); }
        @Override public Map<String, DiscoveredDatapack> getDiscoveredPacks() { return Map.copyOf(discovered); }
        @Override public boolean removeDiscoveredPack(String name) {
            if (discovered.remove(name) == null) return false;
            try { deleteTree(folder().resolve(name)); } catch (IOException e) { throw new IllegalStateException("Cannot remove datapack " + name, e); }
            return true;
        }
        @Override public DiscoveredDatapack discoverPack(URI uri, String id, Consumer<Configurer> configurer) throws IOException { return discover(meta, uri, id, configurer); }
        @Override public DiscoveredDatapack discoverPack(Path path, String id, Consumer<Configurer> configurer) throws IOException { return discover(meta, path, id, configurer); }
        @Override public DiscoveredDatapack discoverPack(PluginMeta owner, URI uri, String id, Consumer<Configurer> configurer) throws IOException { return discover(owner, uri, id, configurer); }
        @Override public DiscoveredDatapack discoverPack(PluginMeta owner, Path path, String id, Consumer<Configurer> configurer) throws IOException { return discover(owner, path, id, configurer); }

        private DiscoveredDatapack discover(PluginMeta owner, URI uri, String id, Consumer<Configurer> configurer) throws IOException {
            Objects.requireNonNull(uri, "uri");
            if (!"jar".equals(uri.getScheme())) return discover(owner, Path.of(uri), id, configurer);
            // A pack bundled in the plugin jar: open the jar as a file system for the copy.
            var specific = uri.getSchemeSpecificPart();
            var jar = URI.create("jar:" + specific.substring(0, specific.indexOf("!/")));
            try (var fs = FileSystems.newFileSystem(jar, Map.of())) {
                return discover(owner, fs.getPath(specific.substring(specific.indexOf("!/") + 1)), id, configurer);
            } catch (FileSystemAlreadyExistsException existing) {
                return discover(owner, FileSystems.getFileSystem(jar).getPath(specific.substring(specific.indexOf("!/") + 1)), id, configurer);
            }
        }

        private DiscoveredDatapack discover(PluginMeta owner, Path path, String id, Consumer<Configurer> configurer) throws IOException {
            Objects.requireNonNull(path, "path"); Objects.requireNonNull(id, "id");
            if (id.isEmpty() || id.startsWith(".") || id.contains("/") || id.contains("\\")) throw new IllegalArgumentException("Invalid datapack id '" + id + "'");
            var state = new Config();
            if (configurer != null) configurer.accept(state);
            var name = owner.getName() + "-" + id;
            var target = folder().resolve(name);
            deleteTree(target);
            Files.createDirectories(target);
            if (Files.isDirectory(path)) copyTree(path, target);
            else unzip(path, target);
            if (!Files.isRegularFile(target.resolve("pack.mcmeta"))) {
                deleteTree(target);
                throw new IOException("Datapack " + id + " has no pack.mcmeta");
            }
            var disabled = new TreeSet<>(readLines(folder().resolve(".ferrite-disabled")));
            if (state.autoEnable) disabled.remove(name); else disabled.add(name);
            Files.writeString(folder().resolve(".ferrite-disabled"), String.join("\n", disabled));
            if (state.position != null) {
                var order = new ArrayList<>(readLines(folder().resolve(".ferrite-order")));
                order.remove(name);
                order.add(state.position.first() ? 0 : order.size(), name);
                Files.writeString(folder().resolve(".ferrite-order"), String.join("\n", order));
            }
            var title = state.title != null ? state.title : net.kyori.adventure.text.Component.text(id);
            var pack = new Discovered(name, title, net.kyori.adventure.text.Component.empty());
            discovered.put(name, pack);
            return pack;
        }
    }

    private record Position(boolean first, io.papermc.paper.datapack.Datapack.Position relative) {}

    private static final class Config implements DatapackRegistrar.Configurer {
        net.kyori.adventure.text.Component title;
        boolean autoEnable = true;
        Position position;
        @Override public DatapackRegistrar.Configurer title(net.kyori.adventure.text.Component value) { title = Objects.requireNonNull(value, "title"); return this; }
        @Override public DatapackRegistrar.Configurer autoEnableOnServerStart(boolean value) { autoEnable = value; return this; }
        @Override public DatapackRegistrar.Configurer position(boolean fixed, io.papermc.paper.datapack.Datapack.Position value) { position = new Position(value == io.papermc.paper.datapack.Datapack.Position.TOP, value); return this; }
    }

    private static List<String> readLines(Path file) throws IOException {
        if (!Files.isRegularFile(file)) return List.of();
        return Files.readAllLines(file).stream().map(String::trim).filter(line -> !line.isEmpty()).toList();
    }

    private static void copyTree(Path from, Path to) throws IOException {
        try (var walk = Files.walk(from)) {
            for (var entry : (Iterable<Path>) walk::iterator) {
                var destination = to.resolve(from.relativize(entry).toString());
                if (Files.isDirectory(entry)) Files.createDirectories(destination);
                else Files.copy(entry, destination, StandardCopyOption.REPLACE_EXISTING);
            }
        }
    }

    private static void unzip(Path zip, Path to) throws IOException {
        try (var in = new java.util.zip.ZipInputStream(Files.newInputStream(zip))) {
            for (java.util.zip.ZipEntry entry; (entry = in.getNextEntry()) != null;) {
                var destination = to.resolve(entry.getName()).normalize();
                if (!destination.startsWith(to)) throw new IOException("Zip entry escapes the pack: " + entry.getName());
                if (entry.isDirectory()) Files.createDirectories(destination);
                else { Files.createDirectories(destination.getParent()); Files.copy(in, destination, StandardCopyOption.REPLACE_EXISTING); }
            }
        }
    }

    static void deleteTree(Path root) throws IOException {
        if (!Files.exists(root)) return;
        try (var walk = Files.walk(root)) {
            for (var entry : (Iterable<Path>) walk.sorted(Comparator.reverseOrder())::iterator) Files.delete(entry);
        }
    }
}
