package dev.ferrite.server;

import io.papermc.paper.datapack.Datapack;
import io.papermc.paper.datapack.DatapackManager;
import io.papermc.paper.datapack.DatapackSource;
import java.util.Collection;
import java.util.List;
import java.util.Objects;
import java.util.Set;
import net.kyori.adventure.text.Component;
import org.bukkit.FeatureFlag;

/**
 * Datapacks known to the server (Server#getDatapackManager).
 *
 * <p>Ferrite only ships the built-in vanilla pack; it is required, so it is always enabled and
 * cannot be toggled. Reading the server and world datapack folders needs the Rust host.
 */
final class FerriteDatapackManager implements DatapackManager {
    private final Datapack vanilla = new VanillaDatapack();
    private final java.util.function.Supplier<java.nio.file.Path> worldPath;

    FerriteDatapackManager() { this(FerriteDatapackManager::defaultWorldPath); }
    FerriteDatapackManager(java.util.function.Supplier<java.nio.file.Path> worldPath) { this.worldPath = worldPath; }

    private static java.nio.file.Path defaultWorldPath() {
        var worlds = org.bukkit.Bukkit.getWorlds();
        if (worlds.isEmpty()) return null;
        try { return worlds.get(0).getWorldPath(); } catch (UnsupportedOperationException unsaved) { return null; }
    }

    /** The built-in pack plus every pack folder in the world's {@code datapacks} directory, in the host's load order. */
    private List<Datapack> packs() {
        var all = new java.util.ArrayList<Datapack>();
        all.add(vanilla);
        var world = worldPath.get();
        if (world == null) return all;
        var folder = world.resolve("datapacks");
        try (var entries = java.nio.file.Files.list(folder)) {
            var disabled = lines(folder.resolve(".ferrite-disabled"));
            var order = lines(folder.resolve(".ferrite-order"));
            var names = entries.filter(java.nio.file.Files::isDirectory).map(path -> path.getFileName().toString()).filter(name -> !name.startsWith(".")).sorted(
                java.util.Comparator.<String>comparingInt(name -> order.indexOf(name) < 0 ? Integer.MAX_VALUE : order.indexOf(name)).thenComparing(java.util.Comparator.naturalOrder())).toList();
            for (var name : names) all.add(new FolderDatapack(folder, name, !disabled.contains(name)));
        } catch (java.io.IOException none) { /* no datapacks folder */ }
        return all;
    }

    private static List<String> lines(java.nio.file.Path file) {
        try { return java.nio.file.Files.isRegularFile(file) ? java.nio.file.Files.readAllLines(file).stream().map(String::trim).filter(line -> !line.isEmpty()).toList() : List.of(); }
        catch (java.io.IOException unreadable) { return List.of(); }
    }

    private final class FolderDatapack implements Datapack {
        private final java.nio.file.Path folder;
        private final String name;
        private volatile boolean enabled;
        FolderDatapack(java.nio.file.Path folder, String name, boolean enabled) { this.folder = folder; this.name = name; this.enabled = enabled; }
        @Override public String getName() { return "file/" + name; }
        @Override public Component getTitle() { return Component.text(name); }
        @Override public Component getDescription() { return Component.empty(); }
        @Override public boolean isRequired() { return false; }
        @Override public Compatibility getCompatibility() { return Compatibility.COMPATIBLE; }
        @Override public Set<FeatureFlag> getRequiredFeatures() { return Set.of(); }
        @Override public DatapackSource getSource() { return DatapackSource.WORLD; }
        @Override public boolean isEnabled() { return enabled; }
        @Override public void setEnabled(boolean value) {
            if (value == enabled) return;
            var disabled = new java.util.TreeSet<>(lines(folder.resolve(".ferrite-disabled")));
            if (value) disabled.remove(name); else disabled.add(name);
            try { java.nio.file.Files.writeString(folder.resolve(".ferrite-disabled"), String.join("\n", disabled)); }
            catch (java.io.IOException failure) { throw new IllegalStateException("Cannot save the datapack state of " + name, failure); }
            enabled = value;
            refreshPacks();
        }
        @Override public Component computeDisplayName() { return getTitle(); }
    }

    /** Rebuilds the host's datapack function store from disk; the built-in vanilla pack is fixed. */
    @Override public void refreshPacks() {
        HostBridge.callVoid("Server", "server", "reloadData()");
    }

    @Override public Datapack getPack(String name) {
        Objects.requireNonNull(name, "name");
        for (var pack : packs()) if (pack.getName().equals(name) || pack.getName().equals("file/" + name)) return pack;
        return null;
    }

    @Override public Collection<Datapack> getPacks() { return packs(); }

    @Override public Collection<Datapack> getEnabledPacks() {
        return packs().stream().filter(Datapack::isEnabled).toList();
    }

    private static final class VanillaDatapack implements Datapack {
        private volatile boolean enabled = true;

        @Override public String getName() { return "vanilla"; }
        @Override public Component getTitle() { return Component.text("Vanilla"); }
        @Override public Component getDescription() { return Component.text("The built-in vanilla data pack"); }
        @Override public boolean isRequired() { return true; }
        @Override public Compatibility getCompatibility() { return Compatibility.COMPATIBLE; }
        @Override public Set<FeatureFlag> getRequiredFeatures() { return Set.of(); }
        @Override public DatapackSource getSource() { return DatapackSource.BUILT_IN; }
        @Override public boolean isEnabled() { return enabled; }
        @Override public void setEnabled(boolean enabled) {
            if (!enabled) throw new IllegalArgumentException("The vanilla data pack is required and cannot be disabled");
            this.enabled = true;
        }
        @Override public Component computeDisplayName() { return getTitle(); }
    }
}
