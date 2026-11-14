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

    /** The block data of a block material, as CraftBukkit's {@code getBlockData(Material)}. */
    public static org.bukkit.block.data.BlockData getBlockData(org.bukkit.Material material) { return getDefaultBlockData(material); }

    /** The material of a block data instance. */
    public static org.bukkit.Material getMaterial(org.bukkit.block.data.BlockData data) { return java.util.Objects.requireNonNull(data, "data").getMaterial(); }

    /** Minecraft 26.3 addresses block states by name and properties, not by a global numeric id. */
    public static int getBlockStateId(org.bukkit.block.data.BlockData data) {
        throw new UnsupportedOperationException("Ferrite: Minecraft 26.3 has no global block state ids; address the state by its material and properties instead");
    }

    /** Ferrite does not publish the numeric vanilla data version for 26.3; the protocol version is the supported identity. */
    public static int getDataVersion() {
        throw new UnsupportedOperationException("Ferrite: no numeric data version is published for Minecraft 26.3; use the protocol version instead");
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
