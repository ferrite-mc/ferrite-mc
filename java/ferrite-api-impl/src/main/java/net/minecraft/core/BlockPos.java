package net.minecraft.core;

public class BlockPos extends Vec3i {
    public static final BlockPos ZERO = new BlockPos(0, 0, 0);
    public BlockPos(int x, int y, int z) { super(x, y, z); }
    public static BlockPos containing(double x, double y, double z) { return new BlockPos((int) Math.floor(x), (int) Math.floor(y), (int) Math.floor(z)); }
    public BlockPos offset(int dx, int dy, int dz) { return new BlockPos(getX() + dx, getY() + dy, getZ() + dz); }
    public BlockPos above() { return offset(0, 1, 0); }
    public BlockPos below() { return offset(0, -1, 0); }
    public BlockPos north() { return offset(0, 0, -1); }
    public BlockPos south() { return offset(0, 0, 1); }
    public BlockPos east() { return offset(1, 0, 0); }
    public BlockPos west() { return offset(-1, 0, 0); }
    @Override public String toString() { return "BlockPos{x=" + getX() + ", y=" + getY() + ", z=" + getZ() + "}"; }
}
