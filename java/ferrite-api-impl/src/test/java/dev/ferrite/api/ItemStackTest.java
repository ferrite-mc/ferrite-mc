package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotSame;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.item.FerriteDataComponents;
import io.papermc.paper.datacomponent.DataComponentType;
import io.papermc.paper.datacomponent.item.ItemLore;
import io.papermc.paper.persistence.PersistentDataContainerView;
import java.util.List;
import java.util.Map;
import java.util.Set;
import net.kyori.adventure.text.Component;
import org.bukkit.Color;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.inventory.ItemFlag;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.Damageable;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.inventory.meta.components.CustomModelDataComponent;
import org.bukkit.persistence.PersistentDataAdapterContext;
import org.bukkit.persistence.PersistentDataContainer;
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
        assertFalse(stack.isEmpty());

        assertTrue(ItemStack.empty().isEmpty());
        assertTrue(new ItemStack(Material.DIAMOND, 0).isEmpty());

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
    void getItemMetaDoesNotPolluteItemWithoutMetaAndUsesCopySemantics() {
        ItemStack plain = new ItemStack(Material.DIAMOND, 1);
        ItemMeta detached = plain.getItemMeta();
        detached.setDisplayName("Detached");
        detached.setLore(List.of("ignored"));
        assertFalse(plain.hasItemMeta(), "reading empty ItemMeta must not pollute original");
        assertEquals(new ItemStack(Material.DIAMOND, 1), plain);
        assertTrue(plain.isSimilar(new ItemStack(Material.DIAMOND, 4)));

        plain.setItemMeta(detached);
        assertTrue(plain.hasItemMeta());
        detached.setDisplayName("MutatedAfterSet");
        assertEquals("Detached", plain.getItemMeta().getDisplayName());

        ItemStack air = new ItemStack(Material.AIR, 1);
        assertNull(air.getItemMeta());
        assertFalse(air.setItemMeta(detached));
        assertFalse(air.hasItemMeta());
    }

    @Test
    void displayNameLoreDamageCustomModelDataAndPdcRoundTrip() {
        ItemStack stack = new ItemStack(Material.DIAMOND_SWORD, 1);
        ItemMeta meta = stack.getItemMeta();
        meta.setDisplayName("Ferrite Blade");
        meta.setLore(List.of("line one", "line two"));
        assertTrue(meta instanceof Damageable);
        Damageable damageable = (Damageable) meta;
        damageable.setDamage(15);
        damageable.setMaxDamage(250);

        meta.setCustomModelData(77);
        CustomModelDataComponent cmd = meta.getCustomModelDataComponent();
        cmd.setStrings(List.of("blade_v1"));
        cmd.setFlags(List.of(true));
        cmd.setColors(List.of(Color.AQUA));
        meta.setCustomModelDataComponent(cmd);

        NamespacedKey key = new NamespacedKey("ferrite", "greeting");
        meta.getPersistentDataContainer().set(key, PersistentDataType.STRING, "hello");
        stack.setItemMeta(meta);

        assertTrue(stack.hasItemMeta());
        assertEquals("Ferrite Blade", stack.getItemMeta().getDisplayName());
        assertEquals(Component.text("Ferrite Blade"), stack.getItemMeta().customName());
        assertEquals(List.of("line one", "line two"), stack.getLore());
        assertEquals((short) 15, stack.getDurability());
        assertEquals(15, ((Damageable) stack.getItemMeta()).getDamage());
        assertEquals(250, ((Damageable) stack.getItemMeta()).getMaxDamage());
        assertTrue(stack.getItemMeta().hasCustomModelData());
        assertEquals(77, stack.getItemMeta().getCustomModelData());
        assertEquals(List.of("blade_v1"), stack.getItemMeta().getCustomModelDataComponent().getStrings());
        assertEquals(List.of(true), stack.getItemMeta().getCustomModelDataComponent().getFlags());
        assertEquals(List.of(Color.AQUA), stack.getItemMeta().getCustomModelDataComponent().getColors());
        assertEquals("hello", stack.getItemMeta().getPersistentDataContainer()
                .get(key, PersistentDataType.STRING));
        assertEquals("hello", stack.getPersistentDataContainer().get(key, PersistentDataType.STRING));
        assertNull(stack.getItemMeta().getPersistentDataContainer()
                .get(key, PersistentDataType.INTEGER), "type mismatch must not return a value");

        ItemStack clone = stack.clone();
        clone.getItemMeta().setDisplayName("Changed");
        assertEquals("Ferrite Blade", clone.getItemMeta().getDisplayName());
        assertEquals("Ferrite Blade", stack.getItemMeta().getDisplayName());

        clone.editMeta(m -> m.setDisplayName("ChangedViaEdit"));
        assertEquals("ChangedViaEdit", clone.getItemMeta().getDisplayName());
        assertEquals("Ferrite Blade", stack.getItemMeta().getDisplayName());

        PersistentDataContainerView view = stack.getPersistentDataContainer();
        assertFalse(view instanceof PersistentDataContainer,
                "getPersistentDataContainer() view must not expose mutable PersistentDataContainer");
    }

    @Test
    void durabilityAndLoreHelpersWorkWithoutSilentLoss() {
        ItemStack sword = new ItemStack(Material.IRON_SWORD, 1, (short) 9);
        assertEquals((short) 9, sword.getDurability());
        sword.setDurability((short) 24);
        assertEquals((short) 24, sword.getDurability());
        sword.setDurability((short) 0);
        assertEquals((short) 0, sword.getDurability());
        assertFalse(sword.hasItemMeta());

        sword.setLore(List.of("first", "second"));
        assertEquals(List.of("first", "second"), sword.getLore());
        sword.lore(List.of(Component.text("adventure")));
        assertEquals(List.of(Component.text("adventure")), sword.lore());

        assertThrows(IllegalArgumentException.class, () -> sword.setDurability((short) -1));
        assertThrows(IllegalArgumentException.class, () -> new ItemStack(Material.IRON_SWORD, 1, (short) -1));
        assertThrows(UnsupportedOperationException.class,
                () -> new ItemStack(Material.IRON_SWORD, 1, (short) 0, (byte) 1));
    }

    @Test
    void supportedPdcTypesRoundTripAndUnsupportedPdcTypesFailExplicitly() {
        ItemStack stack = new ItemStack(Material.PAPER, 1);
        NamespacedKey kByte = new NamespacedKey("ferrite", "byte");
        NamespacedKey kShort = new NamespacedKey("ferrite", "short");
        NamespacedKey kInt = new NamespacedKey("ferrite", "int");
        NamespacedKey kLong = new NamespacedKey("ferrite", "long");
        NamespacedKey kFloat = new NamespacedKey("ferrite", "float");
        NamespacedKey kDouble = new NamespacedKey("ferrite", "double");
        NamespacedKey kBool = new NamespacedKey("ferrite", "bool");
        NamespacedKey kStr = new NamespacedKey("ferrite", "str");
        NamespacedKey kBytes = new NamespacedKey("ferrite", "bytes");
        NamespacedKey kInts = new NamespacedKey("ferrite", "ints");
        NamespacedKey kLongs = new NamespacedKey("ferrite", "longs");

        byte[] sourceBytes = new byte[] {1, 2, 3};
        int[] sourceInts = new int[] {10, 20};
        long[] sourceLongs = new long[] {100L, 200L};

        stack.editPersistentDataContainer(pdc -> {
            pdc.set(kByte, PersistentDataType.BYTE, (byte) 7);
            pdc.set(kShort, PersistentDataType.SHORT, (short) 300);
            pdc.set(kInt, PersistentDataType.INTEGER, 42);
            pdc.set(kLong, PersistentDataType.LONG, 9999999999L);
            pdc.set(kFloat, PersistentDataType.FLOAT, 1.5f);
            pdc.set(kDouble, PersistentDataType.DOUBLE, 3.25d);
            pdc.set(kBool, PersistentDataType.BOOLEAN, true);
            pdc.set(kStr, PersistentDataType.STRING, "val");
            pdc.set(kBytes, PersistentDataType.BYTE_ARRAY, sourceBytes);
            pdc.set(kInts, PersistentDataType.INTEGER_ARRAY, sourceInts);
            pdc.set(kLongs, PersistentDataType.LONG_ARRAY, sourceLongs);
        });

        // Mutating caller arrays must not affect stored PDC arrays.
        sourceBytes[0] = 99;
        sourceInts[0] = 99;
        sourceLongs[0] = 99L;

        ItemStack restored = ItemStack.deserialize(stack.serialize());
        assertEquals(stack, restored);
        PersistentDataContainerView pdc = restored.getPersistentDataContainer();
        assertEquals((byte) 7, pdc.get(kByte, PersistentDataType.BYTE));
        assertEquals((short) 300, pdc.get(kShort, PersistentDataType.SHORT));
        assertEquals(42, pdc.get(kInt, PersistentDataType.INTEGER));
        assertEquals(9999999999L, pdc.get(kLong, PersistentDataType.LONG));
        assertEquals(1.5f, pdc.get(kFloat, PersistentDataType.FLOAT));
        assertEquals(3.25d, pdc.get(kDouble, PersistentDataType.DOUBLE));
        assertEquals(true, pdc.get(kBool, PersistentDataType.BOOLEAN));
        assertEquals("val", pdc.get(kStr, PersistentDataType.STRING));
        assertArrayEquals(new byte[] {1, 2, 3}, pdc.get(kBytes, PersistentDataType.BYTE_ARRAY));
        assertArrayEquals(new int[] {10, 20}, pdc.get(kInts, PersistentDataType.INTEGER_ARRAY));
        assertArrayEquals(new long[] {100L, 200L}, pdc.get(kLongs, PersistentDataType.LONG_ARRAY));

        PersistentDataType<String, List<String>> customType = new PersistentDataType<>() {
            @Override
            public Class<String> getPrimitiveType() {
                return String.class;
            }

            @Override
            @SuppressWarnings("unchecked")
            public Class<List<String>> getComplexType() {
                return (Class<List<String>>) (Class<?>) List.class;
            }

            @Override
            public String toPrimitive(List<String> complex, PersistentDataAdapterContext context) {
                return String.join(",", complex);
            }

            @Override
            public List<String> fromPrimitive(String primitive, PersistentDataAdapterContext context) {
                return List.of(primitive.split(","));
            }
        };

        assertThrows(UnsupportedOperationException.class, () ->
                stack.editPersistentDataContainer(c -> c.set(kStr, customType, List.of("a", "b"))));
        assertThrows(UnsupportedOperationException.class, () ->
                stack.editPersistentDataContainer(c -> c.set(kStr, PersistentDataType.TAG_CONTAINER, null)));
        assertThrows(UnsupportedOperationException.class, () ->
                stack.editPersistentDataContainer(c -> c.set(kStr, PersistentDataType.LIST.strings(), List.of("a"))));

        assertEquals(PersistentDataType.BYTE, pdc.getPrimitiveStorageType(kBool));
        assertEquals(PersistentDataType.INTEGER, pdc.getPrimitiveStorageType(kInt));
        assertEquals(11, pdc.getSize());
        assertEquals(11, pdc.getKeys().size());

        ItemStack copyTarget = new ItemStack(Material.PAPER, 1);
        copyTarget.editPersistentDataContainer(target -> {
            target.set(kStr, PersistentDataType.STRING, "existing");
            pdc.copyTo(target, false);
        });
        assertEquals("existing", copyTarget.getPersistentDataContainer().get(kStr, PersistentDataType.STRING));
        assertEquals(42, copyTarget.getPersistentDataContainer().get(kInt, PersistentDataType.INTEGER));
        copyTarget.editPersistentDataContainer(target -> pdc.copyTo(target, true));
        assertEquals("val", copyTarget.getPersistentDataContainer().get(kStr, PersistentDataType.STRING));
    }

    @Test
    void serializationRoundTripsTheSupportedSlice() {
        ItemStack stack = new ItemStack(Material.DIAMOND_PICKAXE, 1);
        ItemMeta meta = stack.getItemMeta();
        meta.setDisplayName("Shiny");
        meta.setLore(List.of("lore"));
        ((Damageable) meta).setDamage(12);
        ((Damageable) meta).setMaxDamage(1561);
        meta.setCustomModelData(123);
        meta.getPersistentDataContainer().set(
                new NamespacedKey("ferrite", "tag"), PersistentDataType.INTEGER, 99);
        stack.setItemMeta(meta);

        ItemStack restored = ItemStack.deserialize(stack.serialize());
        assertEquals(stack.getType(), restored.getType());
        assertEquals(stack.getAmount(), restored.getAmount());
        assertEquals("Shiny", restored.getItemMeta().getDisplayName());
        assertEquals(List.of("lore"), restored.getLore());
        assertEquals(12, ((Damageable) restored.getItemMeta()).getDamage());
        assertEquals(1561, ((Damageable) restored.getItemMeta()).getMaxDamage());
        assertEquals(123, restored.getItemMeta().getCustomModelData());
        assertEquals(99, restored.getPersistentDataContainer().get(
                new NamespacedKey("ferrite", "tag"), PersistentDataType.INTEGER));
        assertEquals(stack, restored);
    }

    @Test
    void dataComponentsSupportSliceAndRejectUnsupportedComponentsWithItemAndKey() {
        ItemStack stack = new ItemStack(Material.DIAMOND_SWORD, 1);
        DataComponentType.Valued<Component> customNameType = valued("minecraft:custom_name");
        DataComponentType.Valued<ItemLore> loreType = valued("minecraft:lore");
        DataComponentType.Valued<Integer> damageType = valued("minecraft:damage");
        DataComponentType.Valued<Object> foodType = valued("minecraft:food");
        DataComponentType.NonValued unbreakableType = nonValued("minecraft:unbreakable");

        stack.setData(customNameType, Component.text("Blade"));
        stack.setData(loreType, FerriteDataComponents.lore(List.of(Component.text("Line 1"))));
        stack.setData(damageType, 7);

        assertTrue(stack.hasData(customNameType));
        assertEquals(Component.text("Blade"), stack.getData(customNameType));
        assertEquals(List.of(Component.text("Line 1")), stack.getData(loreType).lines());
        assertEquals(7, stack.getData(damageType));
        assertEquals(Set.of(customNameType, loreType, damageType), stack.getDataTypes());

        ItemStack copy = new ItemStack(Material.DIAMOND_SWORD, 1);
        copy.copyDataFrom(stack, t -> t.equals(customNameType) || t.equals(loreType));
        assertTrue(copy.hasData(customNameType));
        assertFalse(copy.hasData(damageType));
        assertTrue(stack.matchesWithoutData(copy, Set.of(damageType)));

        UnsupportedOperationException foodEx =
                assertThrows(UnsupportedOperationException.class, () -> stack.getData(foodType));
        assertTrue(foodEx.getMessage().contains("DIAMOND_SWORD"), foodEx.getMessage());
        assertTrue(foodEx.getMessage().contains("minecraft:food"), foodEx.getMessage());

        UnsupportedOperationException unbreakableEx =
                assertThrows(UnsupportedOperationException.class, () -> stack.setData(unbreakableType));
        assertTrue(unbreakableEx.getMessage().contains("DIAMOND_SWORD"), unbreakableEx.getMessage());
        assertTrue(unbreakableEx.getMessage().contains("minecraft:unbreakable"), unbreakableEx.getMessage());
    }

    @Test
    void unsupportedItemMethodsFailExplicitly() {
        ItemStack stack = new ItemStack(Material.DIAMOND, 1);
        UnsupportedOperationException data =
                assertThrows(UnsupportedOperationException.class, stack::getData);
        assertTrue(data.getMessage().contains("ItemStack#getData"));
        assertThrows(UnsupportedOperationException.class, stack::serializeAsBytes);
        assertThrows(UnsupportedOperationException.class, () -> stack.addItemFlags(ItemFlag.HIDE_ENCHANTS));
        assertThrows(UnsupportedOperationException.class, () -> stack.getItemMeta().isUnbreakable());
        assertThrows(UnsupportedOperationException.class, () -> stack.getItemMeta().getAttributeModifiers());
        assertTrue(stack.getEnchantments().isEmpty());
        assertThrows(UnsupportedOperationException.class, () ->
                dev.ferrite.api.item.FerriteItemMeta.deserialize(
                        Map.of("enchants", Map.of("minecraft:sharpness", 5))));
        assertThrows(NullPointerException.class, () -> stack.containsEnchantment(null));
        assertThrows(NullPointerException.class, () -> stack.addUnsafeEnchantment(null, 1));
        assertThrows(NullPointerException.class, () -> stack.removeEnchantment(null));
        assertThrows(NullPointerException.class, () -> stack.getItemMeta().hasEnchant(null));
    }

    private static <T> DataComponentType.Valued<T> valued(String key) {
        NamespacedKey namespaced = NamespacedKey.fromString(key);
        return new DataComponentType.Valued<>() {
            @Override
            public NamespacedKey getKey() {
                return namespaced;
            }

            @Override
            public boolean isPersistent() {
                return true;
            }
        };
    }

    private static DataComponentType.NonValued nonValued(String key) {
        NamespacedKey namespaced = NamespacedKey.fromString(key);
        return new DataComponentType.NonValued() {
            @Override
            public NamespacedKey getKey() {
                return namespaced;
            }

            @Override
            public boolean isPersistent() {
                return true;
            }
        };
    }
}
