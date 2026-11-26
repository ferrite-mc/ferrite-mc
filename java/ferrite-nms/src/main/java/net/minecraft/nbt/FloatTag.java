// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.nbt;

public final class FloatTag extends NumericTag {
    public static final FloatTag ZERO = valueOf(0.0F);
    private final float data;

    private FloatTag(float data) {
        this.data = data;
    }

    public static FloatTag valueOf(float data) {
        return new FloatTag(data);
    }

    @Override
    public byte getId() {
        return TAG_FLOAT;
    }

    @Override
    public FloatTag copy() {
        return this;
    }

    @Override
    public long getAsLong() {
        return (long) data;
    }

    @Override
    public int getAsInt() {
        return (int) Math.floor(data);
    }

    @Override
    public short getAsShort() {
        return (short) (getAsInt() & 0xFFFF);
    }

    @Override
    public byte getAsByte() {
        return (byte) (getAsInt() & 0xFF);
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
        return this == o || o instanceof FloatTag f && Float.compare(data, f.data) == 0;
    }

    @Override
    public int hashCode() {
        return Float.hashCode(data);
    }

    @Override
    public String toString() {
        return data + "f";
    }
}
