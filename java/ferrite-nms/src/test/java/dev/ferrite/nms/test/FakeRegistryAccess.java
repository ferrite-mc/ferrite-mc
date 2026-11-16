// Clean-room Ferrite NMS test support. No Mojang code or assets.
package dev.ferrite.nms.test;

import io.papermc.paper.registry.RegistryAccess;
import io.papermc.paper.registry.RegistryKey;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.util.Collections;
import java.util.EnumSet;
import java.util.Locale;
import java.util.Set;
import org.bukkit.Keyed;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.block.BlockType;
import org.bukkit.inventory.ItemType;

/**
 * Test-only Paper registry so {@code Material.isBlock()/isItem()} works without Lane 2's
 * server. It knows only the few materials the tests distinguish, and it never creates a Paper
 * ItemStack; production code must use Lane 2's real registry.
 */
public final class FakeRegistryAccess implements RegistryAccess {
    private static final Set<Material> NON_ITEMS =
            EnumSet.of(Material.WATER, Material.LAVA, Material.FIRE);
    private static final Set<Material> NON_BLOCKS =
            EnumSet.of(Material.DIAMOND, Material.APPLE, Material.STICK, Material.IRON_INGOT, Material.DIAMOND_SWORD);

    @Override
    @SuppressWarnings({"unchecked", "removal"})
    public <T extends Keyed> Registry<T> getRegistry(Class<T> type) {
        return (Registry<T>) registry(type);
    }

    @Override
    @SuppressWarnings("unchecked")
    public <T extends Keyed> Registry<T> getRegistry(RegistryKey<T> key) {
        String id = key.key().asString();
        if ("minecraft:item".equals(id)) {
            return (Registry<T>) registry(ItemType.class);
        }
        if ("minecraft:block".equals(id)) {
            return (Registry<T>) registry(BlockType.class);
        }
        return (Registry<T>) registry(Keyed.class);
    }

    private static Registry<?> registry(Class<?> elementType) {
        return (Registry<?>) Proxy.newProxyInstance(
                FakeRegistryAccess.class.getClassLoader(),
                new Class<?>[] {Registry.class},
                (proxy, method, args) -> {
                    switch (method.getName()) {
                        case "get":
                            return element(elementType, (NamespacedKey) args[0]);
                        case "iterator":
                            return Collections.emptyIterator();
                        case "toString":
                            return "FakeRegistry<" + elementType.getSimpleName() + ">";
                        case "hashCode":
                            return System.identityHashCode(proxy);
                        case "equals":
                            return proxy == args[0];
                        default:
                            return defaultValue(proxy, method, args);
                    }
                });
    }

    private static Object element(Class<?> elementType, NamespacedKey key) {
        Material material = materialFor(key);
        if (elementType == Material.class) {
            return material;
        }
        if (elementType == ItemType.class) {
            if (material != null && NON_ITEMS.contains(material)) {
                return null;
            }
            return typedProxy(ItemType.class, key, material);
        }
        if (elementType == BlockType.class) {
            if (material != null && NON_BLOCKS.contains(material)) {
                return null;
            }
            return typedProxy(BlockType.class, key, material);
        }
        return typedProxy(elementType, key, material);
    }

    private static Material materialFor(NamespacedKey key) {
        if (!"minecraft".equals(key.getNamespace())) {
            return null;
        }
        try {
            return Material.valueOf(key.getKey().toUpperCase(Locale.ROOT));
        } catch (IllegalArgumentException unknown) {
            return null;
        }
    }

    private static Object typedProxy(Class<?> type, NamespacedKey key, Material material) {
        if (!type.isInterface()) {
            return null;
        }
        return Proxy.newProxyInstance(
                FakeRegistryAccess.class.getClassLoader(),
                new Class<?>[] {type},
                (proxy, method, args) -> {
                    switch (method.getName()) {
                        case "getKey":
                            return key;
                        case "getMaxStackSize":
                            return 64;
                        case "isAir":
                            return "air".equals(key.getKey());
                        case "asMaterial":
                            return material;
                        case "getItemType":
                            return typedProxy(ItemType.class, key, material);
                        case "createBlockData":
                        case "createItemStack":
                            throw new UnsupportedOperationException("FakeRegistryAccess does not build item data");
                        case "createBlockDataStates":
                            return Collections.emptyList();
                        case "toString":
                            return type.getSimpleName() + "{" + key + "}";
                        case "hashCode":
                            return System.identityHashCode(proxy);
                        case "equals":
                            return proxy == args[0];
                        default:
                            return defaultValue(proxy, method, args);
                    }
                });
    }

    private static Object defaultValue(Object proxy, java.lang.reflect.Method method, Object[] args)
            throws Throwable {
        if (method.isDefault()) {
            return InvocationHandler.invokeDefault(proxy, method, args);
        }
        Class<?> returnType = method.getReturnType();
        if (returnType == boolean.class) {
            return false;
        }
        if (returnType == int.class) {
            return 0;
        }
        if (returnType == long.class) {
            return 0L;
        }
        return null;
    }
}
