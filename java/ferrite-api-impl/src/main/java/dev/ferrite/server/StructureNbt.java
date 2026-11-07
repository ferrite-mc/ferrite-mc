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
 * {@code size}, {@code palette} (or {@code palettes}), {@code blocks}, {@code entities} and
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
            named(data, 8, "Name");
            data.writeUTF(open < 0 ? state : state.substring(0, open));
            if (open >= 0) {
                named(data, 10, "Properties");
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
            data.writeByte(0);
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
                cells.add(new CapturedStructure.Cell(((Number) pos.get(0)).intValue(), ((Number) pos.get(1)).intValue(), ((Number) pos.get(2)).intValue(), states.get(state)));
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
            if (!(map.get("Name") instanceof String name)) throw new IOException("Palette entry has no Name");
            var text = new StringBuilder(name);
            if (map.get("Properties") instanceof Map<?, ?> props && !props.isEmpty()) {
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
