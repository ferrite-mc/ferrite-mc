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
    private final List<Datapack> packs = List.of(vanilla);

    /** Rebuilds the host's datapack function store from disk; the built-in vanilla pack is fixed. */
    @Override public void refreshPacks() {
        HostBridge.callVoid("Server", "server", "reloadData()");
    }

    @Override public Datapack getPack(String name) {
        Objects.requireNonNull(name, "name");
        for (var pack : packs) if (pack.getName().equals(name)) return pack;
        return null;
    }

    @Override public Collection<Datapack> getPacks() { return packs; }

    @Override public Collection<Datapack> getEnabledPacks() {
        return packs.stream().filter(Datapack::isEnabled).toList();
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
