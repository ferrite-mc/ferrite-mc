package dev.ferrite.api.server;

import dev.ferrite.api.registry.FerriteAttribute;
import dev.ferrite.api.registry.FerriteBiome;
import dev.ferrite.api.registry.FerriteDamageType;
import dev.ferrite.api.registry.FerriteDataComponentType;
import dev.ferrite.api.registry.FerriteEnchantment;
import dev.ferrite.api.registry.FerriteJukeboxSong;
import dev.ferrite.api.registry.FerritePoiType;
import dev.ferrite.api.registry.FerriteRegistry;
import dev.ferrite.api.registry.FerriteSound;
import dev.ferrite.api.registry.VanillaData;
import io.papermc.paper.datacomponent.DataComponentType;
import io.papermc.paper.entity.poi.PoiType;
import io.papermc.paper.registry.RegistryAccess;
import io.papermc.paper.registry.RegistryKey;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.EnumSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.Spliterators;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.Supplier;
import java.util.stream.Stream;
import org.bukkit.JukeboxSong;
import org.bukkit.Keyed;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.Sound;
import org.bukkit.attribute.Attribute;
import org.bukkit.block.Biome;
import org.bukkit.block.BlockType;
import org.bukkit.damage.DamageType;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.entity.EntityType;
import org.bukkit.inventory.ItemType;
import org.bukkit.inventory.MenuType;

