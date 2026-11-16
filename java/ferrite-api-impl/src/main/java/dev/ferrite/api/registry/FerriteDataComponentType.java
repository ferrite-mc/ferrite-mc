package dev.ferrite.api.registry;

import io.papermc.paper.datacomponent.DataComponentType;
import org.bukkit.NamespacedKey;

/** A data component type from {@code reports/registries.json}. */
public final class FerriteDataComponentType implements DataComponentType {

    private final NamespacedKey key;

    public FerriteDataComponentType(NamespacedKey key) {
        this.key = key;
    }

    @Override
    public NamespacedKey getKey() {
        return this.key;
    }

    @Override
    public boolean isPersistent() {
        // Every type in the vanilla data component registry is serialized into item
        // stacks; transient component types are code-only and never registered.
        return true;
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof FerriteDataComponentType type && this.key.equals(type.key);
    }

    @Override
    public int hashCode() {
        return this.key.hashCode();
    }

    @Override
    public String toString() {
        return "FerriteDataComponentType[" + this.key + "]";
    }
}
