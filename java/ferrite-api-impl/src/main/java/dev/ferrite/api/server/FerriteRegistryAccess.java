package dev.ferrite.api.server;

import io.papermc.paper.registry.RegistryAccess;
import io.papermc.paper.registry.RegistryKey;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.Collections;
import java.util.EnumSet;
import java.util.Locale;
import java.util.Set;
import java.util.Spliterators;
import java.util.stream.Stream;
import org.bukkit.Keyed;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.block.BlockType;
import org.bukkit.inventory.ItemType;

/**
 * Supplies the Paper API's {@code RegistryAccess} service so {@code org.bukkit.Registry},
 * {@code Material} and other keyed enums initialize without CraftBukkit.
 *
 * <p>Provides {@code minecraft:block} ({@link BlockType}) and {@code minecraft:item}
 * ({@link ItemType}) lookups backed by {@link Material} so {@link Material#isBlock()} and
 * {@link Material#isItem()} behave consistently; all other registries remain empty.
 */
public final class FerriteRegistryAccess implements RegistryAccess {

    private static final Set<Material> NON_ITEMS =
            EnumSet.of(Material.WATER, Material.LAVA, Material.FIRE);
    private static final Set<Material> NON_BLOCKS =
            EnumSet.of(
                    Material.DIAMOND,
                    Material.APPLE,
                    Material.STICK,
                    Material.IRON_INGOT,
                    Material.DIAMOND_SWORD);

    private static final Registry<?> EMPTY = createEmptyRegistry();
    private static final Registry<?> BLOCK_REGISTRY = createTypedRegistry(BlockType.class);
    private static final Registry<?> ITEM_REGISTRY = createTypedRegistry(ItemType.class);

    @Override
    @SuppressWarnings("unchecked")
    public <T extends Keyed> Registry<T> getRegistry(Class<T> type) {
        if (type == BlockType.class) {
            return (Registry<T>) BLOCK_REGISTRY;
        }
        if (type == ItemType.class) {
            return (Registry<T>) ITEM_REGISTRY;
        }
        return empty();
    }

    @Override
    @SuppressWarnings("unchecked")
    public <T extends Keyed> Registry<T> getRegistry(RegistryKey<T> key) {
        if (key != null) {
            String id = key.key().asString();
            if ("minecraft:block".equals(id)) {
                return (Registry<T>) BLOCK_REGISTRY;
            }
            if ("minecraft:item".equals(id)) {
                return (Registry<T>) ITEM_REGISTRY;
            }
        }
        return empty();
    }

    @SuppressWarnings("unchecked")
    private static <T extends Keyed> Registry<T> empty() {
        return (Registry<T>) EMPTY;
    }

    private static Registry<?> createEmptyRegistry() {
        return (Registry<?>) Proxy.newProxyInstance(
                FerriteRegistryAccess.class.getClassLoader(),
                new Class<?>[] {Registry.class},
                new EmptyRegistryHandler());
    }

    private static Registry<?> createTypedRegistry(Class<?> elementType) {
        return (Registry<?>) Proxy.newProxyInstance(
                FerriteRegistryAccess.class.getClassLoader(),
                new Class<?>[] {Registry.class},
                new TypedRegistryHandler(elementType));
    }

    private static final class TypedRegistryHandler extends EmptyRegistryHandler {
        private final Class<?> elementType;

        private TypedRegistryHandler(Class<?> elementType) {
            this.elementType = elementType;
        }

        @Override
        public Object invoke(Object proxy, Method method, Object[] args) {
            if ("get".equals(method.getName()) && args != null && args.length == 1
                    && args[0] instanceof NamespacedKey key) {
                return resolveElement(elementType, key);
            }
            if ("toString".equals(method.getName())) {
                return "FerriteRegistry<" + elementType.getSimpleName() + ">";
            }
            return super.invoke(proxy, method, args);
        }
    }

    private static Object resolveElement(Class<?> elementType, NamespacedKey key) {
        if (!"minecraft".equals(key.getNamespace())) {
            return null;
        }
        Material material;
        try {
            material = Material.valueOf(key.getKey().toUpperCase(Locale.ROOT));
        } catch (IllegalArgumentException unknown) {
            return null;
        }
        if (material.isLegacy()) {
            return null;
        }
        if (elementType == BlockType.class && NON_BLOCKS.contains(material)) {
            return null;
        }
        if (elementType == ItemType.class && NON_ITEMS.contains(material)) {
            return null;
        }
        return Proxy.newProxyInstance(
                FerriteRegistryAccess.class.getClassLoader(),
                new Class<?>[] {elementType},
                (proxy, method, args) -> switch (method.getName()) {
                    case "getKey" -> key;
                    case "getMaxStackSize" -> 64;
                    case "isAir" -> material == Material.AIR
                            || material == Material.CAVE_AIR
                            || material == Material.VOID_AIR;
                    case "asMaterial" -> material;
                    case "getItemType" -> resolveElement(ItemType.class, key);
                    case "createBlockDataStates" -> Collections.emptyList();
                    case "toString" -> elementType.getSimpleName() + "{" + key + "}";
                    case "hashCode" -> key.hashCode();
                    case "equals" -> proxy == (args == null ? null : args[0]);
                    default -> defaultReturn(method);
                });
    }

    private static Object defaultReturn(Method method) {
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
        if (returnType == double.class) {
            return 0.0D;
        }
        if (returnType == float.class) {
            return 0.0F;
        }
        if (returnType == short.class) {
            return (short) 0;
        }
        if (returnType == byte.class) {
            return (byte) 0;
        }
        if (returnType == char.class) {
            return (char) 0;
        }
        return null;
    }

    private static class EmptyRegistryHandler implements InvocationHandler {

        @Override
        public Object invoke(Object proxy, Method method, Object[] args) {
            switch (method.getName()) {
                case "toString":
                    return "FerriteEmptyRegistry";
                case "hashCode":
                    return System.identityHashCode(proxy);
                case "equals":
                    return proxy == (args == null ? null : args[0]);
                case "iterator":
                    return Collections.emptyIterator();
                case "spliterator":
                    return Spliterators.emptySpliterator();
                case "stream":
                case "streamTags":
                    return Stream.empty();
                case "hasTag":
                    return false;
                case "get":
                case "getKey":
                case "getTag":
                    return null;
                default:
                    return defaultReturn(method);
            }
        }
    }
}
