// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.nbt;

public final class EndTag implements Tag {
    public static final EndTag INSTANCE = new EndTag();

    private EndTag() {}

    @Override
    public byte getId() {
        return TAG_END;
    }

    @Override
    public EndTag copy() {
        return this;
    }

    @Override
    public String toString() {
        return "END";
    }
}
