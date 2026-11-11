package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import org.bukkit.Material;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.inventory.ItemStack;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** Enchantment weight, cost, exclusivity and item support come from the data pack, not the host. */
class FerriteEnchantmentCostsTest {
    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        // Any host call these methods still made would throw: nothing answers here.
        HostBridge.transport = (owner, subject, method, args) -> null;
    }

    @org.junit.jupiter.api.AfterAll static void stop() { HostBridge.transport = null; }

    @Test void weightAnvilCostAndLinearCostsMatchTheSharpnessDefinition() {
        // data/minecraft/enchantment/sharpness.json: weight 10, anvil_cost 1,
        // min_cost 1 + 11 per level above first, max_cost 21 + 11 per level above first.
        var sharpness = Enchantment.SHARPNESS;
        assertEquals(10, sharpness.getWeight());
        assertEquals(1, sharpness.getAnvilCost());
        assertEquals(1, sharpness.getMinModifiedCost(1));
        assertEquals(1 + 11 * 4, sharpness.getMinModifiedCost(5));
        assertEquals(21, sharpness.getMaxModifiedCost(1));
        assertEquals(21 + 11 * 4, sharpness.getMaxModifiedCost(5));
    }

    @Test void rarityAndWeightAgree() {
        assertEquals(2, Enchantment.MENDING.getWeight());
        assertEquals(io.papermc.paper.enchantments.EnchantmentRarity.RARE, Enchantment.MENDING.getRarity());
        assertEquals(io.papermc.paper.enchantments.EnchantmentRarity.COMMON, Enchantment.SHARPNESS.getRarity());
    }

    @Test void conflictsFollowTheExclusiveSetAndAlwaysIncludeItself() {
        assertTrue(Enchantment.SHARPNESS.conflictsWith(Enchantment.SMITE));
        assertTrue(Enchantment.SMITE.conflictsWith(Enchantment.SHARPNESS));
        assertTrue(Enchantment.PROTECTION.conflictsWith(Enchantment.FIRE_PROTECTION));
        assertTrue(Enchantment.SHARPNESS.conflictsWith(Enchantment.SHARPNESS), "vanilla areCompatible rejects the same enchantment");
        assertFalse(Enchantment.SHARPNESS.conflictsWith(Enchantment.MENDING));
        assertFalse(Enchantment.MENDING.conflictsWith(Enchantment.UNBREAKING));
    }

    @Test void supportedItemsDecideWhatCanBeEnchanted() {
        assertTrue(Enchantment.SHARPNESS.canEnchantItem(new ItemStack(Material.DIAMOND_SWORD)));
        assertTrue(Enchantment.SHARPNESS.canEnchantItem(new ItemStack(Material.DIAMOND_AXE)));
        assertFalse(Enchantment.SHARPNESS.canEnchantItem(new ItemStack(Material.DIRT)));
        assertTrue(Enchantment.UNBREAKING.canEnchantItem(new ItemStack(Material.DIAMOND_PICKAXE)));
        assertFalse(Enchantment.UNBREAKING.canEnchantItem(new ItemStack(Material.DIRT)));
    }
}
