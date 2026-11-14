package net.minecraft.world.level.chunk;

import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;

/** A loaded chunk as NMS names it; block reads go through the Bukkit chunk view. */
public class LevelChunk {
    private final org.bukkit.Chunk chunk;

    public LevelChunk(org.bukkit.Chunk chunk) {
        this.chunk = java.util.Objects.requireNonNull(chunk, "chunk");
    }

    public org.bukkit.Chunk getBukkitChunk() { return chunk; }
    public ServerLevel getLevel() { return ServerLevel.forWorld(chunk.getWorld()); }
    public int getPosX() { return chunk.getX(); }
    public int getPosZ() { return chunk.getZ(); }

    /** NMS takes a world-space position here. */
    public net.minecraft.world.level.block.state.BlockState getBlockState(BlockPos pos) {
        return new net.minecraft.world.level.block.state.BlockState(
                chunk.getWorld().getBlockAt(pos.getX(), pos.getY(), pos.getZ()).getBlockData());
    }

    /** Section-local coordinates, as the vanilla chunk sections index them. */
    public net.minecraft.world.level.block.state.BlockState getBlockState(int x, int y, int z) {
        if (x < 0 || x > 15 || z < 0 || z > 15) throw new IllegalArgumentException("Section-local x/z must be 0..15: " + x + "," + z);
        return new net.minecraft.world.level.block.state.BlockState(chunk.getBlock(x, y, z).getBlockData());
    }

    @Override public boolean equals(Object other) {
        return other instanceof LevelChunk level && level.chunk.getWorld().equals(chunk.getWorld())
                && level.chunk.getX() == chunk.getX() && level.chunk.getZ() == chunk.getZ();
    }
    @Override public int hashCode() { return java.util.Objects.hash(chunk.getWorld().getUID(), chunk.getX(), chunk.getZ()); }
    @Override public String toString() { return "LevelChunk[" + chunk.getWorld().getName() + ":" + chunk.getX() + "," + chunk.getZ() + "]"; }
}
