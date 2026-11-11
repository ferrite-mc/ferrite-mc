package dev.ferrite.server;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import io.papermc.paper.registry.RegistryKey;
import io.papermc.paper.registry.TypedKey;
import io.papermc.paper.registry.set.RegistryKeySet;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.gson.GsonComponentSerializer;
import org.bukkit.Keyed;
import org.bukkit.NamespacedKey;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.enchantments.EnchantmentTarget;
import org.bukkit.inventory.EquipmentSlotGroup;
import org.bukkit.inventory.ItemType;

/**
 * Enchantment definitions read from the host's extracted vanilla data pack.
 *
 * <p>The 26.3 data pack stores everything that is still data-driven in
 * {@code data/<namespace>/enchantment/<key>.json}: level range, weight, slots, item and
 * enchantment sets, and the description component. The legacy Bukkit groupings are no longer
 * stored per enchantment; treasure, curses and the other tags live under
 * {@code data/<namespace>/tags/enchantment}, item tags under {@code data/<namespace>/tags/item}.
 */
final class FerriteEnchantmentData {
    private FerriteEnchantmentData() {}

    /** One enchantment's data-pack definition, with every reference already resolved to keys. */
    record Definition(
            int maxLevel,
            int startLevel,
            int weight,
            EnchantmentTarget itemTarget,
            Set<EquipmentSlotGroup> slots,
            RegistryKeySet<ItemType> supportedItems,
            RegistryKeySet<ItemType> primaryItems,
            RegistryKeySet<Enchantment> exclusiveWith,
            Component description,
            boolean treasure,
            boolean cursed,
            int anvilCost,
            Cost minCost,
            Cost maxCost) {}

    /** Vanilla {@code Enchantment.Cost}: {@code base + per_level_above_first * (level - 1)}. */
    record Cost(int base, int perLevelAboveFirst) {
        int calculate(int level) { return base + perLevelAboveFirst * (level - 1); }
    }

    private static final ConcurrentHashMap<NamespacedKey, Definition> DEFINITIONS = new ConcurrentHashMap<>();
    private static final ConcurrentHashMap<String, Set<NamespacedKey>> TAGS = new ConcurrentHashMap<>();

    static Definition get(NamespacedKey key) {
        java.util.Objects.requireNonNull(key, "key");
        var cached = DEFINITIONS.get(key);
        if (cached != null) return cached;
        var json = read(key);
        var supportedReference = firstReference(json.get("supported_items"));
        var definition = new Definition(
                integer(json, "max_level", key),
                json.has("min_level") ? integer(json, "min_level", key) : 1,
                integer(json, "weight", key),
                itemTarget(supportedReference),
                slots(json, key),
                keySet(RegistryKey.ITEM, references(json.get("supported_items"), "item", key, "supported_items")),
                json.has("primary_items")
                        ? keySet(RegistryKey.ITEM, references(json.get("primary_items"), "item", key, "primary_items"))
                        : null,
                json.has("exclusive_set")
                        ? keySet(RegistryKey.ENCHANTMENT, references(json.get("exclusive_set"), "enchantment", key, "exclusive_set"))
                        : keySet(RegistryKey.ENCHANTMENT, Set.of()),
                description(json, key),
                tagged("enchantment", NamespacedKey.minecraft("treasure"), key),
                tagged("enchantment", NamespacedKey.minecraft("curse"), key),
                integer(json, "anvil_cost", key),
                cost(json, "min_cost", key),
                cost(json, "max_cost", key));
        var previous = DEFINITIONS.putIfAbsent(key, definition);
        return previous == null ? definition : previous;
    }

    /** The legacy Paper rarity is the weight bucket vanilla used before 1.20.5. */
    static io.papermc.paper.enchantments.EnchantmentRarity rarity(Definition definition, NamespacedKey key) {
        return switch (definition.weight()) {
            case 10 -> io.papermc.paper.enchantments.EnchantmentRarity.COMMON;
            case 5 -> io.papermc.paper.enchantments.EnchantmentRarity.UNCOMMON;
            case 2 -> io.papermc.paper.enchantments.EnchantmentRarity.RARE;
            case 1 -> io.papermc.paper.enchantments.EnchantmentRarity.VERY_RARE;
            default -> throw new UnsupportedOperationException(
                    "Ferrite: enchantment " + key + " has no rarity for weight " + definition.weight());
        };
    }

