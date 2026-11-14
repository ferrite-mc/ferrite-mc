package org.bukkit.craftbukkit.util;

/**
 * The Bukkit-typed half of CraftBukkit's {@code CraftMagicNumbers}. Its NMS-typed conversions
 * ({@code getBlock(Material)}, {@code getItem(Material)}, ...) cannot exist without
 * {@code net.minecraft} block/item classes, so they stay absent; everything expressible through
 * the Bukkit registries is answered from the 26.3 data and unknown values fail loudly instead of
 * becoming air.
 */
public final class CraftMagicNumbers {
    private CraftMagicNumbers() {}

    /** The live {@link org.bukkit.UnsafeValues} (what CraftBukkit exposes as {@code INSTANCE}). */
    public static final org.bukkit.UnsafeValues INSTANCE = org.bukkit.Bukkit.getUnsafe();

    /** The material registered under a namespaced key such as {@code minecraft:stone}. */
    public static org.bukkit.Material getMaterial(String key) {
        java.util.Objects.requireNonNull(key, "key");
        var resolved = org.bukkit.NamespacedKey.fromString(key);
        var material = resolved == null ? null : org.bukkit.Registry.MATERIAL.get(resolved);
        if (material == null) throw new IllegalArgumentException("Unknown material " + key + " in Minecraft 26.3");
        return material;
    }

    /** The namespaced key of a material. */
    public static org.bukkit.NamespacedKey getKey(org.bukkit.Material material) {
        return java.util.Objects.requireNonNull(material, "material").getKey();
    }

    /** The default block data of a block material. */
    public static org.bukkit.block.data.BlockData getDefaultBlockData(org.bukkit.Material material) {
        if (!material.isBlock()) throw new IllegalArgumentException(material + " is not a block");
        return material.createBlockData();
    }

    /** Whether the material is one of the pre-1.13 legacy names. */
    public static boolean isLegacy(org.bukkit.Material material) { return material.isLegacy(); }

    /** The current material for a legacy one, as {@code Material.getMaterial(name, true)} does. */
    public static org.bukkit.Material fromLegacy(org.bukkit.Material material) {
        var modern = INSTANCE.fromLegacy(material);
        if (modern == null) throw new IllegalArgumentException("No modern material for " + material);
        return modern;
    }
}
