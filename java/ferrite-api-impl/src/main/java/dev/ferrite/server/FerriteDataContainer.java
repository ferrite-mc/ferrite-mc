package dev.ferrite.server;

import java.io.IOException;

final class FerriteDataContainer implements org.bukkit.persistence.PersistentDataContainer {
    private record Entry(org.bukkit.persistence.PersistentDataType<?, ?> type, Object primitive) {}
    // New kinds are appended: the ordinal is part of the byte format.
    private enum Kind { BYTE, SHORT, INTEGER, LONG, FLOAT, DOUBLE, BOOLEAN, STRING, BYTE_ARRAY, INTEGER_ARRAY, LONG_ARRAY, TAG_CONTAINER, TAG_CONTAINER_ARRAY, LIST }
    /** A type's kind, with the element's shape for a list (lists nest). */
    private record Shape(Kind kind, Shape element) {
        Shape(Kind kind) { this(kind, null); }
        /** The name the host reads: `STRING`, `LIST:STRING`, `LIST:LIST:TAG_CONTAINER`. */
        String wire() { return kind == Kind.LIST ? "LIST:" + element.wire() : kind.name(); }
    }
    private static final int MAX_DEPTH = 16;
    private static final byte[] MAGIC = {'F', 'P', 'D', 'C'};
    private static final int VERSION = 1;
    private static final int MAX_ENTRIES = 1_000_000;
    private static final int MAX_ARRAY = 16 * 1024 * 1024;
    private final java.util.Map<org.bukkit.NamespacedKey, Entry> values = new java.util.HashMap<>();
    private final org.bukkit.persistence.PersistentDataAdapterContext context = FerriteDataContainer::new;

    private static Shape shape(org.bukkit.persistence.PersistentDataType<?, ?> type) {
        java.util.Objects.requireNonNull(type, "type");
        if (type instanceof org.bukkit.persistence.ListPersistentDataType<?, ?> list) return new Shape(Kind.LIST, shape(list.elementType()));
        if (type == org.bukkit.persistence.PersistentDataType.BYTE) return new Shape(Kind.BYTE);
        if (type == org.bukkit.persistence.PersistentDataType.SHORT) return new Shape(Kind.SHORT);
        if (type == org.bukkit.persistence.PersistentDataType.INTEGER) return new Shape(Kind.INTEGER);
        if (type == org.bukkit.persistence.PersistentDataType.LONG) return new Shape(Kind.LONG);
        if (type == org.bukkit.persistence.PersistentDataType.FLOAT) return new Shape(Kind.FLOAT);
        if (type == org.bukkit.persistence.PersistentDataType.DOUBLE) return new Shape(Kind.DOUBLE);
        if (type == org.bukkit.persistence.PersistentDataType.BOOLEAN) return new Shape(Kind.BOOLEAN);
        if (type == org.bukkit.persistence.PersistentDataType.STRING) return new Shape(Kind.STRING);
        if (type == org.bukkit.persistence.PersistentDataType.BYTE_ARRAY) return new Shape(Kind.BYTE_ARRAY);
        if (type == org.bukkit.persistence.PersistentDataType.INTEGER_ARRAY) return new Shape(Kind.INTEGER_ARRAY);
        if (type == org.bukkit.persistence.PersistentDataType.LONG_ARRAY) return new Shape(Kind.LONG_ARRAY);
        if (type == org.bukkit.persistence.PersistentDataType.TAG_CONTAINER) return new Shape(Kind.TAG_CONTAINER);
        if (type == org.bukkit.persistence.PersistentDataType.TAG_CONTAINER_ARRAY) return new Shape(Kind.TAG_CONTAINER_ARRAY);
        throw new UnsupportedOperationException("Ferrite: persistent data type " + type.getPrimitiveType().getName() + " is not supported");
    }

    @SuppressWarnings("unchecked")
    private static org.bukkit.persistence.PersistentDataType<?, ?> typeOf(Shape shape) {
        return switch (shape.kind()) {
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
            case TAG_CONTAINER -> org.bukkit.persistence.PersistentDataType.TAG_CONTAINER;
            case TAG_CONTAINER_ARRAY -> org.bukkit.persistence.PersistentDataType.TAG_CONTAINER_ARRAY;
            case LIST -> org.bukkit.persistence.PersistentDataType.LIST.listTypeFrom((org.bukkit.persistence.PersistentDataType<Object, Object>) typeOf(shape.element()));
        };
    }

