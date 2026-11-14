package dev.ferrite.server;

import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.zip.GZIPInputStream;
import java.util.zip.GZIPOutputStream;
import org.bukkit.util.BlockVector;

/**
 * Reads and writes the vanilla structure template file: a gzip-compressed NBT compound with
 * {@code size}, {@code palette} (or {@code palettes}; entries are {@code id} and {@code properties}
 * since 26.3, {@code Name} and {@code Properties} before), {@code blocks}, {@code entities} and
 * {@code DataVersion}. Entities are not carried; block entity data is dropped.
 */
final class StructureNbt {
    static final int DATA_VERSION = 5023;

    private StructureNbt() {}

    static void write(OutputStream out, CapturedStructure structure) throws IOException {
        var gzip = new GZIPOutputStream(out);
        var data = new DataOutputStream(gzip);
        data.writeByte(10);
        data.writeUTF("");
        var size = structure.getSize();
        named(data, 9, "size");
        data.writeByte(3);
        data.writeInt(3);
        data.writeInt(size.getBlockX());
        data.writeInt(size.getBlockY());
        data.writeInt(size.getBlockZ());

        var palettes = structure.cellsByPalette();
        var first = palettes.isEmpty() ? List.<CapturedStructure.Cell>of() : palettes.get(0);
        // One palette is deduplicated; several keep one entry per block so the shared block list
        // can index every palette with the same number.
        if (palettes.size() <= 1) {
            var states = new ArrayList<String>();
            var index = new java.util.HashMap<String, Integer>();
            for (var cell : first) index.computeIfAbsent(cell.state(), s -> { states.add(s); return states.size() - 1; });
            named(data, 9, "palette");
            writePalette(data, states);
            writeBlocks(data, first, cell -> index.get(cell.state()));
        } else {
            named(data, 9, "palettes");
            data.writeByte(9);
            data.writeInt(palettes.size());
            for (var palette : palettes) {
                var states = new ArrayList<String>();
                for (var cell : palette) states.add(cell.state());
                writePalette(data, states);
            }
            var counter = new int[] {0};
            writeBlocks(data, first, cell -> counter[0]++);
        }
        named(data, 9, "entities");
        data.writeByte(0);
        data.writeInt(0);
        named(data, 3, "DataVersion");
        data.writeInt(DATA_VERSION);
        data.writeByte(0);
        data.flush();
        gzip.finish();
    }

    private static void named(DataOutputStream data, int type, String name) throws IOException {
        data.writeByte(type);
        data.writeUTF(name);
    }

    private static void writePalette(DataOutputStream data, List<String> states) throws IOException {
        data.writeByte(10);
        data.writeInt(states.size());
        for (var state : states) {
            int open = state.indexOf('[');
            // 26.3 palette entries are {id, properties}; older files used {Name, Properties}.
            named(data, 8, "id");
            data.writeUTF(open < 0 ? state : state.substring(0, open));
            if (open >= 0) {
                named(data, 10, "properties");
                for (var pair : state.substring(open + 1, state.length() - 1).split(",")) {
                    int eq = pair.indexOf('=');
                    named(data, 8, pair.substring(0, eq));
                    data.writeUTF(pair.substring(eq + 1));
                }
                data.writeByte(0);
            }
            data.writeByte(0);
        }
    }

    private static void writeBlocks(DataOutputStream data, List<CapturedStructure.Cell> cells, java.util.function.ToIntFunction<CapturedStructure.Cell> state) throws IOException {
        named(data, 9, "blocks");
        data.writeByte(10);
        data.writeInt(cells.size());
        for (var cell : cells) {
            named(data, 3, "state");
            data.writeInt(state.applyAsInt(cell));
            named(data, 9, "pos");
            data.writeByte(3);
            data.writeInt(3);
            data.writeInt(cell.x());
            data.writeInt(cell.y());
            data.writeInt(cell.z());
            if (cell.nbt() != null) {
                named(data, 10, "nbt");
                writePayload(data, cell.nbt());
            }
            data.writeByte(0);
        }
    }

