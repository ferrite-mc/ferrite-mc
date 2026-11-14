package dev.ferrite.server;

import java.nio.charset.StandardCharsets;
import java.util.Base64;
import java.util.List;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.atomic.AtomicInteger;
import org.bukkit.World;
import org.bukkit.map.MapCursor;
import org.bukkit.map.MapCursorCollection;
import org.bukkit.map.MapRenderer;

/**
 * A map view from Server#createMap. With a host the id comes from the server's persistent map
 * store (`data/minecraft/maps`) and every setting change is written back, so the id in a filled
 * map's {@code minecraft:map_id} names real saved data. Without a host (unit tests) ids are
 * in-memory counters. Renderers are listed only; pixels come from {@link #render}.
 */
final class FerriteMapView implements org.bukkit.map.MapView {
    private static final AtomicInteger NEXT_ID = new AtomicInteger(1);
    private final int id;
    private final List<MapRenderer> renderers = new CopyOnWriteArrayList<>();
    private volatile World world;
    private volatile Scale scale = Scale.NORMAL;
    private volatile int centerX, centerZ;
    private volatile boolean locked, trackingPosition = true, unlimitedTracking;

    private final boolean hosted;

    FerriteMapView(World world) {
        this.world = java.util.Objects.requireNonNull(world, "world");
        int allocated = HostBridge.available() ? allocate(world) : -1;
        // A host that does not answer with an id (a test transport, an older host) leaves the
        // view in-memory with a local counter id; it is then never written back.
        this.hosted = allocated >= 0;
        this.id = hosted ? allocated : NEXT_ID.getAndIncrement();
    }

    private int allocate(World world) {
        try {
            return HostBridge.callInt("Server", "server", "allocateMap(String,int,int,int,boolean,boolean)", dimensionKey(world), scale.ordinal(), 0, 0, true, false);
        } catch (NumberFormatException e) {
            return -1;
        }
    }

    private static String dimensionKey(World world) { return world.getKey().toString(); }

    /** Writes the current settings to the saved map; a no-op without a host. */
    private void sync() {
        if (!hosted) return;
        HostBridge.callVoid("Server", "server", "updateMap(int,String,int,int,int,boolean,boolean,boolean)", id, dimensionKey(world), scale.ordinal(), centerX, centerZ, trackingPosition, unlimitedTracking, locked);
    }

    @Override public int getId() { return id; }
    @Override public boolean isVirtual() { return false; }
    @Override public Scale getScale() { return scale; }
    @Override public void setScale(Scale value) { scale = java.util.Objects.requireNonNull(value, "scale"); sync(); }
    @Override public int getCenterX() { return centerX; }
    @Override public int getCenterZ() { return centerZ; }
    @Override public void setCenterX(int value) { centerX = value; sync(); }
    @Override public void setCenterZ(int value) { centerZ = value; sync(); }
    @Override public World getWorld() { return world; }
    @Override public void setWorld(World value) { world = java.util.Objects.requireNonNull(value, "world"); sync(); }
    @Override public List<MapRenderer> getRenderers() { return List.copyOf(renderers); }
    @Override public void addRenderer(MapRenderer renderer) { renderers.add(java.util.Objects.requireNonNull(renderer, "renderer")); }
    @Override public boolean removeRenderer(MapRenderer renderer) { return renderers.remove(renderer); }
    @Override public boolean isTrackingPosition() { return trackingPosition; }
    @Override public void setTrackingPosition(boolean value) { trackingPosition = value; sync(); }
    @Override public boolean isUnlimitedTracking() { return unlimitedTracking; }
    @Override public void setUnlimitedTracking(boolean value) { unlimitedTracking = value; sync(); }
    @Override public boolean isLocked() { return locked; }
    @Override public void setLocked(boolean value) { locked = value; sync(); }
    @Override public String toString() { return "MapView[" + id + "]"; }

    /** Pixels and encoded visible cursors for one viewer; nothing is kept between renders. */
    record Rendered(byte[] pixels, String decorations) {}