    /**
     * The value as this container stores it: a deep copy, so nothing a plugin keeps a reference to
     * can change what an item holds. Foreign containers become Ferrite ones.
     */
    private static Object adopt(Shape shape, Object value) { return adopt(shape, value, 0); }

    private static Object adopt(Shape shape, Object value, int depth) {
        if (depth > MAX_DEPTH) throw new IllegalArgumentException("Ferrite: persistent data nests deeper than " + MAX_DEPTH);
        java.util.Objects.requireNonNull(value, "value");
        return switch (shape.kind()) {
            case BYTE_ARRAY -> ((byte[]) value).clone();
            case INTEGER_ARRAY -> ((int[]) value).clone();
            case LONG_ARRAY -> ((long[]) value).clone();
            case TAG_CONTAINER -> adoptContainer((org.bukkit.persistence.PersistentDataContainer) value, depth + 1);
            case TAG_CONTAINER_ARRAY -> {
                var in = (org.bukkit.persistence.PersistentDataContainer[]) value;
                var out = new FerriteDataContainer[in.length];
                for (int i = 0; i < in.length; i++) out[i] = adoptContainer(java.util.Objects.requireNonNull(in[i], "container"), depth + 1);
                yield out;
            }
            case LIST -> {
                var out = new java.util.ArrayList<Object>();
                for (var item : (java.util.List<?>) value) out.add(adopt(shape.element(), item, depth + 1));
                yield java.util.Collections.unmodifiableList(out);
            }
            default -> value;
        };
    }

    private static FerriteDataContainer adoptContainer(org.bukkit.persistence.PersistentDataContainer source, int depth) {
        if (depth > MAX_DEPTH) throw new IllegalArgumentException("Ferrite: persistent data nests deeper than " + MAX_DEPTH);
        if (source instanceof FerriteDataContainer own) return own.copy();
        var copy = new FerriteDataContainer();
        source.copyTo(copy, true);
        return copy;
    }

