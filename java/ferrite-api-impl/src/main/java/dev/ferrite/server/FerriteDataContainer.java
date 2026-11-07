package dev.ferrite.server;

import java.io.IOException;

final class FerriteDataContainer implements org.bukkit.persistence.PersistentDataContainer {
    private record Entry(org.bukkit.persistence.PersistentDataType<?, ?> type, Object primitive) {}
    private enum Kind { BYTE, SHORT, INTEGER, LONG, FLOAT, DOUBLE, BOOLEAN, STRING, BYTE_ARRAY, INTEGER_ARRAY, LONG_ARRAY }
    private static final byte[] MAGIC = {'F', 'P', 'D', 'C'};
    private static final int VERSION = 1;
    private static final int MAX_ENTRIES = 1_000_000;
    private static final int MAX_ARRAY = 16 * 1024 * 1024;
    private final java.util.Map<org.bukkit.NamespacedKey, Entry> values = new java.util.HashMap<>();
    private final org.bukkit.persistence.PersistentDataAdapterContext context = FerriteDataContainer::new;

    private static Kind kind(org.bukkit.persistence.PersistentDataType<?, ?> type) {
        java.util.Objects.requireNonNull(type, "type");
        if (type == org.bukkit.persistence.PersistentDataType.BYTE) return Kind.BYTE;
        if (type == org.bukkit.persistence.PersistentDataType.SHORT) return Kind.SHORT;
        if (type == org.bukkit.persistence.PersistentDataType.INTEGER) return Kind.INTEGER;
        if (type == org.bukkit.persistence.PersistentDataType.LONG) return Kind.LONG;
        if (type == org.bukkit.persistence.PersistentDataType.FLOAT) return Kind.FLOAT;
        if (type == org.bukkit.persistence.PersistentDataType.DOUBLE) return Kind.DOUBLE;
        if (type == org.bukkit.persistence.PersistentDataType.BOOLEAN) return Kind.BOOLEAN;
        if (type == org.bukkit.persistence.PersistentDataType.STRING) return Kind.STRING;
        if (type == org.bukkit.persistence.PersistentDataType.BYTE_ARRAY) return Kind.BYTE_ARRAY;
        if (type == org.bukkit.persistence.PersistentDataType.INTEGER_ARRAY) return Kind.INTEGER_ARRAY;
        if (type == org.bukkit.persistence.PersistentDataType.LONG_ARRAY) return Kind.LONG_ARRAY;
        throw new UnsupportedOperationException("Ferrite: persistent data type " + type.getPrimitiveType().getName() + " is not supported");
    }

    private static Object copy(Object primitive) {
        if (primitive instanceof byte[] bytes) return bytes.clone();
        if (primitive instanceof int[] ints) return ints.clone();
        if (primitive instanceof long[] longs) return longs.clone();
        return primitive;
    }