    /** Writes the payload of a compound, list or array of the plain values {@link #payload} reads. */
    @SuppressWarnings("unchecked")
    private static void writePayload(DataOutputStream data, Object value) throws IOException {
        if (value instanceof Map<?, ?> map) {
            for (var entry : ((Map<String, Object>) map).entrySet()) {
                data.writeByte(typeOf(entry.getValue()));
                data.writeUTF(entry.getKey());
                writePayload(data, entry.getValue());
            }
            data.writeByte(0);
        } else if (value instanceof List<?> list) {
            data.writeByte(list.isEmpty() ? 0 : typeOf(list.get(0)));
            data.writeInt(list.size());
            for (var item : list) writePayload(data, item);
        } else if (value instanceof Byte b) data.writeByte(b);
        else if (value instanceof Short s) data.writeShort(s);
        else if (value instanceof Integer i) data.writeInt(i);
        else if (value instanceof Long l) data.writeLong(l);
        else if (value instanceof Float f) data.writeFloat(f);
        else if (value instanceof Double d) data.writeDouble(d);
        else if (value instanceof String s) data.writeUTF(s);
        else if (value instanceof byte[] bytes) { data.writeInt(bytes.length); data.write(bytes); }
        else if (value instanceof int[] ints) { data.writeInt(ints.length); for (int i : ints) data.writeInt(i); }
        else if (value instanceof long[] longs) { data.writeInt(longs.length); for (long l : longs) data.writeLong(l); }
        else throw new IOException("Not an NBT value: " + value);
    }

    /** Test hook: reads a payload of the given type. */
    static Object payloadForTest(DataInputStream in, int type) throws IOException { return payload(in, type); }

    /** A nameless network tag (type byte, then the payload) of the plain values {@link #payload} reads. */
    static byte[] network(Object root) throws IOException {
        var bytes = new java.io.ByteArrayOutputStream();
        try (var data = new DataOutputStream(bytes)) {
            data.writeByte(typeOf(root));
            writePayload(data, root);
        }
        return bytes.toByteArray();
    }

    private static int typeOf(Object value) throws IOException {
        return switch (value) {
            case Byte b -> 1; case Short s -> 2; case Integer i -> 3; case Long l -> 4;
            case Float f -> 5; case Double d -> 6; case byte[] b -> 7; case String s -> 8;
            case List<?> l -> 9; case Map<?, ?> m -> 10; case int[] i -> 11; case long[] l -> 12;
            default -> throw new IOException("Not an NBT value: " + value);
        };
    }

    /** A block entity from the host: hex of binary NBT (a named root compound). */
    @SuppressWarnings("unchecked")
    static Map<String, Object> compound(String hex) {
        try {
            var bytes = new byte[hex.length() / 2];
            for (int i = 0; i < bytes.length; i++) bytes[i] = (byte) Integer.parseInt(hex.substring(2 * i, 2 * i + 2), 16);
            var in = new DataInputStream(new java.io.ByteArrayInputStream(bytes));
            if (in.readByte() != 10) return null;
            in.skipBytes(in.readUnsignedShort());
            return (Map<String, Object>) payload(in, 10);
        } catch (IOException | NumberFormatException e) {
            return null;
        }
    }

