package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.List;
import org.bukkit.Material;
import org.bukkit.inventory.ItemStack;
import org.junit.jupiter.api.Test;

class FerriteLootTableTest {
    @Test void decodesNamespacedItemLines() {
        List<ItemStack> stacks = FerriteLootTable.decode("minecraft:cobblestone 2\nminecraft:bone 1");
        assertEquals(2, stacks.size());
        assertEquals(Material.COBBLESTONE, stacks.get(0).getType());
        assertEquals(2, stacks.get(0).getAmount());
        assertEquals(Material.BONE, stacks.get(1).getType());
    }

    @Test void skipsUnknownItemsAndMalformedLines() {
        assertTrue(FerriteLootTable.decode("minecraft:not_an_item 1\nbroken\nminecraft:bone x\nminecraft:bone 0").isEmpty());
    }

    @Test void aNullOrEmptyReplyIsNoDrops() {
        assertTrue(FerriteLootTable.decode(null).isEmpty());
        assertTrue(FerriteLootTable.decode("").isEmpty());
    }
}
