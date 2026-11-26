package dev.ferrite.api.registry;

import io.papermc.paper.registry.RegistryKey;
import io.papermc.paper.registry.TypedKey;
import io.papermc.paper.registry.set.RegistryKeySet;
import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import org.bukkit.Keyed;
import org.bukkit.Registry;

/** A {@link RegistryKeySet} built from resolved vanilla datapack selectors. */
public final class FerriteKeySet<T extends Keyed> implements RegistryKeySet<T> {

    private final RegistryKey<T> registryKey;
    private final List<TypedKey<T>> values;

    public FerriteKeySet(RegistryKey<T> registryKey, Collection<String> keys) {
        this.registryKey = registryKey;
        List<TypedKey<T>> typed = new ArrayList<>(keys.size());
        for (String key : keys) {
            typed.add(TypedKey.create(registryKey, key));
        }
        this.values = List.copyOf(typed);
    }

    @Override
    public RegistryKey<T> registryKey() {
        return this.registryKey;
    }

    @Override
    public int size() {
        return this.values.size();
    }

    @Override
    public Collection<TypedKey<T>> values() {
        return this.values;
    }

    @Override
    public Collection<T> resolve(Registry<T> registry) {
        List<T> resolved = new ArrayList<>(this.values.size());
        for (TypedKey<T> key : this.values) {
            T value = registry.get(key.key());
            if (value != null) {
                resolved.add(value);
            }
        }
        return List.copyOf(resolved);
    }

    @Override
    public boolean contains(TypedKey<T> key) {
        if (key == null) {
            return false;
        }
        String wanted = key.key().asString();
        for (TypedKey<T> value : this.values) {
            if (value.key().asString().equals(wanted)) {
                return true;
            }
        }
        return false;
    }

    @Override
    public String toString() {
        return "FerriteKeySet[" + this.registryKey + ", size=" + this.values.size() + "]";
    }
}
