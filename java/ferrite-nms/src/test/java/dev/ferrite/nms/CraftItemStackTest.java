// Clean-room Ferrite NMS test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import net.minecraft.world.item.ItemStack;
import org.bukkit.Material;
import org.bukkit.craftbukkit.inventory.CraftItemStack;
import org.junit.jupiter.api.Test;

class CraftItemStackTest {
    @Test
    void nullAndEmptyStacksConvertToTheDocumentedValues() {
        assertTrue(CraftItemStack.asNMSCopy(null).isEmpty());
        assertNull(CraftItemStack.asBukkitCopy((ItemStack) null));
        assertNull(CraftItemStack.asBukkitCopy(ItemStack.EMPTY));
        assertNull(CraftItemStack.asCraftCopy(null));
    }

    @Test
    void materialAndCountSurviveTheValueRoundTrip() {
        ItemStack stack = ItemStack.of(Material.STONE, 3);
        assertEquals(3, stack.getCount());
        assertEquals(Material.STONE, stack.getBukkitMaterial());
        assertEquals("minecraft:stone", stack.getItem().getName());

        stack.grow(2);
        assertEquals(5, stack.getCount());
        stack.shrink(4);
        assertEquals(1, stack.getCount());
        stack.setCount(0);
        assertTrue(stack.isEmpty());
        assertEquals(0, stack.getCount());

        ItemStack copied = ItemStack.of(Material.DIAMOND, 2).copy();
        assertEquals(2, copied.getCount());
        assertEquals(Material.DIAMOND, copied.getBukkitMaterial());
    }

    @Test
    void emptyStackRefusesToBeResurrectedBySetCount() {
        assertThrows(IllegalStateException.class, () -> ItemStack.EMPTY.setCount(1));
        assertThrows(IllegalArgumentException.class, () -> ItemStack.of(Material.STONE, -1));
    }

    @Test
    void mirrorIsRefusedWithClassMethodAndReason() {
        UnsupportedOperationException error =
                assertThrows(UnsupportedOperationException.class,
                        () -> CraftItemStack.asCraftMirror(ItemStack.EMPTY));
        assertTrue(error.getMessage().contains("CraftItemStack#asCraftMirror"), error.getMessage());
        assertTrue(error.getMessage().contains("mirror semantics"), error.getMessage());
    }
}
