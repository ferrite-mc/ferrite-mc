// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.nbt;

import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;
import java.util.Set;

/** Clean-room compound NBT map matching 26.3 {@code net.minecraft.nbt.CompoundTag}. */
public final class CompoundTag implements Tag {
    private final Map<String, Tag> tags = new LinkedHashMap<>();

    public CompoundTag() {}

    public Set<String> keySet() {
        return tags.keySet();
    }

    public int size() {
        return tags.size();
    }

    public boolean isEmpty() {
        return tags.isEmpty();
    }

    public Tag put(String key, Tag value) {
        return tags.put(Objects.requireNonNull(key, "key"), Objects.requireNonNull(value, "value"));
    }

    public void putByte(String key, byte value) {
        put(key, ByteTag.valueOf(value));
    }

    public void putShort(String key, short value) {
        put(key, ShortTag.valueOf(value));
    }

    public void putInt(String key, int value) {
        put(key, IntTag.valueOf(value));
    }

    public void putLong(String key, long value) {
        put(key, LongTag.valueOf(value));
    }

    public void putFloat(String key, float value) {
        put(key, FloatTag.valueOf(value));
    }

    public void putDouble(String key, double value) {
        put(key, DoubleTag.valueOf(value));
    }

    public void putString(String key, String value) {
        put(key, StringTag.valueOf(value));
    }

    public void putByteArray(String key, byte[] value) {
        put(key, new ByteArrayTag(value));
    }

    public void putIntArray(String key, int[] value) {
        put(key, new IntArrayTag(value));
    }

    public void putLongArray(String key, long[] value) {
        put(key, new LongArrayTag(value));
    }

    public void putBoolean(String key, boolean value) {
        put(key, ByteTag.valueOf(value));
    }

    public Tag get(String key) {
        return tags.get(key);
    }

    public boolean contains(String key) {
        return tags.containsKey(key);
    }

    public boolean contains(String key, int type) {
        Tag t = tags.get(key);
        if (t == null) {
            return false;
        }
        if (type == TAG_ANY_NUMERIC) {
            return t instanceof NumericTag;
        }
        return t.getId() == type;
    }

    public byte getByte(String key) {
        return tags.get(key) instanceof NumericTag n ? n.getAsByte() : 0;
    }

    public short getShort(String key) {
        return tags.get(key) instanceof NumericTag n ? n.getAsShort() : 0;
    }

    public int getInt(String key) {
        return tags.get(key) instanceof NumericTag n ? n.getAsInt() : 0;
    }

    public long getLong(String key) {
        return tags.get(key) instanceof NumericTag n ? n.getAsLong() : 0L;
    }

    public float getFloat(String key) {
        return tags.get(key) instanceof NumericTag n ? n.getAsFloat() : 0.0F;
    }

    public double getDouble(String key) {
        return tags.get(key) instanceof NumericTag n ? n.getAsDouble() : 0.0D;
    }

    public boolean getBoolean(String key) {
        return getByte(key) != 0;
    }

    public String getString(String key) {
        return tags.get(key) instanceof StringTag s ? s.asString() : "";
    }

    public Optional<String> getStringOrEmpty(String key) {
        return tags.get(key) instanceof StringTag s ? Optional.of(s.asString()) : Optional.empty();
    }

    public byte[] getByteArray(String key) {
        return tags.get(key) instanceof ByteArrayTag b ? b.getAsByteArray() : new byte[0];
    }

    public int[] getIntArray(String key) {
        return tags.get(key) instanceof IntArrayTag i ? i.getAsIntArray() : new int[0];
    }

    public long[] getLongArray(String key) {
        return tags.get(key) instanceof LongArrayTag l ? l.getAsLongArray() : new long[0];
    }

    public CompoundTag getCompound(String key) {
        return tags.get(key) instanceof CompoundTag c ? c : new CompoundTag();
    }

    public ListTag getList(String key) {
        return tags.get(key) instanceof ListTag l ? l : new ListTag();
    }

    public Tag remove(String key) {
        return tags.remove(key);
    }

    @Override
    public byte getId() {
        return TAG_COMPOUND;
    }

    @Override
    public CompoundTag copy() {
        CompoundTag copy = new CompoundTag();
        for (Map.Entry<String, Tag> entry : tags.entrySet()) {
            copy.put(entry.getKey(), entry.getValue().copy());
        }
        return copy;
    }

    @Override
    public boolean equals(Object o) {
        return this == o || o instanceof CompoundTag c && tags.equals(c.tags);
    }

    @Override
    public int hashCode() {
        return tags.hashCode();
    }

    @Override
    public String toString() {
        return tags.toString();
    }
}
