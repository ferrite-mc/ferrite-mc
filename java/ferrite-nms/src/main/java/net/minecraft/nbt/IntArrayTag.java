// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.nbt;

import java.util.Arrays;

public final class IntArrayTag implements Tag {
    private int[] data;

    public IntArrayTag(int[] data) {
        this.data = data.clone();
    }

    public int[] getAsIntArray() {
        return data;
    }

    @Override
    public byte getId() {
        return TAG_INT_ARRAY;
    }

    @Override
    public IntArrayTag copy() {
        return new IntArrayTag(data);
    }

    @Override
    public boolean equals(Object o) {
        return this == o || o instanceof IntArrayTag a && Arrays.equals(data, a.data);
    }

    @Override
    public int hashCode() {
        return Arrays.hashCode(data);
    }

    @Override
    public String toString() {
        return "[I;" + data.length + "]";
    }
}
