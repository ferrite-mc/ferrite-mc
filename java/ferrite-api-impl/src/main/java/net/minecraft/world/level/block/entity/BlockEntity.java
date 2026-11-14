package net.minecraft.world.level.block.entity;

import net.minecraft.core.BlockPos;

/**
 * A block entity as NMS names it. It never holds a copy of the server's tile data: every read
 * goes back to the owning world, which routes the read to the region that owns the block.
 */
public class BlockEntity {
    protected final org.bukkit.World world;
    protected final BlockPos pos;

    public BlockEntity(org.bukkit.World world, BlockPos pos) {
        this.world = java.util.Objects.requireNonNull(world, "world");
        this.pos = java.util.Objects.requireNonNull(pos, "pos");
    }

    public BlockPos getBlockPos() { return pos; }
    public net.minecraft.world.level.Level getLevel() { return net.minecraft.server.level.ServerLevel.forWorld(world); }
    public boolean hasLevel() { return true; }

    public net.minecraft.world.level.block.state.BlockState getBlockState() {
        return new net.minecraft.world.level.block.state.BlockState(
                world.getBlockAt(pos.getX(), pos.getY(), pos.getZ()).getBlockData());
    }

    /** Bukkit's block entity id when this position holds a tile state, else the block's material key. */
    public String getBukkitId() {
        var state = world.getBlockAt(pos.getX(), pos.getY(), pos.getZ()).getState();
        return state instanceof org.bukkit.block.TileState ? state.getType().getKey().toString() : getBlockState().getBlock().getKey().toString();
    }

    @Override public boolean equals(Object other) {
        return other instanceof BlockEntity entity && entity.world.equals(world) && entity.pos.equals(pos);
    }
    @Override public int hashCode() { return java.util.Objects.hash(world.getUID(), pos); }
    @Override public String toString() { return "BlockEntity[" + world.getName() + ":" + pos + "]"; }
}
