// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.world.phys;

import java.util.Objects;

/** Minimal immutable 3D vector used by entity snapshots. */
public final class Vec3 {
    public static final Vec3 ZERO = new Vec3(0, 0, 0);

    public final double x;
    public final double y;
    public final double z;

    public Vec3(double x, double y, double z) {
        this.x = x;
        this.y = y;
        this.z = z;
    }

    public double x() {
        return x;
    }

    public double y() {
        return y;
    }

    public double z() {
        return z;
    }

    public Vec3 add(Vec3 other) {
        Objects.requireNonNull(other, "other");
        return new Vec3(x + other.x, y + other.y, z + other.z);
    }

    public Vec3 scale(double factor) {
        return new Vec3(x * factor, y * factor, z * factor);
    }

    public double length() {
        return Math.sqrt(x * x + y * y + z * z);
    }

    @Override
    public boolean equals(Object other) {
        if (this == other) {
            return true;
        }
        if (!(other instanceof Vec3 vec)) {
            return false;
        }
        return Double.compare(x, vec.x) == 0
                && Double.compare(y, vec.y) == 0
                && Double.compare(z, vec.z) == 0;
    }

    @Override
    public int hashCode() {
        return Objects.hash(x, y, z);
    }

    @Override
    public String toString() {
        return "Vec3{x=" + x + ", y=" + y + ", z=" + z + "}";
    }
}
