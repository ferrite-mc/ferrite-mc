package org.bukkit.craftbukkit.util;

/**
 * The CraftBukkit compatibility conversions backed by Ferrite's 26.3 registries.
 *
 * <p>Bukkit- and NMS-typed lookups agree on the same registry identity: a material, its
 * {@code net.minecraft.world.level.block.Block}/{@code net.minecraft.world.item.Item} wrapper,
 * and its block state all come from the generated vanilla data. Unknown values fail loudly
 * instead of becoming air, and values that have no 26.3 representation are refused explicitly.
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

    /** The NMS block wrapper of a block material. */
    public static net.minecraft.world.level.block.Block getBlock(org.bukkit.Material material) {
        java.util.Objects.requireNonNull(material, "material");
        if (!material.isBlock()) throw new IllegalArgumentException(material + " is not a block");
        return new net.minecraft.world.level.block.Block(material);
    }

    /** The NMS item wrapper of an item material. */
    public static net.minecraft.world.item.Item getItem(org.bukkit.Material material) {
        java.util.Objects.requireNonNull(material, "material");
        if (!material.isItem()) throw new IllegalArgumentException(material + " is not an item");
        return new net.minecraft.world.item.Item(material);
    }

    /** The NMS block state for a Bukkit block data value. */
    public static net.minecraft.world.level.block.state.BlockState getBlockState(org.bukkit.block.data.BlockData data) {
        return new net.minecraft.world.level.block.state.BlockState(java.util.Objects.requireNonNull(data, "data"));
    }

    /** The material of an NMS block wrapper. */
    public static org.bukkit.Material getMaterial(net.minecraft.world.level.block.Block block) {
        return java.util.Objects.requireNonNull(block, "block").getBukkitMaterial();
    }

    /** The material of an NMS item wrapper. */
    public static org.bukkit.Material getMaterial(net.minecraft.world.item.Item item) {
        return java.util.Objects.requireNonNull(item, "item").getBukkitMaterial();
    }

    /** The material of an NMS block state. */
    public static org.bukkit.Material getMaterial(net.minecraft.world.level.block.state.BlockState state) {
        return java.util.Objects.requireNonNull(state, "state").getBukkitMaterial();
    }

    /** The 26.3 global block-state id used by level events, from the generated block table. */
    public static int getBlockStateId(org.bukkit.block.data.BlockData data) {
        return dev.ferrite.server.FerriteUnsafe.blockStateId(data);
    }

    /** The 26.3 global block-state id of an NMS block state. */
    public static int getBlockStateId(net.minecraft.world.level.block.state.BlockState state) {
        return getBlockStateId(java.util.Objects.requireNonNull(state, "state").getBukkitBlockData());
    }

    /** The numeric data version of the 26.3 data pack. */
    public static int getDataVersion() { return INSTANCE.getDataVersion(); }

    /** Whether the material is one of the pre-1.13 legacy names. */
    public static boolean isLegacy(org.bukkit.Material material) { return material.isLegacy(); }

    /** The current material for a legacy one, as {@code Material.getMaterial(name, true)} does. */
    public static org.bukkit.Material fromLegacy(org.bukkit.Material material) {
        var modern = INSTANCE.fromLegacy(material);
        if (modern == null) throw new IllegalArgumentException("No modern material for " + material);
        return modern;
    }
}
