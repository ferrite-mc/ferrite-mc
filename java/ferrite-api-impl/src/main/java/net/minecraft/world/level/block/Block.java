package net.minecraft.world.level.block;

/** A block type as NMS names it; the registry identity is the Bukkit material key. */
public class Block {
    private final org.bukkit.Material material;

    public Block(org.bukkit.Material material) {
        this.material = java.util.Objects.requireNonNull(material, "material");
    }

    public org.bukkit.Material getBukkitMaterial() { return material; }
    public org.bukkit.NamespacedKey getKey() { return material.getKey(); }
    public String getName() { return material.getKey().getKey(); }

    /** The block's default state, built by the server's block registry. */
    public net.minecraft.world.level.block.state.BlockState defaultBlockState() {
        return new net.minecraft.world.level.block.state.BlockState(org.bukkit.Bukkit.createBlockData(material));
    }

    @Override public boolean equals(Object other) {
        return other instanceof Block block && block.material == material;
    }
    @Override public int hashCode() { return material.hashCode(); }
    @Override public String toString() { return "Block[" + material.getKey() + "]"; }
}
