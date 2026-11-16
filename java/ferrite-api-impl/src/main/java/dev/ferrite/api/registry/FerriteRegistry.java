package dev.ferrite.api.registry;

import io.papermc.paper.registry.tag.Tag;
import io.papermc.paper.registry.tag.TagKey;
import java.util.Collection;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.function.Supplier;
import java.util.stream.Stream;
import org.bukkit.Keyed;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;

/**
 * A {@link Registry} backed by generated vanilla data.
 *
 * <p>Entries are created lazily. That matters for {@code Enchantment}, {@code Biome} and
 * {@code DamageType}: their static initializers ask this registry for every constant with
 * {@code getOrThrow}, and those classes are still initializing at that point. Building the
 * entries eagerly would re-enter their initializers from inside the registry lookup; a
 * supplier defers construction to the lookup itself, which the JVM permits re-entrantly on
 * the same thread.
 */
public final class FerriteRegistry<T extends Keyed> implements Registry<T> {

    private final String name;
    private final Map<NamespacedKey, Supplier<T>> suppliers;
    private final Map<NamespacedKey, T> values = new LinkedHashMap<>();

    public FerriteRegistry(String name, Map<NamespacedKey, Supplier<T>> suppliers) {
        this.name = name;
        this.suppliers = Map.copyOf(suppliers);
    }

    @Override
    public T get(NamespacedKey key) {
        if (key == null) {
            return null;
        }
        synchronized (this.values) {
            T cached = this.values.get(key);
            if (cached != null) {
                return cached;
            }
            Supplier<T> supplier = this.suppliers.get(key);
            if (supplier == null) {
                return null;
            }
            T created = supplier.get();
            if (created != null) {
                this.values.put(key, created);
            }
            return created;
        }
    }

    @Override
    public NamespacedKey getKey(T value) {
        if (value == null) {
            return null;
        }
        synchronized (this.values) {
            for (Map.Entry<NamespacedKey, T> entry : this.values.entrySet()) {
                if (entry.getValue() == value) {
                    return entry.getKey();
                }
            }
        }
        return null;
    }

    @Override
    public boolean hasTag(TagKey<T> key) {
        return false;
    }

    @Override
    public Tag<T> getTag(TagKey<T> key) {
        return null;
    }

    @Override
    public Collection<Tag<T>> getTags() {
        return List.of();
    }

    @Override
    public Stream<T> stream() {
        return this.suppliers.keySet().stream().map(this::get).filter(Objects::nonNull);
    }

    @Override
    public Stream<NamespacedKey> keyStream() {
        return this.suppliers.keySet().stream();
    }

    @Override
    public int size() {
        return this.suppliers.size();
    }

    @Override
    public Iterator<T> iterator() {
        return stream().iterator();
    }

    @Override
    public String toString() {
        return "FerriteRegistry[" + this.name + ", size=" + size() + "]";
    }
}
