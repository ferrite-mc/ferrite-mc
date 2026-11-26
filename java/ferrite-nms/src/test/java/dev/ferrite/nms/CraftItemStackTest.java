// Clean-room Ferrite NMS test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.papermc.paper.datacomponent.DataComponentTypes;
import java.util.Arrays;
import net.kyori.adventure.text.Component;
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
        assertThrows(UnsupportedOperationException.class,
                () -> CraftItemStack.asCraftMirror((ItemStack) null));
    }

    @Test
    void paperComponentsSurviveBothConversions() {
        org.bukkit.inventory.ItemStack bukkit =
                new org.bukkit.inventory.ItemStack(Material.DIAMOND_SWORD, 2);
        bukkit.setData(DataComponentTypes.CUSTOM_NAME, Component.text("Ferrite NMS"));
        bukkit.setData(DataComponentTypes.DAMAGE, 3);
        bukkit.setData(DataComponentTypes.MAX_DAMAGE, 500);
        bukkit.setData(DataComponentTypes.MAX_STACK_SIZE, 16);
        bukkit.setData(DataComponentTypes.UNBREAKABLE);

        ItemStack nms = CraftItemStack.asNMSCopy(bukkit);
        assertEquals(Component.text("Ferrite NMS"),
                nms.getComponent(DataComponentTypes.CUSTOM_NAME));
        assertEquals(3, nms.getDamageValue());
        assertEquals(500, nms.getMaxDamage());
        assertEquals(16, nms.getMaxStackSize());
        assertTrue(nms.isUnbreakable());
        assertTrue(nms.isDamageableItem());
        assertTrue(nms.componentTypes().contains(DataComponentTypes.CUSTOM_NAME));
        assertEquals(Component.text("Ferrite NMS"), nms.getHoverName());
        assertTrue(nms.getDescriptionId().contains("diamond_sword"));

        org.bukkit.inventory.ItemStack back = CraftItemStack.asBukkitCopy(nms);
        assertNotNull(back);
        assertEquals(Component.text("Ferrite NMS"),
                back.getData(DataComponentTypes.CUSTOM_NAME));
        assertEquals(Integer.valueOf(3), back.getData(DataComponentTypes.DAMAGE));
        assertEquals(Integer.valueOf(500), back.getData(DataComponentTypes.MAX_DAMAGE));
        assertEquals(Integer.valueOf(16), back.getData(DataComponentTypes.MAX_STACK_SIZE));
        assertTrue(back.getDataTypes().contains(DataComponentTypes.UNBREAKABLE));

        back.setAmount(5);
        assertEquals(2, nms.getCount(), "asBukkitCopy must not share the amount with the NMS copy");
    }

    @Test
    void componentMutationsStayOnTheNmsCopy() {
        org.bukkit.inventory.ItemStack bukkit =
                new org.bukkit.inventory.ItemStack(Material.DIAMOND_SWORD);
        bukkit.setData(DataComponentTypes.DAMAGE, 3);
        ItemStack nms = CraftItemStack.asNMSCopy(bukkit);
        assertEquals(3, nms.getDamageValue());

        nms.setDamageValue(7);
        assertEquals(7, nms.getDamageValue());
        assertEquals(Integer.valueOf(3), bukkit.getData(DataComponentTypes.DAMAGE),
                "the NMS copy mutated the source Paper stack");

        nms.setHoverName(Component.text("Renamed"));
        assertEquals(Component.text("Renamed"), nms.getHoverName());
        assertNull(bukkit.getData(DataComponentTypes.CUSTOM_NAME));
        assertTrue(nms.componentTypes().contains(DataComponentTypes.CUSTOM_NAME));
        nms.unsetComponent(DataComponentTypes.CUSTOM_NAME);
        assertFalse(nms.componentTypes().contains(DataComponentTypes.CUSTOM_NAME));

        org.bukkit.inventory.ItemStack back = CraftItemStack.asBukkitCopy(nms);
        assertNotNull(back);
        assertEquals(Integer.valueOf(7), back.getData(DataComponentTypes.DAMAGE));
        assertNull(back.getData(DataComponentTypes.CUSTOM_NAME));
    }

    @Test
    void componentReadsRequireAPaperBackedStack() {
        ItemStack plain = ItemStack.of(Material.STONE, 1);
        UnsupportedOperationException error = assertThrows(
                UnsupportedOperationException.class,
                () -> plain.getComponent(DataComponentTypes.DAMAGE));
        assertTrue(error.getMessage().contains("ItemStack#getComponent"), error.getMessage());
        assertTrue(error.getMessage().contains("CraftItemStack.asNMSCopy"), error.getMessage());
        assertThrows(UnsupportedOperationException.class, ItemStack.EMPTY::componentTypes);
        assertThrows(IllegalArgumentException.class, () -> plain.setDamageValue(-1));
    }

    @Test
    void onlyCraftBukkitsMirrorSignatureExists() {
        long mirrorMethods = Arrays.stream(CraftItemStack.class.getMethods())
                .filter(method -> method.getName().equals("asCraftMirror"))
                .count();
        assertEquals(1L, mirrorMethods,
                "a second overload would make asCraftMirror(null) ambiguous for CraftBukkit plugins");
    }
}
