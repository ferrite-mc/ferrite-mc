// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.nbt;

public final class ShortTag extends NumericTag {
    private final short data;

    private ShortTag(short data) {
        this.data = data;
    }

    public static ShortTag valueOf(short data) {
        return new ShortTag(data);
    }

    @Override
    public byte getId() {
        return TAG_SHORT;
    }

    @Override
    public ShortTag copy() {
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
        return this == o || o instanceof ShortTag s && data == s.data;
    }

    @Override
    public int hashCode() {
        return Short.hashCode(data);
    }

    @Override
    public String toString() {
        return data + "s";
    }
}
