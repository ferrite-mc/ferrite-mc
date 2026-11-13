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
        var call = only("addCraftingRecipe(String,String)");
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
        var json = JsonParser.parseString(only("addCraftingRecipe(String,String)")[2].split(String.valueOf(HostBridge.ARG))[1]).getAsJsonObject();
        assertEquals("minecraft:crafting_shapeless", json.get("type").getAsString());
        assertEquals(2, json.getAsJsonArray("ingredients").size());
        assertEquals("minecraft:sand", json.getAsJsonArray("ingredients").get(0).getAsString());
    }

    @Test void recipesTheHostCannotCraftAreRefusedNotDropped() {
        var exact = new ShapelessRecipe(key("exact"), new ItemStack(Material.EMERALD));
        exact.addIngredient(new RecipeChoice.ExactChoice(new ItemStack(Material.SAND)));
        assertThrows(UnsupportedOperationException.class, () -> FerriteRecipes.add(exact));
        var named = new ItemStack(Material.DIAMOND);
        var meta = named.getItemMeta();
        meta.setDisplayName("Shiny");
        named.setItemMeta(meta);
        var withMeta = new ShapelessRecipe(key("meta"), named);
        withMeta.addIngredient(Material.SAND);
        assertThrows(UnsupportedOperationException.class, () -> FerriteRecipes.add(withMeta));
        assertNull(FerriteRecipes.get(key("exact")));
        assertNull(FerriteRecipes.get(key("meta")));
        assertTrue(calls.isEmpty(), "nothing reached the host");
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
