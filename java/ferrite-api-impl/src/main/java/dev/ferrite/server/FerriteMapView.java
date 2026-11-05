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

    /** The 128x128 palette bytes the renderers paint for this viewer (0 stays transparent). */
    @SuppressWarnings("deprecation")
    byte[] render(org.bukkit.entity.Player viewer) {
        final byte[] pixels = new byte[128 * 128];
        final var cursors = new org.bukkit.map.MapCursorCollection();
        final var view = this;
        var canvas = (org.bukkit.map.MapCanvas) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.map.MapCanvas.class}, (self, method, args) -> {
            switch (method.getName()) {
                case "getMapView": return view;
                case "getCursors": return cursors;
                case "setCursors": return null;
                case "setPixel": { int x = (int) args[0], y = (int) args[1]; if (x >= 0 && y >= 0 && x < 128 && y < 128) pixels[y * 128 + x] = (byte) args[2]; return null; }
                case "getPixel": { int x = (int) args[0], y = (int) args[1]; return x >= 0 && y >= 0 && x < 128 && y < 128 ? pixels[y * 128 + x] : (byte) 0; }
                case "getBasePixel": return (byte) 0;
                case "setPixelColor": { int x = (int) args[0], y = (int) args[1]; if (x >= 0 && y >= 0 && x < 128 && y < 128) pixels[y * 128 + x] = args[2] == null ? 0 : org.bukkit.map.MapPalette.matchColor((java.awt.Color) args[2]); return null; }
                case "getPixelColor": { int x = (int) args[0], y = (int) args[1]; if (x < 0 || y < 0 || x >= 128 || y >= 128) return null; byte b = pixels[y * 128 + x]; return b == 0 ? null : org.bukkit.map.MapPalette.getColor(b); }
                case "getBasePixelColor": return org.bukkit.map.MapPalette.getColor((byte) 0);
                case "drawImage": {
                    int ox = (int) args[0], oy = (int) args[1];
                    var image = (java.awt.Image) args[2];
                    byte[] data = org.bukkit.map.MapPalette.imageToBytes(image);
                    int width = image.getWidth(null), height = image.getHeight(null);
                    for (int y = 0; y < height; y++) for (int x = 0; x < width; x++) {
                        int px = ox + x, py = oy + y;
                        byte b = data[y * width + x];
                        if (b != 0 && px >= 0 && py >= 0 && px < 128 && py < 128) pixels[py * 128 + px] = b;
                    }
                    return null;
                }
                case "drawText": {
                    int x = (int) args[0], y = (int) args[1];
                    var font = (org.bukkit.map.MapFont) args[2];
                    String text = (String) args[3];
                    int startX = x;
                    for (char ch : text.toCharArray()) {
                        if (ch == (char) 10) { x = startX; y += font.getHeight() + 1; continue; }
                        var glyph = font.getChar(ch);
                        if (glyph == null) continue;
                        for (int row = 0; row < glyph.getHeight(); row++) for (int col = 0; col < glyph.getWidth(); col++) {
                            int px = x + col, py = y + row;
                            if (glyph.get(row, col) && px >= 0 && py >= 0 && px < 128 && py < 128) pixels[py * 128 + px] = 44;
                        }
                        x += glyph.getWidth() + 1;
                    }
                    return null;
                }
                case "hashCode": return System.identityHashCode(self);
                case "equals": return self == args[0];
                case "toString": return "MapCanvas";
                default: throw new UnsupportedOperationException("Ferrite: not implemented: MapCanvas#" + method.getName());
            }
        });
        for (var renderer : renderers) renderer.render(this, canvas, viewer);
        return pixels;
    }
}
