package dev.ferrite.api.item;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import net.kyori.adventure.key.Key;
import org.bukkit.NamespacedKey;
import org.bukkit.persistence.ListPersistentDataType;
import org.bukkit.persistence.PersistentDataAdapterContext;
import org.bukkit.persistence.PersistentDataContainer;
import org.bukkit.persistence.PersistentDataType;

/**
 * Map-backed persistent data container for items.
 *
 * <p>Supports all primitive scalar/array {@link PersistentDataType}s ({@code BYTE},
 * {@code SHORT}, {@code INTEGER}, {@code LONG}, {@code FLOAT}, {@code DOUBLE},
 * {@code BOOLEAN}, {@code STRING}, {@code BYTE_ARRAY}, {@code INTEGER_ARRAY},
 * {@code LONG_ARRAY}), nested {@code TAG_CONTAINER} / {@code TAG_CONTAINER_ARRAY},
 * {@link ListPersistentDataType} over supported element types, and
 * {@link PersistentDataAdapterContext#newPersistentDataContainer()}. Byte serialization
 * remains an explicit {@link UnsupportedOperationException}.
 */
public final class FerritePersistentDataContainer extends UnsupportedPersistentDataContainerBase {

    private static final PersistentDataAdapterContext ADAPTER_CONTEXT =
            FerritePersistentDataContainer::new;

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
        LONG_ARRAY(PersistentDataType.LONG_ARRAY, "long_array"),
        TAG_CONTAINER(PersistentDataType.TAG_CONTAINER, "tag_container"),
        TAG_CONTAINER_ARRAY(PersistentDataType.TAG_CONTAINER_ARRAY, "tag_container_array"),
        LIST(null, "list");

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
            if (type instanceof ListPersistentDataType<?, ?> listType) {
                // Lists nest recursively; their element type only needs to be supported at the
                // bottom of the chain.
                requireSupported(listType.elementType(), operation);
                return LIST;
            }
            for (SupportedType candidate : values()) {
                if (candidate.canonicalType != null && sameType(candidate.canonicalType, type)) {
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
                            + "): only primitive scalar, primitive array, tag container and list types are preserved");
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
    public PersistentDataAdapterContext getAdapterContext() {
        return ADAPTER_CONTEXT;
    }

    @Override
    public <P, C> void set(NamespacedKey key, PersistentDataType<P, C> type, C value) {
        Objects.requireNonNull(key, "key");
        SupportedType kind = SupportedType.requireSupported(type, "set");
        if (value == null) {
            values.remove(key);
            return;
        }
        values.put(key, new Entry(kind, type, copyValue(kind, type, value)));
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
        return (C) copyValue(entry.kind(), entry.type(), entry.value());
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
    @SuppressWarnings({"unchecked", "rawtypes"})
    public PersistentDataType<?, ?> getPrimitiveStorageType(Key key) {
        Objects.requireNonNull(key, "key");
        NamespacedKey namespaced = new NamespacedKey(key.namespace(), key.value());
        Entry entry = values.get(namespaced);
        if (entry == null) {
            return null;
        }
        if (entry.type() instanceof ListPersistentDataType<?, ?> listType) {
            return listPrimitiveStorageType(listType);
        }
        return entry.kind().primitiveStorageType();
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
                    copyValue(entry.kind(), entry.type(), entry.value()));
        }
    }

    public void copyFrom(FerritePersistentDataContainer source) {
        Objects.requireNonNull(source, "source");
        values.clear();
        for (Map.Entry<NamespacedKey, Entry> mapEntry : source.values.entrySet()) {
            Entry entry = mapEntry.getValue();
            values.put(
                    mapEntry.getKey(),
                    new Entry(
                            entry.kind(),
                            entry.type(),
                            copyValue(entry.kind(), entry.type(), entry.value())));
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
            if (entry.type() instanceof ListPersistentDataType<?, ?> listType) {
                serializedEntry.put(
                        "element-type", serializeTypeDescriptor(listType.elementType(), "serialize"));
            }
            serializedEntry.put("value", serializeValue(entry.kind(), entry.type(), entry.value()));
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
                if (kind == SupportedType.LIST) {
                    Object descriptor = entryMap.get("element-type");
                    if (descriptor == null) {
                        throw new IllegalArgumentException(
                                "Missing PDC list element-type for key: " + keyText);
                    }
                    // `element-type` describes the list's element, not the list itself: a bare
                    // scalar id or a descriptor for a nested list.
                    PersistentDataType<?, ?> elementType =
                            deserializeTypeDescriptor(descriptor, keyText);
                    PersistentDataType<?, ?> listType =
                            PersistentDataType.LIST.listTypeFrom(
                                    (PersistentDataType) elementType);
                    Object decoded = deserializeListValue(
                            (ListPersistentDataType<?, ?>) listType, entryMap.get("value"));
                    container.set(key, (PersistentDataType) listType, decoded);
                } else {
                    Object decoded = deserializeValue(kind, entryMap.get("value"));
                    container.set(key, (PersistentDataType) kind.canonicalType(), decoded);
                }
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
                    || !sameType(thisEntry.type(), otherEntry.type())
                    || !valueEquals(thisEntry.kind(), thisEntry.value(), otherEntry.value())) {
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
            int valueHash = valueHashCode(entry.kind(), entry.value());
            hash = 31 * hash + Objects.hash(mapEntry.getKey(), entry.kind(), valueHash);
        }
        return hash;
    }

