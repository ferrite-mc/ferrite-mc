package dev.ferrite.api.item;

import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import org.bukkit.NamespacedKey;
import org.bukkit.persistence.PersistentDataContainer;
import org.bukkit.persistence.PersistentDataType;

/**
 * Map-backed persistent data for items.
 *
 * <p>Only typed values that the facade can round-trip in memory are stored; the type is
 * checked on read. Byte serialization and adapter contexts are outside the slice and
 * inherit explicit refusals from {@link UnsupportedPersistentDataContainerBase}.
 */
public final class FerritePersistentDataContainer extends UnsupportedPersistentDataContainerBase {

    private record Entry(PersistentDataType<?, ?> type, Object value) {
    }

    private final Map<NamespacedKey, Entry> values = new LinkedHashMap<>();

    @Override
    @SuppressWarnings("unchecked")
    public <P, C> void set(NamespacedKey key, PersistentDataType<P, C> type, C value) {
        Objects.requireNonNull(key, "key");
        Objects.requireNonNull(type, "type");
        if (value == null) {
            values.remove(key);
            return;
        }
        values.put(key, new Entry(type, value));
    }

    @Override
    @SuppressWarnings("unchecked")
    public <P, C> C get(NamespacedKey key, PersistentDataType<P, C> type) {
        Objects.requireNonNull(key, "key");
        Objects.requireNonNull(type, "type");
        Entry entry = values.get(key);
        if (entry == null || !sameType(entry.type(), type)) {
            return null;
        }
        return (C) entry.value();
    }

    @Override
    public <P, C> C getOrDefault(NamespacedKey key, PersistentDataType<P, C> type, C defaultValue) {
        C value = get(key, type);
        return value == null ? defaultValue : value;
    }

    @Override
    public <P, C> boolean has(NamespacedKey key, PersistentDataType<P, C> type) {
        Objects.requireNonNull(key, "key");
        Objects.requireNonNull(type, "type");
        Entry entry = values.get(key);
        return entry != null && sameType(entry.type(), type);
    }

    @Override
    public boolean has(NamespacedKey key) {
        return values.containsKey(key);
    }

    @Override
    public Set<NamespacedKey> getKeys() {
        return Set.copyOf(values.keySet());
    }

    @Override
    public boolean isEmpty() {
        return values.isEmpty();
    }

    @Override
    public int getSize() {
        return values.size();
    }

    @Override
    public void remove(NamespacedKey key) {
        values.remove(key);
    }

    @Override
    @SuppressWarnings({"unchecked", "rawtypes"})
    public void copyTo(PersistentDataContainer target, boolean replace) {
        Objects.requireNonNull(target, "target");
        for (Map.Entry<NamespacedKey, Entry> entry : values.entrySet()) {
            boolean present = target.has(entry.getKey());
            if (present && !replace) {
                continue;
            }
            Entry value = entry.getValue();
            target.set(entry.getKey(), (PersistentDataType) value.type(), value.value());
        }
    }

    public void copyFrom(FerritePersistentDataContainer source) {
        values.clear();
        values.putAll(source.values);
    }

    public Map<String, Object> serialize() {
        Map<String, Object> result = new LinkedHashMap<>();
        for (Map.Entry<NamespacedKey, Entry> entry : values.entrySet()) {
            result.put(entry.getKey().toString(), entry.getValue().value());
        }
        return result;
    }

    private static boolean sameType(PersistentDataType<?, ?> first, PersistentDataType<?, ?> second) {
        return first.getPrimitiveType() == second.getPrimitiveType()
                && first.getComplexType() == second.getComplexType();
    }
}
