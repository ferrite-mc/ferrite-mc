package dev.ferrite.server;

import java.util.ArrayList;
import java.util.HexFormat;
import java.util.List;
import java.util.Map;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.format.NamedTextColor;
import net.kyori.adventure.text.format.TextColor;
import net.kyori.adventure.text.format.TextDecoration;
import net.kyori.adventure.text.serializer.gson.GsonComponentSerializer;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.Damageable;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.inventory.meta.MapMeta;
import org.bukkit.inventory.meta.Repairable;
import org.bukkit.persistence.PersistentDataContainer;
import org.bukkit.persistence.PersistentDataType;

/** Decodes the 26.3 item save NBT carried by a player snapshot into a JVM item. */
final class FerriteItemNbt {
    private FerriteItemNbt() {}

    static ItemStack decode(String encoded) {
        var root = StructureNbt.compound(HexFormat.of().formatHex(java.util.Base64.getDecoder().decode(encoded)));
        if (root == null) throw new IllegalArgumentException("Ferrite: invalid inventory item NBT");
        if (!(root.get("id") instanceof String id)) throw new IllegalArgumentException("Ferrite: inventory item has no id");
        var material = Material.matchMaterial(id);
        if (material == null) throw new IllegalArgumentException("Ferrite: unknown inventory material " + id);
        int count = root.get("count") instanceof Number number ? number.intValue() : 1;
        if (count < 0 || count > 99) throw new IllegalArgumentException("Ferrite: inventory item count " + count);
        var stack = new FerriteItemStack(material, count);
        var meta = stack.getItemMeta();
        if (meta == null) return stack;
        if (root.get("components") instanceof Map<?, ?> components) applyComponents(meta, components);
        if (root.get("display") instanceof Map<?, ?> display) applyDisplay(meta, display);
        if (root.get("Enchantments") instanceof List<?> enchantments) applyEnchantmentList(meta, enchantments);
        if (truthy(root.get("Unbreakable"))) meta.setUnbreakable(true);
        if (root.get("Damage") instanceof Number damage && meta instanceof Damageable damageable) damageable.setDamage(damage.intValue());
        if (root.get("RepairCost") instanceof Number repair && meta instanceof Repairable repairable) repairable.setRepairCost(repair.intValue());
        if (root.get("CustomModelData") instanceof Number model) meta.setCustomModelData(model.intValue());
        stack.setItemMeta(meta);
        return stack;
    }

    private static void applyComponents(ItemMeta meta, Map<?, ?> components) {
        if (component(components, "minecraft:item_name", "item_name") instanceof Object name) meta.displayName(text(name));
        if (component(components, "minecraft:custom_name", "custom_name") instanceof Object name) meta.displayName(text(name));
        if (component(components, "minecraft:lore", "lore") instanceof List<?> lore) {
            var lines = new ArrayList<Component>();
            for (var line : lore) lines.add(text(line));
            meta.lore(lines);
        }
        if (component(components, "minecraft:damage", "damage") instanceof Number damage && meta instanceof Damageable damageable) {
            damageable.setDamage(damage.intValue());
        }
        if (component(components, "minecraft:repair_cost", "repair_cost") instanceof Number repair && meta instanceof Repairable repairable) {
            repairable.setRepairCost(repair.intValue());
        }
        if (component(components, "minecraft:unbreakable", "unbreakable") != null) meta.setUnbreakable(true);
        if (component(components, "minecraft:enchantments", "enchantments") instanceof Map<?, ?> enchantments) applyEnchantments(meta, enchantments);
        if (component(components, "minecraft:stored_enchantments", "stored_enchantments") instanceof Map<?, ?> enchantments) applyEnchantments(meta, enchantments);
        if (component(components, "minecraft:custom_model_data", "custom_model_data") instanceof Number model) {
            meta.setCustomModelData(model.intValue());
        } else if (component(components, "minecraft:custom_model_data", "custom_model_data") instanceof Map<?, ?> model
                && model.get("floats") instanceof List<?> floats && !floats.isEmpty() && floats.getFirst() instanceof Number number) {
            int value = number.intValue();
            if (Float.compare(number.floatValue(), value) == 0) meta.setCustomModelData(value);
        }
        if (component(components, "minecraft:map_id", "map_id") instanceof Number mapId && meta instanceof MapMeta map) {
            try {
                var view = org.bukkit.Bukkit.getMap(mapId.intValue());
                if (view != null) map.setMapId(view.getId());
            } catch (RuntimeException unavailable) {
                // A map that is not loaded still keeps its material and the rest of its metadata.
            }
        }
        if (component(components, "minecraft:custom_data", "custom_data") instanceof Map<?, ?> data) applyCustomData(meta, data);
    }

