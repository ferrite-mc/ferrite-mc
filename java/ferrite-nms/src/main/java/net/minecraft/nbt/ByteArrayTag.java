// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.nbt;

import java.util.Arrays;

public final class ByteArrayTag implements Tag {
    private byte[] data;

    public ByteArrayTag(byte[] data) {
        this.data = data.clone();
    }

    public byte[] getAsByteArray() {
        return data;
    }

    @Override
    public byte getId() {
        return TAG_BYTE_ARRAY;
    }

    @Override
    public ByteArrayTag copy() {
        return new ByteArrayTag(data);
    }

    @Override
    public boolean equals(Object o) {
        return this == o || o instanceof ByteArrayTag b && Arrays.equals(data, b.data);
    }

    @Override
    public int hashCode() {
        return Arrays.hashCode(data);
    }

    @Override
    public String toString() {
        return "[B;" + data.length + "]";
    }
}
