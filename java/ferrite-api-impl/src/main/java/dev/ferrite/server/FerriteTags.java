package dev.ferrite.server;

import com.google.gson.JsonArray;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeMap;

/** Vanilla block and item tags read from the generated data pack. */
final class FerriteTags {
    private FerriteTags() {}

    static final class MaterialTag implements org.bukkit.Tag<org.bukkit.Material> {
        private final org.bukkit.NamespacedKey key;
        private final Set<org.bukkit.Material> values;

        MaterialTag(org.bukkit.NamespacedKey key, Set<org.bukkit.Material> values) {
            this.key = key;
            this.values = Set.copyOf(values);
        }

        @Override public org.bukkit.NamespacedKey getKey() { return key; }
        @Override public boolean isTagged(org.bukkit.Material material) { return material != null && values.contains(material); }
        @Override public Set<org.bukkit.Material> getValues() { return values; }
        @Override public String toString() { return "#" + key; }
    }

    private static final java.util.concurrent.ConcurrentMap<String, List<MaterialTag>> CACHE =
            new java.util.concurrent.ConcurrentHashMap<>();

    static List<MaterialTag> tags(String registry) {
        return CACHE.computeIfAbsent(folder(registry), FerriteTags::load);
    }

    /** Bukkit accepts the plural registry names; map both spellings to the data-pack folder. */
    private static String folder(String registry) {
        return switch (registry) {
            case "blocks", "block", "minecraft:block" -> "block";
            case "items", "item", "minecraft:item" -> "item";
            default -> registry;
        };
    }

    private static List<MaterialTag> load(String folder) {
        var dir = Bootstrap.dataFile("data/minecraft/tags/" + folder);
        if (dir == null || !dir.isDirectory()) return List.of();
        Map<String, JsonArray> raw = new TreeMap<>();
        try (var files = java.nio.file.Files.list(dir.toPath())) {
            for (var path : files.sorted().toList()) {
                var name = path.getFileName().toString();
                if (!name.endsWith(".json")) continue;
                var json = com.google.gson.JsonParser.parseString(java.nio.file.Files.readString(path)).getAsJsonObject();
                var values = json.getAsJsonArray("values");
                if (values != null) raw.put("minecraft:" + name.substring(0, name.length() - 5), values);
            }
        } catch (java.io.IOException e) {
            throw new java.io.UncheckedIOException(e);
        }
        var tags = new ArrayList<MaterialTag>();
        for (var name : raw.keySet()) {
            tags.add(new MaterialTag(org.bukkit.NamespacedKey.fromString(name), resolve(name, raw, new HashSet<>())));
        }
        return List.copyOf(tags);
    }

    /** Resolves a tag and every `#tag` it references; cycles and unknown tags contribute nothing. */
    private static Set<org.bukkit.Material> resolve(String name, Map<String, JsonArray> raw, Set<String> visiting) {
        var values = new HashSet<org.bukkit.Material>();
        if (!visiting.add(name)) return values;
        var entries = raw.get(name);
        if (entries == null) return values;
        for (var entry : entries) {
            String id = entry.isJsonPrimitive() ? entry.getAsString()
                    : entry.isJsonObject() && entry.getAsJsonObject().has("id")
                            ? entry.getAsJsonObject().get("id").getAsString()
                            : null;
            if (id == null) continue;
            if (id.startsWith("#")) values.addAll(resolve(normalize(id.substring(1)), raw, visiting));
            else {
                var material = org.bukkit.Material.matchMaterial(id);
                if (material != null) values.add(material);
            }
        }
        visiting.remove(name);
        return values;
    }

    private static String normalize(String id) {
        return id.contains(":") ? id : "minecraft:" + id;
    }
}
