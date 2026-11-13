package dev.ferrite.server;

/** Registry identities extracted from locally generated vanilla data. */
public final class FerriteRegistryAccess implements io.papermc.paper.registry.RegistryAccess {
    private static final java.util.Map<String, java.util.Set<String>> KEYS = loadKeys();
    private static final java.util.Map<String, org.bukkit.Registry<?>> REGISTRIES = new java.util.concurrent.ConcurrentHashMap<>();
    /** {@link org.bukkit.Registry}'s static initializer calls back into this class, so memoize without computeIfAbsent. */
    @SuppressWarnings("unchecked")
    private static <T extends org.bukkit.Keyed> org.bukkit.Registry<T> cached(String name, java.util.function.Supplier<org.bukkit.Registry<?>> factory) {
        var existing = (org.bukkit.Registry<T>) REGISTRIES.get(name);
        if (existing != null) return existing;
        var created = (org.bukkit.Registry<T>) factory.get();
        var prior = (org.bukkit.Registry<T>) REGISTRIES.putIfAbsent(name, created);
        return prior == null ? created : prior;
    }
    /** Protocol ids from the generated {@code reports/registries.json}, keyed by registry then full key. */
    private static final java.util.Map<String, java.util.Map<String, Integer>> IDS = loadIds();
    private static java.util.Map<String, java.util.Map<String, Integer>> loadIds() {
        var result = new java.util.HashMap<String, java.util.Map<String, Integer>>();
        try (var stream = java.util.Objects.requireNonNull(FerriteRegistryAccess.class.getResourceAsStream("/ferrite/registry-ids.tsv"), "Missing generated registry ids"); var reader = new java.io.BufferedReader(new java.io.InputStreamReader(stream, java.nio.charset.StandardCharsets.UTF_8))) {
            for (String line; (line = reader.readLine()) != null;) { String[] parts = line.split("\\t", 3); result.computeIfAbsent(parts[0], ignored -> new java.util.HashMap<>()).put(parts[1], Integer.parseInt(parts[2])); }
        } catch (java.io.IOException e) { throw new IllegalStateException("Cannot read registry ids", e); }
        return result;
    }
    /** The vanilla protocol (raw) id of a generated registry entry, or -1 when the entry is not in that registry. */
    static int protocolId(String registry, org.bukkit.NamespacedKey key) { return IDS.getOrDefault(registry, java.util.Map.of()).getOrDefault(key.toString(), -1); }
    private static java.util.Map<String, java.util.Set<String>> loadKeys() {
        var result = new java.util.HashMap<String, java.util.Set<String>>();
        try (var stream = java.util.Objects.requireNonNull(FerriteRegistryAccess.class.getResourceAsStream("/ferrite/registry-keys.tsv"), "Missing generated registry keys"); var reader = new java.io.BufferedReader(new java.io.InputStreamReader(stream, java.nio.charset.StandardCharsets.UTF_8))) {
            for (String line; (line = reader.readLine()) != null;) { String[] parts = line.split("\\t", 2); result.computeIfAbsent(parts[0], ignored -> new java.util.HashSet<>()).add(parts[1]); }
        } catch (java.io.IOException e) { throw new IllegalStateException("Cannot read registry keys", e); }
        return result;
    }
    static boolean contains(String registry, org.bukkit.NamespacedKey key) { return KEYS.getOrDefault(registry, java.util.Set.of()).contains(key.toString()); }
    @SuppressWarnings("unchecked") @Override public <T extends org.bukkit.Keyed> org.bukkit.Registry<T> getRegistry(Class<T> type) {
        if (type == org.bukkit.GameRule.class) return cast(cached("game_rule", FerriteRegistryAccess::gameRuleRegistry));
        if (type == org.bukkit.block.BlockType.class) return (org.bukkit.Registry<T>) getRegistry(io.papermc.paper.registry.RegistryKey.BLOCK);
        if (type == org.bukkit.inventory.ItemType.class) return (org.bukkit.Registry<T>) getRegistry(io.papermc.paper.registry.RegistryKey.ITEM);
        if (type == org.bukkit.inventory.MenuType.class) return (org.bukkit.Registry<T>) getRegistry(io.papermc.paper.registry.RegistryKey.MENU);
        if (type == org.bukkit.entity.EntityType.class) return (org.bukkit.Registry<T>) getRegistry(io.papermc.paper.registry.RegistryKey.ENTITY_TYPE);
        if (type == org.bukkit.Fluid.class) return (org.bukkit.Registry<T>) getRegistry(io.papermc.paper.registry.RegistryKey.FLUID);
        if (type == org.bukkit.GameEvent.class) return (org.bukkit.Registry<T>) getRegistry(io.papermc.paper.registry.RegistryKey.GAME_EVENT);
        if (type == org.bukkit.attribute.Attribute.class) return (org.bukkit.Registry<T>) getRegistry(io.papermc.paper.registry.RegistryKey.ATTRIBUTE);
        if (type == org.bukkit.Sound.class) return (org.bukkit.Registry<T>) getRegistry(io.papermc.paper.registry.RegistryKey.SOUND_EVENT);
        if (type == org.bukkit.enchantments.Enchantment.class) return (org.bukkit.Registry<T>) cached("enchantment", FerriteRegistryAccess::enchantmentRegistry);
        if (type == org.bukkit.potion.PotionEffectType.class) return (org.bukkit.Registry<T>) cached("mob_effect", FerriteRegistryAccess::potionRegistry);
        if (type == org.bukkit.block.Biome.class) return (org.bukkit.Registry<T>) cached("worldgen/biome", () -> identityRegistry("worldgen/biome", org.bukkit.block.Biome.class));
        if (type == org.bukkit.generator.structure.Structure.class) return (org.bukkit.Registry<T>) getRegistry(io.papermc.paper.registry.RegistryKey.STRUCTURE);
        if (type == org.bukkit.generator.structure.StructureType.class) return (org.bukkit.Registry<T>) getRegistry(io.papermc.paper.registry.RegistryKey.STRUCTURE_TYPE);
        if (type == org.bukkit.damage.DamageType.class) return (org.bukkit.Registry<T>) cached("damage_type", FerriteRegistryAccess::damageTypeRegistry);
        if (type == org.bukkit.map.MapCursor.Type.class) return (org.bukkit.Registry<T>) cached("map_decoration_type", FerriteRegistryAccess::mapDecorationRegistry);
        var kind = KIND_BY_CLASS.get(type);
        if (kind != null) return cast(byKind(kind));
        return unavailable(type.getName());
    }
    @Override public <T extends org.bukkit.Keyed> org.bukkit.Registry<T> getRegistry(io.papermc.paper.registry.RegistryKey<T> type) {
        if (type.equals(io.papermc.paper.registry.RegistryKey.GAME_RULE)) return cast(cached("game_rule", FerriteRegistryAccess::gameRuleRegistry));
        if (type.equals(io.papermc.paper.registry.RegistryKey.MENU)) return cast(cached("menu", FerriteRegistryAccess::menuRegistry));
        if (type.equals(io.papermc.paper.registry.RegistryKey.BLOCK)) return cast(cached("block", FerriteRegistryAccess::blockRegistry));
        if (type.equals(io.papermc.paper.registry.RegistryKey.ITEM)) return cast(cached("item", FerriteRegistryAccess::itemRegistry));
        if (type.equals(io.papermc.paper.registry.RegistryKey.ENTITY_TYPE)) return cast(cached("entity_type", FerriteRegistryAccess::entityRegistry));
        if (type.equals(io.papermc.paper.registry.RegistryKey.FLUID)) return cast(cached("fluid", () -> identityRegistry("fluid", org.bukkit.Fluid.class)));
        if (type.equals(io.papermc.paper.registry.RegistryKey.GAME_EVENT)) return cast(cached("game_event", FerriteRegistryAccess::gameEventRegistry));
        if (type.equals(io.papermc.paper.registry.RegistryKey.ENCHANTMENT)) return cast(cached("enchantment", FerriteRegistryAccess::enchantmentRegistry));
        if (type.equals(io.papermc.paper.registry.RegistryKey.MOB_EFFECT)) return cast(cached("mob_effect", FerriteRegistryAccess::potionRegistry));
        if (type.equals(io.papermc.paper.registry.RegistryKey.ATTRIBUTE)) return cast(cached("attribute", () -> identityRegistry("attribute", org.bukkit.attribute.Attribute.class)));
        if (type.equals(io.papermc.paper.registry.RegistryKey.SOUND_EVENT)) return cast(cached("sound_event", () -> identityRegistry("sound_event", org.bukkit.Sound.class)));
        if (type.equals(io.papermc.paper.registry.RegistryKey.BIOME)) return cast(cached("worldgen/biome", () -> identityRegistry("worldgen/biome", org.bukkit.block.Biome.class)));
        if (type.equals(io.papermc.paper.registry.RegistryKey.STRUCTURE)) return cast(cached("worldgen/structure", () -> identityRegistry("worldgen/structure", org.bukkit.generator.structure.Structure.class, FerriteStructure::new)));
        if (type.equals(io.papermc.paper.registry.RegistryKey.STRUCTURE_TYPE)) return cast(cached("worldgen/structure_type", FerriteRegistryAccess::structureTypeRegistry));
        if (type.equals(io.papermc.paper.registry.RegistryKey.DAMAGE_TYPE)) return cast(cached("damage_type", FerriteRegistryAccess::damageTypeRegistry));
        if (type.equals(io.papermc.paper.registry.RegistryKey.MAP_DECORATION_TYPE)) return cast(cached("map_decoration_type", FerriteRegistryAccess::mapDecorationRegistry));
        var name = type.key().toString();
        if (name.startsWith("minecraft:") && KINDS.contains(name.substring("minecraft:".length())))
            return cast(byKind(name.substring("minecraft:".length())));
        return unavailable(name);
    }

