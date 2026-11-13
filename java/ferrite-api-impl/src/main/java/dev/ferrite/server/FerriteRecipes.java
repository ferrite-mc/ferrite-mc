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
        // Shaped and shapeless recipes also go to the host, whose crafting menus then craft them. Other kinds
        // (cooking, smithing, ...) are only listed here: the host cannot add them at runtime.
        var json = craftingJson(recipe);
        if (json != null) {
            var reply = host("addCraftingRecipe(String,String)", key.toString(), json);
            if (reply != null && reply.startsWith("!")) throw new IllegalArgumentException("Ferrite: recipe " + key + " cannot be added: " + reply.substring(1));
            if ("false".equals(reply)) return false;
        }
        custom.put(key, recipe);
        return true;
    }

    /** The host's answer to a recipe call, or null when no host is attached (plain Java keeps the Bukkit view only). */
    private static String host(String method, Object... args) {
        try { return (String) HostBridge.call("Server", "server", method, String.class, args); }
        catch (UnsupportedOperationException | IllegalStateException noHost) { return null; }
    }

    private static com.google.gson.JsonElement ingredient(org.bukkit.inventory.RecipeChoice choice) {
        if (!(choice instanceof org.bukkit.inventory.RecipeChoice.MaterialChoice materials))
            throw new UnsupportedOperationException("Ferrite: Server#addRecipe cannot add a " + choice.getClass().getSimpleName() + ": crafting menus match items by type and cannot compare item metadata");
        var keys = new com.google.gson.JsonArray();
        for (var material : materials.getChoices()) keys.add(material.getKey().toString());
        return keys.size() == 1 ? keys.get(0) : keys;
    }

    /** A shaped or shapeless recipe as vanilla data pack JSON, or null for any other kind of recipe. */
    static String craftingJson(org.bukkit.inventory.Recipe recipe) {
        var json = new com.google.gson.JsonObject();
        if (recipe instanceof org.bukkit.inventory.ShapedRecipe shaped) {
            json.addProperty("type", "minecraft:crafting_shaped");
            var choices = shaped.getChoiceMap();
            var pattern = new com.google.gson.JsonArray();
            for (var row : shaped.getShape()) {
                var line = new StringBuilder();
                for (char c : row.toCharArray()) line.append(choices.get(c) == null ? ' ' : c);
                pattern.add(line.toString());
            }
            json.add("pattern", pattern);
            var key = new com.google.gson.JsonObject();
            for (var entry : choices.entrySet()) if (entry.getValue() != null) key.add(String.valueOf(entry.getKey()), ingredient(entry.getValue()));
            json.add("key", key);
        } else if (recipe instanceof org.bukkit.inventory.ShapelessRecipe shapeless) {
            json.addProperty("type", "minecraft:crafting_shapeless");
            var ingredients = new com.google.gson.JsonArray();
            for (var choice : shapeless.getChoiceList()) ingredients.add(ingredient(choice));
            json.add("ingredients", ingredients);
        } else {
            return null;
        }
        var result = recipe.getResult();
        if (result.hasItemMeta())
            throw new UnsupportedOperationException("Ferrite: Server#addRecipe cannot add a recipe whose result has item metadata (" + result.getType() + "): the crafting menus craft the item and count only");
        var out = new com.google.gson.JsonObject();
        out.addProperty("id", result.getType().getKey().toString());
        out.addProperty("count", result.getAmount());
        json.add("result", out);
        return json.toString();
    }

    static synchronized boolean remove(org.bukkit.NamespacedKey key) {
        boolean removedOne = custom.remove(key) != null
            || (entries().stream().anyMatch(e -> e.key().equals(key)) && removed.add(key));
        if (removedOne) host("removeCraftingRecipe(String)", key.toString());
        return removedOne;
    }

    static synchronized void clear() {
        custom.clear();
        for (var entry : entries()) removed.add(entry.key());
        host("clearCraftingRecipes()");
    }

    static synchronized void reset() {
        custom.clear();
        removed.clear();
        host("resetCraftingRecipes()");
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
