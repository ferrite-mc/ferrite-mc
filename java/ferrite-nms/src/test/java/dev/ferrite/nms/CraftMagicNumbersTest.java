// Clean-room Ferrite NMS test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.test.FakeNmsHost;
import java.lang.reflect.Proxy;
import net.minecraft.world.level.block.state.BlockState;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.block.BlockType;
import org.bukkit.block.data.BlockData;
import org.bukkit.craftbukkit.util.CraftMagicNumbers;
import org.bukkit.inventory.ItemType;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class CraftMagicNumbersTest {
    private FakeNmsHost host;

    @BeforeEach
    void installFakeHost() {
        host = new FakeNmsHost();
        NmsHostBinding.install(host);
    }

    @AfterEach
    void removeFakeHost() {
        try {
            NmsHostBinding.uninstall(host);
        } catch (IllegalStateException ignored) {
            // Deliberately uninstalled by a test.
        }
        host.close();
    }

    @Test
    void materialsAndKeysResolveByName() {
        assertEquals(Material.STONE, CraftMagicNumbers.getMaterial("minecraft:stone"));
        assertEquals(NamespacedKey.minecraft("stone"), CraftMagicNumbers.getKey(Material.STONE));
        assertEquals(Material.STONE, CraftMagicNumbers.getMaterial("STONE"));
        IllegalArgumentException error = assertThrows(IllegalArgumentException.class,
                () -> CraftMagicNumbers.getMaterial("minecraft:not_a_material"));
        assertTrue(error.getMessage().contains("not_a_material"), error.getMessage());
    }

    @Test
    void blockItemAndStateConversionsKeepOneIdentity() {
        assertEquals(Material.STONE, CraftMagicNumbers.getBlock(Material.STONE).getBukkitMaterial());
        assertEquals(Material.STONE, CraftMagicNumbers.getMaterial(CraftMagicNumbers.getBlock(Material.STONE)));
        assertEquals(Material.DIAMOND, CraftMagicNumbers.getItem(Material.DIAMOND).getBukkitMaterial());
        assertEquals(Material.DIAMOND, CraftMagicNumbers.getMaterial(CraftMagicNumbers.getItem(Material.DIAMOND)));

        assertThrows(IllegalArgumentException.class, () -> CraftMagicNumbers.getBlock(Material.DIAMOND));
        assertThrows(IllegalArgumentException.class, () -> CraftMagicNumbers.getItem(Material.WATER));
    }

    @Test
    void blockStateAndGlobalIdsComeFromTheHostRegistry() {
        BlockData data = blockData(Material.OAK_STAIRS,
                "minecraft:oak_stairs[facing=north,half=top,shape=straight,waterlogged=true]");
        BlockState state = CraftMagicNumbers.getBlockState(data);
        assertEquals(Material.OAK_STAIRS, state.getBukkitMaterial());
        assertEquals(data.getAsString(), state.getAsString());
        assertEquals(5463, CraftMagicNumbers.getBlockStateId(data));
        assertEquals(5463, CraftMagicNumbers.getBlockStateId(state));
        assertEquals(5023, CraftMagicNumbers.getDataVersion());
    }

    @Test
    void anUnknownBlockStateIsNotSilentlyReplacedByAir() {
        assertThrows(dev.ferrite.nms.host.NmsHostException.class,
                () -> CraftMagicNumbers.getBlockStateId(
                        blockData(Material.STONE, "minecraft:stone[not_a_property=true]")));
    }

    @Test
    void instanceAndTypeConversionsCoverTheRegistrySurface() {
        assertNotNull(CraftMagicNumbers.INSTANCE);

        ItemType diamond = CraftMagicNumbers.getItemType(Material.DIAMOND);
        assertNotNull(diamond);
        assertEquals(Material.DIAMOND, CraftMagicNumbers.getMaterial(diamond));

        BlockType stone = CraftMagicNumbers.getBlockType(Material.STONE);
        assertNotNull(stone);
        assertEquals(Material.STONE, CraftMagicNumbers.getMaterial(stone));

        assertThrows(IllegalArgumentException.class,
                () -> CraftMagicNumbers.getItemType(Material.WATER));
        assertThrows(IllegalArgumentException.class,
                () -> CraftMagicNumbers.getBlockType(Material.DIAMOND));
        assertEquals(5023, CraftMagicNumbers.INSTANCE.getDataVersion());
    }

    @Test
    void defaultBlockDataFailsExplicitlyWithoutABukkitServer() {
        // Paper's Material.createBlockData delegates to Bukkit.getServer(), which unit tests do
        // not have. Pin that this is a named capability failure, not a fake AIR block.
        IllegalStateException error = assertThrows(IllegalStateException.class,
                () -> CraftMagicNumbers.getDefaultBlockData(Material.STONE));
        assertTrue(error.getMessage().contains("BlockData factory"), error.getMessage());
        assertThrows(IllegalStateException.class,
                () -> CraftMagicNumbers.getDefaultBlockState(Material.STONE));

        assertThrows(IllegalArgumentException.class,
                () -> CraftMagicNumbers.getDefaultBlockData(Material.DIAMOND));
    }

    @Test
    void blockStateStringsResolveThroughTheSameHostRegistry() {
        assertEquals(1, CraftMagicNumbers.getBlockStateId("minecraft:stone"));
        assertThrows(dev.ferrite.nms.host.NmsHostException.class,
                () -> CraftMagicNumbers.getBlockStateId("minecraft:not_a_state"));
        assertThrows(IllegalArgumentException.class,
                () -> CraftMagicNumbers.getBlockStateId("  "));
    }

    @Test
    void legacyConversionIsExplicitlyRefused() {
        UnsupportedOperationException error = assertThrows(UnsupportedOperationException.class,
                () -> CraftMagicNumbers.fromLegacy(Material.STONE));
        assertTrue(error.getMessage().contains("CraftMagicNumbers#fromLegacy"), error.getMessage());
        assertTrue(error.getMessage().contains("UnsafeValues"), error.getMessage());

        UnsupportedOperationException dataError = assertThrows(UnsupportedOperationException.class,
                () -> CraftMagicNumbers.fromLegacy(Material.STONE, (byte) 1));
        assertTrue(dataError.getMessage().contains("CraftMagicNumbers#fromLegacy"), dataError.getMessage());

        UnsupportedOperationException idError = assertThrows(UnsupportedOperationException.class,
                () -> CraftMagicNumbers.getMaterial(1));
        assertTrue(idError.getMessage().contains("CraftMagicNumbers#getMaterial(int)"), idError.getMessage());
        assertTrue(idError.getMessage().contains("namespaced key"), idError.getMessage());
    }

    @Test
    void registryReadsFailWhenNoHostIsAttached() {
        NmsHostBinding.uninstall(host);
        IllegalStateException error =
                assertThrows(IllegalStateException.class, CraftMagicNumbers::getDataVersion);
        assertTrue(error.getMessage().contains("not attached"), error.getMessage());
        assertFalse(error.getMessage().isBlank());
    }

    private static BlockData blockData(Material material, String text) {
        return (BlockData) Proxy.newProxyInstance(
                CraftMagicNumbersTest.class.getClassLoader(),
                new Class<?>[] {BlockData.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "getMaterial" -> material;
                    case "getAsString" -> text;
                    case "clone" -> proxy;
                    case "toString" -> text;
                    case "hashCode" -> System.identityHashCode(proxy);
                    case "equals" -> proxy == args[0];
                    default -> method.isDefault()
                            ? java.lang.reflect.InvocationHandler.invokeDefault(proxy, method, args)
                            : null;
                });
    }
}
