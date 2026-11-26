// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.core;

import java.util.Objects;

/** Immutable 3D integer vector matching 26.3 {@code net.minecraft.core.Vec3i}. */
public class Vec3i implements Comparable<Vec3i> {
    public static final Vec3i ZERO = new Vec3i(0, 0, 0);

    private int x;
    private int y;
    private int z;

    public Vec3i(int x, int y, int z) {
        this.x = x;
        this.y = y;
        this.z = z;
    }

    public int getX() {
        return x;
    }

    public int getY() {
        return y;
    }

    public int getZ() {
        return z;
    }

    protected Vec3i setX(int x) {
        this.x = x;
        return this;
    }

    protected Vec3i setY(int y) {
        this.y = y;
        return this;
    }

    protected Vec3i setZ(int z) {
        this.z = z;
        return this;
    }

    public double distSqr(Vec3i to) {
        return distToLowCornerSqr(to.getX(), to.getY(), to.getZ());
    }

    public double distToLowCornerSqr(double x, double y, double z) {
        double dx = this.x - x;
        double dy = this.y - y;
        double dz = this.z - z;
        return dx * dx + dy * dy + dz * dz;
    }

    public int distManhattan(Vec3i vector) {
        return Math.abs(vector.getX() - x)
                + Math.abs(vector.getY() - y)
                + Math.abs(vector.getZ() - z);
    }

    @Override
    public int compareTo(Vec3i other) {
        if (y != other.y) {
            return Integer.compare(y, other.y);
        }
        return z != other.z ? Integer.compare(z, other.z) : Integer.compare(x, other.x);
    }

    @Override
    public boolean equals(Object other) {
        if (this == other) {
            return true;
        }
        if (!(other instanceof Vec3i vec)) {
            return false;
        }
        return x == vec.x && y == vec.y && z == vec.z;
    }

    @Override
    public int hashCode() {
        return Objects.hash(x, y, z);
    }

    @Override
    public String toString() {
        return "(" + x + ", " + y + ", " + z + ")";
    }
}