/**
 * Supplies the Paper API's {@code RegistryAccess} service from the generated 26.3 data.
 *
 * <p>{@code Enchantment}, {@code Biome} and {@code DamageType} build their static constants
 * with {@code Registry#getOrThrow}, so an empty registry makes those classes fail to initialize.
 * This implementation answers the keys the facade can model from vanilla data: enchantments,
 * biomes, damage types, point-of-interest types, data component types, entity types and
 * materials. Registries without a facade representation stay empty.
 *
 * <p>{@code minecraft:block} and {@code minecraft:item} are additionally backed by
 * {@link Material} proxies so {@link Material#isBlock()} and {@link Material#isItem()} behave
 * consistently while the generated data is loaded lazily.
 *
 * <p>Lookups are lazy and cached per registry. Laziness is required, not an optimization:
 * the value types subclass API types whose initializers are still running when they ask for
 * their entries.
 *
 * <p>When no generated tree is present the registries are empty; the facade then behaves as it
 * did before (absent data rather than a class-init crash).
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

    private static final class BlockRegistryHolder {
        static final Registry<?> INSTANCE = createTypedRegistry(BlockType.class);
    }

    private static final class ItemRegistryHolder {
        static final Registry<?> INSTANCE = createTypedRegistry(ItemType.class);
    }

    private static final class MenuRegistryHolder {
        static final Registry<?> INSTANCE = createTypedRegistry(MenuType.Typed.class);
    }

    // Registries are process-wide: Bukkit's static constants cache the first value a registry
    // returns, so every access instance must hand out the same entry objects.
    private static final Map<String, Registry<?>> SHARED_REGISTRIES = new ConcurrentHashMap<>();
    private static volatile Registry<Material> sharedMaterials;

    private final VanillaData data;

    public FerriteRegistryAccess() {
        this(VanillaData.get());
    }

    FerriteRegistryAccess(VanillaData data) {
        this.data = data;
    }

    @Override
    @SuppressWarnings("unchecked")
    public <T extends Keyed> Registry<T> getRegistry(Class<T> type) {
        if (type == BlockType.class) {
            return (Registry<T>) BlockRegistryHolder.INSTANCE;
        }
        if (type == ItemType.class) {
            return (Registry<T>) ItemRegistryHolder.INSTANCE;
        }
        if (type == MenuType.class || type == MenuType.Typed.class) {
            return (Registry<T>) MenuRegistryHolder.INSTANCE;
        }
        if (type == Enchantment.class) {
            return as(getRegistry(RegistryKey.ENCHANTMENT));
        }
        if (type == Biome.class) {
            return as(getRegistry(RegistryKey.BIOME));
        }
        if (type == DamageType.class) {
            return as(getRegistry(RegistryKey.DAMAGE_TYPE));
        }
        if (type == PoiType.class) {
            return as(getRegistry(RegistryKey.POINT_OF_INTEREST_TYPE));
        }
        if (type == DataComponentType.class) {
            return as(getRegistry(RegistryKey.DATA_COMPONENT_TYPE));
        }
        if (type == EntityType.class) {
            return as(getRegistry(RegistryKey.ENTITY_TYPE));
        }
        if (type == Attribute.class) {
            return as(getRegistry(RegistryKey.ATTRIBUTE));
        }
        if (type == Sound.class) {
            return as(getRegistry(RegistryKey.SOUND_EVENT));
        }
        if (type == JukeboxSong.class) {
            return as(getRegistry(RegistryKey.JUKEBOX_SONG));
        }
        if (type == org.bukkit.potion.PotionEffectType.class) {
            return as(getRegistry(RegistryKey.MOB_EFFECT));
        }
        if (type == Material.class) {
            return as(materials());
        }
        return empty();
    }

    @Override
    @SuppressWarnings("unchecked")
    public <T extends Keyed> Registry<T> getRegistry(RegistryKey<T> key) {
        if (key == null) {
            return empty();
        }
        String id = key.key().asString();
        if ("minecraft:block".equals(id)) {
            return (Registry<T>) BlockRegistryHolder.INSTANCE;
        }
        if ("minecraft:item".equals(id)) {
            return (Registry<T>) ItemRegistryHolder.INSTANCE;
        }
        if ("minecraft:menu".equals(id)) {
            return (Registry<T>) MenuRegistryHolder.INSTANCE;
        }
        String registryId = key.toString();
        Registry<?> existing = SHARED_REGISTRIES.get(registryId);
        if (existing != null) {
            return (Registry<T>) existing;
        }
        Registry<?> built = build(key);
        Registry<?> raced = SHARED_REGISTRIES.putIfAbsent(registryId, built);
        return (Registry<T>) (raced != null ? raced : built);
    }

    private Registry<?> build(RegistryKey<?> key) {
        if (key == RegistryKey.ENCHANTMENT) {
            return buildEnchantments();
        }
        if (key == RegistryKey.BIOME) {
            return buildBiomes();
        }
        if (key == RegistryKey.DAMAGE_TYPE) {
            return buildDamageTypes();
        }
        if (key == RegistryKey.POINT_OF_INTEREST_TYPE) {
            return buildSimple("point_of_interest_type", FerritePoiType::new);
        }
        if (key == RegistryKey.DATA_COMPONENT_TYPE) {
            return buildSimple("data_component_type", FerriteDataComponentType::create);
        }
        if (key == RegistryKey.ENTITY_TYPE) {
            return buildEntityTypes();
        }
        if (key == RegistryKey.ATTRIBUTE) {
            return buildAttributes();
        }
        if (key == RegistryKey.SOUND_EVENT) {
            return buildSounds();
        }
        if (key == RegistryKey.JUKEBOX_SONG) {
            return buildJukeboxSongs();
        }
        if (key == RegistryKey.MOB_EFFECT) {
            return buildMobEffects();
        }
        return empty();
    }

    private Registry<org.bukkit.potion.PotionEffectType> buildMobEffects() {
        List<String> names = new ArrayList<>(this.data.registry("minecraft:mob_effect").keySet());
        if (names.isEmpty()) {
            names = List.of(
                    "minecraft:speed", "minecraft:slowness", "minecraft:haste", "minecraft:mining_fatigue",
                    "minecraft:strength", "minecraft:instant_health", "minecraft:instant_damage",
                    "minecraft:jump_boost", "minecraft:nausea", "minecraft:regeneration",
                    "minecraft:resistance", "minecraft:fire_resistance", "minecraft:water_breathing",
                    "minecraft:invisibility", "minecraft:blindness", "minecraft:night_vision",
                    "minecraft:hunger", "minecraft:weakness", "minecraft:poison", "minecraft:wither",
                    "minecraft:health_boost", "minecraft:absorption", "minecraft:saturation",
                    "minecraft:glowing", "minecraft:levitation", "minecraft:luck", "minecraft:unluck",
                    "minecraft:slow_falling", "minecraft:conduit_power", "minecraft:dolphins_grace",
                    "minecraft:bad_omen", "minecraft:hero_of_the_village", "minecraft:darkness",
                    "minecraft:trial_omen", "minecraft:raid_omen", "minecraft:wind_charged",
                    "minecraft:weaving", "minecraft:oozing", "minecraft:infested",
                    "minecraft:breath_of_the_nautilus");
        } else {
            names.sort(String::compareTo);
        }
        Map<NamespacedKey, Supplier<org.bukkit.potion.PotionEffectType>> suppliers = new LinkedHashMap<>();
        for (int index = 0; index < names.size(); index++) {
            NamespacedKey key = NamespacedKey.fromString(names.get(index));
            if (key == null) {
                continue;
            }
            int id = index + 1;
            suppliers.put(key, () -> new FerritePotionEffectType(key, id));
        }
        return new FerriteRegistry<>("mob_effect", suppliers);
    }

    private Registry<Attribute> buildAttributes() {
        List<String> names = new ArrayList<>(this.data.registry("minecraft:attribute").keySet());
        names.sort(String::compareTo);
        Map<NamespacedKey, Supplier<Attribute>> suppliers = new LinkedHashMap<>();
        for (int index = 0; index < names.size(); index++) {
            NamespacedKey key = NamespacedKey.fromString(names.get(index));
            if (key == null) {
                continue;
            }
            int ordinal = index;
            suppliers.put(key, () -> new FerriteAttribute(key, ordinal));
        }
        return new FerriteRegistry<>("attribute", suppliers);
    }

    private Registry<Sound> buildSounds() {
        List<String> names = new ArrayList<>(this.data.registry("minecraft:sound_event").keySet());
        names.sort(String::compareTo);
        Map<NamespacedKey, Supplier<Sound>> suppliers = new LinkedHashMap<>();
        for (int index = 0; index < names.size(); index++) {
            NamespacedKey key = NamespacedKey.fromString(names.get(index));
            if (key == null) {
                continue;
            }
            int ordinal = index;
            suppliers.put(key, () -> new FerriteSound(key, ordinal));
        }
        return new FerriteRegistry<>("sound_event", suppliers);
    }

    private Registry<JukeboxSong> buildJukeboxSongs() {
        Map<NamespacedKey, Supplier<JukeboxSong>> suppliers = new LinkedHashMap<>();
        for (Map.Entry<String, com.google.gson.JsonObject> entry : this.data.jukeboxSongs().entrySet()) {
            NamespacedKey key = NamespacedKey.minecraft(entry.getKey());
            suppliers.put(key, () -> new FerriteJukeboxSong(key, entry.getValue()));
        }
        return new FerriteRegistry<>("jukebox_song", suppliers);
    }

    private Registry<Enchantment> buildEnchantments() {
        // See buildBiomes: Enchantment's static init queries this registry re-entrantly.
        Enchantment initialized = Enchantment.PROTECTION;
        Map<NamespacedKey, Supplier<Enchantment>> suppliers = new LinkedHashMap<>();
        for (String name : this.data.enchantments().keySet()) {
            NamespacedKey key = NamespacedKey.minecraft(name);
            suppliers.put(key, () -> new FerriteEnchantment(this.data, key));
        }
        return new FerriteRegistry<>("enchantment", suppliers);
    }

    private Registry<Biome> buildBiomes() {
        // Constructing a FerriteBiome triggers Biome's static initializer, which asks this very
        // registry for every constant. Initialize Biome first so that re-entrant lookup caches
        // the same instance the constant is assigned from.
        Biome initialized = Biome.CUSTOM;
        List<String> names = new ArrayList<>(this.data.biomes().keySet());
        names.sort(String::compareTo);
        Map<NamespacedKey, Supplier<Biome>> suppliers = new LinkedHashMap<>();
        for (int index = 0; index < names.size(); index++) {
            NamespacedKey key = NamespacedKey.minecraft(names.get(index));
            int ordinal = index;
            suppliers.put(key, () -> new FerriteBiome(key, ordinal));
        }
        return new FerriteRegistry<>("biome", suppliers);
    }

    private Registry<DamageType> buildDamageTypes() {
        // See buildBiomes: DamageType's static init queries this registry re-entrantly.
        DamageType initialized = DamageType.ARROW;
        Map<NamespacedKey, Supplier<DamageType>> suppliers = new LinkedHashMap<>();
        for (Map.Entry<String, com.google.gson.JsonObject> entry : this.data.damageTypes().entrySet()) {
            NamespacedKey key = NamespacedKey.minecraft(entry.getKey());
            suppliers.put(key, () -> new FerriteDamageType(key, entry.getValue()));
        }
        return new FerriteRegistry<>("damage_type", suppliers);
    }

    private <T extends Keyed> Registry<T> buildSimple(
            String name, java.util.function.Function<NamespacedKey, T> factory) {
        Map<NamespacedKey, Supplier<T>> suppliers = new LinkedHashMap<>();
        for (String entry : this.data.registry("minecraft:" + name).keySet()) {
            // reports/registries.json keys are already namespaced (minecraft:armorer).
            NamespacedKey key = NamespacedKey.fromString(entry);
            if (key == null) {
                continue;
            }
            suppliers.put(key, () -> factory.apply(key));
        }
        return new FerriteRegistry<>(name, suppliers);
    }

    private Registry<EntityType> buildEntityTypes() {
        Collection<String> known = this.data.registry("minecraft:entity_type").keySet();
        Map<NamespacedKey, Supplier<EntityType>> suppliers = new LinkedHashMap<>();
        for (EntityType type : EntityType.values()) {
            NamespacedKey key;
            try {
                key = type.getKey();
            } catch (IllegalArgumentException | IllegalStateException keyless) {
                // EntityType.UNKNOWN (and any future keyless entry) has no registry id.
                continue;
            }
            if (key == null || !known.contains(key.toString())) {
                continue;
            }
            suppliers.put(key, () -> type);
        }
        return new FerriteRegistry<>("entity_type", suppliers);
    }

    private Registry<Material> materials() {
        Registry<Material> local = sharedMaterials;
        if (local == null) {
            synchronized (FerriteRegistryAccess.class) {
                local = sharedMaterials;
                if (local == null) {
                    local = buildMaterials();
                    sharedMaterials = local;
                }
            }
        }
        return local;
    }

    private static Registry<Material> buildMaterials() {
        Map<NamespacedKey, Supplier<Material>> suppliers = new LinkedHashMap<>();
        for (Material material : Material.values()) {
            NamespacedKey key;
            try {
                key = material.getKey();
            } catch (IllegalStateException | IllegalArgumentException legacy) {
                continue;
            }
            if (key == null) {
                continue;
            }
            suppliers.put(key, () -> material);
        }
        return new FerriteRegistry<>("material", suppliers);
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
        Set<String> knownBlocks = VanillaData.get().blocks();
        if (elementType == BlockType.class
                && (!knownBlocks.isEmpty()
                        ? !knownBlocks.contains(key.toString())
                        : NON_BLOCKS.contains(material))) {
            return null;
        }
        if (elementType == ItemType.class && NON_ITEMS.contains(material)) {
            return null;
        }
        String translationKey = translationKey(elementType, material, key, knownBlocks);
        return Proxy.newProxyInstance(
                FerriteRegistryAccess.class.getClassLoader(),
                new Class<?>[] {elementType},
                (proxy, method, args) -> switch (method.getName()) {
                    case "getKey" -> key;
                    case "getMaxStackSize" -> 64;
                    case "getTranslationKey", "translationKey" -> translationKey;
                    case "hasItemType" -> resolveElement(ItemType.class, key) != null;
                    case "getBlockType" -> resolveElement(BlockType.class, key);
                    case "isAir" -> material == Material.AIR
                            || material == Material.CAVE_AIR
                            || material == Material.VOID_AIR;
                    case "isSolid", "hasCollision" ->
                            dev.ferrite.api.block.FerriteBlock.isSolidMaterial(material);
                    case "isOccluding" ->
                            dev.ferrite.api.block.FerriteBlock.isOccludingMaterial(material);
                    case "asMaterial" -> material;
                    case "getItemType" -> resolveElement(ItemType.class, key);
                    case "createBlockDataStates" -> Collections.emptyList();
                    case "toString" -> elementType.getSimpleName() + "{" + key + "}";
                    case "hashCode" -> key.hashCode();
                    case "equals" -> proxy == (args == null ? null : args[0]);
                    default -> defaultReturn(method);
                });
    }

    /**
     * The 26.3 translation key for a registry element: block items use the
     * {@code block.minecraft.*} key, everything else {@code item.minecraft.*}.
     */
    private static String translationKey(
            Class<?> elementType, Material material, NamespacedKey key, Set<String> knownBlocks) {
        boolean block = elementType == BlockType.class
                || (!knownBlocks.isEmpty()
                        ? knownBlocks.contains(key.toString())
                        : material == Material.AIR || material.isBlock());
        return (block ? "block." : "item.") + key.getNamespace() + "." + key.getKey();
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
            return switch (method.getName()) {
                case "toString" -> "FerriteEmptyRegistry";
                case "hashCode" -> System.identityHashCode(proxy);
                case "equals" -> proxy == (args == null ? null : args[0]);
                case "iterator" -> Collections.emptyIterator();
                case "spliterator" -> Spliterators.emptySpliterator();
                case "stream", "streamTags" -> Stream.empty();
                case "hasTag" -> false;
                case "get", "getKey", "getTag" -> null;
                default -> defaultReturn(method);
            };
        }
    }

    @SuppressWarnings("unchecked")
    private static <T extends Keyed> Registry<T> as(Registry<?> registry) {
        return (Registry<T>) registry;
    }

    private static <T extends Keyed> Registry<T> empty() {
        return new FerriteRegistry<>("empty", Map.of());
    }
}
