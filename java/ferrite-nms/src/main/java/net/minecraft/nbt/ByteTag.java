// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.nbt;

public final class ByteTag extends NumericTag {
    public static final ByteTag ZERO = valueOf((byte) 0);
    public static final ByteTag ONE = valueOf((byte) 1);

    private final byte data;

    private ByteTag(byte data) {
        this.data = data;
    }

    public static ByteTag valueOf(byte data) {
        return new ByteTag(data);
    }

    public static ByteTag valueOf(boolean data) {
        return data ? ONE : ZERO;
    }

    @Override
    public byte getId() {
        return TAG_BYTE;
    }

    @Override
    public ByteTag copy() {
        return this;
    }

    @Override
    public long getAsLong() {
        return data;
    }

    @Override
    public int getAsInt() {
        return data;
    }

    @Override
    public short getAsShort() {
        return data;
    }

    @Override
    public byte getAsByte() {
        return data;
    }

    @Override
    public double getAsDouble() {
        return data;
    }

    @Override
    public float getAsFloat() {
        return data;
    }

    @Override
    public Number getAsNumber() {
        return data;
    }

    @Override
    public boolean equals(Object o) {
        return this == o || o instanceof ByteTag b && data == b.data;
    }

    @Override
    public int hashCode() {
        return Byte.hashCode(data);
    }

    @Override
    public String toString() {
        return data + "b";
    }
}
