package dev.ferrite.api.registry;

import io.papermc.paper.datacomponent.DataComponentType;
import java.util.Set;
import org.bukkit.NamespacedKey;

/** A data component type from {@code reports/registries.json}. */
public sealed class FerriteDataComponentType implements DataComponentType
        permits FerriteDataComponentType.Valued, FerriteDataComponentType.NonValued {

    private static final Set<String> NON_VALUED_KEYS = Set.of(
            "minecraft:unbreakable",
            "minecraft:intangible_projectile",
            "minecraft:glider",
            "minecraft:waxed");

    private final NamespacedKey key;

    private FerriteDataComponentType(NamespacedKey key) {
        this.key = key;
    }

    public static FerriteDataComponentType create(NamespacedKey key) {
        if (NON_VALUED_KEYS.contains(key.toString())) {
            return new NonValued(key);
        }
        return new Valued(key);
    }

    @Override
    public NamespacedKey getKey() {
        return this.key;
    }

    @Override
    public boolean isPersistent() {
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

    public static final class Valued extends FerriteDataComponentType
            implements DataComponentType.Valued<Object> {
        private Valued(NamespacedKey key) {
            super(key);
        }
    }

    public static final class NonValued extends FerriteDataComponentType
            implements DataComponentType.NonValued {
        private NonValued(NamespacedKey key) {
            super(key);
        }
    }
}
