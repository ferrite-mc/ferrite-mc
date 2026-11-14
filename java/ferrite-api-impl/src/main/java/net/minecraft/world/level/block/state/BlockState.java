package net.minecraft.world.level.block.state;

/** A block state as NMS names it; every value is the Bukkit block data at that position. */
public class BlockState {
    private final org.bukkit.block.data.BlockData data;

    public BlockState(org.bukkit.block.data.BlockData data) {
        this.data = java.util.Objects.requireNonNull(data, "data");
    }

    public org.bukkit.block.data.BlockData getBukkitBlockData() { return data; }
    public org.bukkit.Material getBukkitMaterial() { return data.getMaterial(); }
    public net.minecraft.world.level.block.Block getBlock() { return new net.minecraft.world.level.block.Block(data.getMaterial()); }
    public boolean isAir() { return data.getMaterial().isAir(); }
    public boolean isSolid() { return data.getMaterial().isSolid(); }
    public java.util.Collection<org.bukkit.block.data.BlockData> getStates() { return java.util.List.of(data); }

    @Override public boolean equals(Object other) {
        return other instanceof BlockState state && state.data.getAsString().equals(data.getAsString());
    }
    @Override public int hashCode() { return data.getAsString().hashCode(); }
    @Override public String toString() { return "BlockState[" + data.getAsString() + "]"; }
}
