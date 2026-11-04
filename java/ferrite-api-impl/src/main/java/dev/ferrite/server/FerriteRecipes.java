package dev.ferrite.server;

import com.google.gson.JsonParser;
import java.util.ArrayList;
import java.util.List;

/**
 * Vanilla 26.3 recipes read from the generated data pack.
 *
 * <p>The Rust server owns recipe matching for crafting menus, so the Bukkit view only carries
 * the recipe id and its result. Recipes that compute their result (the {@code crafting_special_*}
 * family) expose an empty stack, matching CraftBukkit's behaviour for a recipe without a static
 * result.
 */
final class FerriteRecipes {
    private FerriteRecipes() {}

    record Entry(org.bukkit.NamespacedKey key, org.bukkit.inventory.ItemStack result)
            implements org.bukkit.inventory.Recipe, org.bukkit.Keyed {
        @Override public org.bukkit.inventory.ItemStack getResult() { return result.clone(); }
        @Override public org.bukkit.NamespacedKey getKey() { return key; }
    }

    private static volatile List<Entry> entries;
    /** Recipes plugins added, by key; the Rust crafting menus do not know them. */
    private static final java.util.Map<org.bukkit.NamespacedKey, org.bukkit.inventory.Recipe> custom = new java.util.LinkedHashMap<>();
    /** Vanilla recipes a plugin removed from the Bukkit view. */
    private static final java.util.Set<org.bukkit.NamespacedKey> removed = new java.util.HashSet<>();

    /** Vanilla (minus removed) followed by plugin recipes. */
    static synchronized List<org.bukkit.inventory.Recipe> all() {
        var out = new ArrayList<org.bukkit.inventory.Recipe>();
        for (var entry : entries()) if (!removed.contains(entry.key())) out.add(entry);
        out.addAll(custom.values());
        return List.copyOf(out);
    }

    static synchronized boolean add(org.bukkit.inventory.Recipe recipe) {
        if (!(recipe instanceof org.bukkit.Keyed keyed)) return false;
        var key = keyed.getKey();
        boolean vanilla = entries().stream().anyMatch(e -> e.key().equals(key)) && !removed.contains(key);
        if (vanilla || custom.containsKey(key)) return false;
        custom.put(key, recipe);
        return true;
    }

    static synchronized boolean remove(org.bukkit.NamespacedKey key) {
        if (custom.remove(key) != null) return true;
        return entries().stream().anyMatch(e -> e.key().equals(key)) && removed.add(key);
    }

    static synchronized void clear() {
        custom.clear();
        for (var entry : entries()) removed.add(entry.key());
    }

    static synchronized void reset() {
        custom.clear();
        removed.clear();
    }

    static synchronized org.bukkit.inventory.Recipe get(org.bukkit.NamespacedKey key) {
        for (var recipe : all()) if (recipe instanceof org.bukkit.Keyed k && k.getKey().equals(key)) return recipe;
        return null;
    }

    static List<Entry> entries() {
        var loaded = entries;
        if (loaded == null) synchronized (FerriteRecipes.class) {
            loaded = entries;
            if (loaded == null) entries = loaded = load();
        }
        return loaded;
    }

    private static List<Entry> load() {
        var dir = Bootstrap.dataFile("data/minecraft/recipe");
        if (dir == null || !dir.isDirectory()) return List.of();
        var recipes = new ArrayList<Entry>();
        try (var files = java.nio.file.Files.list(dir.toPath())) {
            for (var path : files.sorted().toList()) {
                var name = path.getFileName().toString();
                if (!name.endsWith(".json")) continue;
                var json = JsonParser.parseString(java.nio.file.Files.readString(path)).getAsJsonObject();
                var material = org.bukkit.Material.AIR;
                var count = 0;
                var result = json.getAsJsonObject("result");
                if (result != null) {
                    String id = result.has("id") ? result.get("id").getAsString()
                            : result.has("item") ? result.get("item").getAsString() : null;
                    var resolved = id == null ? null : org.bukkit.Material.matchMaterial(id);
                    if (resolved != null) {
                        material = resolved;
                        count = result.has("count") ? Math.max(1, result.get("count").getAsInt()) : 1;
                    }
                }
                recipes.add(new Entry(
                        new org.bukkit.NamespacedKey("minecraft", name.substring(0, name.length() - 5)),
                        count == 0 ? org.bukkit.inventory.ItemStack.empty() : new org.bukkit.inventory.ItemStack(material, count)));
            }
        } catch (java.io.IOException e) {
            throw new java.io.UncheckedIOException(e);
        }
        return List.copyOf(recipes);
    }
}
