package dev.ferrite.server;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;

/**
 * Global vanilla block-state ids, the numbers level events carry for a block.
 *
 * <p>The generated {@code ferrite/block-states.tsv} holds, per block, the id of its first state, its properties
 * (alphabetical, each with its values in report order) and its default state. Vanilla numbers a block's states
 * consecutively with the last property varying fastest, so a state's id is the first id plus a mixed-radix index;
 * the generator verifies that rule against every state of the report.
 */
final class FerriteBlockStates {
    private FerriteBlockStates() {}

    private record Block(int firstId, Map<String, List<String>> properties, Map<String, String> defaults) {}

    private static final class Holder {
        static final Map<String, Block> BLOCKS = load();
    }

    private static Map<String, Block> load() {
        var blocks = new HashMap<String, Block>();
        try (var stream = Objects.requireNonNull(FerriteBlockStates.class.getResourceAsStream("/ferrite/block-states.tsv"), "Missing generated block states");
             var reader = new BufferedReader(new InputStreamReader(stream, StandardCharsets.UTF_8))) {
            for (String line; (line = reader.readLine()) != null;) {
                var columns = line.split("\t", -1);
                var properties = new LinkedHashMap<String, List<String>>();
                if (!columns[2].isEmpty()) for (var property : columns[2].split(";")) {
                    var parts = property.split("=", 2);
                    properties.put(parts[0], List.of(parts[1].split(",")));
                }
                var defaults = new HashMap<String, String>();
                if (!columns[3].isEmpty()) for (var property : columns[3].split(";")) {
                    var parts = property.split("=", 2);
                    defaults.put(parts[0], parts[1]);
                }
                blocks.put(columns[0], new Block(Integer.parseInt(columns[1]), properties, defaults));
            }
        } catch (IOException e) {
            throw new IllegalStateException("Cannot read block states", e);
        }
        return blocks;
    }

    /**
     * The global state id of a block-data string such as {@code minecraft:oak_stairs[facing=east]}; properties that
     * are not named keep their default value.
     *
     * @throws IllegalArgumentException for an unknown block, property or value
     */
    static int stateId(String blockData) {
        Objects.requireNonNull(blockData, "blockData");
        int open = blockData.indexOf('[');
        var name = open < 0 ? blockData : blockData.substring(0, open);
        if (!name.contains(":")) name = "minecraft:" + name;
        var block = Holder.BLOCKS.get(name);
        if (block == null) throw new IllegalArgumentException("Unknown block type: " + blockData);
        var given = new HashMap<String, String>();
        if (open >= 0) {
            if (!blockData.endsWith("]")) throw new IllegalArgumentException("Unterminated block state: " + blockData);
            var inner = blockData.substring(open + 1, blockData.length() - 1);
            if (!inner.isEmpty()) for (var pair : inner.split(",")) {
                var parts = pair.split("=", 2);
                if (parts.length != 2 || !block.properties().containsKey(parts[0]))
                    throw new IllegalArgumentException("Block " + name + " has no property '" + pair + "'");
                if (given.put(parts[0], parts[1]) != null) throw new IllegalArgumentException("Duplicate property " + parts[0] + " in " + blockData);
            }
        }
        long index = 0;
        for (var property : new ArrayList<>(block.properties().entrySet())) {
            var values = property.getValue();
            var chosen = given.getOrDefault(property.getKey(), block.defaults().get(property.getKey()));
            int position = values.indexOf(chosen);
            if (position < 0) throw new IllegalArgumentException("Block " + name + " property " + property.getKey() + " cannot be '" + chosen + "'");
            index = index * values.size() + position;
        }
        return block.firstId() + (int) index;
    }

    /** The block names (`minecraft:bed`-style keys) that own at least one of the given global state ids. */
    static java.util.Set<String> blockNames(java.util.Collection<Integer> stateIds) {
        var names = new java.util.HashSet<String>();
        for (var entry : Holder.BLOCKS.entrySet()) {
            var block = entry.getValue();
            long count = 1;
            for (var values : block.properties().values()) count *= values.size();
            for (int id : stateIds) {
                if (id >= block.firstId() && id < block.firstId() + count) { names.add(entry.getKey()); break; }
            }
        }
        return names;
    }
}
