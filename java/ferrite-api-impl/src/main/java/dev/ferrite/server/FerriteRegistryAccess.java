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
    private static java.util.Map<String, java.util.Set<String>> loadKeys() {
        var result = new java.util.HashMap<String, java.util.Set<String>>();
        try (var stream = java.util.Objects.requireNonNull(FerriteRegistryAccess.class.getResourceAsStream("/ferrite/registry-keys.tsv"), "Missing generated registry keys"); var reader = new java.io.BufferedReader(new java.io.InputStreamReader(stream, java.nio.charset.StandardCharsets.UTF_8))) {
            for (String line; (line = reader.readLine()) != null;) { String[] parts = line.split("\\t", 2); result.computeIfAbsent(parts[0], ignored -> new java.util.HashSet<>()).add(parts[1]); }
        } catch (java.io.IOException e) { throw new IllegalStateException("Cannot read registry keys", e); }
        return result;
    }
    private static boolean contains(String registry, org.bukkit.NamespacedKey key) { return KEYS.getOrDefault(registry, java.util.Set.of()).contains(key.toString()); }
    @SuppressWarnings("unchecked") @Override public <T extends org.bukkit.Keyed> org.bukkit.Registry<T> getRegistry(Class<T> type) {
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
        if (type == org.bukkit.damage.DamageType.class) return (org.bukkit.Registry<T>) cached("damage_type", FerriteRegistryAccess::damageTypeRegistry);
        return unavailable(type.getName());
    }
    @Override public <T extends org.bukkit.Keyed> org.bukkit.Registry<T> getRegistry(io.papermc.paper.registry.RegistryKey<T> type) {
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
        if (type.equals(io.papermc.paper.registry.RegistryKey.DAMAGE_TYPE)) return cast(cached("damage_type", FerriteRegistryAccess::damageTypeRegistry));
        return unavailable(type.key().toString());
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
        return (org.bukkit.Registry<T>) new BaseRegistry<org.bukkit.inventory.MenuType>("menu") {
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
        return (org.bukkit.Registry<T>) new BaseRegistry<org.bukkit.inventory.ItemType>("item") {
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
        return new BaseRegistry<>("block") {
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
        return new BaseRegistry<>("entity_type") {
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
        };
    }
    private static org.bukkit.Registry<org.bukkit.potion.PotionEffectType> potionRegistry() {
        return new BaseRegistry<>("mob_effect") {
            private final java.util.Map<org.bukkit.NamespacedKey, org.bukkit.potion.PotionEffectType> entries = new java.util.concurrent.ConcurrentHashMap<>();
            @Override public org.bukkit.potion.PotionEffectType get(org.bukkit.NamespacedKey key) {
                if (!contains("mob_effect", key)) return null;
                var value = entries.get(key); if (value != null) return value;
                value = new FerritePotionEffectType(key); var prior = entries.putIfAbsent(key, value); return prior == null ? value : prior;
            }
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
                T value = factory != null ? factory.apply(key) : type.cast(java.lang.reflect.Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[]{type}, (proxy, method, args) -> switch (method.getName()) {
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
    private static <T extends org.bukkit.Keyed> org.bukkit.Registry<T> unavailable(String name) { return new BaseRegistry<T>(name) {}; }
    private abstract static class BaseRegistry<T extends org.bukkit.Keyed> implements org.bukkit.Registry<T> {
        private final String name;
        BaseRegistry(String name) { this.name = name; }
        private UnsupportedOperationException unavailable() { return new UnsupportedOperationException("Ferrite: registry data not implemented: " + name); }
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
