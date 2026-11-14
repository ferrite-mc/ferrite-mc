package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import com.google.gson.JsonParser;
import java.util.ArrayList;
import java.util.List;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.RecipeChoice;
import org.bukkit.inventory.ShapedRecipe;
import org.bukkit.inventory.ShapelessRecipe;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

/** Server#addRecipe and friends: shaped and shapeless recipes reach the host that crafts them. */
class PluginRecipeHostTest {
    private static final String ADD = "addRecipe(String,String,String,int)";
    private final List<String[]> calls = new ArrayList<>();
    private String reply = "true";

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
    }

    @BeforeEach void install() {
        calls.clear();
        reply = "true";
        HostBridge.transport = (owner, subject, method, args) -> {
            calls.add(new String[] {owner, method, args});
            return method.startsWith("add") ? reply : "";
        };
    }

    @AfterEach void reset() {
        HostBridge.transport = null;
        FerriteRecipes.reset();
    }

    private static NamespacedKey key(String name) { return new NamespacedKey("plugintest", name); }

    private String[] only(String method) {
        var matching = calls.stream().filter(call -> call[1].equals(method)).toList();
        assertEquals(1, matching.size(), calls.stream().map(call -> call[1]).toList().toString());
        return matching.get(0);
    }

    @Test void aShapedRecipeIsSentAsVanillaJsonWithItsChoices() {
        var recipe = new ShapedRecipe(key("shaped"), new ItemStack(Material.DIAMOND, 2));
        recipe.shape("AB", "A ");
        recipe.setIngredient('A', Material.DIRT);
        recipe.setIngredient('B', new RecipeChoice.MaterialChoice(Material.SAND, Material.GRAVEL));
        assertTrue(FerriteRecipes.add(recipe));
        var call = only(ADD);
        var fields = call[2].split(String.valueOf(HostBridge.ARG));
        assertEquals("plugintest:shaped", fields[0]);
        var json = JsonParser.parseString(fields[1]).getAsJsonObject();
        assertEquals("minecraft:crafting_shaped", json.get("type").getAsString());
        assertEquals("AB", json.getAsJsonArray("pattern").get(0).getAsString());
        assertEquals("minecraft:dirt", json.getAsJsonObject("key").get("A").getAsString());
        assertEquals(2, json.getAsJsonObject("key").getAsJsonArray("B").size());
        assertEquals("minecraft:diamond", json.getAsJsonObject("result").get("id").getAsString());
        assertEquals(2, json.getAsJsonObject("result").get("count").getAsInt());
        assertNotNull(FerriteRecipes.get(key("shaped")), "the Bukkit view lists it too");
    }

    @Test void aShapelessRecipeListsItsIngredients() {
        var recipe = new ShapelessRecipe(key("shapeless"), new ItemStack(Material.EMERALD));
        recipe.addIngredient(2, Material.SAND);
        assertTrue(FerriteRecipes.add(recipe));
        var json = JsonParser.parseString(only(ADD)[2].split(String.valueOf(HostBridge.ARG))[1]).getAsJsonObject();
        assertEquals("minecraft:crafting_shapeless", json.get("type").getAsString());
        assertEquals(2, json.getAsJsonArray("ingredients").size());
        assertEquals("minecraft:sand", json.getAsJsonArray("ingredients").get(0).getAsString());
    }

    private com.google.gson.JsonObject sent() {
        return JsonParser.parseString(only(ADD)[2].split(String.valueOf(HostBridge.ARG))[1]).getAsJsonObject();
    }

    @Test void recipesTheHostCannotMatchAreRefusedWithAReasonNotDropped() {
        var exact = new ShapelessRecipe(key("exact"), new ItemStack(Material.EMERALD));
        exact.addIngredient(new RecipeChoice.ExactChoice(new ItemStack(Material.SAND)));
        var refused = assertThrows(UnsupportedOperationException.class, () -> FerriteRecipes.add(exact));
        assertTrue(refused.getMessage().contains("HolderSet<Item>"), "names why: " + refused.getMessage());
        assertTrue(refused.getMessage().contains("MaterialChoice"), "names the alternative: " + refused.getMessage());
        var predicate = new ShapelessRecipe(key("predicate"), new ItemStack(Material.EMERALD));
        predicate.addIngredient(RecipeChoice.predicateChoice(item -> true, new ItemStack(Material.SAND)));
        assertThrows(UnsupportedOperationException.class, () -> FerriteRecipes.add(predicate));
        var furnaceExact = new org.bukkit.inventory.FurnaceRecipe(key("furnace_exact"), new ItemStack(Material.STONE),
            new RecipeChoice.ExactChoice(new ItemStack(Material.DIRT)), 0.1f, 100);
        assertThrows(UnsupportedOperationException.class, () -> FerriteRecipes.add(furnaceExact));
        var smithingExact = new org.bukkit.inventory.SmithingTransformRecipe(key("smith_exact"), new ItemStack(Material.STONE),
            RecipeChoice.empty(), new RecipeChoice.ExactChoice(new ItemStack(Material.DIRT)), RecipeChoice.empty());
        assertThrows(UnsupportedOperationException.class, () -> FerriteRecipes.add(smithingExact));
        for (var name : List.of("exact", "predicate", "furnace_exact", "smith_exact")) assertNull(FerriteRecipes.get(key(name)));
        assertTrue(calls.isEmpty(), "nothing reached the host");
    }

    @Test void aResultsMetadataTravelsWithTheRecipeInsteadOfBeingRefused() {
        var named = new ItemStack(Material.DIAMOND, 3);
        var meta = named.getItemMeta();
        meta.setDisplayName("Shiny");
        meta.setLore(List.of("sparkles"));
        meta.addEnchant(org.bukkit.enchantments.Enchantment.SHARPNESS, 2, true);
        named.setItemMeta(meta);
        var recipe = new ShapelessRecipe(key("meta"), named);
        recipe.addIngredient(Material.SAND);
        assertTrue(FerriteRecipes.add(recipe));
        var call = only(ADD)[2].split(String.valueOf(HostBridge.ARG));
        assertEquals("minecraft:diamond", sent().getAsJsonObject("result").get("id").getAsString());
        assertEquals(3, sent().getAsJsonObject("result").get("count").getAsInt());
        var fields = call[2].split("\u001e", -1);
        assertTrue(fields[0].contains("Shiny"), "the name: " + call[2]);
        assertTrue(fields[1].contains("sparkles"), "the lore");
        assertEquals("minecraft:sharpness=2", fields[2]);
        assertEquals("0", call[3], "no wear");
        // Wear is its own argument.
        var worn = new ItemStack(Material.DIAMOND_SWORD);
        var wornMeta = (org.bukkit.inventory.meta.Damageable) worn.getItemMeta();
        wornMeta.setDamage(9);
        worn.setItemMeta(wornMeta);
        calls.clear();
        var wornRecipe = new ShapelessRecipe(key("worn"), worn);
        wornRecipe.addIngredient(Material.SAND);
        assertTrue(FerriteRecipes.add(wornRecipe));
        assertEquals("9", only(ADD)[2].split(String.valueOf(HostBridge.ARG))[3]);
    }

    @Test void aCookingRecipeIsSentForEachFurnaceFamilyAndTheCampfire() {
        var cases = new java.util.LinkedHashMap<String, org.bukkit.inventory.Recipe>();
        cases.put("minecraft:smelting", new org.bukkit.inventory.FurnaceRecipe(key("f"), new ItemStack(Material.GLASS, 2), Material.SAND, 0.7f, 123));
        cases.put("minecraft:blasting", new org.bukkit.inventory.BlastingRecipe(key("b"), new ItemStack(Material.GLASS), new RecipeChoice.MaterialChoice(Material.SAND, Material.GRAVEL), 0.7f, 60));
        cases.put("minecraft:smoking", new org.bukkit.inventory.SmokingRecipe(key("s"), new ItemStack(Material.COOKED_BEEF), Material.BEEF, 0.35f, 100));
        cases.put("minecraft:campfire_cooking", new org.bukkit.inventory.CampfireRecipe(key("c"), new ItemStack(Material.COOKED_COD), Material.COD, 0.35f, 600));
        for (var entry : cases.entrySet()) {
            calls.clear();
            assertTrue(FerriteRecipes.add(entry.getValue()), entry.getKey());
            var json = sent();
            assertEquals(entry.getKey(), json.get("type").getAsString());
            var cooking = (org.bukkit.inventory.CookingRecipe<?>) entry.getValue();
            assertEquals(cooking.getExperience(), json.get("experience").getAsFloat(), 1e-6);
            assertEquals(cooking.getCookingTime(), json.get("cookingtime").getAsInt());
            assertTrue(json.has("ingredient"));
            assertEquals(cooking.getResult().getType().getKey().toString(), json.getAsJsonObject("result").get("id").getAsString());
            assertEquals(cooking.getResult().getAmount(), json.getAsJsonObject("result").get("count").getAsInt());
            assertNotNull(FerriteRecipes.get(entry.getValue() instanceof org.bukkit.Keyed k ? k.getKey() : null));
            assertTrue(FerriteRecipes.remove(((org.bukkit.Keyed) entry.getValue()).getKey()));
        }
    }

    @Test void theRecipeBookCategoryTravelsWithCraftingAndCookingRecipes() {
        var shaped = new ShapedRecipe(key("cat_shaped"), new ItemStack(Material.DIAMOND));
        shaped.shape("A"); shaped.setIngredient('A', Material.DIRT);
        shaped.setCategory(org.bukkit.inventory.recipe.CraftingBookCategory.REDSTONE);
        assertTrue(FerriteRecipes.add(shaped));
        assertEquals("redstone", sent().get("category").getAsString());
        calls.clear();
        var food = new org.bukkit.inventory.SmokingRecipe(key("cat_food"), new ItemStack(Material.COOKED_BEEF), Material.BEEF, 0.1f, 100);
        food.setCategory(org.bukkit.inventory.recipe.CookingBookCategory.FOOD);
        assertTrue(FerriteRecipes.add(food));
        assertEquals("food", sent().get("category").getAsString());
    }

    @Test void aStonecuttingRecipeListsItsInputAndResult() {
        var recipe = new org.bukkit.inventory.StonecuttingRecipe(key("cut"), new ItemStack(Material.STICK, 4), new RecipeChoice.MaterialChoice(Material.OAK_PLANKS, Material.SPRUCE_PLANKS));
        assertTrue(FerriteRecipes.add(recipe));
        var json = sent();
        assertEquals("minecraft:stonecutting", json.get("type").getAsString());
        assertEquals(2, json.getAsJsonArray("ingredient").size());
        assertEquals(4, json.getAsJsonObject("result").get("count").getAsInt());
    }

    @Test void smithingRecipesSendTheirSlotsAndLeaveAnEmptyChoiceOut() {
        var transform = new org.bukkit.inventory.SmithingTransformRecipe(key("smith"), new ItemStack(Material.GOLDEN_SWORD),
            new RecipeChoice.MaterialChoice(Material.STICK), new RecipeChoice.MaterialChoice(Material.DIAMOND_SWORD), RecipeChoice.empty());
        assertTrue(FerriteRecipes.add(transform));
        var json = sent();
        assertEquals("minecraft:smithing_transform", json.get("type").getAsString());
        assertEquals("minecraft:stick", json.get("template").getAsString());
        assertEquals("minecraft:diamond_sword", json.get("base").getAsString());
        assertFalse(json.has("addition"), "an empty choice is an absent optional ingredient");
        assertEquals("minecraft:golden_sword", json.getAsJsonObject("result").get("id").getAsString());
        calls.clear();
        var trim = new org.bukkit.inventory.SmithingTrimRecipe(key("trim"),
            new RecipeChoice.MaterialChoice(Material.STICK), new RecipeChoice.MaterialChoice(Material.IRON_HELMET),
            new RecipeChoice.MaterialChoice(Material.AMETHYST_SHARD), org.bukkit.inventory.meta.trim.TrimPattern.WILD);
        assertTrue(FerriteRecipes.add(trim));
        var trimJson = sent();
        assertEquals("minecraft:smithing_trim", trimJson.get("type").getAsString());
        assertEquals("minecraft:wild", trimJson.get("pattern").getAsString());
    }

    @Test void aRecipeKindTheHostCannotMatchIsRefused() {
        var transmute = new org.bukkit.inventory.TransmuteRecipe(key("transmute"), Material.STONE, new RecipeChoice.MaterialChoice(Material.DIRT), new RecipeChoice.MaterialChoice(Material.SAND));
        var refused = assertThrows(UnsupportedOperationException.class, () -> FerriteRecipes.add(transmute));
        assertTrue(refused.getMessage().contains("Transmute"), refused.getMessage());
        assertNull(FerriteRecipes.get(key("transmute")));
        assertTrue(calls.isEmpty());
    }

    @Test void aTakenIdOrARefusalFromTheHostLeavesNoRecipe() {
        var recipe = new ShapelessRecipe(key("taken"), new ItemStack(Material.EMERALD));
        recipe.addIngredient(Material.SAND);
        reply = "false";
        assertFalse(FerriteRecipes.add(recipe));
        assertNull(FerriteRecipes.get(key("taken")));
        reply = "!why not";
        var error = assertThrows(IllegalArgumentException.class, () -> FerriteRecipes.add(recipe));
        assertTrue(error.getMessage().contains("why not"), error.getMessage());
        assertNull(FerriteRecipes.get(key("taken")));
    }

    @Test void removingClearingAndResettingTellTheHost() {
        var recipe = new ShapelessRecipe(key("gone"), new ItemStack(Material.EMERALD));
        recipe.addIngredient(Material.SAND);
        assertTrue(FerriteRecipes.add(recipe));
        assertTrue(FerriteRecipes.remove(key("gone")));
        assertEquals("plugintest:gone", only("removeCraftingRecipe(String)")[2]);
        assertFalse(FerriteRecipes.remove(key("gone")), "already gone, nothing to tell the host");
        assertEquals(1, calls.stream().filter(c -> c[1].equals("removeCraftingRecipe(String)")).count());
        FerriteRecipes.clear();
        only("clearCraftingRecipes()");
        FerriteRecipes.reset();
        only("resetCraftingRecipes()");
    }

    @Test void withoutAHostTheBukkitViewStillWorks() {
        HostBridge.transport = null;
        var recipe = new ShapelessRecipe(key("offline"), new ItemStack(Material.EMERALD));
        recipe.addIngredient(Material.SAND);
        assertTrue(FerriteRecipes.add(recipe));
        assertNotNull(FerriteRecipes.get(key("offline")));
    }
}