    private static boolean sameType(PersistentDataType<?, ?> first, PersistentDataType<?, ?> second) {
        if (first instanceof ListPersistentDataType<?, ?> firstList
                && second instanceof ListPersistentDataType<?, ?> secondList) {
            return sameType(firstList.elementType(), secondList.elementType());
        }
        return first.getPrimitiveType() == second.getPrimitiveType()
                && first.getComplexType() == second.getComplexType();
    }

    private static boolean valueEquals(SupportedType kind, Object first, Object second) {
        if (kind == SupportedType.LIST && first instanceof List<?> a && second instanceof List<?> b) {
            if (a.size() != b.size()) {
                return false;
            }
            for (int i = 0; i < a.size(); i++) {
                if (!Objects.deepEquals(a.get(i), b.get(i))) {
                    return false;
                }
            }
            return true;
        }
        return Objects.deepEquals(first, second);
    }

    private static int valueHashCode(SupportedType kind, Object value) {
        return switch (kind) {
            case BYTE_ARRAY -> Arrays.hashCode((byte[]) value);
            case INTEGER_ARRAY -> Arrays.hashCode((int[]) value);
            case LONG_ARRAY -> Arrays.hashCode((long[]) value);
            case TAG_CONTAINER_ARRAY -> Arrays.hashCode((PersistentDataContainer[]) value);
            case LIST -> {
                int h = 1;
                for (Object elem : (List<?>) value) {
                    int elemHash = switch (elem) {
                        case byte[] bytes -> Arrays.hashCode(bytes);
                        case int[] ints -> Arrays.hashCode(ints);
                        case long[] longs -> Arrays.hashCode(longs);
                        default -> Objects.hashCode(elem);
                    };
                    h = 31 * h + elemHash;
                }
                yield h;
            }
            default -> Objects.hashCode(value);
        };
    }

    private static Object copyValue(
            SupportedType kind, PersistentDataType<?, ?> type, Object value) {
        return switch (kind) {
            case BYTE_ARRAY -> ((byte[]) value).clone();
            case INTEGER_ARRAY -> ((int[]) value).clone();
            case LONG_ARRAY -> ((long[]) value).clone();
            case TAG_CONTAINER -> copyContainer(value);
            case TAG_CONTAINER_ARRAY -> {
                PersistentDataContainer[] source = (PersistentDataContainer[]) value;
                PersistentDataContainer[] copy = new PersistentDataContainer[source.length];
                for (int i = 0; i < source.length; i++) {
                    copy[i] = copyContainer(
                            Objects.requireNonNull(source[i], "TAG_CONTAINER_ARRAY element"));
                }
                yield copy;
            }
            case LIST -> {
                ListPersistentDataType<?, ?> listType = (ListPersistentDataType<?, ?>) type;
                SupportedType elemKind =
                        SupportedType.requireSupported(listType.elementType(), "copy");
                List<?> source = (List<?>) value;
                List<Object> copy = new ArrayList<>(source.size());
                for (Object element : source) {
                    Objects.requireNonNull(element, "list element");
                    copy.add(copyValue(elemKind, listType.elementType(), element));
                }
                yield List.copyOf(copy);
            }
            default -> value;
        };
    }

    private static FerritePersistentDataContainer copyContainer(Object value) {
        if (!(value instanceof FerritePersistentDataContainer ferrite)) {
            throw new IllegalArgumentException(
                    "Unsupported nested PersistentDataContainer implementation: "
                            + (value == null ? "null" : value.getClass().getName()));
        }
        return ferrite.copy();
    }

