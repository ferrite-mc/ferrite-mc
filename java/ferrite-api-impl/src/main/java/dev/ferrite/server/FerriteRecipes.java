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
        // Every recipe the host can match goes to it as vanilla data pack JSON: crafting, the furnace family, the
        // campfire, the stonecutter and the smithing table. Anything else is refused, never listed without effect.
        // A potion mix (BrewingRecipe, from PotionBrewer) goes to the host's brewing table too; the vanilla view's own
        // entries stay Bukkit-side only.
        if (recipe instanceof Entry) { custom.put(key, recipe); return true; }
        if (recipe instanceof org.bukkit.inventory.BrewingRecipe brewing) {
            var result = brewing.getResult();
            String potion = "";
            if (result.getItemMeta() instanceof org.bukkit.inventory.meta.PotionMeta meta) {
                if (meta.hasCustomEffects())
                    throw new UnsupportedOperationException("Ferrite: Server#addRecipe cannot add a BrewingRecipe whose result carries custom potion effects: the host stores a base potion type by id, not an arbitrary effect list. Use a PotionType result, or grant the effects from a PotionEffect event");
                if (meta.getBasePotionType() != null) potion = meta.getBasePotionType().getKey().toString();
            }
            var reply = host("addBrewingRecipe(String,String,String,String,String,int)", key.toString(),
                brewingItems(brewing.getInput(), "input"), brewingItems(brewing.getIngredient(), "ingredient"),
                result.getType().getKey().toString(), potion, Integer.toString(result.getAmount()));
            if (reply != null && reply.startsWith("!"))
                throw new IllegalArgumentException("Ferrite: recipe " + key + " cannot be added: " + reply.substring(1));
            if ("false".equals(reply)) return false;
            custom.put(key, recipe);
            return true;
        }
        var json = recipeJson(recipe);
        var extras = resultExtras(recipe);
        var reply = host("addRecipe(String,String,String,int)", key.toString(), json, extras.meta(), Integer.toString(extras.damage()));
        if (reply != null && reply.startsWith("!")) throw new IllegalArgumentException("Ferrite: recipe " + key + " cannot be added: " + reply.substring(1));
        if ("false".equals(reply)) return false;
        custom.put(key, recipe);
        return true;
    }

    /** The host's answer to a recipe call, or null when no host is attached (plain Java keeps the Bukkit view only). */
    private static String host(String method, Object... args) {
        try { return (String) HostBridge.call("Server", "server", method, String.class, args); }
        catch (UnsupportedOperationException | IllegalStateException noHost) { return null; }
    }

    private static com.google.gson.JsonElement ingredient(org.bukkit.inventory.RecipeChoice choice) {
        if (choice instanceof org.bukkit.inventory.RecipeChoice.ExactChoice)
            throw new UnsupportedOperationException("Ferrite: Server#addRecipe cannot add a RecipeChoice.ExactChoice: a 26.3 recipe ingredient is a set of item types or an item tag with no data-component predicate (net.minecraft.world.item.crafting.Ingredient wraps a HolderSet<Item>), "
                + "so neither the recipe matching nor the recipe the client is sent can compare item metadata. Use a MaterialChoice and check the stack's metadata in an InventoryClickEvent handler on the result slot");
        if (choice instanceof org.bukkit.inventory.RecipeChoice.PredicateChoice)
            throw new UnsupportedOperationException("Ferrite: Server#addRecipe cannot add a RecipeChoice.PredicateChoice: an arbitrary predicate cannot be expressed as a 26.3 ingredient (a set of item types or an item tag). Use a MaterialChoice or RecipeChoice.itemType");
        if (!(choice instanceof org.bukkit.inventory.RecipeChoice.MaterialChoice materials))
            throw new UnsupportedOperationException("Ferrite: Server#addRecipe cannot add a " + choice.getClass().getSimpleName() + ": only MaterialChoice and RecipeChoice.itemType ingredients are supported");
        var keys = new com.google.gson.JsonArray();
        for (var material : materials.getChoices()) keys.add(material.getKey().toString());
        return keys.size() == 1 ? keys.get(0) : keys;
    }

    /** The item ids a brewing mix slot accepts. A potion mix predicates on items, not metadata. */
    private static String brewingItems(org.bukkit.inventory.RecipeChoice choice, String name) {
        java.util.List<org.bukkit.inventory.ItemStack> stacks;
        if (choice instanceof org.bukkit.inventory.RecipeChoice.MaterialChoice materials) {
            return materials.getChoices().stream().map(material -> material.getKey().toString())
                .collect(java.util.stream.Collectors.joining(String.valueOf(HostBridge.ITEM)));
        } else if (choice instanceof org.bukkit.inventory.RecipeChoice.ExactChoice exact) {
            stacks = exact.getChoices();
        } else {
            throw new UnsupportedOperationException("Ferrite: Server#addRecipe cannot add a BrewingRecipe with a "
                + choice.getClass().getSimpleName() + " " + name + ": the host's brewing table matches item ids");
        }
        return stacks.stream().map(stack -> stack.getType().getKey().toString()).distinct()
            .collect(java.util.stream.Collectors.joining(String.valueOf(HostBridge.ITEM)));
    }

    private static boolean isEmpty(org.bukkit.inventory.RecipeChoice choice) {
        return choice == null || choice.equals(org.bukkit.inventory.RecipeChoice.empty());
    }

    private static void put(com.google.gson.JsonObject json, String name, org.bukkit.inventory.RecipeChoice choice, boolean required) {
        if (isEmpty(choice)) {
            if (required) throw new IllegalArgumentException("Ferrite: a " + name + " ingredient is required");
            return;
        }
        json.add(name, ingredient(choice));
    }

    /** The wear and metadata a recipe's result stack carries; refused when the host cannot hold them. */
    private record Extras(String meta, int damage) {}

    private static Extras resultExtras(org.bukkit.inventory.Recipe recipe) {
        if (recipe instanceof org.bukkit.inventory.SmithingTrimRecipe) return new Extras("", 0);
        var result = recipe.getResult();
        if (result == null || result.getType().isAir()) throw new IllegalArgumentException("Ferrite: a recipe needs a result item");
        var wire = ItemMetaWire.of(result);
        if (wire.mapId() != null) throw new UnsupportedOperationException("Ferrite: Server#addRecipe cannot add a recipe whose result is a filled map view: the result is made when the recipe matches, not per map");
        return new Extras(wire.meta() == null ? "" : wire.meta(), wire.damage());
    }

    private static com.google.gson.JsonObject resultJson(org.bukkit.inventory.ItemStack result) {
        var out = new com.google.gson.JsonObject();
        out.addProperty("id", result.getType().getKey().toString());
        out.addProperty("count", result.getAmount());
        return out;
    }

    private static void cookingFields(com.google.gson.JsonObject json, org.bukkit.inventory.CookingRecipe<?> recipe) {
        put(json, "ingredient", recipe.getInputChoice(), true);
        json.addProperty("category", recipe.getCategory().name().toLowerCase(java.util.Locale.ROOT));
        json.add("result", resultJson(recipe.getResult()));
        json.addProperty("experience", recipe.getExperience());
        json.addProperty("cookingtime", recipe.getCookingTime());
    }

    /** A recipe the host can match as vanilla data pack JSON; anything else is refused with the reason. */
    static String recipeJson(org.bukkit.inventory.Recipe recipe) {
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
            json.addProperty("category", shaped.getCategory().name().toLowerCase(java.util.Locale.ROOT));
            var key = new com.google.gson.JsonObject();
            for (var entry : choices.entrySet()) if (entry.getValue() != null) key.add(String.valueOf(entry.getKey()), ingredient(entry.getValue()));
            json.add("key", key);
            json.add("result", resultJson(recipe.getResult()));
        } else if (recipe instanceof org.bukkit.inventory.ShapelessRecipe shapeless) {
            json.addProperty("type", "minecraft:crafting_shapeless");
            var ingredients = new com.google.gson.JsonArray();
            for (var choice : shapeless.getChoiceList()) ingredients.add(ingredient(choice));
            json.add("ingredients", ingredients);
            json.addProperty("category", shapeless.getCategory().name().toLowerCase(java.util.Locale.ROOT));
            json.add("result", resultJson(recipe.getResult()));
        } else if (recipe instanceof org.bukkit.inventory.FurnaceRecipe furnace) {
            json.addProperty("type", "minecraft:smelting");
            cookingFields(json, furnace);
        } else if (recipe instanceof org.bukkit.inventory.BlastingRecipe blasting) {
            json.addProperty("type", "minecraft:blasting");
            cookingFields(json, blasting);
        } else if (recipe instanceof org.bukkit.inventory.SmokingRecipe smoking) {
            json.addProperty("type", "minecraft:smoking");
            cookingFields(json, smoking);
        } else if (recipe instanceof org.bukkit.inventory.CampfireRecipe campfire) {
            json.addProperty("type", "minecraft:campfire_cooking");
            cookingFields(json, campfire);
        } else if (recipe instanceof org.bukkit.inventory.StonecuttingRecipe cutting) {
            json.addProperty("type", "minecraft:stonecutting");
            put(json, "ingredient", cutting.getInputChoice(), true);
            json.add("result", resultJson(recipe.getResult()));
        } else if (recipe instanceof org.bukkit.inventory.SmithingTransformRecipe smithing) {
            json.addProperty("type", "minecraft:smithing_transform");
            put(json, "template", smithing.getTemplate(), false);
            put(json, "base", smithing.getBase(), true);
            put(json, "addition", smithing.getAddition(), false);
            json.add("result", resultJson(recipe.getResult()));
        } else if (recipe instanceof org.bukkit.inventory.SmithingTrimRecipe trim) {
            json.addProperty("type", "minecraft:smithing_trim");
            put(json, "template", trim.getTemplate(), false);
            put(json, "base", trim.getBase(), true);
            put(json, "addition", trim.getAddition(), false);
            if (trim.getTrimPattern() != null) json.addProperty("pattern", trim.getTrimPattern().key().asString());
        } else {
            throw new UnsupportedOperationException("Ferrite: Server#addRecipe cannot add a " + recipe.getClass().getSimpleName()
                + ": the host matches shaped, shapeless, furnace, blasting, smoking, campfire, stonecutting and smithing (transform and trim) recipes. Transmute and the special crafting recipes are code in vanilla, not data");
        }
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