    private static boolean sameValue(Shape shape, Object a, Object b) {
        return switch (shape.kind()) {
            case BYTE_ARRAY -> java.util.Arrays.equals((byte[]) a, (byte[]) b);
            case INTEGER_ARRAY -> java.util.Arrays.equals((int[]) a, (int[]) b);
            case LONG_ARRAY -> java.util.Arrays.equals((long[]) a, (long[]) b);
            case TAG_CONTAINER -> ((FerriteDataContainer) a).sameAs((FerriteDataContainer) b);
            case TAG_CONTAINER_ARRAY -> {
                var x = (FerriteDataContainer[]) a;
                var y = (FerriteDataContainer[]) b;
                if (x.length != y.length) yield false;
                for (int i = 0; i < x.length; i++) if (!x[i].sameAs(y[i])) yield false;
                yield true;
            }
            case LIST -> {
                var x = (java.util.List<?>) a;
                var y = (java.util.List<?>) b;
                if (x.size() != y.size()) yield false;
                for (int i = 0; i < x.size(); i++) if (!sameValue(shape.element(), x.get(i), y.get(i))) yield false;
                yield true;
            }
            default -> a.equals(b);
        };
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

    private static String snbtValue(Shape shape, Object value) {
        return switch (shape.kind()) {
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
            // A nested container is the same map of typed wrappers the top level is.
            case TAG_CONTAINER -> ((FerriteDataContainer) value).toSnbt();
            case TAG_CONTAINER_ARRAY -> {
                var out = new StringBuilder("[");
                var containers = (FerriteDataContainer[]) value;
                for (int i = 0; i < containers.length; i++) { if (i != 0) out.append(','); out.append(containers[i].toSnbt()); }
                yield out.append(']').toString();
            }
            case LIST -> {
                var out = new StringBuilder("[");
                boolean first = true;
                for (var item : (java.util.List<?>) value) { if (!first) out.append(','); first = false; out.append(snbtValue(shape.element(), item)); }
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
            Shape shape = shape(entry.getValue().type());
            out.append(quote(entry.getKey())).append(":{type:\"").append(shape.kind()).append('"');
            if (shape.kind() == Kind.LIST) out.append(",element:\"").append(shape.element().wire()).append('"');
            out.append(",value:").append(snbtValue(shape, entry.getValue().primitive())).append('}');
        }
        return out.append('}').toString();
    }

    /** Same keys holding equal typed values, which is what makes two otherwise equal item stacks similar. */
    boolean sameAs(FerriteDataContainer other) {
        if (values.size() != other.values.size()) return false;
        for (var entry : values.entrySet()) {
            var theirs = other.values.get(entry.getKey());
            if (theirs == null) return false;
            var shape = shape(entry.getValue().type());
            if (!shape.equals(shape(theirs.type())) || !sameValue(shape, entry.getValue().primitive(), theirs.primitive())) return false;
        }
        return true;
    }

    FerriteDataContainer copy() {
        var copy = new FerriteDataContainer();
        for (var entry : values.entrySet()) copy.values.put(entry.getKey(), new Entry(entry.getValue().type(), adopt(shape(entry.getValue().type()), entry.getValue().primitive())));
        return copy;
    }

    /** How deeply containers and lists nest inside this value; deeper than the host reads is refused up front. */
    private static int nesting(Shape shape, Object value) {
        return switch (shape.kind()) {
            case TAG_CONTAINER -> 1 + ((FerriteDataContainer) value).nesting();
            case TAG_CONTAINER_ARRAY -> {
                int deepest = 0;
                for (var container : (FerriteDataContainer[]) value) deepest = Math.max(deepest, container.nesting());
                yield 1 + deepest;
            }
            case LIST -> {
                int deepest = 0;
                for (var item : (java.util.List<?>) value) deepest = Math.max(deepest, nesting(shape.element(), item));
                yield 1 + deepest;
            }
            default -> 0;
        };
    }

    private int nesting() {
        int deepest = 0;
        for (var entry : values.values()) deepest = Math.max(deepest, nesting(shape(entry.type()), entry.primitive()));
        return deepest;
    }

    public <P,C> void set(org.bukkit.NamespacedKey key, org.bukkit.persistence.PersistentDataType<P,C> type, C value) {
        Shape shape = shape(type);
        java.util.Objects.requireNonNull(key, "key");
        java.util.Objects.requireNonNull(value, "value");
        Object primitive = java.util.Objects.requireNonNull(type.toPrimitive(value, context), "primitive");
        if (!type.getPrimitiveType().isInstance(primitive)) throw new IllegalArgumentException("Ferrite: persistent data type produced the wrong primitive");
        Object stored = adopt(shape, primitive);
        if (nesting(shape, stored) > MAX_DEPTH) throw new IllegalArgumentException("Ferrite: persistent data nests deeper than " + MAX_DEPTH);
        values.put(key, new Entry(type, stored));
    }

    public <P,C> boolean has(org.bukkit.NamespacedKey key, org.bukkit.persistence.PersistentDataType<P,C> type) {
        Shape shape = shape(type);
        Entry entry = values.get(key);
        return entry != null && shape(entry.type()).equals(shape);
    }

    public boolean has(org.bukkit.NamespacedKey key) { return values.containsKey(key); }

    @SuppressWarnings("unchecked")
    public <P,C> C get(org.bukkit.NamespacedKey key, org.bukkit.persistence.PersistentDataType<P,C> type) {
        Shape shape = shape(type);
        Entry entry = values.get(key);
        if (entry == null || !shape(entry.type()).equals(shape)) return null;
        return (C) ((org.bukkit.persistence.PersistentDataType<Object,Object>) type).fromPrimitive(adopt(shape, entry.primitive()), context);
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
            target.set(entry.getKey(), type, type.fromPrimitive(adopt(shape(entry.getValue().type()), entry.getValue().primitive()), context));
        }
    }

    public org.bukkit.persistence.PersistentDataAdapterContext getAdapterContext() { return context; }
    public void remove(org.bukkit.NamespacedKey key) { values.remove(key); }

    private static void writeShape(java.io.DataOutputStream out, Shape shape) throws IOException {
        out.writeByte(shape.kind().ordinal());
        if (shape.kind() == Kind.LIST) writeShape(out, shape.element());
    }

    private static Shape readShape(java.io.DataInputStream in, int depth) throws IOException {
        if (depth > MAX_DEPTH) throw new IOException("Persistent data nests too deep");
        int ordinal = in.readUnsignedByte();
        if (ordinal >= Kind.values().length) throw new IOException("Unknown persistent data type " + ordinal);
        Kind kind = Kind.values()[ordinal];
        return kind == Kind.LIST ? new Shape(kind, readShape(in, depth + 1)) : new Shape(kind);
    }

    private static void writeValue(java.io.DataOutputStream out, Shape shape, Object value) throws IOException {
        switch (shape.kind()) {
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
            case TAG_CONTAINER -> { byte[] nested = ((FerriteDataContainer) value).serializeToBytes(); out.writeInt(nested.length); out.write(nested); }
            case TAG_CONTAINER_ARRAY -> {
                var containers = (FerriteDataContainer[]) value;
                out.writeInt(containers.length);
                for (var container : containers) writeValue(out, new Shape(Kind.TAG_CONTAINER), container);
            }
            case LIST -> {
                var items = (java.util.List<?>) value;
                out.writeInt(items.size());
                for (var item : items) writeValue(out, shape.element(), item);
            }
        }
    }

    private static Object readValue(java.io.DataInputStream input, Shape shape, int depth) throws IOException {
        if (depth > MAX_DEPTH) throw new IOException("Persistent data nests too deep");
        return switch (shape.kind()) {
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
            case TAG_CONTAINER -> {
                int length = input.readInt();
                if (length < 0 || length > MAX_ARRAY) throw new IOException("Invalid nested persistent data length " + length);
                byte[] nested = new byte[length];
                input.readFully(nested);
                var container = new FerriteDataContainer();
                container.readFromBytes(nested, true, depth + 1);
                yield container;
            }
            case TAG_CONTAINER_ARRAY -> {
                int length = input.readInt();
                if (length < 0 || length > MAX_ENTRIES) throw new IOException("Invalid persistent container array length " + length);
                var list = new java.util.ArrayList<FerriteDataContainer>();
                for (int item = 0; item < length; item++) list.add((FerriteDataContainer) readValue(input, new Shape(Kind.TAG_CONTAINER), depth + 1));
                yield list.toArray(new FerriteDataContainer[0]);
            }
            case LIST -> {
                int length = input.readInt();
                if (length < 0 || length > MAX_ENTRIES) throw new IOException("Invalid persistent list length " + length);
                var list = new java.util.ArrayList<Object>();
                for (int item = 0; item < length; item++) list.add(readValue(input, shape.element(), depth + 1));
                yield java.util.Collections.unmodifiableList(list);
            }
        };
    }

    public byte[] serializeToBytes() throws java.io.IOException {
        var bytes = new java.io.ByteArrayOutputStream();
        try (var out = new java.io.DataOutputStream(bytes)) {
            out.write(MAGIC);
            out.writeByte(VERSION);
            out.writeInt(values.size());
            for (var entry : values.entrySet()) {
                Shape shape = shape(entry.getValue().type());
                out.writeUTF(entry.getKey().toString());
                writeShape(out, shape);
                writeValue(out, shape, entry.getValue().primitive());
            }
        }
        return bytes.toByteArray();
    }

    public void readFromBytes(byte[] bytes, boolean clear) throws java.io.IOException { readFromBytes(bytes, clear, 0); }

    private void readFromBytes(byte[] bytes, boolean clear, int depth) throws java.io.IOException {
        if (bytes == null) throw new IOException("Persistent data bytes are null");
        if (depth > MAX_DEPTH) throw new IOException("Persistent data nests too deep");
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
                Shape shape = readShape(input, 0);
                parsed.put(key, new Entry(typeOf(shape), readValue(input, shape, depth)));
            }
            if (input.available() != 0) throw new IOException("Trailing bytes after persistent data");
        }
        if (clear) values.clear();
        values.putAll(parsed);
    }
}
