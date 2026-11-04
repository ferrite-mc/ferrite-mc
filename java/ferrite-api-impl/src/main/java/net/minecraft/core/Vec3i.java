package net.minecraft.core;

public class Vec3i implements Comparable<Vec3i> {
    public static final Vec3i ZERO = new Vec3i(0, 0, 0);
    private final int x, y, z;
    public Vec3i(int x, int y, int z) { this.x = x; this.y = y; this.z = z; }
    public int getX() { return x; }
    public int getY() { return y; }
    public int getZ() { return z; }
    @Override public int compareTo(Vec3i o) { return y != o.y ? Integer.compare(y, o.y) : z != o.z ? Integer.compare(z, o.z) : Integer.compare(x, o.x); }
    @Override public boolean equals(Object other) { return other instanceof Vec3i v && v.x == x && v.y == y && v.z == z; }
    @Override public int hashCode() { return (y + z * 31) * 31 + x; }
    @Override public String toString() { return "Vec3i{x=" + x + ", y=" + y + ", z=" + z + "}"; }
}
