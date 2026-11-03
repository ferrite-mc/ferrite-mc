package dev.ferrite.server;

import com.destroystokyo.paper.SkinParts;

/** Minecraft's seven client skin-layer flags. */
class FerriteSkinParts implements SkinParts {
    private int flags;

    FerriteSkinParts(int flags) { this.flags = flags; }
    private boolean enabled(int bit) { return (flags & bit) != 0; }
    @Override public boolean hasCapeEnabled() { return enabled(0x01); }
    @Override public boolean hasJacketEnabled() { return enabled(0x02); }
    @Override public boolean hasLeftSleeveEnabled() { return enabled(0x04); }
    @Override public boolean hasRightSleeveEnabled() { return enabled(0x08); }
    @Override public boolean hasLeftPantsEnabled() { return enabled(0x10); }
    @Override public boolean hasRightPantsEnabled() { return enabled(0x20); }
    @Override public boolean hasHatsEnabled() { return enabled(0x40); }
    @Override public int getRaw() { return flags; }
    @Override public SkinParts.Mutable mutableCopy() { return new Mutable(flags); }

    static final class Mutable extends FerriteSkinParts implements SkinParts.Mutable {
        Mutable(int flags) { super(flags); }
        private void set(int bit, boolean enabled) {
            super.flags = enabled ? super.flags | bit : super.flags & ~bit;
        }
        @Override public void setCapeEnabled(boolean value) { set(0x01, value); }
        @Override public void setJacketEnabled(boolean value) { set(0x02, value); }
        @Override public void setLeftSleeveEnabled(boolean value) { set(0x04, value); }
        @Override public void setRightSleeveEnabled(boolean value) { set(0x08, value); }
        @Override public void setLeftPantsEnabled(boolean value) { set(0x10, value); }
        @Override public void setRightPantsEnabled(boolean value) { set(0x20, value); }
        @Override public void setHatsEnabled(boolean value) { set(0x40, value); }
        @Override public SkinParts immutableCopy() { return new FerriteSkinParts(getRaw()); }
    }
}
