package dev.ferrite.api.item;

import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import net.kyori.adventure.key.Key;
import org.bukkit.NamespacedKey;
import org.bukkit.persistence.PersistentDataContainer;
import org.bukkit.persistence.PersistentDataType;

/**
 * Map-backed persistent data container for items.
 *
 * <p>Only the primitive scalar and primitive array {@link PersistentDataType}s preserved by
 * the Ferrite host snapshot boundary are supported: {@code BYTE}, {@code SHORT},
 * {@code INTEGER}, {@code LONG}, {@code FLOAT}, {@code DOUBLE}, {@code BOOLEAN},
 * {@code STRING}, {@code BYTE_ARRAY}, {@code INTEGER_ARRAY} and {@code LONG_ARRAY}.
 * Nested tag containers, list types, custom adapter types, byte serialization and adapter
 * contexts inherit or throw explicit {@link UnsupportedOperationException}s.
 */
public final class FerritePersistentDataContainer extends UnsupportedPersistentDataContainerBase {

    /** Host-preserved PDC value kinds. */
    public enum SupportedType {
        BYTE(PersistentDataType.BYTE, "byte"),
        SHORT(PersistentDataType.SHORT, "short"),
        INTEGER(PersistentDataType.INTEGER, "int"),
        LONG(PersistentDataType.LONG, "long"),
        FLOAT(PersistentDataType.FLOAT, "float"),
        DOUBLE(PersistentDataType.DOUBLE, "double"),
        BOOLEAN(PersistentDataType.BOOLEAN, "boolean"),
        STRING(PersistentDataType.STRING, "string"),
        BYTE_ARRAY(PersistentDataType.BYTE_ARRAY, "byte_array"),
        INTEGER_ARRAY(PersistentDataType.INTEGER_ARRAY, "int_array"),
        LONG_ARRAY(PersistentDataType.LONG_ARRAY, "long_array");

        private final PersistentDataType<?, ?> canonicalType;
        private final String id;

        SupportedType(PersistentDataType<?, ?> canonicalType, String id) {
            this.canonicalType = canonicalType;
            this.id = id;
        }

        public PersistentDataType<?, ?> canonicalType() {
            return canonicalType;
        }

        public PersistentDataType<?, ?> primitiveStorageType() {
            return this == BOOLEAN ? PersistentDataType.BYTE : canonicalType;
        }

        public String id() {
            return id;
        }

        public static SupportedType requireSupported(PersistentDataType<?, ?> type, String operation) {
            Objects.requireNonNull(type, "type");
            for (SupportedType candidate : values()) {
                if (sameType(candidate.canonicalType, type)) {
                    return candidate;
                }
            }
            throw dev.ferrite.api.internal.Unsupported.exception(
                    "PersistentDataContainer#"
                            + operation
                            + " for type "
                            + type.getComplexType().getName()
                            + " (primitive "
                            + type.getPrimitiveType().getName()
                            + "): only primitive scalar and primitive array types are preserved");
        }

        public static SupportedType fromId(String id) {
            for (SupportedType candidate : values()) {
                if (candidate.id.equals(id)) {
                    return candidate;
                }
            }
            throw new UnsupportedOperationException(
                    "PersistentDataContainer#deserialize: unsupported PDC type '" + id + "'");
        }
    }

    private record Entry(SupportedType kind, PersistentDataType<?, ?> type, Object value) {
    }

    private final Map<NamespacedKey, Entry> values = new LinkedHashMap<>();

    @Override
    public <P, C> void set(NamespacedKey key, PersistentDataType<P, C> type, C value) {
        Objects.requireNonNull(key, "key");
        SupportedType kind = SupportedType.requireSupported(type, "set");
        if (value == null) {
            values.remove(key);
            return;
        }
        values.put(key, new Entry(kind, type, copyValue(kind, value)));
    }

    @Override
    @SuppressWarnings("unchecked")
    public <P, C> C get(NamespacedKey key, PersistentDataType<P, C> type) {
        Objects.requireNonNull(key, "key");
        SupportedType.requireSupported(type, "get");
        Entry entry = values.get(key);
        if (entry == null || !sameType(entry.type(), type)) {
            return null;
        }
        return (C) copyValue(entry.kind(), entry.value());
    }

