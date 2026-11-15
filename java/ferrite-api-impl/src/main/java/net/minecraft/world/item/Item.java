package net.minecraft.world.item;

/** An item type as NMS names it; the registry identity is the Bukkit material key. */
public final class Item {
    private final org.bukkit.Material material;

    public Item(org.bukkit.Material material) {
        this.material = java.util.Objects.requireNonNull(material, "material");
        if (!material.isItem()) throw new IllegalArgumentException(material + " is not an item");
    }

    public org.bukkit.Material getBukkitMaterial() { return material; }
    public org.bukkit.NamespacedKey getKey() { return material.getKey(); }
    public String getName() { return material.getKey().getKey(); }
    public int getDefaultMaxStackSize() { return material.getMaxStackSize(); }
    public boolean isEdible() { return material.isEdible(); }
    public ItemStack getDefaultInstance() { return ItemStack.of(new org.bukkit.inventory.ItemStack(material)); }

    @Override public boolean equals(Object other) {
        return other instanceof Item item && item.material == material;
    }
    @Override public int hashCode() { return material.hashCode(); }
    @Override public String toString() { return "Item[" + material.getKey() + "]"; }
}
