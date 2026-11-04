package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import org.bukkit.Material;
import org.bukkit.inventory.ItemStack;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

class FerriteCraftingTest {
    private static final String SEP = String.valueOf(HostBridge.ITEM);
    private static String lastCall;

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        HostBridge.transport = (owner, subject, method, args) -> {
            if (!method.equals("craftMatrix(int,String)")) return null;
            lastCall = args;
            return args.contains("OAK_LOG") ? "minecraft:oak_planks" + SEP + "minecraft:oak_planks" + SEP + "4" : "";
        };
    }

    @AfterAll static void stop() { HostBridge.transport = null; }

    private static ItemStack[] grid2(Material... slots) {
        var out = new ItemStack[4];
        for (int i = 0; i < slots.length; i++) out[i] = slots[i] == null ? null : new ItemStack(slots[i], 2);
        return out;
    }

    @Test void aMatchedGridReturnsTheRecipeAndConsumesOneOfEach() {
        var matrix = grid2(null, Material.OAK_LOG);
        var result = org.bukkit.Bukkit.getServer().craftItemResult(matrix, null);
        assertEquals(new ItemStack(Material.OAK_PLANKS, 4), result.getResult());
        assertEquals(1, result.getResultingMatrix()[1].getAmount());
        assertNull(result.getResultingMatrix()[0]);
        assertEquals(2, matrix[1].getAmount(), "the caller's matrix is not modified");
        assertTrue(lastCall.startsWith("2"));
        assertTrue(result.getOverflowItems().isEmpty());
    }

    @Test void anUnmatchedGridCraftsNothing() {
        var matrix = grid2(Material.DIRT);
        assertNull(org.bukkit.Bukkit.getServer().getCraftingRecipe(matrix, null));
        var result = org.bukkit.Bukkit.getServer().craftItemResult(matrix, null);
        assertTrue(result.getResult().getType().isAir());
        assertEquals(2, result.getResultingMatrix()[0].getAmount());
    }

    @Test void onlyTwoByTwoAndThreeByThreeGridsAreAccepted() {
        assertThrows(IllegalArgumentException.class, () -> org.bukkit.Bukkit.getServer().getCraftingRecipe(new ItemStack[5], null));
        assertDoesNotThrow(() -> org.bukkit.Bukkit.getServer().getCraftingRecipe(new ItemStack[9], null));
    }
}