    @Override
    public <P, C> C getOrDefault(NamespacedKey key, PersistentDataType<P, C> type, C defaultValue) {
        C value = get(key, type);
        return value == null ? defaultValue : value;
    }

    @Override
    public <P, C> boolean has(NamespacedKey key, PersistentDataType<P, C> type) {
        Objects.requireNonNull(key, "key");
        SupportedType.requireSupported(type, "has");
        Entry entry = values.get(key);
        return entry != null && sameType(entry.type(), type);
    }

    @Override
    public boolean has(NamespacedKey key) {
        Objects.requireNonNull(key, "key");
        return values.containsKey(key);
    }

    @Override
    public PersistentDataType<?, ?> getPrimitiveStorageType(Key key) {
        Objects.requireNonNull(key, "key");
        NamespacedKey namespaced = new NamespacedKey(key.namespace(), key.value());
        Entry entry = values.get(namespaced);
        return entry == null ? null : entry.kind().primitiveStorageType();
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
        Objects.requireNonNull(key, "key");
        values.remove(key);
    }

    @Override
    @SuppressWarnings({"unchecked", "rawtypes"})
    public void copyTo(PersistentDataContainer target, boolean replace) {
        Objects.requireNonNull(target, "target");
        for (Map.Entry<NamespacedKey, Entry> mapEntry : values.entrySet()) {
            boolean present = target.has(mapEntry.getKey());
            if (present && !replace) {
                continue;
            }
            Entry entry = mapEntry.getValue();
            target.set(
                    mapEntry.getKey(),
                    (PersistentDataType) entry.type(),
                    copyValue(entry.kind(), entry.value()));
        }
    }

    public void copyFrom(FerritePersistentDataContainer source) {
        Objects.requireNonNull(source, "source");
        values.clear();
        for (Map.Entry<NamespacedKey, Entry> mapEntry : source.values.entrySet()) {
            Entry entry = mapEntry.getValue();
            values.put(
                    mapEntry.getKey(),
                    new Entry(entry.kind(), entry.type(), copyValue(entry.kind(), entry.value())));
        }
    }

    public FerritePersistentDataContainer copy() {
        FerritePersistentDataContainer clone = new FerritePersistentDataContainer();
        clone.copyFrom(this);
        return clone;
    }

