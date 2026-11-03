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

    static final class DataTag<T extends org.bukkit.Keyed> implements org.bukkit.Tag<T> {
        private final org.bukkit.NamespacedKey key;
        private final Set<T> values;

        DataTag(org.bukkit.NamespacedKey key, Set<T> values) {
            this.key = key;
            this.values = Set.copyOf(values);
        }

        @Override public org.bukkit.NamespacedKey getKey() { return key; }
        @Override public boolean isTagged(T value) { return value != null && values.contains(value); }
        @Override public Set<T> getValues() { return values; }
        @Override public String toString() { return "#" + key; }
    }

    private static final java.util.concurrent.ConcurrentMap<String, List<? extends org.bukkit.Tag<?>>> CACHE =
            new java.util.concurrent.ConcurrentHashMap<>();

    @SuppressWarnings("unchecked")
    static <T extends org.bukkit.Keyed> List<org.bukkit.Tag<T>> tags(String registry, Class<T> type) {
        String folder = folder(registry);
        boolean supported = (type == org.bukkit.Material.class && (folder.equals("block") || folder.equals("item")))
                || (type == org.bukkit.Fluid.class && folder.equals("fluid"))
                || (type == org.bukkit.entity.EntityType.class && folder.equals("entity_type"))
                || (type == org.bukkit.GameEvent.class && folder.equals("game_event"));
        if (!supported) throw new UnsupportedOperationException("Ferrite: unsupported tag registry/type: " + registry + "/" + type.getName());
        return (List<org.bukkit.Tag<T>>) (List<?>) CACHE.computeIfAbsent(folder, ignored -> load(folder, type));
    }

    /** Bukkit accepts the plural registry names; map both spellings to the data-pack folder. */
    private static String folder(String registry) {
        return switch (registry) {
            case "blocks", "block", "minecraft:block" -> "block";
            case "items", "item", "minecraft:item" -> "item";
            case "fluids", "fluid", "minecraft:fluid" -> "fluid";
            case "entity_types", "entity_type", "minecraft:entity_type" -> "entity_type";
            case "game_events", "game_event", "minecraft:game_event" -> "game_event";
            default -> registry;
        };
    }

    private static <T extends org.bukkit.Keyed> List<DataTag<T>> load(String folder, Class<T> type) {
        var dir = Bootstrap.dataFile("data/minecraft/tags/" + folder);
        if (dir == null || !dir.isDirectory()) return List.of();
        Map<String, JsonArray> raw = new TreeMap<>();
        try (var files = java.nio.file.Files.walk(dir.toPath())) {
            for (var path : files.sorted().toList()) {
                var name = dir.toPath().relativize(path).toString().replace('\\', '/');
                if (!name.endsWith(".json")) continue;
                var json = com.google.gson.JsonParser.parseString(java.nio.file.Files.readString(path)).getAsJsonObject();
                var values = json.getAsJsonArray("values");
                if (values != null) raw.put("minecraft:" + name.substring(0, name.length() - 5), values);
            }
        } catch (java.io.IOException e) {
            throw new java.io.UncheckedIOException(e);
        }
        var tags = new ArrayList<DataTag<T>>();
        for (var name : raw.keySet()) {
            tags.add(new DataTag<>(org.bukkit.NamespacedKey.fromString(name), resolve(name, raw, new HashSet<>(), type)));
        }
        return List.copyOf(tags);
    }

    /** Resolves a tag and every `#tag` it references; cycles and unknown tags contribute nothing. */
    private static <T extends org.bukkit.Keyed> Set<T> resolve(String name, Map<String, JsonArray> raw, Set<String> visiting, Class<T> type) {
        var values = new HashSet<T>();
        if (!visiting.add(name)) return values;
        var entries = raw.get(name);
        if (entries == null) return values;
        for (var entry : entries) {
            String id = entry.isJsonPrimitive() ? entry.getAsString()
                    : entry.isJsonObject() && entry.getAsJsonObject().has("id")
                            ? entry.getAsJsonObject().get("id").getAsString()
                            : null;
            if (id == null) continue;
            if (id.startsWith("#")) values.addAll(resolve(normalize(id.substring(1)), raw, visiting, type));
            else {
                org.bukkit.Keyed value = null;
                if (type == org.bukkit.Material.class) value = org.bukkit.Material.matchMaterial(id);
                else if (type == org.bukkit.entity.EntityType.class) {
                    var key = org.bukkit.NamespacedKey.fromString(id);
                    if (key != null) value = org.bukkit.Bukkit.getRegistry(org.bukkit.entity.EntityType.class).get(key);
                } else if (type == org.bukkit.Fluid.class) {
                    var key = org.bukkit.NamespacedKey.fromString(id);
                    if (key != null) value = org.bukkit.Bukkit.getRegistry(org.bukkit.Fluid.class).get(key);
                } else if (type == org.bukkit.GameEvent.class) {
                    var key = org.bukkit.NamespacedKey.fromString(id);
                    if (key != null) value = org.bukkit.Bukkit.getRegistry(org.bukkit.GameEvent.class).get(key);
                }
                if (value != null) values.add(type.cast(value));
            }
        }
        visiting.remove(name);
        return values;
    }

    private static String normalize(String id) {
        return id.contains(":") ? id : "minecraft:" + id;
    }
}