    private static Object serializeValue(
            SupportedType kind, PersistentDataType<?, ?> type, Object value) {
        return switch (kind) {
            case BYTE_ARRAY -> ((byte[]) value).clone();
            case INTEGER_ARRAY -> ((int[]) value).clone();
            case LONG_ARRAY -> ((long[]) value).clone();
            case TAG_CONTAINER -> ((FerritePersistentDataContainer) value).serialize();
            case TAG_CONTAINER_ARRAY -> {
                PersistentDataContainer[] arr = (PersistentDataContainer[]) value;
                List<Map<String, Object>> out = new ArrayList<>(arr.length);
                for (PersistentDataContainer entry : arr) {
                    out.add(((FerritePersistentDataContainer) entry).serialize());
                }
                yield out;
            }
            case LIST -> {
                ListPersistentDataType<?, ?> listType = (ListPersistentDataType<?, ?>) type;
                SupportedType elemKind =
                        SupportedType.requireSupported(listType.elementType(), "serialize");
                List<?> source = (List<?>) value;
                List<Object> out = new ArrayList<>(source.size());
                for (Object element : source) {
                    out.add(serializeValue(elemKind, listType.elementType(), element));
                }
                yield out;
            }
            default -> value;
        };
    }

    private static List<Object> deserializeListValue(
            ListPersistentDataType<?, ?> type, Object raw) {
        if (!(raw instanceof List<?> list)) {
            throw new IllegalArgumentException("Expected List for PDC list entry");
        }
        PersistentDataType<?, ?> elementType = type.elementType();
        List<Object> result = new ArrayList<>(list.size());
        for (Object item : list) {
            if (elementType instanceof ListPersistentDataType<?, ?> nested) {
                result.add(deserializeListValue(nested, item));
            } else {
                result.add(deserializeValue(
                        SupportedType.requireSupported(elementType, "deserialize"), item));
            }
        }
        return result;
    }

    /** Serializes a list element type: an id for a scalar type, a descriptor map for a list. */
    private static Object serializeTypeDescriptor(
            PersistentDataType<?, ?> type, String operation) {
        if (type instanceof ListPersistentDataType<?, ?> listType) {
            Map<String, Object> descriptor = new LinkedHashMap<>();
            descriptor.put("type", SupportedType.LIST.id());
            descriptor.put(
                    "element-type",
                    serializeTypeDescriptor(listType.elementType(), operation));
            return descriptor;
        }
        return SupportedType.requireSupported(type, operation).id();
    }

    /** Reads the element-type shape written by {@link #serializeTypeDescriptor}. */
    @SuppressWarnings({"unchecked", "rawtypes"})
    private static PersistentDataType<?, ?> deserializeTypeDescriptor(
            Object raw, String keyText) {
        if (raw instanceof Map<?, ?> descriptor) {
            Object typeId = descriptor.get("type");
            if (typeId == null) {
                throw new IllegalArgumentException(
                        "Missing PDC list type in element-type for key: " + keyText);
            }
            if (SupportedType.fromId(String.valueOf(typeId)) != SupportedType.LIST) {
                throw new IllegalArgumentException(
                        "PDC element-type descriptor for key '" + keyText + "' must be a list");
            }
            Object nested = descriptor.get("element-type");
            if (nested == null) {
                throw new IllegalArgumentException(
                        "Missing PDC nested list element-type for key: " + keyText);
            }
            return PersistentDataType.LIST.listTypeFrom(
                    (PersistentDataType) deserializeTypeDescriptor(nested, keyText));
        }
        SupportedType kind = SupportedType.fromId(String.valueOf(raw));
        if (kind == SupportedType.LIST) {
            throw new IllegalArgumentException(
                    "PDC list element-type for key '" + keyText + "' needs a descriptor object");
        }
        return kind.canonicalType();
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private static PersistentDataType<?, ?> listPrimitiveStorageType(
            ListPersistentDataType<?, ?> listType) {
        PersistentDataType<?, ?> elementType = listType.elementType();
        PersistentDataType<?, ?> primitive =
                elementType instanceof ListPersistentDataType<?, ?> nested
                        ? listPrimitiveStorageType(nested)
                        : SupportedType.requireSupported(elementType, "getPrimitiveStorageType")
                                .primitiveStorageType();
        return PersistentDataType.LIST.listTypeFrom((PersistentDataType) primitive);
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
            case TAG_CONTAINER -> {
                if (!(raw instanceof Map<?, ?> map)) {
                    throw new IllegalArgumentException("Expected Map for PDC tag_container");
                }
                yield FerritePersistentDataContainer.deserialize(map);
            }
            case TAG_CONTAINER_ARRAY -> {
                if (!(raw instanceof List<?> list)) {
                    throw new IllegalArgumentException("Expected List for PDC tag_container_array");
                }
                PersistentDataContainer[] arr = new PersistentDataContainer[list.size()];
                for (int i = 0; i < list.size(); i++) {
                    if (!(list.get(i) instanceof Map<?, ?> map)) {
                        throw new IllegalArgumentException(
                                "Expected Map element for PDC tag_container_array");
                    }
                    arr[i] = FerritePersistentDataContainer.deserialize(map);
                }
                yield arr;
            }
            case LIST -> throw new IllegalStateException(
                    "PDC list values must be decoded by deserializeListValue");
        };
    }
}
