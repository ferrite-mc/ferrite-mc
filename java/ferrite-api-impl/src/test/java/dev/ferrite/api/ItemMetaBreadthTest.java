package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.item.FerriteItemMeta;
import dev.ferrite.api.item.FerriteItemNbt;
import dev.ferrite.api.test.TestServer;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import net.kyori.adventure.text.Component;
import org.bukkit.Material;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.Damageable;
import org.bukkit.inventory.meta.ItemMeta;
import org.junit.jupiter.api.Test;

class ItemMetaBreadthTest {

    @Test
    void itemMetaBreadthRoundTripsThroughItemStackAndNbtBridge() {
        try (TestServer ignored = new TestServer()) {
            ItemStack sword = new ItemStack(Material.DIAMOND_SWORD, 1);
            ItemMeta meta = sword.getItemMeta();
            meta.customName(Component.text("Custom Blade"));
            meta.itemName(Component.text("Base Item Name"));
            meta.lore(List.of(Component.text("Line A"), Component.text("Line B")));
            meta.setCustomModelData(77);
            meta.addEnchant(Enchantment.SHARPNESS, 4, true);
            meta.setUnbreakable(true);
            ((Damageable) meta).setDamage(5);
            ((Damageable) meta).setMaxDamage(150);
            sword.setItemMeta(meta);

            assertTrue(sword.hasItemMeta());
            ItemMeta read = sword.getItemMeta();
            assertTrue(read.hasCustomName());
            assertEquals("Custom Blade", read.getDisplayName());
            assertEquals(Component.text("Custom Blade"), read.customName());
            assertTrue(read.hasItemName());
            assertEquals("Base Item Name", read.getItemName());
            assertEquals(Component.text("Base Item Name"), read.itemName());
            assertTrue(read.hasLore());
            assertEquals(List.of("Line A", "Line B"), read.getLore());
            assertEquals(List.of(Component.text("Line A"), Component.text("Line B")), read.lore());
            assertTrue(read.hasCustomModelData());
            assertEquals(77, read.getCustomModelData());
            assertTrue(read.hasCustomModelDataComponent());
            assertEquals(List.of(77.0f), read.getCustomModelDataComponent().getFloats());
            assertTrue(read.hasEnchants());
            assertTrue(read.hasEnchant(Enchantment.SHARPNESS));
            assertEquals(4, read.getEnchantLevel(Enchantment.SHARPNESS));
            assertTrue(read.isUnbreakable());
            assertEquals(5, ((Damageable) read).getDamage());
            assertEquals(150, ((Damageable) read).getMaxDamage());

            String encoded = FerriteItemNbt.encodeItemBase64(sword);
            assertNotNull(encoded);
            ItemStack decoded = FerriteItemNbt.decodeItemBase64(encoded, Material.DIAMOND_SWORD, 1);
            assertNotNull(decoded);
            ItemMeta decodedMeta = decoded.getItemMeta();
            assertEquals("Custom Blade", decodedMeta.getDisplayName());
            assertEquals("Base Item Name", decodedMeta.getItemName());
            assertEquals(List.of("Line A", "Line B"), decodedMeta.getLore());
            assertEquals(77, decodedMeta.getCustomModelData());
            assertEquals(4, decodedMeta.getEnchantLevel(Enchantment.SHARPNESS));
            assertTrue(decodedMeta.isUnbreakable());
            assertEquals(5, ((Damageable) decodedMeta).getDamage());
            assertEquals(150, ((Damageable) decodedMeta).getMaxDamage());

            @SuppressWarnings("unchecked")
            Map<String, Object> serialized = read.serialize();
            FerriteItemMeta mapRestored = FerriteItemMeta.deserialize(serialized);
            assertEquals(read, mapRestored);
        }
    }

