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
