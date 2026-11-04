package dev.ferrite.server;

final class FerriteDataContainer implements org.bukkit.persistence.PersistentDataContainer {
    private record Entry(org.bukkit.persistence.PersistentDataType<?, ?> type, Object primitive) {}
    private final java.util.Map<org.bukkit.NamespacedKey, Entry> values = new java.util.HashMap<>();
    private final org.bukkit.persistence.PersistentDataAdapterContext context = FerriteDataContainer::new;
    /** Same keys holding equal primitives, which is what makes two otherwise equal item stacks similar. */
    boolean sameAs(FerriteDataContainer other) {
        if (values.size() != other.values.size()) return false;
        for (var entry : values.entrySet()) { var theirs = other.values.get(entry.getKey()); if (theirs == null || !java.util.Objects.deepEquals(entry.getValue().primitive(), theirs.primitive())) return false; }
        return true;
    }
    FerriteDataContainer copy() { var copy = new FerriteDataContainer(); copy.values.putAll(values); return copy; }
    public <P,C> void set(org.bukkit.NamespacedKey key, org.bukkit.persistence.PersistentDataType<P,C> type, C value) { values.put(java.util.Objects.requireNonNull(key), new Entry(type, type.toPrimitive(java.util.Objects.requireNonNull(value), context))); }
    public <P,C> boolean has(org.bukkit.NamespacedKey key, org.bukkit.persistence.PersistentDataType<P,C> type) { var entry = values.get(key); return entry != null && type.getPrimitiveType().isInstance(entry.primitive()); }
    public boolean has(org.bukkit.NamespacedKey key) { return values.containsKey(key); }
    public <P,C> C get(org.bukkit.NamespacedKey key, org.bukkit.persistence.PersistentDataType<P,C> type) { var entry = values.get(key); if (entry == null) return null; return type.fromPrimitive(type.getPrimitiveType().cast(entry.primitive()), context); }
    public <P,C> C getOrDefault(org.bukkit.NamespacedKey key, org.bukkit.persistence.PersistentDataType<P,C> type, C fallback) { C value = get(key, type); return value == null ? fallback : value; }
    public org.bukkit.persistence.PersistentDataType<?,?> getPrimitiveStorageType(net.kyori.adventure.key.Key key) { var entry = values.get(org.bukkit.NamespacedKey.fromString(key.asString())); return entry == null ? null : entry.type(); }
    public java.util.Set<org.bukkit.NamespacedKey> getKeys() { return java.util.Set.copyOf(values.keySet()); }
    public boolean isEmpty() { return values.isEmpty(); }
    public int getSize() { return values.size(); }
    @SuppressWarnings("unchecked") public void copyTo(org.bukkit.persistence.PersistentDataContainer target, boolean replace) {
        for (var entry : values.entrySet()) if (replace || !target.has(entry.getKey())) {
            var type = (org.bukkit.persistence.PersistentDataType<Object,Object>) entry.getValue().type();
            target.set(entry.getKey(), type, type.fromPrimitive(entry.getValue().primitive(), context));
        }
    }
    public org.bukkit.persistence.PersistentDataAdapterContext getAdapterContext() { return context; }
    public void remove(org.bukkit.NamespacedKey key) { values.remove(key); }
    public byte[] serializeToBytes() throws java.io.IOException { throw new java.io.IOException("Persistent data serialization is not implemented"); }
    public void readFromBytes(byte[] bytes, boolean clear) throws java.io.IOException { throw new java.io.IOException("Persistent data serialization is not implemented"); }
}