    public Map<String, Object> serialize() {
        Map<String, Object> result = new LinkedHashMap<>();
        for (Map.Entry<NamespacedKey, Entry> mapEntry : values.entrySet()) {
            Entry entry = mapEntry.getValue();
            Map<String, Object> serializedEntry = new LinkedHashMap<>();
            serializedEntry.put("type", entry.kind().id());
            serializedEntry.put("value", serializeValue(entry.kind(), entry.value()));
            result.put(mapEntry.getKey().toString(), serializedEntry);
        }
        return result;
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    public static FerritePersistentDataContainer deserialize(Map<?, ?> args) {
        FerritePersistentDataContainer container = new FerritePersistentDataContainer();
        if (args == null) {
            return container;
        }
        for (Map.Entry<?, ?> raw : args.entrySet()) {
            String keyText = String.valueOf(raw.getKey());
            NamespacedKey key = NamespacedKey.fromString(keyText);
            if (key == null) {
                throw new IllegalArgumentException("Invalid PDC key: " + keyText);
            }
            Object rawValue = raw.getValue();
            if (rawValue instanceof Map<?, ?> entryMap) {
                Object typeId = entryMap.get("type");
                if (typeId == null) {
                    throw new IllegalArgumentException("Missing PDC type for key: " + keyText);
                }
                SupportedType kind = SupportedType.fromId(String.valueOf(typeId));
                Object decoded = deserializeValue(kind, entryMap.get("value"));
                container.set(key, (PersistentDataType) kind.canonicalType(), decoded);
            } else if (rawValue instanceof String text) {
                container.set(key, PersistentDataType.STRING, text);
            } else {
                throw new UnsupportedOperationException(
                        "PersistentDataContainer#deserialize: unsupported entry shape for key '"
                                + keyText
                                + "'");
            }
        }
        return container;
    }

    @Override
    public boolean equals(Object object) {
        if (this == object) {
            return true;
        }
        if (!(object instanceof FerritePersistentDataContainer other)) {
            return false;
        }
        if (values.size() != other.values.size()) {
            return false;
        }
        for (Map.Entry<NamespacedKey, Entry> mapEntry : values.entrySet()) {
            Entry otherEntry = other.values.get(mapEntry.getKey());
            if (otherEntry == null) {
                return false;
            }
            Entry thisEntry = mapEntry.getValue();
            if (thisEntry.kind() != otherEntry.kind()
                    || !Objects.deepEquals(thisEntry.value(), otherEntry.value())) {
                return false;
            }
        }
        return true;
    }

    @Override
    public int hashCode() {
        int hash = 1;
        for (Map.Entry<NamespacedKey, Entry> mapEntry : values.entrySet()) {
            Entry entry = mapEntry.getValue();
            int valueHash = switch (entry.kind()) {
                case BYTE_ARRAY -> Arrays.hashCode((byte[]) entry.value());
                case INTEGER_ARRAY -> Arrays.hashCode((int[]) entry.value());
                case LONG_ARRAY -> Arrays.hashCode((long[]) entry.value());
                default -> Objects.hashCode(entry.value());
            };
            hash = 31 * hash + Objects.hash(mapEntry.getKey(), entry.kind(), valueHash);
        }
        return hash;
    }

    private static boolean sameType(PersistentDataType<?, ?> first, PersistentDataType<?, ?> second) {
        return first.getPrimitiveType() == second.getPrimitiveType()
                && first.getComplexType() == second.getComplexType();
    }

    private static Object copyValue(SupportedType kind, Object value) {
        return switch (kind) {
            case BYTE_ARRAY -> ((byte[]) value).clone();
            case INTEGER_ARRAY -> ((int[]) value).clone();
            case LONG_ARRAY -> ((long[]) value).clone();
            default -> value;
        };
    }

    private static Object serializeValue(SupportedType kind, Object value) {
        return switch (kind) {
            case BYTE_ARRAY -> ((byte[]) value).clone();
            case INTEGER_ARRAY -> ((int[]) value).clone();
            case LONG_ARRAY -> ((long[]) value).clone();
            default -> value;
        };
    }

    private static Object deserializeValue(SupportedType kind, Object raw) {
        return switch (kind) {
            case BYTE -> ((Number) raw).byteValue();
            case SHORT -> ((Number) raw).shortValue();
            case INTEGER -> ((Number) raw).intValue();
            case LONG -> ((Number) raw).longValue();
            case FLOAT -> ((Number) raw).floatValue();
            case DOUBLE -> ((Number) raw).doubleValue();
            case BOOLEAN -> {
                if (raw instanceof Boolean bool) {
                    yield bool;
                }
                if (raw instanceof Number number) {
                    yield number.byteValue() != 0;
                }
                yield Boolean.parseBoolean(String.valueOf(raw));
            }
            case STRING -> String.valueOf(raw);
            case BYTE_ARRAY -> {
                if (raw instanceof byte[] bytes) {
                    yield bytes.clone();
                }
                if (raw instanceof List<?> list) {
                    byte[] bytes = new byte[list.size()];
                    for (int i = 0; i < list.size(); i++) {
                        bytes[i] = ((Number) list.get(i)).byteValue();
                    }
                    yield bytes;
                }
                throw new IllegalArgumentException("Expected byte[] for PDC byte_array");
            }
            case INTEGER_ARRAY -> {
                if (raw instanceof int[] ints) {
                    yield ints.clone();
                }
                if (raw instanceof List<?> list) {
                    int[] ints = new int[list.size()];
                    for (int i = 0; i < list.size(); i++) {
                        ints[i] = ((Number) list.get(i)).intValue();
                    }
                    yield ints;
                }
                throw new IllegalArgumentException("Expected int[] for PDC int_array");
            }
            case LONG_ARRAY -> {
                if (raw instanceof long[] longs) {
                    yield longs.clone();
                }
                if (raw instanceof List<?> list) {
                    long[] longs = new long[list.size()];
                    for (int i = 0; i < list.size(); i++) {
                        longs[i] = ((Number) list.get(i)).longValue();
                    }
                    yield longs;
                }
                throw new IllegalArgumentException("Expected long[] for PDC long_array");
            }
        };
    }
}
