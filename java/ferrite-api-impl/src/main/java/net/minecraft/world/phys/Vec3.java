package net.minecraft.world.phys;

public class Vec3 {
    public static final Vec3 ZERO = new Vec3(0, 0, 0);
    public final double x, y, z;
    public Vec3(double x, double y, double z) { this.x = x; this.y = y; this.z = z; }
    public double x() { return x; }
    public double y() { return y; }
    public double z() { return z; }
    public Vec3 add(double dx, double dy, double dz) { return new Vec3(x + dx, y + dy, z + dz); }
    public Vec3 add(Vec3 o) { return add(o.x, o.y, o.z); }
    public Vec3 subtract(Vec3 o) { return new Vec3(x - o.x, y - o.y, z - o.z); }
    public Vec3 scale(double f) { return new Vec3(x * f, y * f, z * f); }
    public double lengthSqr() { return x * x + y * y + z * z; }
    public double dot(Vec3 o) { return x * o.x + y * o.y + z * o.z; }
    public Vec3 normalize() { double l = length(); return l < 1.0E-5 ? ZERO : new Vec3(x / l, y / l, z / l); }
    public org.bukkit.util.Vector toBukkit() { return new org.bukkit.util.Vector(x, y, z); }
    public double length() { return Math.sqrt(x * x + y * y + z * z); }
    public double distanceTo(Vec3 o) { return new Vec3(o.x - x, o.y - y, o.z - z).length(); }
    @Override public boolean equals(Object other) { return other instanceof Vec3 v && Double.compare(v.x, x) == 0 && Double.compare(v.y, y) == 0 && Double.compare(v.z, z) == 0; }
    @Override public int hashCode() { return java.util.Objects.hash(x, y, z); }
    @Override public String toString() { return "(" + x + ", " + y + ", " + z + ")"; }
}
