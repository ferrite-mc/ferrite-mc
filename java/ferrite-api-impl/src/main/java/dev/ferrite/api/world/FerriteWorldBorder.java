package dev.ferrite.api.world;

import java.util.Objects;
import org.bukkit.Location;
import org.bukkit.World;
import org.bukkit.WorldBorder;

/** State-backed {@link WorldBorder} implementation for {@link FerriteWorld}. */
public final class FerriteWorldBorder implements WorldBorder {

    public static final double MAX_SIZE = 5.9999968E7D;
    public static final double MAX_CENTER_COORDINATE = 2.9999984E7D;

    private static final double DEFAULT_DAMAGE_BUFFER = 5.0;
    private static final double DEFAULT_DAMAGE_AMOUNT = 0.2;
    private static final int DEFAULT_WARNING_TIME_TICKS = 15 * 20;
    private static final int DEFAULT_WARNING_DISTANCE = 5;

    private final FerriteWorld world;
    private double size;
    private double centerX;
    private double centerZ;
    private double damageBuffer;
    private double damageAmount;
    private int warningTimeTicks;
    private int warningDistance;

    public FerriteWorldBorder(FerriteWorld world) {
        this.world = Objects.requireNonNull(world, "world");
        reset();
    }

    @Override
    public World getWorld() {
        return world;
    }

    @Override
    public void reset() {
        this.size = MAX_SIZE;
        this.centerX = 0.0;
        this.centerZ = 0.0;
        this.damageBuffer = DEFAULT_DAMAGE_BUFFER;
        this.damageAmount = DEFAULT_DAMAGE_AMOUNT;
        this.warningTimeTicks = DEFAULT_WARNING_TIME_TICKS;
        this.warningDistance = DEFAULT_WARNING_DISTANCE;
    }

    @Override
    public double getSize() {
        return size;
    }

    @Override
    public void setSize(double newSize) {
        changeSize(newSize, 0L);
    }

    @Override
    public void changeSize(double newSize, long ticks) {
        if (!Double.isFinite(newSize)) {
            throw new IllegalArgumentException("WorldBorder size must be finite: " + newSize);
        }
        if (ticks < 0L) {
            throw new IllegalArgumentException("WorldBorder transition ticks cannot be negative: " + ticks);
        }
        this.size = Math.clamp(newSize, 1.0, MAX_SIZE);
    }

    @Override
    public Location getCenter() {
        return new Location(world, centerX, 0.0, centerZ);
    }

    @Override
    public void setCenter(double x, double z) {
        if (!Double.isFinite(x) || !Double.isFinite(z)) {
            throw new IllegalArgumentException("WorldBorder center coordinates must be finite");
        }
        this.centerX = Math.clamp(x, -MAX_CENTER_COORDINATE, MAX_CENTER_COORDINATE);
        this.centerZ = Math.clamp(z, -MAX_CENTER_COORDINATE, MAX_CENTER_COORDINATE);
    }

    @Override
    public void setCenter(Location location) {
        Objects.requireNonNull(location, "location");
        setCenter(location.getX(), location.getZ());
    }

    @Override
    public double getDamageBuffer() {
        return damageBuffer;
    }

    @Override
    public void setDamageBuffer(double blocks) {
        if (Double.isNaN(blocks) || blocks < 0.0) {
            throw new IllegalArgumentException("damageBuffer cannot be negative or NaN: " + blocks);
        }
        this.damageBuffer = blocks;
    }

    @Override
    public double getDamageAmount() {
        return damageAmount;
    }

    @Override
    public void setDamageAmount(double damage) {
        if (Double.isNaN(damage) || damage < 0.0) {
            throw new IllegalArgumentException("damageAmount cannot be negative or NaN: " + damage);
        }
        this.damageAmount = damage;
    }

    @Override
    public int getWarningTimeTicks() {
        return warningTimeTicks;
    }

    @Override
    public void setWarningTimeTicks(int ticks) {
        if (ticks < 0) {
            throw new IllegalArgumentException("warningTimeTicks cannot be negative: " + ticks);
        }
        this.warningTimeTicks = ticks;
    }

    @Override
    public int getWarningDistance() {
        return warningDistance;
    }

    @Override
    public void setWarningDistance(int distance) {
        if (distance < 0) {
            throw new IllegalArgumentException("warningDistance cannot be negative: " + distance);
        }
        this.warningDistance = distance;
    }

    @Override
    public boolean isInside(Location location) {
        Objects.requireNonNull(location, "location");
        if (location.getWorld() != null && !world.equals(location.getWorld())) {
            return false;
        }
        double half = size / 2.0;
        double minX = Math.max(-MAX_CENTER_COORDINATE, centerX - half);
        double maxX = Math.min(MAX_CENTER_COORDINATE, centerX + half);
        double minZ = Math.max(-MAX_CENTER_COORDINATE, centerZ - half);
        double maxZ = Math.min(MAX_CENTER_COORDINATE, centerZ + half);
        return location.getX() >= minX
                && location.getX() <= maxX
                && location.getZ() >= minZ
                && location.getZ() <= maxZ;
    }

    @Override
    public double getMaxSize() {
        return MAX_SIZE;
    }

    @Override
    public double getMaxCenterCoordinate() {
        return MAX_CENTER_COORDINATE;
    }
}
