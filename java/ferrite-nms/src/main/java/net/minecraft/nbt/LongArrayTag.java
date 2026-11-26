// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.nbt;

import java.util.Arrays;

public final class LongArrayTag implements Tag {
    private long[] data;

    public LongArrayTag(long[] data) {
        this.data = data.clone();
    }

    public long[] getAsLongArray() {
        return data;
    }

    @Override
    public byte getId() {
        return TAG_LONG_ARRAY;
    }

    @Override
    public LongArrayTag copy() {
        return new LongArrayTag(data);
    }

    @Override
    public boolean equals(Object o) {
        return this == o || o instanceof LongArrayTag l && Arrays.equals(data, l.data);
    }

    @Override
    public int hashCode() {
        return Arrays.hashCode(data);
    }

    @Override
    public String toString() {
        return "[L;" + data.length + "]";
    }
}