    static CapturedStructure read(InputStream in) throws IOException {
        Object root;
        try (var data = new DataInputStream(new java.io.BufferedInputStream(new GZIPInputStream(in)))) {
            if (data.readByte() != 10) throw new IOException("Not a compound");
            data.skipBytes(data.readUnsignedShort());
            root = payload(data, 10);
        }
        @SuppressWarnings("unchecked") var map = (Map<String, Object>) root;
        var sizeList = list(map.get("size"));
        if (sizeList == null || sizeList.size() != 3) throw new IOException("Template has no size");
        var size = new BlockVector(((Number) sizeList.get(0)).intValue(), ((Number) sizeList.get(1)).intValue(), ((Number) sizeList.get(2)).intValue());

        var paletteStates = new ArrayList<List<String>>();
        var palettes = list(map.get("palettes"));
        if (palettes != null) {
            for (var palette : palettes) paletteStates.add(states(list(palette)));
        } else {
            var single = list(map.get("palette"));
            if (single == null) throw new IOException("Template has no palette");
            paletteStates.add(states(single));
        }
        var blocks = list(map.get("blocks"));
        if (blocks == null) blocks = List.of();
        var result = new ArrayList<List<CapturedStructure.Cell>>();
        for (var states : paletteStates) {
            var cells = new ArrayList<CapturedStructure.Cell>();
            for (var block : blocks) {
                @SuppressWarnings("unchecked") var b = (Map<String, Object>) block;
                var pos = list(b.get("pos"));
                int state = ((Number) b.get("state")).intValue();
                if (pos == null || pos.size() != 3 || state < 0 || state >= states.size()) throw new IOException("Bad block entry");
                @SuppressWarnings("unchecked") var nbt = b.get("nbt") instanceof Map<?, ?> m ? (Map<String, Object>) m : null;
                cells.add(new CapturedStructure.Cell(((Number) pos.get(0)).intValue(), ((Number) pos.get(1)).intValue(), ((Number) pos.get(2)).intValue(), states.get(state), nbt));
            }
            result.add(cells);
        }
        return new CapturedStructure(size, result);
    }

    @SuppressWarnings("unchecked")
    private static List<Object> list(Object value) { return value instanceof List<?> l ? (List<Object>) l : null; }

    private static List<String> states(List<Object> palette) throws IOException {
        var states = new ArrayList<String>();
        for (var entry : palette) {
            @SuppressWarnings("unchecked") var map = (Map<String, Object>) entry;
            var named = map.containsKey("id") ? map.get("id") : map.get("Name");
            if (!(named instanceof String name)) throw new IOException("Palette entry has no id");
            var text = new StringBuilder(name);
            var properties = map.containsKey("properties") ? map.get("properties") : map.get("Properties");
            if (properties instanceof Map<?, ?> props && !props.isEmpty()) {
                text.append('[');
                boolean firstProperty = true;
                for (var property : props.entrySet()) {
                    if (!firstProperty) text.append(',');
                    firstProperty = false;
                    text.append(property.getKey()).append('=').append(property.getValue());
                }
                text.append(']');
            }
            states.add(text.toString());
        }
        return states;
    }

    /** Reads the payload of a tag of {@code type} into plain Java values. */
    private static Object payload(DataInputStream in, int type) throws IOException {
        return switch (type) {
            case 1 -> in.readByte();
            case 2 -> in.readShort();
            case 3 -> in.readInt();
            case 4 -> in.readLong();
            case 5 -> in.readFloat();
            case 6 -> in.readDouble();
            case 7 -> { var bytes = new byte[in.readInt()]; in.readFully(bytes); yield bytes; }
            case 8 -> in.readUTF();
            case 9 -> {
                int elements = in.readByte();
                int length = in.readInt();
                var items = new ArrayList<Object>();
                for (int i = 0; i < length; i++) items.add(payload(in, elements));
                yield items;
            }
            case 10 -> {
                var map = new LinkedHashMap<String, Object>();
                while (true) {
                    int inner = in.readByte();
                    if (inner == 0) break;
                    var name = in.readUTF();
                    map.put(name, payload(in, inner));
                }
                yield map;
            }
            case 11 -> { var ints = new int[in.readInt()]; for (int i = 0; i < ints.length; i++) ints[i] = in.readInt(); yield ints; }
            case 12 -> { var longs = new long[in.readInt()]; for (int i = 0; i < longs.length; i++) longs[i] = in.readLong(); yield longs; }
            default -> throw new IOException("Unknown tag " + type);
        };
    }
}
