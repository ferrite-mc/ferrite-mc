// Clean-room Ferrite NMS test support. No Mojang code or assets.
package dev.ferrite.nms.test;

import io.papermc.paper.datacomponent.DataComponentType;
import io.papermc.paper.datacomponent.DataComponentTypes;
import io.papermc.paper.registry.RegistryAccess;
import io.papermc.paper.registry.RegistryKey;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Proxy;
import java.util.Collections;
import java.util.EnumSet;
import java.util.HashMap;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import net.kyori.adventure.text.Component;
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
    private static final Set<String> NON_VALUED_COMPONENTS =
            Set.of("unbreakable", "intangible_projectile", "glider", "waxed");
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
        if ("minecraft:data_component_type".equals(id)) {
            return (Registry<T>) dataComponentRegistry();
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
                            return element(elementType, asNamespacedKey(args[0]));
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
                        case "createItemStack":
                            if (material == null) {
                                throw new UnsupportedOperationException(
                                        "FakeRegistryAccess has no material for " + key);
                            }
                            return new FakePaperItemStack(
                                    material, args == null || args.length == 0 ? 1 : (Integer) args[0]);
                        case "createBlockData":
                            throw new UnsupportedOperationException("FakeRegistryAccess does not build block data");
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

    /**
     * A real {@link org.bukkit.inventory.ItemStack} subclass that backs the Paper API item
     * delegation with an in-memory type, amount and 26.3 component map. Paper API normally gets
     * this object from CraftBukkit's server-side item factory; unit tests have no server, so the
     * fake registry supplies this narrowly scoped replacement. It deliberately does not implement
     * ItemMeta, inventory handling or persistence.
     */
    private static final class FakePaperItemStack extends org.bukkit.inventory.ItemStack {
        private final Map<DataComponentType, Object> components = new HashMap<>();
        private Material type;
        private int amount;

        FakePaperItemStack(Material type, int amount) {
            super();
            this.type = Objects.requireNonNull(type, "type");
            this.amount = amount;
        }

        @Override
        public Material getType() {
            return type;
        }

        @Override
        public void setType(Material type) {
            this.type = Objects.requireNonNull(type, "type");
        }

        @Override
        public int getAmount() {
            return amount;
        }

        @Override
        public void setAmount(int amount) {
            this.amount = amount;
        }

        @Override
        public int getMaxStackSize() {
            Integer component = getData(DataComponentTypes.MAX_STACK_SIZE);
            return component == null ? type.getMaxStackSize() : component;
        }

        @Override
        @SuppressWarnings("unchecked")
        public <T> T getData(DataComponentType.Valued<T> type) {
            return (T) components.get(type);
        }

        @Override
        public Set<DataComponentType> getDataTypes() {
            return Set.copyOf(components.keySet());
        }

        @Override
        public <T> void setData(DataComponentType.Valued<T> type, T value) {
            components.put(Objects.requireNonNull(type, "type"), value);
        }

        @Override
        public void setData(DataComponentType.NonValued type) {
            components.put(Objects.requireNonNull(type, "type"), Boolean.TRUE);
        }

        @Override
        public void unsetData(DataComponentType type) {
            components.remove(Objects.requireNonNull(type, "type"));
        }

        @Override
        public Component displayName() {
            Component customName = getData(DataComponentTypes.CUSTOM_NAME);
            return customName == null ? Component.text(type.getKey().toString()) : customName;
        }

        @Override
        public FakePaperItemStack clone() {
            FakePaperItemStack copy = new FakePaperItemStack(type, amount);
            copy.components.putAll(components);
            return copy;
        }

        @Override
        public boolean equals(Object other) {
            return this == other
                    || other instanceof FakePaperItemStack stack
                            && type == stack.type
                            && amount == stack.amount
                            && components.equals(stack.components);
        }

        @Override
        public int hashCode() {
            return Objects.hash(type, amount, components);
        }

        @Override
        public String toString() {
            return amount + " " + type.getKey();
        }
    }

    private static Registry<?> dataComponentRegistry() {
        return (Registry<?>) Proxy.newProxyInstance(
                FakeRegistryAccess.class.getClassLoader(),
                new Class<?>[] {Registry.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "get" -> dataComponent(asNamespacedKey(args[0]));
                    case "iterator" -> Collections.emptyIterator();
                    case "toString" -> "FakeRegistry<DataComponentType>";
                    case "hashCode" -> System.identityHashCode(proxy);
                    case "equals" -> proxy == args[0];
                    default -> defaultValue(proxy, method, args);
                });
    }

    private static DataComponentType dataComponent(NamespacedKey key) {
        if (key == null) {
            return null;
        }
        Class<?> type = NON_VALUED_COMPONENTS.contains(key.getKey())
                ? DataComponentType.NonValued.class
                : DataComponentType.Valued.class;
        return (DataComponentType) Proxy.newProxyInstance(
                FakeRegistryAccess.class.getClassLoader(),
                new Class<?>[] {type},
                (proxy, method, args) -> switch (method.getName()) {
                    case "getKey" -> key;
                    case "isPersistent" -> true;
                    case "toString" -> "FakeDataComponent<" + key + ">";
                    case "hashCode" -> System.identityHashCode(proxy);
                    case "equals" -> proxy == args[0];
                    default -> defaultValue(proxy, method, args);
                });
    }

    private static NamespacedKey asNamespacedKey(Object value) {
        if (value instanceof NamespacedKey key) {
            return key;
        }
        if (value instanceof net.kyori.adventure.key.Key key) {
            return NamespacedKey.fromString(key.asString());
        }
        throw new IllegalArgumentException(
                "Expected a namespaced key but got " + (value == null ? "null" : value.getClass().getName()));
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
