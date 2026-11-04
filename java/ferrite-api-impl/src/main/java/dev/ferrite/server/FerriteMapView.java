package dev.ferrite.server;

import java.util.List;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.atomic.AtomicInteger;
import org.bukkit.World;
import org.bukkit.map.MapRenderer;

/** An in-memory map view from Server#createMap: ids increase, renderers are listed only. */
final class FerriteMapView implements org.bukkit.map.MapView {
    private static final AtomicInteger NEXT_ID = new AtomicInteger(1);
    private final int id;
    private final List<MapRenderer> renderers = new CopyOnWriteArrayList<>();
    private volatile World world;
    private volatile Scale scale = Scale.NORMAL;
    private volatile int centerX, centerZ;
    private volatile boolean locked, trackingPosition = true, unlimitedTracking;

    FerriteMapView(World world) { this.id = NEXT_ID.getAndIncrement(); this.world = java.util.Objects.requireNonNull(world, "world"); }

    @Override public int getId() { return id; }
    @Override public boolean isVirtual() { return false; }
    @Override public Scale getScale() { return scale; }
    @Override public void setScale(Scale value) { scale = java.util.Objects.requireNonNull(value, "scale"); }
    @Override public int getCenterX() { return centerX; }
    @Override public int getCenterZ() { return centerZ; }
    @Override public void setCenterX(int value) { centerX = value; }
    @Override public void setCenterZ(int value) { centerZ = value; }
    @Override public World getWorld() { return world; }
    @Override public void setWorld(World value) { world = java.util.Objects.requireNonNull(value, "world"); }
    @Override public List<MapRenderer> getRenderers() { return List.copyOf(renderers); }
    @Override public void addRenderer(MapRenderer renderer) { renderers.add(java.util.Objects.requireNonNull(renderer, "renderer")); }
    @Override public boolean removeRenderer(MapRenderer renderer) { return renderers.remove(renderer); }
    @Override public boolean isTrackingPosition() { return trackingPosition; }
    @Override public void setTrackingPosition(boolean value) { trackingPosition = value; }
    @Override public boolean isUnlimitedTracking() { return unlimitedTracking; }
    @Override public void setUnlimitedTracking(boolean value) { unlimitedTracking = value; }
    @Override public boolean isLocked() { return locked; }
    @Override public void setLocked(boolean value) { locked = value; }
    @Override public String toString() { return "MapView[" + id + "]"; }
}
