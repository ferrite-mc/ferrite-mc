package dev.ferrite.server;

import org.bukkit.Material;
import org.bukkit.event.inventory.InventoryType;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.ItemStack;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.BeforeAll;
import static org.junit.jupiter.api.Assertions.*;

class FerriteInventoryTest {
    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
    }

    private static Inventory chest() { return FerriteInventory.create(null, InventoryType.CHEST, 9, "Test"); }

    @Test void addItemMergesIntoSimilarStacksThenFillsEmptySlots() {
        var inventory = chest();
        inventory.setItem(3, new ItemStack(Material.STONE, 60));
        var left = inventory.addItem(new ItemStack(Material.STONE, 10));
        assertTrue(left.isEmpty());
        assertEquals(64, inventory.getItem(3).getAmount());
        assertEquals(6, inventory.getItem(0).getAmount());
        assertEquals(Material.STONE, inventory.getItem(0).getType());
    }

    @Test void addItemReportsWhatDoesNotFitByArgumentIndex() {
        var inventory = FerriteInventory.create(null, InventoryType.CHEST, 9, "Full");
        for (int i = 0; i < 9; i++) inventory.setItem(i, new ItemStack(Material.DIRT, 64));
        var left = inventory.addItem(new ItemStack(Material.STONE, 5), new ItemStack(Material.DIRT, 1));
        assertEquals(2, left.size());
        assertEquals(5, left.get(0).getAmount());
        assertEquals(1, left.get(1).getAmount());
    }

    @Test void removeItemTakesFromFirstSlotsAndReportsTheRest() {
        var inventory = chest();
        inventory.setItem(0, new ItemStack(Material.STONE, 4));
        inventory.setItem(5, new ItemStack(Material.STONE, 10));
        var left = inventory.removeItem(new ItemStack(Material.STONE, 6));
        assertTrue(left.isEmpty());
        assertNull(inventory.getItem(0));
        assertEquals(8, inventory.getItem(5).getAmount());
        left = inventory.removeItem(new ItemStack(Material.STONE, 20));
        assertEquals(12, left.get(0).getAmount());
        assertNull(inventory.getItem(5));
    }

    @Test void containsFirstAllAndRemoveFindItems() {
        var inventory = chest();
        inventory.setItem(2, new ItemStack(Material.GOLD_INGOT, 3));
        inventory.setItem(6, new ItemStack(Material.GOLD_INGOT, 5));
        assertTrue(inventory.contains(Material.GOLD_INGOT));
        assertFalse(inventory.contains(Material.DIAMOND));
        assertTrue(inventory.contains(Material.GOLD_INGOT, 8));
        assertFalse(inventory.contains(Material.GOLD_INGOT, 9));
        assertTrue(inventory.containsAtLeast(new ItemStack(Material.GOLD_INGOT), 8));
        assertEquals(2, inventory.first(Material.GOLD_INGOT));
        assertEquals(-1, inventory.first(Material.DIAMOND));
        assertEquals(2, inventory.all(Material.GOLD_INGOT).size());
        inventory.remove(Material.GOLD_INGOT);
        assertTrue(inventory.isEmpty());
    }
}
