package dev.ferrite.server;

import org.bukkit.Material;
import org.bukkit.inventory.ItemStack;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class FerriteItemStackTest {
    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
    }

    @Test void similarityIgnoresAmountButEqualityDoesNot() {
        var a = new ItemStack(Material.STONE, 3);
        var b = new ItemStack(Material.STONE, 5);
        assertTrue(a.isSimilar(b));
        assertNotEquals(a, b);
        assertEquals(a, new ItemStack(Material.STONE, 3));
        assertFalse(a.isSimilar(new ItemStack(Material.DIRT, 3)));
        assertFalse(a.isSimilar(null));
    }

    @Test void maxStackSizeEmptinessAndCopies() {
        assertEquals(64, new ItemStack(Material.STONE).getMaxStackSize());
        HostBridge.transport = (owner, subject, method, args) -> owner.equals("ItemType") ? (subject.endsWith("ender_pearl") ? "16" : "1") : null;
        try {
            assertEquals(16, new ItemStack(Material.ENDER_PEARL).getMaxStackSize());
            assertEquals(1, new ItemStack(Material.DIAMOND_SWORD).getMaxStackSize());
        } finally { HostBridge.transport = null; }
        assertFalse(new ItemStack(Material.STONE).isEmpty());
        assertEquals(1, new ItemStack(Material.STONE, 9).asOne().getAmount());
        assertEquals(4, new ItemStack(Material.STONE, 9).asQuantity(4).getAmount());
        assertEquals(10, new ItemStack(Material.STONE, 9).add().getAmount());
    }

    @Test void metaEditsCarryDisplayNameAndEnchantments() {
        var sword = new ItemStack(Material.DIAMOND_SWORD);
        assertTrue(sword.editMeta(meta -> meta.displayName(net.kyori.adventure.text.Component.text("Blade"))));
        assertTrue(sword.hasItemMeta());
        assertFalse(sword.isSimilar(new ItemStack(Material.DIAMOND_SWORD)));
        assertTrue(sword.isSimilar(sword.clone()));
    }
}
