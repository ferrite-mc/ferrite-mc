package dev.ferrite.server;

import org.bukkit.Location;
import org.bukkit.World;

/** The default world's border: reads the server's border and writes through the worldborder command. */
final class FerriteHostBorder implements org.bukkit.WorldBorder {
    private static final double MAX_SIZE = 5.9999968E7;
    private static final double MAX_CENTER = 2.9999984E7;
    private final World world;
    private final java.util.UUID id;

    FerriteHostBorder(World world, java.util.UUID id) { this.world = world; this.id = id; }

    private double read(String method) { return HostBridge.callDouble("World", id, method); }
    private void write(String method, Object... args) { HostBridge.callBoolean("World", id, method, args); }

    @Override public World getWorld() { return world; }
    @Override public void reset() {
        setSize(MAX_SIZE); setCenter(0, 0); setDamageBuffer(5); setDamageAmount(0.2); setWarningTimeTicks(300); setWarningDistance(5);
    }
    @Override public double getSize() { return read("borderSize()"); }
    @Override public void setSize(double size) { changeSize(size, 0); }
    @Override public void changeSize(double size, long ticks) {
        if (!(size >= 1.0 && size <= MAX_SIZE)) throw new IllegalArgumentException("size must be between 1.0D and 5.9999968E7D");
        write("borderSet(double,long)", size, Math.max(0, ticks));
    }
    @Override public Location getCenter() { return new Location(world, read("borderCenterX()"), 0, read("borderCenterZ()")); }
    @Override public void setCenter(double x, double z) {
        if (Math.abs(x) > MAX_CENTER || Math.abs(z) > MAX_CENTER) throw new IllegalArgumentException("Center coordinates must be between -2.9999984E7 and 2.9999984E7");
        write("borderCenter(double,double)", x, z);
    }
    @Override public void setCenter(Location l) { setCenter(l.getX(), l.getZ()); }
    @Override public double getDamageBuffer() { return read("borderDamageBuffer()"); }
    @Override public void setDamageBuffer(double v) { write("borderDamageBuffer(double)", v); }
    @Override public double getDamageAmount() { return read("borderDamageAmount()"); }
    @Override public void setDamageAmount(double v) { write("borderDamageAmount(double)", v); }
    @Override public int getWarningTimeTicks() { return (int) read("borderWarningTicks()"); }
    @Override public void setWarningTimeTicks(int ticks) { write("borderWarningTime(int)", ticks); }
    @Override public int getWarningDistance() { return (int) read("borderWarningDistance()"); }
    @Override public void setWarningDistance(int d) { write("borderWarningDistance(int)", d); }
    @Override public boolean isInside(Location l) {
        double half = getSize() / 2;
        return Math.abs(l.getX() - read("borderCenterX()")) <= half && Math.abs(l.getZ() - read("borderCenterZ()")) <= half;
    }
    @Override public double getMaxSize() { return MAX_SIZE; }
    @Override public double getMaxCenterCoordinate() { return MAX_CENTER; }
}