    private static JsonObject read(NamespacedKey key) {
        var file = Bootstrap.dataFile("data/" + key.getNamespace() + "/enchantment/" + key.getKey() + ".json");
        if (file == null || !file.isFile())
            throw new UnsupportedOperationException("Ferrite: no enchantment data for " + key);
        try {
            return JsonParser.parseString(Files.readString(file.toPath(), StandardCharsets.UTF_8)).getAsJsonObject();
        } catch (IOException e) {
            throw new java.io.UncheckedIOException(e);
        }
    }

    private static int integer(JsonObject json, String field, NamespacedKey key) {
        var value = json.get(field);
        if (value == null || !value.isJsonPrimitive() || !value.getAsJsonPrimitive().isNumber())
            throw new UnsupportedOperationException("Ferrite: enchantment " + key + " has no " + field);
        return value.getAsInt();
    }

    private static Cost cost(JsonObject json, String field, NamespacedKey key) {
        var value = json.get(field);
        if (value == null || !value.isJsonObject()) throw new UnsupportedOperationException("Ferrite: enchantment " + key + " has no " + field);
        return new Cost(integer(value.getAsJsonObject(), "base", key), integer(value.getAsJsonObject(), "per_level_above_first", key));
    }

    private static Component description(JsonObject json, NamespacedKey key) {
        var value = json.get("description");
        if (value == null)
            throw new UnsupportedOperationException("Ferrite: enchantment " + key + " has no description");
        try {
            return GsonComponentSerializer.gson().deserialize(value.toString());
        } catch (RuntimeException e) {
            throw new UnsupportedOperationException("Ferrite: invalid description for enchantment " + key, e);
        }
    }

    private static Set<EquipmentSlotGroup> slots(JsonObject json, NamespacedKey key) {
        var values = json.getAsJsonArray("slots");
        if (values == null || values.size() == 0)
            throw new UnsupportedOperationException("Ferrite: enchantment " + key + " has no slots");
        var slots = new LinkedHashSet<EquipmentSlotGroup>();
        for (var value : values) {
            var name = value.getAsString().toUpperCase(java.util.Locale.ROOT);
            try {
                slots.add(EquipmentSlotGroup.getByName(name));
            } catch (IllegalArgumentException e) {
                throw new UnsupportedOperationException(
                        "Ferrite: unknown enchantment slot " + value.getAsString() + " for " + key, e);
            }
        }
        return Set.copyOf(slots);
    }

    /**
     * The legacy target follows the {@code enchantable/*} item tag the enchantment supports. A
     * data pack that uses an item list (or an unknown tag) has no single legacy target, so the
     * getter stays explicit instead of guessing.
     */
    private static EnchantmentTarget itemTarget(String reference) {
        if (reference == null || !reference.startsWith("#")) return null;
        var tag = NamespacedKey.fromString(reference.substring(1));
        if (tag == null) return null;
        var path = tag.getKey();
        if (path.startsWith("enchantable/")) path = path.substring("enchantable/".length());
        return switch (path) {
            case "armor" -> EnchantmentTarget.ARMOR;
            case "head_armor" -> EnchantmentTarget.ARMOR_HEAD;
            case "chest_armor" -> EnchantmentTarget.ARMOR_TORSO;
            case "leg_armor" -> EnchantmentTarget.ARMOR_LEGS;
            case "foot_armor" -> EnchantmentTarget.ARMOR_FEET;
            case "weapon", "melee_weapon", "sharp_weapon", "mace", "lunge", "fire_aspect", "sweeping" ->
                    EnchantmentTarget.WEAPON;
            case "mining", "mining_loot" -> EnchantmentTarget.TOOL;
            case "bow" -> EnchantmentTarget.BOW;
            case "crossbow" -> EnchantmentTarget.CROSSBOW;
            case "fishing" -> EnchantmentTarget.FISHING_ROD;
            case "trident" -> EnchantmentTarget.TRIDENT;
            case "durability" -> EnchantmentTarget.BREAKABLE;
            case "equippable" -> EnchantmentTarget.WEARABLE;
            case "vanishing" -> EnchantmentTarget.VANISHABLE;
            default -> null;
        };
    }

    private static String firstReference(JsonElement element) {
        if (element == null) return null;
        if (element.isJsonPrimitive() && element.getAsJsonPrimitive().isString()) return element.getAsString();
        if (element.isJsonArray()) {
            for (var child : element.getAsJsonArray()) {
                var value = firstReference(child);
                if (value != null) return value;
            }
        }
        return null;
    }

