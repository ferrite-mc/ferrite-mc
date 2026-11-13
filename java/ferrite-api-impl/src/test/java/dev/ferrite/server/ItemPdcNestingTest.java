package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import org.bukkit.NamespacedKey;
import org.bukkit.persistence.PersistentDataType;
import org.junit.jupiter.api.Test;

/** Nested persistent data (containers, container arrays, typed lists) on items. */
class ItemPdcNestingTest {
    private static NamespacedKey key(String name) { return new NamespacedKey("ferrite", name); }

    private static FerriteDataContainer container(String text, int number) {
        var c = new FerriteDataContainer();
        c.set(key("text"), PersistentDataType.STRING, text);
        c.set(key("number"), PersistentDataType.INTEGER, number);
        return c;
    }

    @Test void aNestedContainerKeepsItsTypedValues() {
        var root = new FerriteDataContainer();
        root.set(key("inner"), PersistentDataType.TAG_CONTAINER, container("Grüße 日本語 😀", 7));
        var inner = root.get(key("inner"), PersistentDataType.TAG_CONTAINER);
        assertEquals("Grüße 日本語 😀", inner.get(key("text"), PersistentDataType.STRING));
        assertEquals(7, inner.get(key("number"), PersistentDataType.INTEGER));
        assertTrue(root.has(key("inner"), PersistentDataType.TAG_CONTAINER));
        assertFalse(root.has(key("inner"), PersistentDataType.TAG_CONTAINER_ARRAY));
    }

    @Test void containerArraysAndListsKeepOrderAndTypes() {
        var root = new FerriteDataContainer();
        root.set(key("array"), PersistentDataType.TAG_CONTAINER_ARRAY, new org.bukkit.persistence.PersistentDataContainer[]{container("a", 1), container("b", 2)});
        root.set(key("strings"), PersistentDataType.LIST.strings(), java.util.List.of("x", "ÿ", ""));
        root.set(key("ints"), PersistentDataType.LIST.integers(), java.util.List.of(3, -4));
        root.set(key("bools"), PersistentDataType.LIST.booleans(), java.util.List.of(true, false));
        root.set(key("empty"), PersistentDataType.LIST.longs(), java.util.List.of());
        root.set(key("containers"), PersistentDataType.LIST.dataContainers(), java.util.List.of(container("c", 3)));
        root.set(key("matrix"), PersistentDataType.LIST.listTypeFrom(PersistentDataType.LIST.integers()), java.util.List.of(java.util.List.of(1, 2), java.util.List.of()));
        var array = root.get(key("array"), PersistentDataType.TAG_CONTAINER_ARRAY);
        assertEquals(2, array.length);
        assertEquals("b", array[1].get(key("text"), PersistentDataType.STRING));
        assertEquals(java.util.List.of("x", "ÿ", ""), root.get(key("strings"), PersistentDataType.LIST.strings()));
        assertEquals(java.util.List.of(3, -4), root.get(key("ints"), PersistentDataType.LIST.integers()));
        assertEquals(java.util.List.of(true, false), root.get(key("bools"), PersistentDataType.LIST.booleans()));
        assertEquals(java.util.List.of(), root.get(key("empty"), PersistentDataType.LIST.longs()));
        assertEquals(1, root.get(key("containers"), PersistentDataType.LIST.dataContainers()).size());
        assertEquals(java.util.List.of(java.util.List.of(1, 2), java.util.List.of()), root.get(key("matrix"), PersistentDataType.LIST.listTypeFrom(PersistentDataType.LIST.integers())));
        // The element type is part of the type: strings are not ints.
        assertNull(root.get(key("strings"), PersistentDataType.LIST.integers()));
        assertFalse(root.has(key("ints"), PersistentDataType.LIST.strings()));
    }

    @Test void whatAPluginKeepsCannotChangeTheStoredValue() {
        var original = container("one", 1);
        var root = new FerriteDataContainer();
        root.set(key("inner"), PersistentDataType.TAG_CONTAINER, original);
        original.set(key("text"), PersistentDataType.STRING, "changed after set");
        var read = root.get(key("inner"), PersistentDataType.TAG_CONTAINER);
        assertEquals("one", read.get(key("text"), PersistentDataType.STRING), "set copies");
        read.set(key("text"), PersistentDataType.STRING, "changed after get");
        assertEquals("one", root.get(key("inner"), PersistentDataType.TAG_CONTAINER).get(key("text"), PersistentDataType.STRING), "get copies");
        var list = new java.util.ArrayList<>(java.util.List.of("a"));
        root.set(key("list"), PersistentDataType.LIST.strings(), list);
        list.add("b");
        assertEquals(java.util.List.of("a"), root.get(key("list"), PersistentDataType.LIST.strings()));
        var clone = root.copy();
        clone.set(key("inner"), PersistentDataType.TAG_CONTAINER, container("two", 2));
        assertEquals("one", root.get(key("inner"), PersistentDataType.TAG_CONTAINER).get(key("text"), PersistentDataType.STRING), "a clone is independent");
        assertFalse(root.sameAs(clone));
        assertTrue(root.sameAs(root.copy()));
    }