    private record Identity(String kind, Class<? extends org.bukkit.Keyed> type) {}
    /** Interface-typed registries whose members are plain identities backed by the generated key list. */
    private static final java.util.List<Identity> IDENTITIES = java.util.List.of(
        new Identity("villager_profession", org.bukkit.entity.Villager.Profession.class),
        new Identity("point_of_interest_type", io.papermc.paper.entity.poi.PoiType.class),
        new Identity("villager_type", org.bukkit.entity.Villager.Type.class),
        new Identity("data_component_type", io.papermc.paper.datacomponent.DataComponentType.class),
        new Identity("trim_material", org.bukkit.inventory.meta.trim.TrimMaterial.class),
        new Identity("trim_pattern", org.bukkit.inventory.meta.trim.TrimPattern.class),
        new Identity("wolf_variant", org.bukkit.entity.Wolf.Variant.class),
        new Identity("wolf_sound_variant", org.bukkit.entity.Wolf.SoundVariant.class),
        new Identity("jukebox_song", org.bukkit.JukeboxSong.class),
        new Identity("banner_pattern", org.bukkit.block.banner.PatternType.class),
        new Identity("decorated_pot_pattern", io.papermc.paper.block.pot.PotPatternType.class),
        new Identity("painting_variant", org.bukkit.Art.class),
        new Identity("cat_variant", org.bukkit.entity.Cat.Type.class),
        new Identity("cat_sound_variant", org.bukkit.entity.Cat.SoundVariant.class),
        new Identity("frog_variant", org.bukkit.entity.Frog.Variant.class),
        new Identity("chicken_variant", org.bukkit.entity.Chicken.Variant.class),
        new Identity("chicken_sound_variant", org.bukkit.entity.Chicken.SoundVariant.class),
        new Identity("cow_variant", org.bukkit.entity.Cow.Variant.class),
        new Identity("cow_sound_variant", org.bukkit.entity.Cow.SoundVariant.class),
        new Identity("pig_variant", org.bukkit.entity.Pig.Variant.class),
        new Identity("pig_sound_variant", org.bukkit.entity.Pig.SoundVariant.class),
        new Identity("zombie_nautilus_variant", org.bukkit.entity.ZombieNautilus.Variant.class),
        new Identity("sulfur_cube_archetype", org.bukkit.entity.SulfurCube.Archetype.class),
        new Identity("dialog", io.papermc.paper.dialog.Dialog.class));
    private static final java.util.Map<Class<?>, String> KIND_BY_CLASS = kindsByClass();
    private static final java.util.Set<String> KINDS = kinds();
    private static java.util.Map<Class<?>, String> kindsByClass() {
        var result = new java.util.HashMap<Class<?>, String>();
        for (var identity : IDENTITIES) result.put(identity.type(), identity.kind());
        result.put(org.bukkit.Particle.class, "particle_type");
        result.put(org.bukkit.potion.PotionType.class, "potion");
        result.put(org.bukkit.MusicInstrument.class, "instrument");
        return java.util.Map.copyOf(result);
    }
    private static java.util.Set<String> kinds() { return new java.util.HashSet<>(KIND_BY_CLASS.values()); }
    @SuppressWarnings({"unchecked", "rawtypes"})
    private static org.bukkit.Registry<?> byKind(String kind) {
        return switch (kind) {
            case "particle_type" -> cached(kind, () -> enumRegistry(kind, org.bukkit.Particle.class));
            case "potion" -> cached(kind, () -> enumRegistry(kind, org.bukkit.potion.PotionType.class));
            case "instrument" -> cached(kind, FerriteRegistryAccess::instrumentRegistry);
            default -> {
                var identity = IDENTITIES.stream().filter(i -> i.kind().equals(kind)).findFirst().orElseThrow();
                yield cached(kind, () -> identityRegistry(kind, (Class) identity.type()));
            }
        };
    }
    /**
     * Enum-backed registries index the constants by their own key. The index is built on first use because
     * {@link org.bukkit.Registry}'s static initializer calls back into this class while the enums may still be loading.
     */
    private static <E extends Enum<E> & org.bukkit.Keyed> org.bukkit.Registry<E> enumRegistry(String kind, Class<E> type) {
        return new KeyedRegistry<E>(kind) {
            private java.util.Map<org.bukkit.NamespacedKey, E> byKey;
            private synchronized java.util.Map<org.bukkit.NamespacedKey, E> index() {
                if (byKey == null) {
                    var map = new java.util.HashMap<org.bukkit.NamespacedKey, E>();
                    for (var constant : type.getEnumConstants()) map.put(constant.getKey(), constant);
                    byKey = map;
                }
                return byKey;
            }
            @Override public E get(org.bukkit.NamespacedKey key) { return contains(kind, key) ? index().get(key) : null; }
        };
    }
    private static org.bukkit.Registry<org.bukkit.MusicInstrument> instrumentRegistry() {
        return new KeyedRegistry<org.bukkit.MusicInstrument>("instrument") {
            private final java.util.Map<org.bukkit.NamespacedKey, org.bukkit.MusicInstrument> entries = new java.util.concurrent.ConcurrentHashMap<>();
            @Override public org.bukkit.MusicInstrument get(org.bukkit.NamespacedKey key) {
                if (!contains("instrument", key)) return null;
                // MusicInstrument's static constants re-enter get(), so computeIfAbsent would be a recursive update.
                var existing = entries.get(key); if (existing != null) return existing;
                var created = new FerriteMusicInstrument(key);
                var prior = entries.putIfAbsent(key, created); return prior == null ? created : prior;
            }
        };
    }
    /** 26.3 map decoration type keys in protocol-id order. */
    private static final String[] MAP_DECORATION_KEYS = {
        "player", "frame", "red_marker", "blue_marker", "target_x", "target_point",
        "player_off_map", "player_off_limits", "mansion", "monument", "banner_white",
        "banner_orange", "banner_magenta", "banner_light_blue", "banner_yellow",
        "banner_lime", "banner_pink", "banner_gray", "banner_light_gray", "banner_cyan",
        "banner_purple", "banner_blue", "banner_brown", "banner_green", "banner_red",
        "banner_black", "red_x", "village_desert", "village_plains", "village_savanna",
        "village_snowy", "village_taiga", "jungle_temple", "swamp_hut", "trial_chambers",
        "abandoned_camp", "ancient_city", "desert_pyramid", "mineshaft", "ocean_ruin_warm",
    };