    private static Object component(Map<?, ?> components, String namespaced, String plain) {
        var value = components.get(namespaced);
        return value != null ? value : components.get(plain);
    }

    private static void applyDisplay(ItemMeta meta, Map<?, ?> display) {
        var gson = GsonComponentSerializer.gson();
        if (display.get("Name") instanceof String name) {
            try {
                meta.displayName(gson.deserialize(name));
            } catch (RuntimeException malformed) {
                // Keep the item's default name rather than failing the whole snapshot.
            }
        }
        if (display.get("Lore") instanceof List<?> lore) {
            var lines = new ArrayList<Component>();
            for (var line : lore) {
                if (!(line instanceof String text)) continue;
                try {
                    lines.add(gson.deserialize(text));
                } catch (RuntimeException malformed) {
                    lines.add(Component.text(text));
                }
            }
            meta.lore(lines);
        }
    }

    private static void applyEnchantments(ItemMeta meta, Map<?, ?> enchantments) {
        var levels = enchantments.get("levels");
        Map<?, ?> map = levels instanceof Map<?, ?> nested ? nested : enchantments;
        for (var entry : map.entrySet()) {
            if (entry.getValue() instanceof Number level) addEnchantment(meta, String.valueOf(entry.getKey()), level.intValue());
        }
    }

    private static void applyEnchantmentList(ItemMeta meta, List<?> enchantments) {
        for (var entry : enchantments) {
            if (!(entry instanceof Map<?, ?> enchantment)) continue;
            if (enchantment.get("id") instanceof String id && enchantment.get("lvl") instanceof Number level) {
                addEnchantment(meta, id, level.intValue());
            }
        }
    }

