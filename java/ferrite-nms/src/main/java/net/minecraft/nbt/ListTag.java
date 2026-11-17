// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.nbt;

import java.util.AbstractList;
import java.util.ArrayList;
import java.util.List;
import java.util.Objects;

public final class ListTag extends AbstractList<Tag> implements Tag {
    private final List<Tag> list = new ArrayList<>();

    public ListTag() {}

    public ListTag(List<Tag> initial) {
        for (Tag tag : initial) {
            add(tag.copy());
        }
    }

    @Override
    public Tag get(int index) {
        return list.get(index);
    }

    @Override
    public int size() {
        return list.size();
    }

    @Override
    public void add(int index, Tag element) {
        list.add(index, Objects.requireNonNull(element, "element"));
    }

    @Override
    public Tag set(int index, Tag element) {
        return list.set(index, Objects.requireNonNull(element, "element"));
    }

    @Override
    public Tag remove(int index) {
        return list.remove(index);
    }

    public String getString(int index) {
        if (index >= 0 && index < list.size() && list.get(index) instanceof StringTag s) {
            return s.asString();
        }
        return "";
    }

    public CompoundTag getCompound(int index) {
        if (index >= 0 && index < list.size() && list.get(index) instanceof CompoundTag c) {
            return c;
        }
        return new CompoundTag();
    }

    @Override
    public byte getId() {
        return TAG_LIST;
    }

    @Override
    public ListTag copy() {
        return new ListTag(list);
    }
}
