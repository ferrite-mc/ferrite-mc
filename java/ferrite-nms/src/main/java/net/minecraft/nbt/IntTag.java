// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.nbt;

public final class IntTag extends NumericTag {
    private final int data;

    private IntTag(int data) {
        this.data = data;
    }

    public static IntTag valueOf(int data) {
        return new IntTag(data);
    }

    @Override
    public byte getId() {
        return TAG_INT;
    }

    @Override
    public IntTag copy() {
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
        return (short) (data & 0xFFFF);
    }

    @Override
    public byte getAsByte() {
        return (byte) (data & 0xFF);
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
        return this == o || o instanceof IntTag i && data == i.data;
    }

    @Override
    public int hashCode() {
        return Integer.hashCode(data);
    }

    @Override
    public String toString() {
        return Integer.toString(data);
    }
}
