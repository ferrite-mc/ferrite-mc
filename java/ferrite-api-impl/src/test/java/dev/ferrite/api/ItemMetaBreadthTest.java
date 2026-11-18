package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.item.FerriteDataComponents;
import dev.ferrite.api.item.FerriteItemMeta;
import dev.ferrite.api.item.FerriteItemNbt;
import dev.ferrite.api.registry.FerriteKeySet;
import dev.ferrite.api.test.TestServer;
import io.papermc.paper.datacomponent.DataComponentType;
import io.papermc.paper.datacomponent.item.DamageResistant;
import io.papermc.paper.datacomponent.item.Repairable;
import io.papermc.paper.datacomponent.item.UseRemainder;
import io.papermc.paper.registry.RegistryKey;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import net.kyori.adventure.text.Component;
import net.md_5.bungee.api.ChatColor;
import net.md_5.bungee.api.chat.BaseComponent;
import net.md_5.bungee.api.chat.TextComponent;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.damage.DamageType;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.inventory.ItemRarity;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.ItemType;
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
    @SuppressWarnings("deprecation")
    void bungeeCompatibilityViewsShareSingleSourceOfTruthAndCopyDefensively() {
        try (TestServer ignored = new TestServer()) {
            ItemStack stack = new ItemStack(Material.GOLDEN_SWORD, 1);
            TextComponent bungeeName = new TextComponent("Bungee Blade");
            bungeeName.setColor(ChatColor.GOLD);
            BaseComponent[] inputName = new BaseComponent[] {bungeeName};
            List<BaseComponent[]> inputLore = new ArrayList<>();
            inputLore.add(new BaseComponent[] {new TextComponent("First Bungee Line")});
            inputLore.add(new BaseComponent[] {new TextComponent("Second Bungee Line")});

            stack.editMeta(m -> {
                m.setDisplayNameComponent(inputName);
                m.setLoreComponents(inputLore);
            });

            // Mutating caller inputs must not affect stored meta.
            bungeeName.setText("MutatedInput");
            inputLore.add(new BaseComponent[] {new TextComponent("Extra")});

            ItemMeta read = stack.getItemMeta();
            assertTrue(read.hasCustomName());
            assertTrue(read.getDisplayName().contains("Bungee Blade"));
            assertNotNull(read.customName());
            assertEquals(2, read.getLore().size());
            assertEquals("First Bungee Line", read.getLore().get(0));

            BaseComponent[] gotName = read.getDisplayNameComponent();
            assertNotNull(gotName);
            assertEquals("Bungee Blade", BaseComponent.toPlainText(gotName));
            gotName[0] = new TextComponent("MutatedReturned");
            assertEquals("Bungee Blade", BaseComponent.toPlainText(read.getDisplayNameComponent()));

            List<BaseComponent[]> gotLore = read.getLoreComponents();
            assertNotNull(gotLore);
            assertEquals(2, gotLore.size());
            assertEquals("First Bungee Line", BaseComponent.toPlainText(gotLore.get(0)));
            gotLore.clear();
            assertEquals(2, read.getLoreComponents().size());

            // Empty / null semantics clear the same underlying fields.
            stack.editMeta(m -> {
                m.setDisplayNameComponent(new BaseComponent[0]);
                m.setLoreComponents(List.of());
            });
            assertFalse(stack.hasItemMeta());
            assertNull(stack.getItemMeta().getDisplayNameComponent());
            assertNull(stack.getItemMeta().getLoreComponents());

            stack.editMeta(m -> {
                m.customName(Component.text("ViaAdventure"));
                m.lore(List.of(Component.text("AdventureLore")));
            });
            assertEquals("ViaAdventure", BaseComponent.toPlainText(stack.getItemMeta().getDisplayNameComponent()));
            assertEquals(
                    "AdventureLore",
                    BaseComponent.toPlainText(stack.getItemMeta().getLoreComponents().get(0)));

            stack.editMeta(m -> {
                m.setDisplayNameComponent(null);
                m.setLoreComponents(null);
            });
            assertFalse(stack.hasItemMeta());
        }
    }

    @Test
    @SuppressWarnings("unchecked")
    void enchantmentGlintOverrideDistinguishesAbsentTrueAndFalse() {
        try (TestServer ignored = new TestServer()) {
            DataComponentType.Valued<Boolean> glintType = new DataComponentType.Valued<>() {
                @Override
                public NamespacedKey getKey() {
                    return NamespacedKey.minecraft("enchantment_glint_override");
                }

                @Override
                public boolean isPersistent() {
                    return true;
                }
            };

            ItemStack stack = new ItemStack(Material.STICK, 1);
            assertFalse(stack.getItemMeta().hasEnchantmentGlintOverride());
            assertThrows(
                    IllegalStateException.class,
                    () -> stack.getItemMeta().getEnchantmentGlintOverride());
            assertFalse(stack.hasData(glintType));
            assertNull(stack.getData(glintType));

            for (boolean expected : new boolean[] {true, false}) {
                stack.editMeta(m -> m.setEnchantmentGlintOverride(expected));
                assertTrue(stack.hasItemMeta());
                assertTrue(stack.getItemMeta().hasEnchantmentGlintOverride());
                assertEquals(expected, stack.getItemMeta().getEnchantmentGlintOverride());
                assertTrue(stack.hasData(glintType));
                assertEquals(expected, stack.getData(glintType));

                String nbt = FerriteItemNbt.encodeItemBase64(stack);
                assertNotNull(nbt);
                ItemStack decoded = FerriteItemNbt.decodeItemBase64(nbt, Material.STICK, 1);
                assertNotNull(decoded);
                assertTrue(decoded.getItemMeta().hasEnchantmentGlintOverride());
                assertEquals(expected, decoded.getItemMeta().getEnchantmentGlintOverride());

                FerriteItemMeta mapRestored = FerriteItemMeta.deserialize(stack.getItemMeta().serialize());
                assertTrue(mapRestored.hasEnchantmentGlintOverride());
                assertEquals(expected, mapRestored.getEnchantmentGlintOverride());
            }

            stack.unsetData(glintType);
            assertFalse(stack.hasItemMeta());
            assertFalse(stack.getItemMeta().hasEnchantmentGlintOverride());
            assertNull(FerriteItemNbt.encodeItemBase64(stack));

            stack.setData(glintType, Boolean.FALSE);
            assertTrue(stack.getItemMeta().hasEnchantmentGlintOverride());
            assertEquals(Boolean.FALSE, stack.getItemMeta().getEnchantmentGlintOverride());
        }
    }

    @Test
    void gliderAndRarityRoundTripDistinguishingAbsentAndPresent() {
        try (TestServer ignored = new TestServer()) {
            DataComponentType.NonValued gliderType = new DataComponentType.NonValued() {
                @Override
                public NamespacedKey getKey() {
                    return NamespacedKey.minecraft("glider");
                }

                @Override
                public boolean isPersistent() {
                    return true;
                }
            };
            DataComponentType.Valued<ItemRarity> rarityType = new DataComponentType.Valued<>() {
                @Override
                public NamespacedKey getKey() {
                    return NamespacedKey.minecraft("rarity");
                }

                @Override
                public boolean isPersistent() {
                    return true;
                }
            };

            ItemStack stack = new ItemStack(Material.LEATHER_CHESTPLATE, 1);
            assertFalse(stack.getItemMeta().isGlider());
            assertFalse(stack.getItemMeta().hasRarity());
            assertThrows(IllegalStateException.class, () -> stack.getItemMeta().getRarity());
            assertFalse(stack.hasData(gliderType));
            assertFalse(stack.hasData(rarityType));
            assertNull(stack.getData(rarityType));

            for (ItemRarity rarity : ItemRarity.values()) {
                stack.editMeta(m -> {
                    m.setGlider(true);
                    m.setRarity(rarity);
                });
                assertTrue(stack.hasItemMeta());
                assertTrue(stack.getItemMeta().isGlider());
                assertTrue(stack.getItemMeta().hasRarity());
                assertEquals(rarity, stack.getItemMeta().getRarity());
                assertTrue(stack.hasData(gliderType));
                assertTrue(stack.hasData(rarityType));
                assertEquals(rarity, stack.getData(rarityType));

                String nbt = FerriteItemNbt.encodeItemBase64(stack);
                assertNotNull(nbt);
                ItemStack decoded = FerriteItemNbt.decodeItemBase64(nbt, Material.LEATHER_CHESTPLATE, 1);
                assertNotNull(decoded);
                assertTrue(decoded.getItemMeta().isGlider());
                assertTrue(decoded.getItemMeta().hasRarity());
                assertEquals(rarity, decoded.getItemMeta().getRarity());

                ItemMeta cloned = stack.getItemMeta().clone();
                assertEquals(stack.getItemMeta(), cloned);
                FerriteItemMeta mapRestored = FerriteItemMeta.deserialize(stack.getItemMeta().serialize());
                assertEquals(stack.getItemMeta(), mapRestored);
            }

            stack.editMeta(m -> {
                m.setGlider(false);
                m.setRarity(null);
            });
            assertFalse(stack.hasItemMeta());
            assertFalse(stack.getItemMeta().isGlider());
            assertFalse(stack.getItemMeta().hasRarity());
            assertFalse(stack.hasData(gliderType));
            assertFalse(stack.hasData(rarityType));
            assertNull(FerriteItemNbt.encodeItemBase64(stack));

            stack.setData(gliderType);
            stack.setData(rarityType, ItemRarity.EPIC);
            assertTrue(stack.getItemMeta().isGlider());
            assertEquals(ItemRarity.EPIC, stack.getItemMeta().getRarity());
            stack.unsetData(gliderType);
            stack.unsetData(rarityType);
            assertFalse(stack.hasItemMeta());
        }
    }

    @Test
    @SuppressWarnings("unchecked")
    void damageResistantFireResistantUseRemainderAndRepairableRoundTrip() {
        try (TestServer ignored = new TestServer()) {
            DataComponentType.Valued<DamageResistant> dmgResType = new DataComponentType.Valued<>() {
                @Override
                public NamespacedKey getKey() {
                    return NamespacedKey.minecraft("damage_resistant");
                }

                @Override
                public boolean isPersistent() {
                    return true;
                }
            };
            DataComponentType.Valued<UseRemainder> remainderType = new DataComponentType.Valued<>() {
                @Override
                public NamespacedKey getKey() {
                    return NamespacedKey.minecraft("use_remainder");
                }

                @Override
                public boolean isPersistent() {
                    return true;
                }
            };
            DataComponentType.Valued<Repairable> repairableType = new DataComponentType.Valued<>() {
                @Override
                public NamespacedKey getKey() {
                    return NamespacedKey.minecraft("repairable");
                }

                @Override
                public boolean isPersistent() {
                    return true;
                }
            };

            ItemStack stack = new ItemStack(Material.MUSHROOM_STEW, 1);
            assertFalse(stack.getItemMeta().isFireResistant());
            assertFalse(stack.getItemMeta().hasDamageResistant());
            assertNull(stack.getItemMeta().getDamageResistant());
            assertNull(stack.getItemMeta().getDamageResistantTypes());
            assertFalse(stack.getItemMeta().hasUseRemainder());
            assertNull(stack.getItemMeta().getUseRemainder());
            assertFalse(stack.hasData(dmgResType));
            assertFalse(stack.hasData(remainderType));
            assertFalse(stack.hasData(repairableType));

            ItemStack bowl = new ItemStack(Material.BOWL, 1);
            bowl.editMeta(m -> m.customName(Component.text("Empty Bowl")));

            stack.editMeta(m -> {
                m.setFireResistant(true);
                m.setUseRemainder(bowl);
            });
            // Mutating caller stack after setUseRemainder must not affect stored remainder.
            bowl.editMeta(m -> m.customName(Component.text("MutatedCaller")));

            stack.setData(
                    repairableType,
                    FerriteDataComponents.repairable(
                            new FerriteKeySet<>(RegistryKey.ITEM, List.of("minecraft:oak_planks"))));

            ItemMeta meta = stack.getItemMeta();
            assertTrue(meta.isFireResistant());
            assertTrue(meta.hasDamageResistant());
            assertEquals(NamespacedKey.minecraft("is_fire"), meta.getDamageResistant().getKey());
            assertEquals(8, meta.getDamageResistantTypes().size());
            assertTrue(meta.hasUseRemainder());
            assertEquals("Empty Bowl", meta.getUseRemainder().getItemMeta().getDisplayName());
            assertTrue(stack.hasData(dmgResType));
            assertTrue(stack.hasData(remainderType));
            assertTrue(stack.hasData(repairableType));
            assertEquals(1, stack.getData(repairableType).types().size());

            // Mutating returned remainder must not affect stored remainder.
            ItemStack readRemainder = meta.getUseRemainder();
            readRemainder.editMeta(m -> m.customName(Component.text("MutatedRead")));
            assertEquals("Empty Bowl", stack.getItemMeta().getUseRemainder().getItemMeta().getDisplayName());

            // Round-trip through NBT base64 and Map serialization.
            String nbt = FerriteItemNbt.encodeItemBase64(stack);
            assertNotNull(nbt);
            ItemStack decoded = FerriteItemNbt.decodeItemBase64(nbt, Material.MUSHROOM_STEW, 1);
            assertNotNull(decoded);
            assertTrue(decoded.getItemMeta().isFireResistant());
            assertTrue(decoded.getItemMeta().hasDamageResistant());
            assertEquals(NamespacedKey.minecraft("is_fire"), decoded.getItemMeta().getDamageResistant().getKey());
            assertTrue(decoded.getItemMeta().hasUseRemainder());
            assertEquals("Empty Bowl", decoded.getItemMeta().getUseRemainder().getItemMeta().getDisplayName());
            assertTrue(decoded.hasData(repairableType));
            assertEquals(
                    "minecraft:oak_planks",
                    decoded.getData(repairableType).types().values().iterator().next().key().asString());

            FerriteItemMeta mapRestored = FerriteItemMeta.deserialize(stack.getItemMeta().serialize());
            assertEquals(stack.getItemMeta(), mapRestored);

            // Clearing restores absent state.
            stack.editMeta(m -> {
                m.setFireResistant(false);
                m.setUseRemainder(new ItemStack(Material.AIR, 1));
            });
            stack.unsetData(repairableType);
            assertFalse(stack.hasItemMeta());
            assertNull(FerriteItemNbt.encodeItemBase64(stack));
        }
    }

    @Test
    void unsupportedMethodsAndCorruptNbtFailExplicitly() {
        try (TestServer ignored = new TestServer()) {
            ItemStack stack = new ItemStack(Material.STONE, 1);
            ItemMeta meta = stack.getItemMeta();
            assertThrows(UnsupportedOperationException.class, meta::hasLocalizedName);
            assertThrows(UnsupportedOperationException.class, meta::getLocalizedName);
            assertThrows(UnsupportedOperationException.class, () -> meta.setLocalizedName("x"));
            assertThrows(UnsupportedOperationException.class, meta::hasDestroyableKeys);
            assertThrows(UnsupportedOperationException.class, meta::hasPlaceableKeys);

            assertThrows(
                    IllegalArgumentException.class,
                    () -> FerriteItemNbt.decodeItemBase64("not-base64!!!", Material.STONE, 1));
        }
    }
}
