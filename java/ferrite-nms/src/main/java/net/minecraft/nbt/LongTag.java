// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.nbt;

public final class LongTag extends NumericTag {
    private final long data;

    private LongTag(long data) {
        this.data = data;
    }

    public static LongTag valueOf(long data) {
        return new LongTag(data);
    }

    @Override
    public byte getId() {
        return TAG_LONG;
    }

    @Override
    public LongTag copy() {
        return this;
    }

    @Override
    public long getAsLong() {
        return data;
    }

    @Override
    public int getAsInt() {
        return (int) data;
    }

    @Override
    public short getAsShort() {
        return (short) (data & 0xFFFFL);
    }

    @Override
    public byte getAsByte() {
        return (byte) (data & 0xFFL);
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
        return this == o || o instanceof LongTag l && data == l.data;
    }

    @Override
    public int hashCode() {
        return Long.hashCode(data);
    }

    @Override
    public String toString() {
        return data + "L";
    }
}
