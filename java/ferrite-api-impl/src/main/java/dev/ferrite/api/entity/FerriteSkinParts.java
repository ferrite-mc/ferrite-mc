package dev.ferrite.api.entity;

import com.destroystokyo.paper.SkinParts;

/** Clean-room {@link SkinParts.Mutable} implementation for Paper client options and bridge defaults. */
public final class FerriteSkinParts implements SkinParts.Mutable {

    private int raw;

    public FerriteSkinParts(int raw) {
        this.raw = raw & 0x7F;
    }

    public static FerriteSkinParts allParts() {
        return new FerriteSkinParts(0x7F);
    }

    private boolean bit(int mask) {
        return (raw & mask) != 0;
    }

    private void setBit(int mask, boolean enabled) {
        if (enabled) {
            raw |= mask;
        } else {
            raw &= ~mask;
        }
    }

    @Override
    public boolean hasCapeEnabled() {
        return bit(1 << 0);
    }

    @Override
    public void setCapeEnabled(boolean enabled) {
        setBit(1 << 0, enabled);
    }

    @Override
    public boolean hasJacketEnabled() {
        return bit(1 << 1);
    }

    @Override
    public void setJacketEnabled(boolean enabled) {
        setBit(1 << 1, enabled);
    }

    @Override
    public boolean hasLeftSleeveEnabled() {
        return bit(1 << 2);
    }

    @Override
    public void setLeftSleeveEnabled(boolean enabled) {
        setBit(1 << 2, enabled);
    }

    @Override
    public boolean hasRightSleeveEnabled() {
        return bit(1 << 3);
    }

    @Override
    public void setRightSleeveEnabled(boolean enabled) {
        setBit(1 << 3, enabled);
    }

    @Override
    public boolean hasLeftPantsEnabled() {
        return bit(1 << 4);
    }

    @Override
    public void setLeftPantsEnabled(boolean enabled) {
        setBit(1 << 4, enabled);
    }

    @Override
    public boolean hasRightPantsEnabled() {
        return bit(1 << 5);
    }

    @Override
    public void setRightPantsEnabled(boolean enabled) {
        setBit(1 << 5, enabled);
    }

    @Override
    public boolean hasHatsEnabled() {
        return bit(1 << 6);
    }

    @Override
    public void setHatsEnabled(boolean enabled) {
        setBit(1 << 6, enabled);
    }

    @Override
    public int getRaw() {
        return raw;
    }

    @Override
    public SkinParts.Mutable mutableCopy() {
        return new FerriteSkinParts(raw);
    }

    @Override
    public SkinParts immutableCopy() {
        return new FerriteSkinParts(raw);
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof SkinParts parts && parts.getRaw() == raw;
    }

    @Override
    public int hashCode() {
        return raw;
    }
}