    /** Bukkit's MapCursor.Type constants resolve through this registry during class init. */
    private static org.bukkit.Registry<org.bukkit.map.MapCursor.Type> mapDecorationRegistry() {
        return new BaseRegistry<org.bukkit.map.MapCursor.Type>("map_decoration_type") {
            private final java.util.Map<org.bukkit.NamespacedKey, org.bukkit.map.MapCursor.Type> entries = new java.util.concurrent.ConcurrentHashMap<>();
            @Override public org.bukkit.map.MapCursor.Type get(org.bukkit.NamespacedKey key) {
                if (!"minecraft".equals(key.getNamespace())) return null;
                int value = java.util.Arrays.asList(MAP_DECORATION_KEYS).indexOf(key.getKey());
                if (value < 0) return null;
                return entries.computeIfAbsent(key, k -> new MapDecorationType(k, (byte) value));
            }
            @Override public int size() { return MAP_DECORATION_KEYS.length; }
            @Override public java.util.stream.Stream<org.bukkit.NamespacedKey> keyStream() {
                return java.util.stream.IntStream.range(0, MAP_DECORATION_KEYS.length)
                    .mapToObj(i -> org.bukkit.NamespacedKey.minecraft(MAP_DECORATION_KEYS[i]));
            }
            @Override public java.util.stream.Stream<org.bukkit.map.MapCursor.Type> stream() { return keyStream().map(this::get); }
            @Override public java.util.Iterator<org.bukkit.map.MapCursor.Type> iterator() { return stream().iterator(); }
        };
    }

