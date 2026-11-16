// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.world.item;

import java.util.Objects;
import org.bukkit.Material;

/**
 * Clean-room item stack. It is a value object, not world state: when built from a Paper stack it
 * keeps a defensive copy of the whole stack, so display name, lore, persistent data and the
 * specialised metadata all survive the NMS round trip without being reinterpreted.
 */
public final class ItemStack {
    public static final ItemStack EMPTY = new ItemStack(Material.AIR, 0, null);

    private final Material material;
    private final org.bukkit.inventory.ItemStack source;
    private int count;

    private ItemStack(Material material, int count, org.bukkit.inventory.ItemStack source) {
        this.material = material;
        this.count = count;
        this.source = source;
    }

    /** A material/count stack without Paper metadata; useful for fixtures and NMS-side values. */
    public static ItemStack of(Material material, int count) {
        Objects.requireNonNull(material, "material");
        if (count < 0) {
            throw new IllegalArgumentException("count must not be negative");
        }
        if (count == 0 || material.isAir()) {
            return EMPTY;
        }
        return new ItemStack(material, count, null);
    }

    /** A lossless copy of a Paper stack, carrying every metadata field the facade provided. */
    public static ItemStack of(org.bukkit.inventory.ItemStack stack) {
        if (stack == null) {
            return EMPTY;
        }
        Material type = Objects.requireNonNull(stack.getType(), "stack type");
        int amount = stack.getAmount();
        if (type.isAir() || amount <= 0) {
            return EMPTY;
        }
        return new ItemStack(type, amount, stack.clone());
    }

    public boolean isEmpty() {
        return count <= 0 || material.isAir();
    }

    public int getCount() {
        return isEmpty() ? 0 : count;
    }

    public void setCount(int count) {
        if (isEmpty()) {
            if (count > 0) {
                throw new IllegalStateException("Cannot give a non-zero count to an empty Ferrite NMS item stack");
            }
            this.count = 0;
            return;
        }
        this.count = Math.max(count, 0);
        if (source != null) {
            source.setAmount(this.count);
        }
    }

    public void shrink(int amount) {
        if (amount < 0) {
            throw new IllegalArgumentException("shrink amount must not be negative");
        }
        setCount(getCount() - amount);
    }

    public void grow(int amount) {
        if (amount < 0) {
            throw new IllegalArgumentException("grow amount must not be negative");
        }
        setCount(getCount() + amount);
    }

    public Material getBukkitMaterial() {
        return material;
    }

    public Item getItem() {
        return new Item(material);
    }

    public int getMaxStackSize() {
        return material.getMaxStackSize();
    }

    public ItemStack copy() {
        if (isEmpty()) {
            return EMPTY;
        }
        return source == null ? of(material, count) : of(source.clone());
    }

    /**
     * A defensive Paper copy. The metadata-less form materialises through the Bukkit facade; when
     * only Lane 2's item factory can do that, the existing explicit facade error is preserved.
     */
    public org.bukkit.inventory.ItemStack toBukkit() {
        if (isEmpty()) {
            return null;
        }
        if (source != null) {
            org.bukkit.inventory.ItemStack copy = source.clone();
            copy.setAmount(count);
            return copy;
        }
        return new org.bukkit.inventory.ItemStack(material, count);
    }

    @Override
    public boolean equals(Object other) {
        if (this == other) {
            return true;
        }
        if (!(other instanceof ItemStack stack)) {
            return false;
        }
        return count == stack.count
                && material == stack.material
                && Objects.equals(source, stack.source);
    }

    @Override
    public int hashCode() {
        return Objects.hash(material, count, source);
    }

    @Override
    public String toString() {
        return getCount() + " " + material.getKey();
    }
}
