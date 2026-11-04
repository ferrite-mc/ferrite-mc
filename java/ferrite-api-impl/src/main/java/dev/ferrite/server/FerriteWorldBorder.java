package dev.ferrite.server;

import org.bukkit.Location;
import org.bukkit.World;

/** A standalone border from Server#createWorldBorder: state is kept in memory. */
final class FerriteWorldBorder implements org.bukkit.WorldBorder {
    private static final double MAX_SIZE = 5.9999968E7;
    private static final double MAX_CENTER = 2.9999984E7;
    private double size = MAX_SIZE, centerX, centerZ, damageBuffer = 5, damageAmount = 0.2;
    private int warningTicks = 300, warningDistance = 5;
    private long changeStart, changeMillis;
    private double from;

    @Override public World getWorld() { return null; }
    @Override public void reset() { size = MAX_SIZE; centerX = 0; centerZ = 0; damageBuffer = 5; damageAmount = 0.2; warningTicks = 300; warningDistance = 5; changeMillis = 0; }
    @Override public double getSize() {
        if (changeMillis <= 0) return size;
        long elapsed = System.currentTimeMillis() - changeStart;
        if (elapsed >= changeMillis) { changeMillis = 0; return size; }
        return from + (size - from) * ((double) elapsed / changeMillis);
    }
    @Override public void setSize(double s) { check(s); size = s; changeMillis = 0; }
    @Override public void changeSize(double s, long ticks) {
        check(s);
        if (ticks <= 0) { setSize(s); return; }
        from = getSize(); size = s; changeStart = System.currentTimeMillis(); changeMillis = ticks * 50;
    }
    private static void check(double s) {
        if (!(s >= 1.0 && s <= MAX_SIZE)) throw new IllegalArgumentException("size must be between 1.0D and 5.9999968E7D");
    }
    @Override public Location getCenter() { return new Location(null, centerX, 0, centerZ); }
    @Override public void setCenter(double x, double z) {
        if (Math.abs(x) > MAX_CENTER || Math.abs(z) > MAX_CENTER) throw new IllegalArgumentException("Center coordinates must be between -2.9999984E7 and 2.9999984E7");
        centerX = x; centerZ = z;
    }
    @Override public void setCenter(Location l) { setCenter(l.getX(), l.getZ()); }
    @Override public double getDamageBuffer() { return damageBuffer; }
    @Override public void setDamageBuffer(double v) { damageBuffer = v; }
    @Override public double getDamageAmount() { return damageAmount; }
    @Override public void setDamageAmount(double v) { damageAmount = v; }
    @Override public int getWarningTimeTicks() { return warningTicks; }
    @Override public void setWarningTimeTicks(int ticks) { warningTicks = ticks; }
    @Override public int getWarningDistance() { return warningDistance; }
    @Override public void setWarningDistance(int d) { warningDistance = d; }
    @Override public boolean isInside(Location l) {
        double half = getSize() / 2;
        return Math.abs(l.getX() - centerX) <= half && Math.abs(l.getZ() - centerZ) <= half;
    }
    @Override public double getMaxSize() { return MAX_SIZE; }
    @Override public double getMaxCenterCoordinate() { return MAX_CENTER; }
}
