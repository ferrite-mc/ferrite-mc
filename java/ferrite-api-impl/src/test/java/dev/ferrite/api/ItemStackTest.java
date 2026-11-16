package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotSame;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.List;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.persistence.PersistentDataType;
import org.junit.jupiter.api.Test;

class ItemStackTest {

    @Test
    void materialAmountAndCopiesFollowBukkitSemantics() {
        ItemStack stack = new ItemStack(Material.DIAMOND_SWORD, 2);
        assertEquals(Material.DIAMOND_SWORD, stack.getType());
        assertEquals(2, stack.getAmount());
        assertEquals(1, stack.getMaxStackSize());
        assertFalse(stack.hasItemMeta());

        ItemStack changed = stack.withType(Material.IRON_SWORD);
        assertEquals(Material.IRON_SWORD, changed.getType());
        assertEquals(Material.DIAMOND_SWORD, stack.getType());

        ItemStack copy = stack.clone();
        assertNotSame(stack, copy);
        assertEquals(stack, copy);
        assertEquals(3, stack.add(1).getAmount());
        assertEquals(1, stack.subtract(1).getAmount());
        assertEquals(1, stack.asOne().getAmount());
        assertThrows(IllegalArgumentException.class, () -> stack.setAmount(-1));
        assertThrows(IllegalArgumentException.class, () -> new ItemStack(null, 1));
    }

    @Test
    void displayNameLoreAndPersistentDataRoundTrip() {
        ItemStack stack = new ItemStack(Material.PAPER, 1);
        ItemMeta meta = stack.getItemMeta();
        meta.setDisplayName("Ferrite Blade");
        meta.setLore(List.of("line one", "line two"));
        NamespacedKey key = new NamespacedKey("ferrite", "greeting");
        meta.getPersistentDataContainer().set(key, PersistentDataType.STRING, "hello");
        stack.setItemMeta(meta);

        assertTrue(stack.hasItemMeta());
        assertEquals("Ferrite Blade", stack.getItemMeta().getDisplayName());
        assertEquals(List.of("line one", "line two"), stack.getLore());
        assertEquals("hello", stack.getItemMeta().getPersistentDataContainer()
                .get(key, PersistentDataType.STRING));
        assertEquals("hello", stack.getPersistentDataContainer().get(key, PersistentDataType.STRING));
        assertNull(stack.getItemMeta().getPersistentDataContainer()
                .get(key, PersistentDataType.INTEGER), "type mismatch must not return a value");

        ItemStack clone = stack.clone();
        clone.getItemMeta().setDisplayName("Changed");
        assertEquals("Ferrite Blade", stack.getItemMeta().getDisplayName());
    }

    @Test
    void serializationRoundTripsTheSupportedSlice() {
        ItemStack stack = new ItemStack(Material.DIAMOND, 5);
        ItemMeta meta = stack.getItemMeta();
        meta.setDisplayName("Shiny");
        meta.setLore(List.of("lore"));
        stack.setItemMeta(meta);

        ItemStack restored = ItemStack.deserialize(stack.serialize());
        assertEquals(stack.getType(), restored.getType());
        assertEquals(stack.getAmount(), restored.getAmount());
        assertEquals("Shiny", restored.getItemMeta().getDisplayName());
        assertEquals(List.of("lore"), restored.getLore());
        assertEquals(stack, restored);
    }

    @Test
    void unsupportedItemMethodsFailExplicitly() {
        ItemStack stack = new ItemStack(Material.DIAMOND, 1);
        UnsupportedOperationException exception =
                assertThrows(UnsupportedOperationException.class, stack::getDurability);
        assertTrue(exception.getMessage().contains("ItemStack#getDurability"));
        assertThrows(UnsupportedOperationException.class, stack::getEnchantments);
    }
}
