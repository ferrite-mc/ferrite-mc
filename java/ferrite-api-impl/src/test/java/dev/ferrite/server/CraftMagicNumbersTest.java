package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.craftbukkit.util.CraftMagicNumbers;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** CraftMagicNumbers answers every 26.3 conversion, and values with no form fail explicitly. */
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

    @Test void nmsBlockItemAndStateConversionsUseTheSameIdentity() {
        var block = CraftMagicNumbers.getBlock(Material.STONE);
        assertSame(Material.STONE, block.getBukkitMaterial());
        assertSame(Material.STONE, CraftMagicNumbers.getMaterial(block));

        var item = CraftMagicNumbers.getItem(Material.DIAMOND);
        assertSame(Material.DIAMOND, item.getBukkitMaterial());
        assertSame(Material.DIAMOND, CraftMagicNumbers.getMaterial(item));
        assertEquals(Material.DIAMOND.getMaxStackSize(), item.getDefaultMaxStackSize());
        assertSame(Material.DIAMOND, item.getDefaultInstance().toBukkit().getType());

        var data = CraftMagicNumbers.getDefaultBlockData(Material.OAK_STAIRS);
        var state = CraftMagicNumbers.getBlockState(data);
        assertEquals(data.getAsString(), state.getBukkitBlockData().getAsString());
        assertSame(Material.OAK_STAIRS, CraftMagicNumbers.getMaterial(state));

        assertThrows(IllegalArgumentException.class, () -> CraftMagicNumbers.getBlock(Material.DIAMOND));
        assertThrows(IllegalArgumentException.class, () -> CraftMagicNumbers.getItem(Material.WATER));
    }

    @Test void blockStateIdsAndDataVersionComeFromThe26_3Data() {
        assertEquals(1, CraftMagicNumbers.getBlockStateId(CraftMagicNumbers.getDefaultBlockData(Material.STONE)));
        var stairs = org.bukkit.Bukkit.createBlockData(
                "minecraft:oak_stairs[facing=north,half=top,shape=straight,waterlogged=true]");
        assertEquals(5463, CraftMagicNumbers.getBlockStateId(stairs));
        assertEquals(5463, CraftMagicNumbers.getBlockStateId(CraftMagicNumbers.getBlockState(stairs)));
        assertEquals(5023, CraftMagicNumbers.getDataVersion());
    }
}
