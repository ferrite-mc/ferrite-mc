// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.nbt;

public final class DoubleTag extends NumericTag {
    public static final DoubleTag ZERO = valueOf(0.0D);
    private final double data;

    private DoubleTag(double data) {
        this.data = data;
    }

    public static DoubleTag valueOf(double data) {
        return new DoubleTag(data);
    }

    @Override
    public byte getId() {
        return TAG_DOUBLE;
    }

    @Override
    public DoubleTag copy() {
        return this;
    }

    @Override
    public long getAsLong() {
        return (long) Math.floor(data);
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
        return (float) data;
    }

    @Override
    public Number getAsNumber() {
        return data;
    }

    @Override
    public boolean equals(Object o) {
        return this == o || o instanceof DoubleTag d && Double.compare(data, d.data) == 0;
    }

    @Override
    public int hashCode() {
        return Double.hashCode(data);
    }

    @Override
    public String toString() {
        return data + "d";
    }
}