    /** Resolves a data-pack set: a single key, a {@code #tag}, or a list of either. */
    private static Set<NamespacedKey> references(JsonElement element, String registry, NamespacedKey owner, String field) {
        var keys = new LinkedHashSet<NamespacedKey>();
        collect(element, registry, owner, field, keys);
        return keys;
    }

    private static void collect(JsonElement element, String registry, NamespacedKey owner, String field, Set<NamespacedKey> keys) {
        if (element == null) throw missing(owner, field);
        if (element.isJsonPrimitive() && element.getAsJsonPrimitive().isString()) {
            var value = element.getAsString();
            if (value.startsWith("#")) keys.addAll(tag(registry, parse(value.substring(1), owner, field)));
            else keys.add(parse(value, owner, field));
            return;
        }
        if (element.isJsonArray()) {
            for (var child : element.getAsJsonArray()) collect(child, registry, owner, field, keys);
            return;
        }
        throw new UnsupportedOperationException("Ferrite: enchantment " + owner + " has an invalid " + field);
    }

    private static NamespacedKey parse(String value, NamespacedKey owner, String field) {
        var key = NamespacedKey.fromString(value);
        if (key == null)
            throw new UnsupportedOperationException("Ferrite: enchantment " + owner + " has an invalid " + field + ": " + value);
        return key;
    }

    private static UnsupportedOperationException missing(NamespacedKey owner, String field) {
        return new UnsupportedOperationException("Ferrite: enchantment " + owner + " has no " + field);
    }

    /** A tag value read once; nested {@code #tag} entries are expanded and cycles stop. */
    private static Set<NamespacedKey> tag(String registry, NamespacedKey key) {
        var cacheKey = registry + "/" + key;
        var cached = TAGS.get(cacheKey);
        if (cached != null) return cached;
        var resolved = Set.copyOf(readTag(registry, key, new HashSet<>()));
        var previous = TAGS.putIfAbsent(cacheKey, resolved);
        return previous == null ? resolved : previous;
    }

    private static Set<NamespacedKey> readTag(String registry, NamespacedKey key, Set<NamespacedKey> visiting) {
        var values = new LinkedHashSet<NamespacedKey>();
        if (!visiting.add(key)) return values;
        var file = Bootstrap.dataFile("data/" + key.getNamespace() + "/tags/" + registry + "/" + key.getKey() + ".json");
        if (file == null || !file.isFile()) {
            visiting.remove(key);
            return values;
        }
        try {
            var json = JsonParser.parseString(Files.readString(file.toPath(), StandardCharsets.UTF_8)).getAsJsonObject();
            var entries = json.getAsJsonArray("values");
            if (entries != null) for (var entry : entries) {
                String id = entry.isJsonPrimitive() ? entry.getAsString()
                        : entry.isJsonObject() && entry.getAsJsonObject().has("id")
                                ? entry.getAsJsonObject().get("id").getAsString()
                                : null;
                if (id == null) continue;
                if (id.startsWith("#")) {
                    var nested = NamespacedKey.fromString(id.substring(1));
                    if (nested != null) values.addAll(readTag(registry, nested, visiting));
                } else {
                    var value = NamespacedKey.fromString(id);
                    if (value != null) values.add(value);
                }
            }
        } catch (IOException e) {
            throw new java.io.UncheckedIOException(e);
        } finally {
            visiting.remove(key);
        }
        return values;
    }

    private static boolean tagged(String registry, NamespacedKey tag, NamespacedKey key) {
        return tag(registry, tag).contains(key);
    }

    private static <T extends Keyed> RegistryKeySet<T> keySet(RegistryKey<T> registry, Set<NamespacedKey> keys) {
        var typed = new ArrayList<TypedKey<T>>(keys.size());
        for (var key : keys) typed.add(registry.typedKey(key));
        return new KeySet<>(registry, List.copyOf(typed));
    }

    /** A small immutable key set; the registry only resolves it when the caller asks. */
    private record KeySet<T extends Keyed>(RegistryKey<T> registryKey, List<TypedKey<T>> keys) implements RegistryKeySet<T> {
        @Override public java.util.Collection<TypedKey<T>> values() { return keys; }

        @Override public java.util.Collection<T> resolve(org.bukkit.Registry<T> registry) {
            var resolved = new ArrayList<T>(keys.size());
            for (var key : keys) {
                var value = registry.get(key.key());
                if (value != null) resolved.add(value);
            }
            return resolved;
        }

        @Override public boolean contains(TypedKey<T> key) {
            return keys.stream().anyMatch(candidate -> candidate.key().equals(key.key()));
        }
    }
}