    private record MapDecorationType(org.bukkit.NamespacedKey key, byte value) implements org.bukkit.map.MapCursor.Type {
        @Override public org.bukkit.NamespacedKey getKey() { return key; }
        @Override public byte getValue() { return value; }
        @Override public String name() { return key.getKey().toUpperCase(java.util.Locale.ROOT); }
        @Override public int ordinal() { return value; }
        @Override public int compareTo(org.bukkit.map.MapCursor.Type other) { return Integer.compare(value, other.getValue()); }
        @Override public String toString() { return "MapDecorationType[" + key + "]"; }
    }

    /** Vanilla damage types are data-driven; the registry identity is all plugins need here. */
    private static org.bukkit.Registry<org.bukkit.damage.DamageType> damageTypeRegistry() {
        return new BaseRegistry<org.bukkit.damage.DamageType>("damage_type") {
            private final java.util.Map<org.bukkit.NamespacedKey, org.bukkit.damage.DamageType> entries = new java.util.concurrent.ConcurrentHashMap<>();
            @Override public org.bukkit.damage.DamageType get(org.bukkit.NamespacedKey key) {
                if (!contains("damage_type", key)) return null;
                return entries.computeIfAbsent(key, k -> (org.bukkit.damage.DamageType) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.damage.DamageType.class}, (proxy, method, args) -> switch (method.getName()) {
                    case "getKey", "key" -> k;
                    case "getTranslationKey", "translationKey" -> "death.attack." + k.getKey();
                    case "getDamageScaling" -> org.bukkit.damage.DamageScaling.NEVER;
                    case "getDamageEffect" -> org.bukkit.damage.DamageEffect.HURT;
                    case "getDeathMessageType" -> org.bukkit.damage.DeathMessageType.DEFAULT;
                    case "getExhaustion" -> 0.1f;
                    case "hashCode" -> k.hashCode();
                    case "equals" -> args[0] instanceof org.bukkit.damage.DamageType other && k.equals(other.getKey());
                    case "toString" -> "DamageType[" + k + "]";
                    default -> HostBridge.dispatch("DamageType", "", method, args);
                }));
            }
            @Override public int size() { return KEYS.get("damage_type").size(); }
            @Override public java.util.stream.Stream<org.bukkit.NamespacedKey> keyStream() { return KEYS.get("damage_type").stream().map(org.bukkit.NamespacedKey::fromString); }
            @Override public java.util.stream.Stream<org.bukkit.damage.DamageType> stream() { return keyStream().map(this::get); }
            @Override public java.util.Iterator<org.bukkit.damage.DamageType> iterator() { return stream().iterator(); }
        };
    }
    @SuppressWarnings("unchecked") private static <T extends org.bukkit.Keyed> org.bukkit.Registry<T> menuRegistry() {
        return (org.bukkit.Registry<T>) new KeyedRegistry<org.bukkit.inventory.MenuType>("menu") {
            private final java.util.Map<org.bukkit.NamespacedKey, org.bukkit.inventory.MenuType> entries = new java.util.concurrent.ConcurrentHashMap<>();
            @Override public org.bukkit.inventory.MenuType get(org.bukkit.NamespacedKey key) {
                if (!contains("menu", key)) return null;
                return entries.computeIfAbsent(key, k -> (org.bukkit.inventory.MenuType) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.inventory.MenuType.Typed.class}, (proxy, method, args) -> switch (method.getName()) {
                    case "getKey" -> k;
                    case "key" -> k;
                    case "typed" -> proxy;
                    case "hashCode" -> k.hashCode();
                    case "equals" -> proxy == args[0];
                    case "toString" -> "MenuType[" + k + "]";
                    default -> HostBridge.dispatch("MenuType", "", method, args);
                }));
            }
        };
    }
    @SuppressWarnings("unchecked") private static <T extends org.bukkit.Keyed> org.bukkit.Registry<T> itemRegistry() {
        return (org.bukkit.Registry<T>) new KeyedRegistry<org.bukkit.inventory.ItemType>("item") {
            private final java.util.Map<org.bukkit.NamespacedKey, org.bukkit.inventory.ItemType> entries = new java.util.concurrent.ConcurrentHashMap<>();
            @Override public org.bukkit.inventory.ItemType get(org.bukkit.NamespacedKey key) {
                if (!contains("item", key)) return null;
                var material = org.bukkit.Material.getMaterial(key.getKey().toUpperCase(java.util.Locale.ROOT));
                if (material == null || material.isLegacy()) return null;
                return entries.computeIfAbsent(key, k -> (org.bukkit.inventory.ItemType) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.inventory.ItemType.Typed.class}, (proxy, method, args) -> switch (method.getName()) {
                    case "getKey", "key" -> k;
                    case "typed" -> proxy;
                    case "asMaterial" -> material;
                    case "hasBlockType" -> contains("block", k);
                    case "getBlockType" -> org.bukkit.Registry.BLOCK.get(k);
                    case "getMaxStackSize" -> HostBridge.callInt("ItemType", k.toString(), "getMaxStackSize()");
                    case "getMaxDurability" -> (short) HostBridge.callInt("ItemType", k.toString(), "getMaxDurability()");
                    case "translationKey", "getTranslationKey" -> (material != null && material.isBlock() ? "block." : "item.") + k.getNamespace() + "." + k.getKey();
                    case "createItemStack" -> new FerriteItemStack(material, args == null || args.length == 0 ? 1 : (int) args[0]);
                    case "hashCode" -> k.hashCode();
                    case "equals" -> proxy == args[0];
                    case "toString" -> "ItemType[" + k + "]";
                    default -> HostBridge.dispatch("ItemType", "", method, args);
                }));
            }
        };
    }
    private static org.bukkit.Registry<org.bukkit.block.BlockType> blockRegistry() {
        return new KeyedRegistry<>("block") {
            private final java.util.Map<org.bukkit.NamespacedKey, org.bukkit.block.BlockType> entries = new java.util.concurrent.ConcurrentHashMap<>();
            @Override public org.bukkit.block.BlockType get(org.bukkit.NamespacedKey key) {
                if (!contains("block", key)) return null;
                var previous = entries.get(key); if (previous != null) return previous;
                var material = org.bukkit.Material.getMaterial(key.getKey().toUpperCase(java.util.Locale.ROOT));
                var value = (org.bukkit.block.BlockType) java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{org.bukkit.block.BlockType.Typed.class}, (proxy, method, args) -> switch (method.getName()) {
                    case "getKey", "key" -> key;
                    case "typed" -> proxy;
                    case "asMaterial" -> material;
                    case "createBlockData" -> FerriteBlockData.create(key.toString());
                    case "translationKey", "getTranslationKey" -> "block." + key.getNamespace() + "." + key.getKey();
                    case "isAir" -> key.getKey().equals("air") || key.getKey().equals("cave_air") || key.getKey().equals("void_air");
                    case "hasItemType" -> contains("item", key);
                    case "getItemType" -> org.bukkit.Registry.ITEM.get(key);
                    case "hashCode" -> key.hashCode();
                    case "equals" -> proxy == args[0];
                    case "toString" -> "BlockType[" + key + "]";
                    default -> HostBridge.dispatch("BlockType", "", method, args);
                });
                previous = entries.putIfAbsent(key, value); return previous == null ? value : previous;
            }
        };
    }
    private static org.bukkit.Registry<org.bukkit.entity.EntityType> entityRegistry() {
        return new KeyedRegistry<>("entity_type") {
            @Override public org.bukkit.entity.EntityType get(org.bukkit.NamespacedKey key) {
                if (!contains("entity_type", key)) return null;
                try { return org.bukkit.entity.EntityType.valueOf(key.getKey().toUpperCase(java.util.Locale.ROOT)); }
                catch (IllegalArgumentException e) { return null; }
            }
        };
    }
    private static org.bukkit.Registry<org.bukkit.enchantments.Enchantment> enchantmentRegistry() {
        return new BaseRegistry<>("enchantment") {
            private final java.util.Map<org.bukkit.NamespacedKey, org.bukkit.enchantments.Enchantment> entries = new java.util.concurrent.ConcurrentHashMap<>();
            @Override public org.bukkit.enchantments.Enchantment get(org.bukkit.NamespacedKey key) {
                if (!contains("enchantment", key)) return null;
                var value = entries.get(key); if (value != null) return value;
                value = new FerriteEnchantment(key); var prior = entries.putIfAbsent(key, value); return prior == null ? value : prior;
            }
            @Override public int size() { return KEYS.getOrDefault("enchantment", java.util.Set.of()).size(); }
            @Override public java.util.stream.Stream<org.bukkit.NamespacedKey> keyStream() { return KEYS.getOrDefault("enchantment", java.util.Set.of()).stream().sorted().map(org.bukkit.NamespacedKey::fromString); }
            @Override public java.util.stream.Stream<org.bukkit.enchantments.Enchantment> stream() { return keyStream().map(this::get); }
            @Override public java.util.Iterator<org.bukkit.enchantments.Enchantment> iterator() { return stream().iterator(); }
        };
    }
    /** StructureType is an abstract class, so it cannot be proxied like the identity registries. */
    private static org.bukkit.Registry<org.bukkit.generator.structure.StructureType> structureTypeRegistry() {
        return new BaseRegistry<org.bukkit.generator.structure.StructureType>("worldgen/structure_type") {
            private final java.util.Map<org.bukkit.NamespacedKey, org.bukkit.generator.structure.StructureType> entries = new java.util.concurrent.ConcurrentHashMap<>();
            @Override public org.bukkit.generator.structure.StructureType get(org.bukkit.NamespacedKey key) {
                if (!contains("worldgen/structure_type", key)) return null;
                return entries.computeIfAbsent(key, RegisteredStructureType::new);
            }
            @Override public int size() { return KEYS.get("worldgen/structure_type").size(); }
            @Override public java.util.stream.Stream<org.bukkit.NamespacedKey> keyStream() { return KEYS.get("worldgen/structure_type").stream().map(org.bukkit.NamespacedKey::fromString); }
            @Override public java.util.stream.Stream<org.bukkit.generator.structure.StructureType> stream() { return keyStream().map(this::get); }
            @Override public java.util.Iterator<org.bukkit.generator.structure.StructureType> iterator() { return stream().iterator(); }
        };
    }

    private static final class RegisteredStructureType extends org.bukkit.generator.structure.StructureType {
        private final org.bukkit.NamespacedKey key;
        RegisteredStructureType(org.bukkit.NamespacedKey key) { this.key = key; }
        @Override public org.bukkit.NamespacedKey getKey() { return key; }
        @Override public String toString() { return "StructureType[" + key + "]"; }
    }

    private static org.bukkit.Registry<org.bukkit.potion.PotionEffectType> potionRegistry() {
        return new BaseRegistry<>("mob_effect") {
            private final java.util.Map<org.bukkit.NamespacedKey, org.bukkit.potion.PotionEffectType> entries = new java.util.concurrent.ConcurrentHashMap<>();
            @Override public org.bukkit.potion.PotionEffectType get(org.bukkit.NamespacedKey key) {
                if (!contains("mob_effect", key)) return null;
                var value = entries.get(key); if (value != null) return value;
                value = new FerritePotionEffectType(key); var prior = entries.putIfAbsent(key, value); return prior == null ? value : prior;
            }
            @Override public int size() { return KEYS.getOrDefault("mob_effect", java.util.Set.of()).size(); }
            @Override public java.util.stream.Stream<org.bukkit.NamespacedKey> keyStream() { return KEYS.getOrDefault("mob_effect", java.util.Set.of()).stream().sorted().map(org.bukkit.NamespacedKey::fromString); }
            @Override public java.util.stream.Stream<org.bukkit.potion.PotionEffectType> stream() { return keyStream().map(this::get); }
            @Override public java.util.Iterator<org.bukkit.potion.PotionEffectType> iterator() { return stream().iterator(); }
        };
    }
    @SuppressWarnings("unchecked") private static <T extends org.bukkit.Keyed> org.bukkit.Registry<T> cast(org.bukkit.Registry<?> registry) { return (org.bukkit.Registry<T>) registry; }
    static org.bukkit.block.Biome customBiome() {
        var key = org.bukkit.NamespacedKey.minecraft("custom");
        return (org.bukkit.block.Biome) java.lang.reflect.Proxy.newProxyInstance(FerriteRegistryAccess.class.getClassLoader(), new Class<?>[]{org.bukkit.block.Biome.class}, (proxy, method, args) -> switch (method.getName()) {
            case "getKey", "key" -> key;
            case "name", "getName" -> "CUSTOM";
            case "hashCode" -> System.identityHashCode(proxy);
            case "equals" -> proxy == args[0];
            case "toString" -> "CUSTOM";
            default -> HostBridge.dispatch("Biome", "", method, args);
        });
    }
    private static <T extends org.bukkit.Keyed> org.bukkit.Registry<T> identityRegistry(String kind, Class<T> type) { return identityRegistry(kind, type, null); }
    private static <T extends org.bukkit.Keyed> org.bukkit.Registry<T> identityRegistry(String kind, Class<T> type, java.util.function.Function<org.bukkit.NamespacedKey, T> factory) {
        return new BaseRegistry<T>(kind) {
            private final java.util.Map<org.bukkit.NamespacedKey,T> entries = new java.util.concurrent.ConcurrentHashMap<>();
            @Override public T get(org.bukkit.NamespacedKey key) {
                if (!contains(kind, key)) return null;
                T existing = entries.get(key);
                if (existing != null) return existing;
                var k = key;
                T value = factory != null ? factory.apply(key) : FerriteRegistryValueProperties.supports(kind)
                    ? FerriteRegistryValueProperties.create(kind, key, type)
                    : type.cast(java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{type}, (proxy, method, args) -> switch (method.getName()) {
                    case "getKey", "key" -> k;
                    case "name", "getName" -> k.getKey().toUpperCase(java.util.Locale.ROOT).replace('.', '_');
                    case "translationKey", "getTranslationKey" -> kind + "." + k.getNamespace() + "." + k.getKey();
                    case "hashCode" -> k.hashCode();
                    case "equals" -> proxy == args[0];
                    case "toString" -> k.toString();
                    default -> HostBridge.dispatch(type.getSimpleName(), k, method, args);
                }));
                T previous = entries.putIfAbsent(key, value);
                return previous == null ? value : previous;
            }
            @Override public int size() { return KEYS.get(kind).size(); }
            @Override public java.util.stream.Stream<org.bukkit.NamespacedKey> keyStream() { return KEYS.get(kind).stream().map(org.bukkit.NamespacedKey::fromString); }
            @Override public java.util.stream.Stream<T> stream() { return keyStream().map(this::get); }
            @Override public java.util.Iterator<T> iterator() { return stream().iterator(); }
        };
    }
    private static org.bukkit.Registry<org.bukkit.GameEvent> gameEventRegistry() {
        return new BaseRegistry<org.bukkit.GameEvent>("game_event") {
            private final java.util.Map<org.bukkit.NamespacedKey, org.bukkit.GameEvent> entries = new java.util.concurrent.ConcurrentHashMap<>();
            @Override public org.bukkit.GameEvent get(org.bukkit.NamespacedKey key) {
                if (!contains("game_event", key)) return null;
                var existing = entries.get(key);
                if (existing != null) return existing;
                var k = key;
                var created = new org.bukkit.GameEvent() {
                    @Override public org.bukkit.NamespacedKey getKey() { return k; }
                    @Override public int getRange() { return switch (k.getKey()) {
                        case "jukebox_play", "jukebox_stop_play" -> 10;
                        case "shriek" -> 32;
                        default -> 16;
                    }; }
                    @Override public int getVibrationLevel() { return vibrationLevel(k.getKey()); }
                    @Override public String toString() { return "GameEvent[" + k + "]"; }
                };
                var previous = entries.putIfAbsent(key, created);
                return previous == null ? created : previous;
            }
            @Override public int size() { return KEYS.get("game_event").size(); }
            @Override public java.util.stream.Stream<org.bukkit.NamespacedKey> keyStream() { return KEYS.get("game_event").stream().map(org.bukkit.NamespacedKey::fromString); }
            @Override public java.util.stream.Stream<org.bukkit.GameEvent> stream() { return keyStream().map(this::get); }
            @Override public java.util.Iterator<org.bukkit.GameEvent> iterator() { return stream().iterator(); }
        };
    }
    private static int vibrationLevel(String name) {
        return switch (name) {
            case "step", "swim", "flap" -> 1;
            case "projectile_land", "hit_ground", "splash", "bounce" -> 2;
            case "item_interact_finish", "projectile_shoot", "instrument_play" -> 3;
            case "entity_action", "elytra_glide", "unequip" -> 4;
            case "entity_dismount", "equip" -> 5;
            case "entity_interact", "shear", "entity_mount" -> 6;
            case "entity_damage" -> 7;
            case "drink", "eat" -> 8;
            case "container_close", "block_close", "block_deactivate", "block_detach" -> 9;
            case "container_open", "block_open", "block_activate", "block_attach", "prime_fuse", "note_block_play" -> 10;
            case "block_change" -> 11;
            case "block_destroy", "fluid_pickup" -> 12;
            case "block_place", "fluid_place" -> 13;
            case "entity_place", "lightning_strike", "teleport" -> 14;
            case "entity_die", "explode" -> 15;
            default -> 0;
        };
    }
    private static org.bukkit.Registry<org.bukkit.GameRule<?>> gameRuleRegistry() {
        var definitions = FerriteGameRuleData.load(FerriteRegistryAccess.class.getResourceAsStream("/ferrite/game-rules.json"), KEYS.get("game_rule"));
        return new KeyedRegistry<>("game_rule") {
            private final java.util.Map<org.bukkit.NamespacedKey, org.bukkit.GameRule<?>> entries = new java.util.concurrent.ConcurrentHashMap<>();
            @Override public org.bukkit.GameRule<?> get(org.bukkit.NamespacedKey key) {
                java.util.Objects.requireNonNull(key, "key");
                var definition = definitions.get(key);
                if (definition == null) return null;
                var existing = entries.get(key);
                if (existing != null) return existing;
                // GameRule's static aliases re-enter this registry; computeIfAbsent cannot support that.
                var created = FerriteGameRule.create(definition);
                var prior = entries.putIfAbsent(key, created);
                return prior == null ? created : prior;
            }
        };
    }
    private static <T extends org.bukkit.Keyed> org.bukkit.Registry<T> unavailable(String name) { return new UnavailableRegistry<>(name); }
    /**
     * A registry the generated vanilla reports cannot back. The Rust host has no registry call, so every read fails
     * with the reason instead of reaching the bridge.
     */
    private static final class UnavailableRegistry<T extends org.bukkit.Keyed> extends BaseRegistry<T> {
        private final String reason;
        UnavailableRegistry(String name) {
            super(name);
            this.reason = name.contains("game_rule") || name.endsWith("GameRule")
                ? "GameRule needs a value type and default that the generated reports do not contain"
                : name.contains("memory_module_type") || name.endsWith("MemoryKey")
                ? "MemoryKey exposes only a typed subset and has a private constructor; the full generated registry has no value types"
                : "no generated vanilla data is mapped to it";
        }
        private UnsupportedOperationException no() { return new UnsupportedOperationException("Ferrite: registry " + name() + " is not available: " + reason); }
        @Override public T get(org.bukkit.NamespacedKey key) { throw no(); }
        @Override public boolean hasTag(io.papermc.paper.registry.tag.TagKey<T> key) { throw no(); }
        @Override public io.papermc.paper.registry.tag.Tag<T> getTag(io.papermc.paper.registry.tag.TagKey<T> key) { throw no(); }
        @Override public java.util.Collection<io.papermc.paper.registry.tag.Tag<T>> getTags() { throw no(); }
        @Override public java.util.stream.Stream<T> stream() { throw no(); }
        @Override public java.util.stream.Stream<org.bukkit.NamespacedKey> keyStream() { throw no(); }
        @Override public int size() { throw no(); }
        @Override public java.util.Iterator<T> iterator() { throw no(); }
    }
    /** A registry whose members are exactly the generated key list; subclasses only resolve one key. */
    private abstract static class KeyedRegistry<T extends org.bukkit.Keyed> extends BaseRegistry<T> {
        private final String kind;
        KeyedRegistry(String kind) { super(kind); this.kind = kind; }
        @Override public java.util.stream.Stream<org.bukkit.NamespacedKey> keyStream() {
            return KEYS.getOrDefault(kind, java.util.Set.of()).stream().sorted().map(org.bukkit.NamespacedKey::fromString).filter(key -> get(key) != null);
        }
        @Override public java.util.stream.Stream<T> stream() { return keyStream().map(this::get); }
        @Override public int size() { return (int) keyStream().count(); }
        @Override public java.util.Iterator<T> iterator() { return stream().iterator(); }
    }
    private abstract static class BaseRegistry<T extends org.bukkit.Keyed> implements org.bukkit.Registry<T> {
        private final String name;
        BaseRegistry(String name) { this.name = name; }
        final String name() { return name; }
        public T get(org.bukkit.NamespacedKey key) { return (T) HostBridge.call("Registry", name, "get(NamespacedKey)", Object.class, key); }
        public org.bukkit.NamespacedKey getKey(T value) { return value.getKey(); }
        public boolean hasTag(io.papermc.paper.registry.tag.TagKey<T> key) { return HostBridge.callBoolean("Registry", name, "hasTag(TagKey)", key); }
        public io.papermc.paper.registry.tag.Tag<T> getTag(io.papermc.paper.registry.tag.TagKey<T> key) { return (io.papermc.paper.registry.tag.Tag<T>) HostBridge.call("Registry", name, "getTag(TagKey)", io.papermc.paper.registry.tag.Tag.class, key); }
        public java.util.Collection<io.papermc.paper.registry.tag.Tag<T>> getTags() { return (java.util.Collection<io.papermc.paper.registry.tag.Tag<T>>) HostBridge.call("Registry", name, "getTags()", java.util.Collection.class); }
        public java.util.stream.Stream<T> stream() { return (java.util.stream.Stream<T>) HostBridge.call("Registry", name, "stream()", java.util.stream.Stream.class); }
        public java.util.stream.Stream<org.bukkit.NamespacedKey> keyStream() { return (java.util.stream.Stream<org.bukkit.NamespacedKey>) HostBridge.call("Registry", name, "keyStream()", java.util.stream.Stream.class); }
        public int size() { return HostBridge.callInt("Registry", name, "size()"); }
        public java.util.Iterator<T> iterator() { return (java.util.Iterator<T>) HostBridge.call("Registry", name, "iterator()", java.util.Iterator.class); }
    }
}