    @Test void nestedValuesSurviveTheByteFormat() throws Exception {
        var root = new FerriteDataContainer();
        root.set(key("inner"), PersistentDataType.TAG_CONTAINER, container("deep ✨", 9));
        root.set(key("array"), PersistentDataType.TAG_CONTAINER_ARRAY, new org.bukkit.persistence.PersistentDataContainer[]{container("a", 1)});
        root.set(key("matrix"), PersistentDataType.LIST.listTypeFrom(PersistentDataType.LIST.strings()), java.util.List.of(java.util.List.of("p", "q"), java.util.List.of()));
        root.set(key("plain"), PersistentDataType.LONG, 5L);
        var reloaded = new FerriteDataContainer();
        reloaded.readFromBytes(root.serializeToBytes(), true);
        assertTrue(root.sameAs(reloaded));
        assertEquals("deep ✨", reloaded.get(key("inner"), PersistentDataType.TAG_CONTAINER).get(key("text"), PersistentDataType.STRING));
        assertEquals(java.util.List.of(java.util.List.of("p", "q"), java.util.List.of()), reloaded.get(key("matrix"), PersistentDataType.LIST.listTypeFrom(PersistentDataType.LIST.strings())));
    }

    @Test void malformedBytesAreErrorsNotCrashes() throws Exception {
        var root = new FerriteDataContainer();
        root.set(key("inner"), PersistentDataType.TAG_CONTAINER, container("x", 1));
        root.set(key("list"), PersistentDataType.LIST.integers(), java.util.List.of(1, 2, 3));
        byte[] good = root.serializeToBytes();
        for (int cut = 0; cut < good.length; cut++) {
            var partial = java.util.Arrays.copyOf(good, cut);
            var target = new FerriteDataContainer();
            assertThrows(java.io.IOException.class, () -> target.readFromBytes(partial, true), "cut at " + cut);
            assertTrue(target.isEmpty(), "a failed read changes nothing");
        }
        var corrupt = good.clone();
        corrupt[corrupt.length - 1] ^= 0x7f; // still parses (a value byte) - must not throw weirdly
        new FerriteDataContainer().readFromBytes(corrupt, true);
    }

    @Test void nestingBeyondTheHostLimitIsRefused() {
        var level = container("leaf", 0);
        for (int i = 0; i < 40; i++) {
            var wrap = new FerriteDataContainer();
            try {
                wrap.set(key("n"), PersistentDataType.TAG_CONTAINER, level);
            } catch (IllegalArgumentException refused) {
                assertTrue(i >= 15, "refused only past the limit, at depth " + i);
                return;
            }
            level = wrap;
        }
        fail("40 levels of nesting were accepted");
    }

    @Test void snbtCarriesNestedTypesForTheHost() {
        var root = new FerriteDataContainer();
        root.set(key("inner"), PersistentDataType.TAG_CONTAINER, container("q\"uote", 4));
        root.set(key("ids"), PersistentDataType.LIST.integers(), java.util.List.of(1, 2));
        root.set(key("flags"), PersistentDataType.LIST.booleans(), java.util.List.of(true));
        root.set(key("array"), PersistentDataType.TAG_CONTAINER_ARRAY, new org.bukkit.persistence.PersistentDataContainer[]{container("a", 1)});
        var snbt = root.toSnbt();
        assertTrue(snbt.contains("\"ferrite:inner\":{type:\"TAG_CONTAINER\",value:{"), snbt);
        assertTrue(snbt.contains("\"ferrite:ids\":{type:\"LIST\",element:\"INTEGER\",value:[1,2]}"), snbt);
        assertTrue(snbt.contains("\"ferrite:flags\":{type:\"LIST\",element:\"BOOLEAN\",value:[1b]}"), snbt);
        assertTrue(snbt.contains("type:\"TAG_CONTAINER_ARRAY\",value:[{"), snbt);
        assertTrue(snbt.contains("q\\\"uote"), snbt);
    }

    @Test void nestedDataTravelsOnTheItemAndItsClone() {
        var item = new org.bukkit.inventory.ItemStack(org.bukkit.Material.DIAMOND_SWORD);
        var meta = item.getItemMeta();
        meta.getPersistentDataContainer().set(key("inner"), PersistentDataType.TAG_CONTAINER, container("kept", 1));
        item.setItemMeta(meta);
        var clone = item.clone();
        assertTrue(clone.isSimilar(item));
        var cloneMeta = clone.getItemMeta();
        cloneMeta.getPersistentDataContainer().set(key("inner"), PersistentDataType.TAG_CONTAINER, container("other", 2));
        clone.setItemMeta(cloneMeta);
        assertFalse(clone.isSimilar(item), "different nested values are different items");
        assertEquals("kept", item.getItemMeta().getPersistentDataContainer().get(key("inner"), PersistentDataType.TAG_CONTAINER).get(key("text"), PersistentDataType.STRING), "the original is untouched");
    }
}
