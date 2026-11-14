package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.craftbukkit.util.CraftMagicNumbers;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** CraftMagicNumbers answers the Bukkit-expressible conversions and refuses the ones 26.3 has no value for. */
class CraftMagicNumbersTest {
    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
    }

    @Test void materialsAndBlockDataResolveByName() {
        assertEquals(Material.STONE, CraftMagicNumbers.getMaterial("minecraft:stone"));
        assertEquals(NamespacedKey.minecraft("stone"), CraftMagicNumbers.getKey(Material.STONE));
        var data = CraftMagicNumbers.getDefaultBlockData(Material.OAK_STAIRS);
        assertEquals(Material.OAK_STAIRS, CraftMagicNumbers.getMaterial(data));
        assertEquals(Material.OAK_STAIRS, CraftMagicNumbers.getBlockData(Material.OAK_STAIRS).getMaterial());
        assertThrows(IllegalArgumentException.class, () -> CraftMagicNumbers.getMaterial("minecraft:not_a_material"));
        assertThrows(IllegalArgumentException.class, () -> CraftMagicNumbers.getDefaultBlockData(Material.DIAMOND));
    }

    @Test void valuesWithoutA26_3FormFailExplicitly() {
        var data = CraftMagicNumbers.getDefaultBlockData(Material.STONE);
        var missing = assertThrows(UnsupportedOperationException.class, () -> CraftMagicNumbers.getBlockStateId(data));
        assertTrue(missing.getMessage().contains("block state"), missing.getMessage());
        assertThrows(UnsupportedOperationException.class, CraftMagicNumbers::getDataVersion);
    }
}
