// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.world.item;

import java.util.Objects;
import org.bukkit.Material;

/** Clean-room item wrapper around a Bukkit material key. */
public final class Item {
    private final Material material;

    public Item(Material material) {
        this.material = Objects.requireNonNull(material, "material");
        if (!material.isItem()) {
            throw new IllegalArgumentException(material + " is not an item in Minecraft 26.3");
        }
    }

    public Material getBukkitMaterial() {
        return material;
    }

    public String getName() {
        return material.getKey().toString();
    }

    public int getDefaultMaxStackSize() {
        return material.getMaxStackSize();
    }

    public ItemStack getDefaultInstance() {
        return ItemStack.of(material, 1);
    }

    @Override
    public boolean equals(Object other) {
        return this == other || other instanceof Item item && material == item.material;
    }

    @Override
    public int hashCode() {
        return material.hashCode();
    }

    @Override
    public String toString() {
        return "Item{" + getName() + "}";
    }
}
