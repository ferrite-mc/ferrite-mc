package dev.ferrite.api.registry;

import io.papermc.paper.entity.poi.PoiType;
import org.bukkit.NamespacedKey;
import org.bukkit.block.data.BlockData;

/**
 * A point-of-interest type from {@code reports/registries.json}.
 *
 * <p>Vanilla defines the matching block states and occupant flag in code, not in the data
 * generator output, so the key is all the facade can reproduce. Block matching reports
 * {@code false} and occupancy {@code true} (every generated POI type is occupiable).
 */
public final class FerritePoiType implements PoiType {

    private final NamespacedKey key;

    public FerritePoiType(NamespacedKey key) {
        this.key = key;
    }

    @Override
    public NamespacedKey getKey() {
        return this.key;
    }

    @Override
    public boolean is(BlockData blockData) {
        return false;
    }

    @Override
    public boolean hasOccupants() {
        return true;
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof FerritePoiType type && this.key.equals(type.key);
    }

    @Override
    public int hashCode() {
        return this.key.hashCode();
    }

    @Override
    public String toString() {
        return "FerritePoiType[" + this.key + "]";
    }
}
