// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.nbt;

import java.util.Objects;

public final class StringTag implements Tag {
    private final String data;

    private StringTag(String data) {
        this.data = Objects.requireNonNull(data, "data");
    }

    public static StringTag valueOf(String data) {
        return new StringTag(data);
    }

    @Override
    public byte getId() {
        return TAG_STRING;
    }

    @Override
    public StringTag copy() {
        return this;
    }

    @Override
    public String asString() {
        return data;
    }

    @Override
    public boolean equals(Object o) {
        return this == o || o instanceof StringTag s && data.equals(s.data);
    }

    @Override
    public int hashCode() {
        return data.hashCode();
    }

    @Override
    public String toString() {
        return "\"" + data.replace("\"", "\\\"") + "\"";
    }
}