    @Test
    void defensiveCopiesProtectItemAndMetaState() {
        try (TestServer ignored = new TestServer()) {
            ItemStack stack = new ItemStack(Material.IRON_SWORD, 1);
            List<String> sourceLore = new ArrayList<>(List.of("first"));
            ItemMeta meta = stack.getItemMeta();
            meta.setDisplayName("Original");
            meta.setItemName("OriginalItemName");
            meta.setLore(sourceLore);
            meta.addEnchant(Enchantment.SHARPNESS, 2, true);
            stack.setItemMeta(meta);

            sourceLore.add("mutated-caller");
            meta.setDisplayName("MutatedAfterSet");
            meta.setItemName("MutatedItemNameAfterSet");

            ItemMeta fresh = stack.getItemMeta();
            assertEquals("Original", fresh.getDisplayName());
            assertEquals("OriginalItemName", fresh.getItemName());
            assertEquals(List.of("first"), fresh.getLore());

            List<String> returnedLore = fresh.getLore();
            returnedLore.add("mutated-getter");
            assertEquals(List.of("first"), fresh.getLore());

            fresh.setDisplayName("MutatedDetached");
            assertEquals("Original", stack.getItemMeta().getDisplayName());

            assertThrows(
                    UnsupportedOperationException.class,
                    () -> fresh.getEnchants().put(Enchantment.SHARPNESS, 99));
        }
    }

    @Test
    void nullAndEmptySemanticsClearFieldsAndNormalizeMeta() {
        try (TestServer ignored = new TestServer()) {
            ItemStack stack = new ItemStack(Material.DIAMOND_PICKAXE, 1);
            stack.editMeta(m -> {
                m.setDisplayName("Pick");
                m.setItemName("Special Pick");
                m.setLore(List.of("l1"));
                m.setCustomModelData(12);
                m.addEnchant(Enchantment.SHARPNESS, 1, true);
                m.setUnbreakable(true);
                ((Damageable) m).setDamage(3);
                ((Damageable) m).setMaxDamage(100);
            });
            assertTrue(stack.hasItemMeta());

            stack.editMeta(m -> {
                m.customName(null);
                m.itemName(null);
                m.setLore(List.of());
                m.setCustomModelData(null);
                assertTrue(m.removeEnchant(Enchantment.SHARPNESS));
                assertFalse(m.removeEnchant(Enchantment.SHARPNESS));
                m.setUnbreakable(false);
                ((Damageable) m).resetDamage();
                ((Damageable) m).setMaxDamage(null);
            });

            assertFalse(stack.hasItemMeta());
            ItemMeta empty = stack.getItemMeta();
            assertFalse(empty.hasCustomName());
            assertNull(empty.customName());
            assertFalse(empty.hasItemName());
            assertNull(empty.itemName());
            assertNull(empty.getItemName());
            assertFalse(empty.hasLore());
            assertNull(empty.getLore());
            assertNull(empty.lore());
            assertFalse(empty.hasCustomModelData());
            assertFalse(empty.hasEnchants());
            assertFalse(empty.isUnbreakable());
            assertFalse(((Damageable) empty).hasDamageValue());
            assertFalse(((Damageable) empty).hasMaxDamage());
            assertNull(FerriteItemNbt.encodeItemBase64(stack));
        }
    }

    @Test
    void unsupportedMethodsAndCorruptNbtFailExplicitly() {
        try (TestServer ignored = new TestServer()) {
            ItemStack stack = new ItemStack(Material.STONE, 1);
            ItemMeta meta = stack.getItemMeta();
            assertThrows(UnsupportedOperationException.class, meta::hasEnchantmentGlintOverride);
            assertThrows(UnsupportedOperationException.class, meta::getEnchantmentGlintOverride);
            assertThrows(
                    UnsupportedOperationException.class, () -> meta.setEnchantmentGlintOverride(true));
            assertThrows(UnsupportedOperationException.class, meta::hasLocalizedName);
            assertThrows(UnsupportedOperationException.class, meta::isGlider);
            assertThrows(UnsupportedOperationException.class, meta::hasRarity);

            assertThrows(
                    IllegalArgumentException.class,
                    () -> FerriteItemNbt.decodeItemBase64("not-base64!!!", Material.STONE, 1));
        }
    }
}
