package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotSame;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.google.common.collect.LinkedHashMultimap;
import com.google.common.collect.Multimap;
import dev.ferrite.api.item.FerriteDataComponents;
import io.papermc.paper.datacomponent.DataComponentType;
import io.papermc.paper.datacomponent.item.Enchantable;
import io.papermc.paper.datacomponent.item.FoodProperties;
import io.papermc.paper.datacomponent.item.ItemAttributeModifiers;
import io.papermc.paper.datacomponent.item.ItemLore;
import io.papermc.paper.persistence.PersistentDataContainerView;
import java.util.List;
import java.util.Map;
import java.util.Set;
import net.kyori.adventure.text.Component;
import org.bukkit.Color;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.attribute.Attribute;
import org.bukkit.attribute.AttributeModifier;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.EquipmentSlotGroup;
import org.bukkit.inventory.ItemFlag;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.Damageable;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.inventory.meta.components.CustomModelDataComponent;
import org.bukkit.inventory.meta.components.FoodComponent;
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
        sword.setLore(List.of());
        assertNull(sword.getLore());
        assertFalse(sword.hasItemMeta());
        sword.lore(List.of(Component.text("again")));
        sword.lore(List.of());
        assertNull(sword.lore());
        assertFalse(sword.hasItemMeta());

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
                stack.editPersistentDataContainer(c -> c.set(
                        kStr,
                        PersistentDataType.LIST.listTypeFrom(PersistentDataType.LIST.strings()),
                        List.of(List.of("nested")))));
        assertThrows(UnsupportedOperationException.class, () ->
                stack.getItemMeta().getPersistentDataContainer().serializeToBytes());

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
        DataComponentType.Valued<Object> useCooldownType = valued("minecraft:use_cooldown");
        DataComponentType.NonValued unbreakableType = nonValued("minecraft:unbreakable");
        DataComponentType.NonValued gliderType = nonValued("minecraft:glider");

        stack.setData(customNameType, Component.text("Blade"));
        stack.setData(loreType, FerriteDataComponents.lore(List.of(Component.text("Line 1"))));
        stack.setData(damageType, 7);
        stack.setData(unbreakableType);

        assertTrue(stack.hasData(customNameType));
        assertTrue(stack.hasData(unbreakableType));
        assertTrue(stack.getItemMeta().isUnbreakable());
        assertEquals(Component.text("Blade"), stack.getData(customNameType));
        assertEquals(List.of(Component.text("Line 1")), stack.getData(loreType).lines());
        assertEquals(7, stack.getData(damageType));
        assertEquals(Set.of(customNameType, loreType, damageType, unbreakableType), stack.getDataTypes());

        ItemStack copy = new ItemStack(Material.DIAMOND_SWORD, 1);
        copy.copyDataFrom(stack, t -> t.equals(customNameType) || t.equals(loreType) || t.equals(unbreakableType));
        assertTrue(copy.hasData(customNameType));
        assertTrue(copy.hasData(unbreakableType));
        assertFalse(copy.hasData(damageType));
        assertTrue(stack.matchesWithoutData(copy, Set.of(damageType)));

        stack.unsetData(unbreakableType);
        assertFalse(stack.hasData(unbreakableType));
        assertFalse(stack.getItemMeta().isUnbreakable());

        UnsupportedOperationException cooldownEx =
                assertThrows(UnsupportedOperationException.class, () -> stack.getData(useCooldownType));
        assertTrue(cooldownEx.getMessage().contains("DIAMOND_SWORD"), cooldownEx.getMessage());
        assertTrue(cooldownEx.getMessage().contains("minecraft:use_cooldown"), cooldownEx.getMessage());

        UnsupportedOperationException gliderEx =
                assertThrows(UnsupportedOperationException.class, () -> stack.setData(gliderType));
        assertTrue(gliderEx.getMessage().contains("DIAMOND_SWORD"), gliderEx.getMessage());
        assertTrue(gliderEx.getMessage().contains("minecraft:glider"), gliderEx.getMessage());
    }

    @Test
    void unbreakableAndItemFlagsRoundTripWithCopyIsolationAndSerialization() {
        ItemStack stack = new ItemStack(Material.NETHERITE_PICKAXE, 1);
        assertFalse(stack.getItemMeta().isUnbreakable());
        assertTrue(stack.getItemFlags().isEmpty());
        assertFalse(stack.hasItemFlag(ItemFlag.HIDE_UNBREAKABLE));

        // Duplicate flags, multi-add, and remove semantics.
        stack.addItemFlags(ItemFlag.HIDE_UNBREAKABLE, ItemFlag.HIDE_ENCHANTS, ItemFlag.HIDE_UNBREAKABLE);
        assertEquals(Set.of(ItemFlag.HIDE_UNBREAKABLE, ItemFlag.HIDE_ENCHANTS), stack.getItemFlags());
        assertTrue(stack.hasItemFlag(ItemFlag.HIDE_UNBREAKABLE));
        assertTrue(stack.hasItemFlag(ItemFlag.HIDE_ENCHANTS));
        assertThrows(UnsupportedOperationException.class,
                () -> stack.getItemFlags().add(ItemFlag.HIDE_DYE));

        stack.editMeta(meta -> meta.setUnbreakable(true));
        assertTrue(stack.getItemMeta().isUnbreakable());
        assertTrue(stack.hasItemMeta());

        // Copy isolation between ItemStack and ItemMeta.
        ItemMeta detached = stack.getItemMeta();
        detached.setUnbreakable(false);
        detached.removeItemFlags(ItemFlag.HIDE_UNBREAKABLE);
        detached.addItemFlags(ItemFlag.HIDE_ATTRIBUTES);
        assertTrue(stack.getItemMeta().isUnbreakable());
        assertTrue(stack.hasItemFlag(ItemFlag.HIDE_UNBREAKABLE));
        assertFalse(stack.hasItemFlag(ItemFlag.HIDE_ATTRIBUTES));

        ItemStack cloned = stack.clone();
        cloned.removeItemFlags(ItemFlag.HIDE_ENCHANTS);
        cloned.editMeta(m -> m.setUnbreakable(false));
        assertTrue(stack.getItemMeta().isUnbreakable());
        assertTrue(stack.hasItemFlag(ItemFlag.HIDE_ENCHANTS));
        assertFalse(cloned.getItemMeta().isUnbreakable());
        assertEquals(Set.of(ItemFlag.HIDE_UNBREAKABLE), cloned.getItemFlags());

        // Serialization round trip preserves unbreakable + flags and DataComponent view.
        ItemStack restored = ItemStack.deserialize(stack.serialize());
        assertEquals(stack, restored);
        assertEquals(stack.hashCode(), restored.hashCode());
        assertTrue(restored.getItemMeta().isUnbreakable());
        assertEquals(Set.of(ItemFlag.HIDE_UNBREAKABLE, ItemFlag.HIDE_ENCHANTS), restored.getItemFlags());
        assertTrue(restored.hasData(nonValued("minecraft:unbreakable")));

        // Clearing unbreakable and flags normalizes meta back to empty.
        restored.removeItemFlags(ItemFlag.HIDE_UNBREAKABLE, ItemFlag.HIDE_ENCHANTS, ItemFlag.HIDE_DYE);
        restored.editMeta(m -> m.setUnbreakable(false));
        assertFalse(restored.hasItemMeta());
        assertTrue(restored.getItemFlags().isEmpty());

        // Unsupported data component interaction still fails explicitly even with flags/unbreakable set.
        DataComponentType.Valued<Object> tooltipDisplay = valued("minecraft:tooltip_display");
        UnsupportedOperationException tooltipEx =
                assertThrows(UnsupportedOperationException.class, () -> stack.getData(tooltipDisplay));
        assertTrue(tooltipEx.getMessage().contains("NETHERITE_PICKAXE"));
        assertTrue(tooltipEx.getMessage().contains("minecraft:tooltip_display"));
    }

    @Test
    void nestedPdcTagContainerArrayAndListsRoundTripWithIsolation() {
        ItemStack stack = new ItemStack(Material.CHEST, 1);
        NamespacedKey kNested = new NamespacedKey("ferrite", "nested");
        NamespacedKey kNestedArr = new NamespacedKey("ferrite", "nested_arr");
        NamespacedKey kListStr = new NamespacedKey("ferrite", "list_str");
        NamespacedKey kListContainers = new NamespacedKey("ferrite", "list_containers");
        NamespacedKey kInner = new NamespacedKey("ferrite", "inner");

        stack.editPersistentDataContainer(pdc -> {
            PersistentDataContainer child = pdc.getAdapterContext().newPersistentDataContainer();
            child.set(kInner, PersistentDataType.STRING, "child-val");
            pdc.set(kNested, PersistentDataType.TAG_CONTAINER, child);
            child.set(kInner, PersistentDataType.STRING, "mutated");

            PersistentDataContainer arrItem = pdc.getAdapterContext().newPersistentDataContainer();
            arrItem.set(kInner, PersistentDataType.INTEGER, 7);
            pdc.set(kNestedArr, PersistentDataType.TAG_CONTAINER_ARRAY,
                    new PersistentDataContainer[] {arrItem});

            pdc.set(kListStr, PersistentDataType.LIST.strings(), List.of("one", "two"));
            pdc.set(kListContainers, PersistentDataType.LIST.dataContainers(), List.of(arrItem));
        });

        PersistentDataContainer readChild =
                stack.getPersistentDataContainer().get(kNested, PersistentDataType.TAG_CONTAINER);
        assertEquals("child-val", readChild.get(kInner, PersistentDataType.STRING));
        readChild.set(kInner, PersistentDataType.STRING, "mutated-on-read");
        assertEquals(
                "child-val",
                stack.getPersistentDataContainer()
                        .get(kNested, PersistentDataType.TAG_CONTAINER)
                        .get(kInner, PersistentDataType.STRING));

        ItemStack restored = ItemStack.deserialize(stack.serialize());
        assertEquals(stack, restored);
        PersistentDataContainerView restoredView = restored.getPersistentDataContainer();
        assertEquals(
                "child-val",
                restoredView.get(kNested, PersistentDataType.TAG_CONTAINER)
                        .get(kInner, PersistentDataType.STRING));
        assertEquals(
                7,
                restoredView.get(kNestedArr, PersistentDataType.TAG_CONTAINER_ARRAY)[0]
                        .get(kInner, PersistentDataType.INTEGER));
        assertEquals(List.of("one", "two"), restoredView.get(kListStr, PersistentDataType.LIST.strings()));
        assertEquals(
                7,
                restoredView.get(kListContainers, PersistentDataType.LIST.dataContainers())
                        .get(0)
                        .get(kInner, PersistentDataType.INTEGER));
        assertEquals(PersistentDataType.TAG_CONTAINER, restoredView.getPrimitiveStorageType(kNested));
        org.bukkit.persistence.PersistentDataType<?, ?> listStorageType =
                restoredView.getPrimitiveStorageType(kListStr);
        assertTrue(listStorageType instanceof org.bukkit.persistence.ListPersistentDataType<?, ?>);
        assertEquals(
                PersistentDataType.STRING,
                ((org.bukkit.persistence.ListPersistentDataType<?, ?>) listStorageType).elementType());
    }

    @Test
    void foodComponentRoundTripsWithCopyIsolationValidationAndDataComponents() {
        ItemStack apple = new ItemStack(Material.APPLE, 3);
        // Reading getFood() on an item without food must not pollute hasItemMeta().
        FoodComponent detachedFood = apple.getItemMeta().getFood();
        detachedFood.setNutrition(6);
        assertFalse(apple.hasItemMeta());
        assertFalse(apple.getItemMeta().hasFood());

        apple.editMeta(meta -> {
            FoodComponent food = meta.getFood();
            food.setNutrition(8);
            food.setSaturation(4.5f);
            food.setCanAlwaysEat(true);
            meta.setFood(food);
            food.setNutrition(99);
        });
        assertEquals(8, apple.getItemMeta().getFood().getNutrition());
        assertEquals(4.5f, apple.getItemMeta().getFood().getSaturation());
        assertTrue(apple.getItemMeta().getFood().canAlwaysEat());

        DataComponentType.Valued<FoodProperties> foodType = valued("minecraft:food");
        assertTrue(apple.hasData(foodType));
        assertEquals(8, apple.getData(foodType).nutrition());

        ItemStack restored = ItemStack.deserialize(apple.serialize());
        assertEquals(apple, restored);
        assertEquals(8, restored.getItemMeta().getFood().getNutrition());
        assertEquals(4.5f, restored.getItemMeta().getFood().getSaturation());
        assertTrue(restored.getItemMeta().getFood().canAlwaysEat());

        assertThrows(IllegalArgumentException.class, () -> apple.getItemMeta().getFood().setNutrition(-1));
        assertThrows(IllegalArgumentException.class,
                () -> apple.getItemMeta().getFood().setSaturation(Float.NaN));

        restored.unsetData(foodType);
        assertFalse(restored.hasItemMeta());
    }

    @Test
    void itemModelTooltipStyleEnchantableAndMaxStackSizeRoundTrip() {
        ItemStack stack = new ItemStack(Material.ENDER_PEARL, 1);
        NamespacedKey modelKey = new NamespacedKey("ferrite", "ruby_pearl");
        NamespacedKey styleKey = new NamespacedKey("ferrite", "ruby_tooltip");

        stack.editMeta(meta -> {
            meta.setItemModel(modelKey);
            meta.setTooltipStyle(styleKey);
            meta.setEnchantable(15);
            meta.setMaxStackSize(32);
        });

        assertEquals(32, stack.getMaxStackSize());
        assertEquals(modelKey, stack.getItemMeta().getItemModel());
        assertEquals(styleKey, stack.getItemMeta().getTooltipStyle());
        assertEquals(15, stack.getItemMeta().getEnchantable());

        DataComponentType.Valued<net.kyori.adventure.key.Key> modelComp = valued("minecraft:item_model");
        DataComponentType.Valued<net.kyori.adventure.key.Key> styleComp = valued("minecraft:tooltip_style");
        DataComponentType.Valued<Enchantable> enchComp = valued("minecraft:enchantable");
        DataComponentType.Valued<Integer> maxStackComp = valued("minecraft:max_stack_size");

        assertEquals(modelKey, stack.getData(modelComp));
        assertEquals(styleKey, stack.getData(styleComp));
        assertEquals(15, stack.getData(enchComp).value());
        assertEquals(32, stack.getData(maxStackComp));

        ItemStack restored = ItemStack.deserialize(stack.serialize());
        assertEquals(stack, restored);
        assertEquals(32, restored.getMaxStackSize());
        assertEquals(modelKey, restored.getItemMeta().getItemModel());
        assertEquals(styleKey, restored.getItemMeta().getTooltipStyle());
        assertEquals(15, restored.getItemMeta().getEnchantable());

        assertThrows(IllegalArgumentException.class, () -> stack.editMeta(m -> m.setEnchantable(0)));
        assertThrows(IllegalArgumentException.class, () -> stack.editMeta(m -> m.setMaxStackSize(0)));
        assertThrows(IllegalArgumentException.class, () -> stack.editMeta(m -> m.setMaxStackSize(100)));

        restored.unsetData(modelComp);
        restored.unsetData(styleComp);
        restored.unsetData(enchComp);
        restored.unsetData(maxStackComp);
        assertFalse(restored.hasItemMeta());
        assertEquals(16, restored.getMaxStackSize());
    }

    @Test
    void attributeModifiersSupportCrudCopyIsolationAndExplicitRegistryRefusalOnDeserialize() {
        Attribute attackDamage = testAttribute("minecraft:attack_damage");
        Attribute armor = testAttribute("minecraft:armor");
        AttributeModifier swordDamage = new AttributeModifier(
                new NamespacedKey("ferrite", "sword_bonus"),
                5.0,
                AttributeModifier.Operation.ADD_NUMBER,
                EquipmentSlotGroup.MAINHAND);
        AttributeModifier armorBonus = new AttributeModifier(
                new NamespacedKey("ferrite", "armor_bonus"),
                2.0,
                AttributeModifier.Operation.ADD_NUMBER,
                EquipmentSlotGroup.CHEST);

        ItemStack stack = new ItemStack(Material.IRON_SWORD, 1);
        stack.editMeta(meta -> {
            assertTrue(meta.addAttributeModifier(attackDamage, swordDamage));
            assertFalse(meta.addAttributeModifier(attackDamage, swordDamage));
            assertTrue(meta.addAttributeModifier(armor, armorBonus));
        });

        assertTrue(stack.getItemMeta().hasAttributeModifiers());
        assertEquals(1, stack.getItemMeta().getAttributeModifiers(EquipmentSlot.HAND).size());
        assertEquals(1, stack.getItemMeta().getAttributeModifiers(EquipmentSlot.CHEST).size());
        assertEquals(List.of(swordDamage), List.copyOf(stack.getItemMeta().getAttributeModifiers(attackDamage)));

        // Copy isolation: mutating returned Multimap must not affect ItemStack.
        Multimap<Attribute, AttributeModifier> readMap = stack.getItemMeta().getAttributeModifiers();
        readMap.clear();
        assertEquals(2, stack.getItemMeta().getAttributeModifiers().size());

        // DataComponentHolder round trip.
        DataComponentType.Valued<ItemAttributeModifiers> attrComp = valued("minecraft:attribute_modifiers");
        assertTrue(stack.hasData(attrComp));
        assertEquals(2, stack.getData(attrComp).modifiers().size());

        ItemStack target = new ItemStack(Material.IRON_SWORD, 1);
        target.setData(attrComp, stack.getData(attrComp));
        assertEquals(stack, target);

        ItemStack copiedTarget = new ItemStack(Material.IRON_SWORD, 1);
        copiedTarget.copyDataFrom(target, t -> t.equals(attrComp));
        assertEquals(stack, copiedTarget);

        // Removing by slot, attribute, and modifier.
        target.editMeta(meta -> {
            assertTrue(meta.removeAttributeModifier(EquipmentSlot.CHEST));
            assertFalse(meta.hasAttributeModifiers() && meta.getAttributeModifiers(armor) != null);
            assertTrue(meta.removeAttributeModifier(attackDamage, swordDamage));
        });
        assertFalse(target.hasItemMeta());

        // Serializing attributes works, and deserializing with empty Registry.ATTRIBUTE fails explicitly.
        Map<String, Object> serialized = stack.serialize();
        assertThrows(UnsupportedOperationException.class, () -> ItemStack.deserialize(serialized));

        assertThrows(NullPointerException.class,
                () -> stack.editMeta(m -> m.addAttributeModifier(null, swordDamage)));
        assertThrows(NullPointerException.class,
                () -> stack.editMeta(m -> m.addAttributeModifier(attackDamage, null)));
        Multimap<Attribute, AttributeModifier> emptyMultimap = LinkedHashMultimap.create();
        stack.editMeta(m -> m.setAttributeModifiers(emptyMultimap));
        assertFalse(stack.hasItemMeta());
    }

    @Test
    void unsupportedItemMethodsFailExplicitly() {
        ItemStack stack = new ItemStack(Material.DIAMOND, 1);
        UnsupportedOperationException data =
                assertThrows(UnsupportedOperationException.class, stack::getData);
        assertTrue(data.getMessage().contains("ItemStack#getData"));
        assertThrows(UnsupportedOperationException.class, stack::serializeAsBytes);
        assertThrows(UnsupportedOperationException.class, stack::getRarity);
        assertThrows(UnsupportedOperationException.class, () -> stack.getItemMeta().isHideTooltip());
        assertThrows(UnsupportedOperationException.class, () -> stack.getItemMeta().getTool());
        assertThrows(UnsupportedOperationException.class, () -> stack.getItemMeta().getEquippable());
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

    private static Attribute testAttribute(String key) {
        NamespacedKey namespaced = NamespacedKey.fromString(key);
        return new Attribute() {
            @Override
            public NamespacedKey getKey() {
                return namespaced;
            }

            @Override
            public Attribute.Sentiment getSentiment() {
                return Attribute.Sentiment.POSITIVE;
            }

            @Override
            public double getDefaultValue() {
                return 0.0;
            }

            @Override
            public String name() {
                return namespaced.getKey();
            }

            @Override
            public int ordinal() {
                return 0;
            }

            @Override
            public String translationKey() {
                return "attribute.name." + namespaced.getKey();
            }

            @Override
            public String getTranslationKey() {
                return translationKey();
            }

            @Override
            public int compareTo(Attribute other) {
                return getKey().toString().compareTo(other.getKey().toString());
            }
        };
    }
}