    private static String quote(String value) {
        var out = new StringBuilder(value.length() + 2).append('"');
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            switch (c) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                case '\b' -> out.append("\\b");
                case '\f' -> out.append("\\f");
                default -> {
                    if (c < 0x20 || c == 0x7f) out.append(String.format("\\u%04x", (int) c));
                    else out.append(c);
                }
            }
        }
        return out.append('"').toString();
    }

    private static String snbtValue(Kind kind, Object value) {
        return switch (kind) {
            case BYTE -> value + "b";
            case SHORT -> value + "s";
            case INTEGER -> value.toString();
            case LONG -> value + "l";
            case FLOAT -> {
                float number = (Float) value;
                yield Float.isNaN(number) ? "NaNf" : Float.isInfinite(number) ? (number < 0 ? "-Infinityf" : "Infinityf") : number + "f";
            }
            case DOUBLE -> {
                double number = (Double) value;
                yield Double.isNaN(number) ? "NaNd" : Double.isInfinite(number) ? (number < 0 ? "-Infinityd" : "Infinityd") : number + "d";
            }
            case BOOLEAN -> ((Byte) value) != 0 ? "1b" : "0b";
            case STRING -> quote((String) value);
            case BYTE_ARRAY -> {
                var out = new StringBuilder("[B;");
                byte[] values = (byte[]) value;
                for (int i = 0; i < values.length; i++) { if (i != 0) out.append(','); out.append(values[i]).append('b'); }
                yield out.append(']').toString();
            }
            case INTEGER_ARRAY -> {
                var out = new StringBuilder("[I;");
                int[] values = (int[]) value;
                for (int i = 0; i < values.length; i++) { if (i != 0) out.append(','); out.append(values[i]); }
                yield out.append(']').toString();
            }
            case LONG_ARRAY -> {
                var out = new StringBuilder("[L;");
                long[] values = (long[]) value;
                for (int i = 0; i < values.length; i++) { if (i != 0) out.append(','); out.append(values[i]).append('l'); }
                yield out.append(']').toString();
            }
        };
    }

    /** The SNBT form sent to the Rust host: each key keeps its declared PDC type. */
    String toSnbt() {
        var sorted = new java.util.TreeMap<String, Entry>();
        for (var entry : values.entrySet()) sorted.put(entry.getKey().toString(), entry.getValue());
        var out = new StringBuilder("{");
        boolean first = true;
        for (var entry : sorted.entrySet()) {
            if (!first) out.append(',');
            first = false;
            Kind kind = kind(entry.getValue().type());
            out.append(quote(entry.getKey())).append(":{type:\"").append(kind).append("\",value:")
                .append(snbtValue(kind, entry.getValue().primitive())).append('}');
        }
        return out.append('}').toString();
    }

    /** Same keys holding equal typed primitives, which is what makes two otherwise equal item stacks similar. */
    boolean sameAs(FerriteDataContainer other) {
        if (values.size() != other.values.size()) return false;
        for (var entry : values.entrySet()) {
            var theirs = other.values.get(entry.getKey());
            if (theirs == null || kind(entry.getValue().type()) != kind(theirs.type())
                || !java.util.Objects.deepEquals(entry.getValue().primitive(), theirs.primitive())) return false;
        }
        return true;
    }

    FerriteDataContainer copy() {
        var copy = new FerriteDataContainer();
        for (var entry : values.entrySet()) copy.values.put(entry.getKey(), new Entry(entry.getValue().type(), copy(entry.getValue().primitive())));
        return copy;
    }

    public <P,C> void set(org.bukkit.NamespacedKey key, org.bukkit.persistence.PersistentDataType<P,C> type, C value) {
        Kind kind = kind(type);
        java.util.Objects.requireNonNull(key, "key");
        java.util.Objects.requireNonNull(value, "value");
        Object primitive = java.util.Objects.requireNonNull(type.toPrimitive(value, context), "primitive");
        if (!type.getPrimitiveType().isInstance(primitive)) throw new IllegalArgumentException("Ferrite: persistent data type produced the wrong primitive");
        values.put(key, new Entry(type, copy(primitive)));
    }

    public <P,C> boolean has(org.bukkit.NamespacedKey key, org.bukkit.persistence.PersistentDataType<P,C> type) {
        Kind kind = kind(type);
        Entry entry = values.get(key);
        return entry != null && kind(entry.type()) == kind;
    }

    public boolean has(org.bukkit.NamespacedKey key) { return values.containsKey(key); }

    @SuppressWarnings("unchecked")
    public <P,C> C get(org.bukkit.NamespacedKey key, org.bukkit.persistence.PersistentDataType<P,C> type) {
        Kind kind = kind(type);
        Entry entry = values.get(key);
        if (entry == null || kind(entry.type()) != kind) return null;
        return (C) ((org.bukkit.persistence.PersistentDataType<Object,Object>) type).fromPrimitive(copy(entry.primitive()), context);
    }

    public <P,C> C getOrDefault(org.bukkit.NamespacedKey key, org.bukkit.persistence.PersistentDataType<P,C> type, C fallback) {
        C value = get(key, type);
        return value == null ? fallback : value;
    }

    public org.bukkit.persistence.PersistentDataType<?,?> getPrimitiveStorageType(net.kyori.adventure.key.Key key) {
        var entry = values.get(org.bukkit.NamespacedKey.fromString(key.asString()));
        return entry == null ? null : entry.type();
    }

    public java.util.Set<org.bukkit.NamespacedKey> getKeys() { return java.util.Set.copyOf(values.keySet()); }
    public boolean isEmpty() { return values.isEmpty(); }
    public int getSize() { return values.size(); }

    @SuppressWarnings("unchecked")
    public void copyTo(org.bukkit.persistence.PersistentDataContainer target, boolean replace) {
        for (var entry : values.entrySet()) if (replace || !target.has(entry.getKey())) {
            var type = (org.bukkit.persistence.PersistentDataType<Object,Object>) entry.getValue().type();
            target.set(entry.getKey(), type, type.fromPrimitive(copy(entry.getValue().primitive()), context));
        }
    }

    public org.bukkit.persistence.PersistentDataAdapterContext getAdapterContext() { return context; }
    public void remove(org.bukkit.NamespacedKey key) { values.remove(key); }

    public byte[] serializeToBytes() throws java.io.IOException {
        var bytes = new java.io.ByteArrayOutputStream();
        try (var out = new java.io.DataOutputStream(bytes)) {
            out.write(MAGIC);
            out.writeByte(VERSION);
            out.writeInt(values.size());
            for (var entry : values.entrySet()) {
                Kind kind = kind(entry.getValue().type());
                out.writeUTF(entry.getKey().toString());
                out.writeByte(kind.ordinal());
                Object value = entry.getValue().primitive();
                switch (kind) {
                    case BYTE, BOOLEAN -> out.writeByte((Byte) value);
                    case SHORT -> out.writeShort((Short) value);
                    case INTEGER -> out.writeInt((Integer) value);
                    case LONG -> out.writeLong((Long) value);
                    case FLOAT -> out.writeFloat((Float) value);
                    case DOUBLE -> out.writeDouble((Double) value);
                    case STRING -> out.writeUTF((String) value);
                    case BYTE_ARRAY -> { byte[] array = (byte[]) value; out.writeInt(array.length); out.write(array); }
                    case INTEGER_ARRAY -> { int[] array = (int[]) value; out.writeInt(array.length); for (int item : array) out.writeInt(item); }
                    case LONG_ARRAY -> { long[] array = (long[]) value; out.writeInt(array.length); for (long item : array) out.writeLong(item); }
                }
            }
        }
        return bytes.toByteArray();
    }

    public void readFromBytes(byte[] bytes, boolean clear) throws java.io.IOException {
        if (bytes == null) throw new IOException("Persistent data bytes are null");
        var parsed = new java.util.LinkedHashMap<org.bukkit.NamespacedKey, Entry>();
        try (var input = new java.io.DataInputStream(new java.io.ByteArrayInputStream(bytes))) {
            for (byte expected : MAGIC) if (input.readByte() != expected) throw new IOException("Not a Ferrite persistent data container");
            int version = input.readUnsignedByte();
            if (version != VERSION) throw new IOException("Unsupported Ferrite persistent data version " + version);
            int count = input.readInt();
            if (count < 0 || count > MAX_ENTRIES) throw new IOException("Invalid persistent data entry count " + count);
            for (int i = 0; i < count; i++) {
                String rawKey = input.readUTF();
                var key = org.bukkit.NamespacedKey.fromString(rawKey);
                if (key == null) throw new IOException("Invalid persistent data key " + rawKey);
                int ordinal = input.readUnsignedByte();
                if (ordinal >= Kind.values().length) throw new IOException("Unknown persistent data type " + ordinal);
                Kind kind = Kind.values()[ordinal];
                Object value = switch (kind) {
                    case BYTE, BOOLEAN -> input.readByte();
                    case SHORT -> input.readShort();
                    case INTEGER -> input.readInt();
                    case LONG -> input.readLong();
                    case FLOAT -> input.readFloat();
                    case DOUBLE -> input.readDouble();
                    case STRING -> input.readUTF();
                    case BYTE_ARRAY -> {
                        int length = input.readInt();
                        if (length < 0 || length > MAX_ARRAY) throw new IOException("Invalid persistent byte array length " + length);
                        byte[] array = new byte[length];
                        input.readFully(array);
                        yield array;
                    }
                    case INTEGER_ARRAY -> {
                        int length = input.readInt();
                        if (length < 0 || length > MAX_ARRAY) throw new IOException("Invalid persistent int array length " + length);
                        int[] array = new int[length];
                        for (int item = 0; item < length; item++) array[item] = input.readInt();
                        yield array;
                    }
                    case LONG_ARRAY -> {
                        int length = input.readInt();
                        if (length < 0 || length > MAX_ARRAY) throw new IOException("Invalid persistent long array length " + length);
                        long[] array = new long[length];
                        for (int item = 0; item < length; item++) array[item] = input.readLong();
                        yield array;
                    }
                };
                var type = switch (kind) {
                    case BYTE -> org.bukkit.persistence.PersistentDataType.BYTE;
                    case SHORT -> org.bukkit.persistence.PersistentDataType.SHORT;
                    case INTEGER -> org.bukkit.persistence.PersistentDataType.INTEGER;
                    case LONG -> org.bukkit.persistence.PersistentDataType.LONG;
                    case FLOAT -> org.bukkit.persistence.PersistentDataType.FLOAT;
                    case DOUBLE -> org.bukkit.persistence.PersistentDataType.DOUBLE;
                    case BOOLEAN -> org.bukkit.persistence.PersistentDataType.BOOLEAN;
                    case STRING -> org.bukkit.persistence.PersistentDataType.STRING;
                    case BYTE_ARRAY -> org.bukkit.persistence.PersistentDataType.BYTE_ARRAY;
                    case INTEGER_ARRAY -> org.bukkit.persistence.PersistentDataType.INTEGER_ARRAY;
                    case LONG_ARRAY -> org.bukkit.persistence.PersistentDataType.LONG_ARRAY;
                };
                parsed.put(key, new Entry(type, value));
            }
            if (input.available() != 0) throw new IOException("Trailing bytes after persistent data");
        }
        if (clear) values.clear();
        values.putAll(parsed);
    }
}