    private static void addEnchantment(ItemMeta meta, String id, int level) {
        var key = NamespacedKey.fromString(id);
        if (key == null) return;
        Enchantment enchantment;
        try {
            enchantment = Registry.ENCHANTMENT.get(key);
        } catch (RuntimeException unavailable) {
            return;
        }
        if (enchantment != null) meta.addEnchant(enchantment, level, true);
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private static void applyCustomData(ItemMeta meta, Map<?, ?> data) {
        var container = meta.getPersistentDataContainer();
        for (var entry : data.entrySet()) {
            var key = NamespacedKey.fromString(String.valueOf(entry.getKey()));
            if (key == null) continue;
            Object value = entry.getValue();
            if (value instanceof Byte byteValue) container.set(key, PersistentDataType.BYTE, byteValue);
            else if (value instanceof Short shortValue) container.set(key, PersistentDataType.SHORT, shortValue);
            else if (value instanceof Integer intValue) container.set(key, PersistentDataType.INTEGER, intValue);
            else if (value instanceof Long longValue) container.set(key, PersistentDataType.LONG, longValue);
            else if (value instanceof Float floatValue) container.set(key, PersistentDataType.FLOAT, floatValue);
            else if (value instanceof Double doubleValue) container.set(key, PersistentDataType.DOUBLE, doubleValue);
            else if (value instanceof String stringValue) container.set(key, PersistentDataType.STRING, stringValue);
            else if (value instanceof byte[] bytes) container.set(key, PersistentDataType.BYTE_ARRAY, bytes);
            else if (value instanceof int[] ints) container.set(key, PersistentDataType.INTEGER_ARRAY, ints);
            else if (value instanceof long[] longs) container.set(key, PersistentDataType.LONG_ARRAY, longs);
            else if (value instanceof Map<?, ?> nested) {
                var child = new FerriteDataContainer();
                for (var childEntry : nested.entrySet()) {
                    var childKey = NamespacedKey.fromString(String.valueOf(childEntry.getKey()));
                    if (childKey == null) continue;
                    putRaw(child, childKey, childEntry.getValue());
                }
                container.set(key, PersistentDataType.TAG_CONTAINER, child);
            }
        }
    }

    private static void putRaw(PersistentDataContainer container, NamespacedKey key, Object value) {
        if (value instanceof Map<?, ?> nested) {
            var child = new FerriteDataContainer();
            for (var entry : nested.entrySet()) {
                var childKey = NamespacedKey.fromString(String.valueOf(entry.getKey()));
                if (childKey != null) putRaw(child, childKey, entry.getValue());
            }
            set(container, key, PersistentDataType.TAG_CONTAINER, child);
        } else if (value instanceof Byte v) set(container, key, PersistentDataType.BYTE, v);
        else if (value instanceof Short v) set(container, key, PersistentDataType.SHORT, v);
        else if (value instanceof Integer v) set(container, key, PersistentDataType.INTEGER, v);
        else if (value instanceof Long v) set(container, key, PersistentDataType.LONG, v);
        else if (value instanceof Float v) set(container, key, PersistentDataType.FLOAT, v);
        else if (value instanceof Double v) set(container, key, PersistentDataType.DOUBLE, v);
        else if (value instanceof String v) set(container, key, PersistentDataType.STRING, v);
        else if (value instanceof byte[] v) set(container, key, PersistentDataType.BYTE_ARRAY, v);
        else if (value instanceof int[] v) set(container, key, PersistentDataType.INTEGER_ARRAY, v);
        else if (value instanceof long[] v) set(container, key, PersistentDataType.LONG_ARRAY, v);
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private static void set(PersistentDataContainer container, NamespacedKey key, PersistentDataType type, Object value) {
        container.set(key, type, value);
    }

    private static boolean truthy(Object value) {
        return value instanceof Boolean bool ? bool : value instanceof Number number && number.byteValue() != 0;
    }

    /** Text components in item NBT are either a plain string or a JSON-like compound. */
    private static Component text(Object tag) {
        if (tag instanceof String text) return Component.text(text);
        if (!(tag instanceof Map<?, ?> map)) return Component.text(String.valueOf(tag));
        Component result;
        if (map.get("text") instanceof String text) result = Component.text(text);
        else if (map.get("translate") instanceof String key) {
            var args = new ArrayList<Component>();
            if (map.get("with") instanceof List<?> with) for (var value : with) args.add(text(value));
            result = Component.translatable(key, args.toArray(Component[]::new));
        } else if (map.get("key") instanceof String key) result = Component.keybind(key);
        else if (map.get("selector") instanceof String selector) result = Component.selector(selector);
        else result = Component.empty();
        if (map.get("color") instanceof String color) {
            TextColor resolved = color.startsWith("#") ? TextColor.fromHexString(color) : NamedTextColor.NAMES.value(color);
            if (resolved != null) result = result.color(resolved);
        }
        if (truthy(map.get("bold"))) result = result.decorate(TextDecoration.BOLD);
        if (truthy(map.get("italic"))) result = result.decorate(TextDecoration.ITALIC);
        if (truthy(map.get("underlined"))) result = result.decorate(TextDecoration.UNDERLINED);
        if (truthy(map.get("strikethrough"))) result = result.decorate(TextDecoration.STRIKETHROUGH);
        if (truthy(map.get("obfuscated"))) result = result.decorate(TextDecoration.OBFUSCATED);
        if (map.get("extra") instanceof List<?> extra) for (var child : extra) result = result.append(text(child));
        return result;
    }
}