    /** The 128x128 palette bytes and visible cursors the renderers paint for this viewer. */
    @SuppressWarnings("deprecation")
    Rendered render(org.bukkit.entity.Player viewer) {
        final byte[] pixels = new byte[128 * 128];
        // Vanilla's CraftMapCanvas#getBasePixel reads the saved map's own colour array.
        final byte[] base = baseColors();
        final var cursors = new org.bukkit.map.MapCursorCollection();
        final var view = this;
        var canvas = (org.bukkit.map.MapCanvas) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.map.MapCanvas.class}, (self, method, args) -> {
            switch (method.getName()) {
                case "getMapView": return view;
                case "getCursors": return cursors;
                case "setCursors": {
                    var incoming = (org.bukkit.map.MapCursorCollection) args[0];
                    if (incoming != null) for (int i = 0; i < incoming.size(); i++) cursors.addCursor(incoming.getCursor(i));
                    return null;
                }
                case "setPixel": { int x = (int) args[0], y = (int) args[1]; if (x >= 0 && y >= 0 && x < 128 && y < 128) pixels[y * 128 + x] = (byte) args[2]; return null; }
                case "getPixel": { int x = (int) args[0], y = (int) args[1]; return x >= 0 && y >= 0 && x < 128 && y < 128 ? pixels[y * 128 + x] : (byte) 0; }
                case "getBasePixel": { int x = (int) args[0], y = (int) args[1]; return x >= 0 && y >= 0 && x < 128 && y < 128 ? base[y * 128 + x] : (byte) 0; }
                case "setPixelColor": { int x = (int) args[0], y = (int) args[1]; if (x >= 0 && y >= 0 && x < 128 && y < 128) pixels[y * 128 + x] = args[2] == null ? 0 : org.bukkit.map.MapPalette.matchColor((java.awt.Color) args[2]); return null; }
                case "getPixelColor": { int x = (int) args[0], y = (int) args[1]; if (x < 0 || y < 0 || x >= 128 || y >= 128) return null; byte b = pixels[y * 128 + x]; return b == 0 ? null : org.bukkit.map.MapPalette.getColor(b); }
                case "getBasePixelColor": { int x = (int) args[0], y = (int) args[1]; if (x < 0 || y < 0 || x >= 128 || y >= 128) return null; byte b = base[y * 128 + x]; return b == 0 ? null : org.bukkit.map.MapPalette.getColor(b); }
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
        return new Rendered(pixels, encodeCursors(cursors));
    }

    /** The saved map's palette bytes; zeroes when this view has no host-backed map (unit tests). */
    private byte[] baseColors() {
        if (!hosted) return new byte[128 * 128];
        try {
            byte[] colors = (byte[]) HostBridge.call("Server", "server", "mapColors(int)", byte[].class, id);
            return colors != null && colors.length == 128 * 128 ? colors : new byte[128 * 128];
        } catch (UnsupportedOperationException noHost) { return new byte[128 * 128]; }
    }

    /**
     * Encodes visible cursors as the host's six fields: type key, x, y, direction, has-caption,
     * base64(plain caption). Invisible cursors are omitted because the 26.3 decoration record
     * has no visibility bit. Styled or translated captions are rejected rather than flattened.
     */
    private static String encodeCursors(MapCursorCollection cursors) {
        var encoded = new StringBuilder();
        for (int i = 0; i < cursors.size(); i++) {
            MapCursor cursor = cursors.getCursor(i);
            if (!cursor.isVisible()) continue;
            var type = cursor.getType();
            if (type == null) throw new UnsupportedOperationException("Ferrite: map cursor has no 26.3 decoration type");
            var caption = cursor.caption();
            char mode = '0';
            String captionText = "";
            if (caption != null) {
                if (caption instanceof net.kyori.adventure.text.TextComponent text && text.style().isEmpty() && text.children().isEmpty()) {
                    mode = '1';
                    captionText = Base64.getEncoder().encodeToString(text.content().getBytes(StandardCharsets.UTF_8));
                } else {
                    // Styled, translated or nested text travels as the component's network NBT.
                    mode = '2';
                    captionText = Base64.getEncoder().encodeToString(componentNbt(caption));
                }
            }
            if (!encoded.isEmpty()) encoded.append(HostBridge.ITEM);
            encoded.append(type.getKey()).append('')
                .append(cursor.getX()).append('')
                .append(cursor.getY()).append('')
                .append(cursor.getDirection()).append('')
                .append(mode).append('')
                .append(captionText);
        }
        return encoded.toString();
    }

    /** The network NBT of a text component: the Adventure JSON turned into compounds, lists and bytes. */
    static byte[] componentNbt(net.kyori.adventure.text.Component component) {
        var json = com.google.gson.JsonParser.parseString(net.kyori.adventure.text.serializer.gson.GsonComponentSerializer.gson().serialize(component));
        try { return StructureNbt.network(componentTag(json, true)); }
        catch (java.io.IOException failure) { throw new IllegalStateException("Cannot encode component", failure); }
    }

    /** {@code text} marks a position that holds a text component: a bare string there becomes {@code {text:"..."}} inside a list. */
    private static Object componentTag(com.google.gson.JsonElement element, boolean root) {
        if (element.isJsonPrimitive()) {
            var primitive = element.getAsJsonPrimitive();
            if (primitive.isBoolean()) return (byte) (primitive.getAsBoolean() ? 1 : 0);
            if (primitive.isNumber()) {
                double number = primitive.getAsDouble();
                return number == Math.rint(number) && Math.abs(number) < Integer.MAX_VALUE ? (Object) (int) number : (Object) number;
            }
            return primitive.getAsString();
        }
        if (element.isJsonObject()) {
            var compound = new java.util.LinkedHashMap<String, Object>();
            for (var entry : element.getAsJsonObject().entrySet()) {
                boolean children = entry.getKey().equals("extra") || entry.getKey().equals("with");
                compound.put(entry.getKey(), children ? componentList(entry.getValue()) : componentTag(entry.getValue(), false));
            }
            return compound;
        }
        if (element.isJsonArray()) return componentList(element);
        throw new IllegalArgumentException("Unsupported component value " + element);
    }

    private static Object componentList(com.google.gson.JsonElement element) {
        var list = new java.util.ArrayList<Object>();
        if (!element.isJsonArray()) { list.add(componentTag(element, false)); return list; }
        for (var item : element.getAsJsonArray()) {
            var tag = componentTag(item, false);
            // A list holds one tag type, so every child becomes a compound.
            if (!(tag instanceof java.util.Map)) { var wrapper = new java.util.LinkedHashMap<String, Object>(); wrapper.put("text", String.valueOf(tag)); tag = wrapper; }
            list.add(tag);
        }
        return list;
    }
}
