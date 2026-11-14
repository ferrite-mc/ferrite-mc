package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import net.kyori.adventure.key.Key;
import org.bukkit.Material;
import org.bukkit.inventory.ItemStack;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** Every UnsafeValues member either works or names the Bukkit API a plugin should use instead. */
class UnsafeValuesContractTest {
    private final FerriteUnsafe unsafe = new FerriteUnsafe();

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
    }

    @Test void runtimeAdvancementRegistrationNamesTheDataPackPath() {
        var load = assertThrows(UnsupportedOperationException.class,
                () -> unsafe.loadAdvancement(Key.key("test", "pack"), "{}", false));
        assertTrue(load.getMessage().contains("data pack"), load.getMessage());
        assertTrue(load.getMessage().contains("reloadData"), load.getMessage());
        assertTrue(load.getMessage().contains("getAdvancement"), load.getMessage());

        var remove = assertThrows(UnsupportedOperationException.class,
                () -> unsafe.removeAdvancement(new org.bukkit.NamespacedKey("test", "pack")));
        assertTrue(remove.getMessage().contains("Server#getAdvancement"), remove.getMessage());
    }

    @Test void internalClassLookupNamesTheTypedRegistries() {
        var error = assertThrows(UnsupportedOperationException.class,
                () -> unsafe.get(Material.class, "DIAMOND"));
        assertTrue(error.getMessage().contains("Registry"), error.getMessage());
        assertTrue(error.getMessage().contains("Server#getAdvancement"), error.getMessage());
    }

    @Test void itemJsonNamesTheMapSerializationAlternative() {
        var stack = new ItemStack(Material.DIAMOND, 3);
        var error = assertThrows(UnsupportedOperationException.class, () -> unsafe.serializeItemAsJson(stack));
        assertTrue(error.getMessage().contains("ItemStack#serialize"), error.getMessage());

        // The map form the deprecated deserializeStack exists for still works.
        var copy = unsafe.deserializeStack(stack.serialize());
        assertEquals(Material.DIAMOND, copy.getType());
        assertEquals(3, copy.getAmount());
    }

    @Test void entitySerializationNamesTheSnapshotBoundary() {
        var entity = (org.bukkit.entity.Entity) java.lang.reflect.Proxy.newProxyInstance(
                getClass().getClassLoader(), new Class<?>[]{org.bukkit.entity.Entity.class},
                (proxy, method, args) -> { throw new UnsupportedOperationException(method.getName()); });
        var serialize = assertThrows(UnsupportedOperationException.class, () -> unsafe.serializeEntity(entity));
        assertTrue(serialize.getMessage().contains("createEntitySnapshot"), serialize.getMessage());

        var deserialize = assertThrows(UnsupportedOperationException.class,
                () -> unsafe.deserializeEntity(new byte[0], null, false, false));
        assertTrue(deserialize.getMessage().contains("EntitySnapshot#createEntity"), deserialize.getMessage());

        var next = assertThrows(UnsupportedOperationException.class, () -> unsafe.nextEntityId(null));
        assertTrue(next.getMessage().contains("Entity#getEntityId"), next.getMessage());
    }

    @Test void aHoverItemWithoutComponentNbtBecomesAStack() {
        var hover = net.kyori.adventure.text.event.HoverEvent.ShowItem.showItem(Key.key("minecraft", "diamond"), 2);
        var stack = unsafe.deserializeItemHover(hover);
        assertEquals(Material.DIAMOND, stack.getType());
        assertEquals(2, stack.getAmount());
    }

    @Test void theMainLevelIsTheVanillaDefaultWorld() {
        assertEquals("world", unsafe.getMainLevelName());
    }

}
