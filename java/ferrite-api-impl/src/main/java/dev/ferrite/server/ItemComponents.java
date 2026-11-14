package dev.ferrite.server;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * {@code UnsafeValues#modifyItemStack}: applies a vanilla component list ({@code [custom_name='x',damage=3]}) to a stack.
 * The components Bukkit's {@code ItemMeta} can hold are applied; any other component is refused by name so nothing is
 * silently dropped.
 */
final class ItemComponents {
    private final String text;
    private int pos;

    private ItemComponents(String text) { this.text = text; }

    static org.bukkit.inventory.ItemStack apply(org.bukkit.inventory.ItemStack stack, String arguments) {
        var reader = new ItemComponents(arguments.trim());
        var components = reader.components();
        var result = stack.clone();
        if (components.isEmpty()) return result;
        var meta = result.getItemMeta();
        for (var entry : components.entrySet()) {
            String name = entry.getKey().startsWith("minecraft:") ? entry.getKey().substring(10) : entry.getKey();
            if (name.startsWith("!")) throw new UnsupportedOperationException("Ferrite: removing the '" + name.substring(1) + "' component is not implemented");
            set(meta, name, entry.getValue());
        }
        result.setItemMeta(meta);
        return result;
    }

    private static void set(org.bukkit.inventory.meta.ItemMeta meta, String name, Object value) {
        var gson = net.kyori.adventure.text.serializer.gson.GsonComponentSerializer.gson();
        switch (name) {
            case "custom_name" -> meta.displayName(component(gson, value));
            case "item_name" -> meta.itemName(component(gson, value));
            case "lore" -> {
                var lines = new ArrayList<net.kyori.adventure.text.Component>();
                for (var line : (List<?>) value) lines.add(component(gson, line));
                meta.lore(lines);
            }
            case "custom_model_data" -> {
                if (value instanceof Number n) meta.setCustomModelData(n.intValue());
                else if (value instanceof Map<?, ?> map && map.get("floats") instanceof List<?> floats && !floats.isEmpty()) meta.setCustomModelData(((Number) floats.get(0)).intValue());
                else throw new UnsupportedOperationException("Ferrite: custom_model_data of this shape is not implemented");
            }
            case "damage" -> { if (meta instanceof org.bukkit.inventory.meta.Damageable d) d.setDamage(((Number) value).intValue()); else throw new IllegalArgumentException("This item cannot be damaged"); }
            case "max_damage" -> { if (meta instanceof org.bukkit.inventory.meta.Damageable d) d.setMaxDamage(((Number) value).intValue()); }
            case "max_stack_size" -> meta.setMaxStackSize(((Number) value).intValue());
            case "unbreakable" -> meta.setUnbreakable(!Boolean.FALSE.equals(value));
            case "repair_cost" -> { if (meta instanceof org.bukkit.inventory.meta.Repairable r) r.setRepairCost(((Number) value).intValue()); }
            case "enchantments", "stored_enchantments" -> {
                Map<?, ?> levels = value instanceof Map<?, ?> m && m.get("levels") instanceof Map<?, ?> inner ? inner : (Map<?, ?>) value;
                for (var e : levels.entrySet()) {
                    var key = org.bukkit.NamespacedKey.fromString(String.valueOf(e.getKey()));
                    var enchantment = key == null ? null : org.bukkit.Registry.ENCHANTMENT.get(key);
                    if (enchantment == null) throw new IllegalArgumentException("Unknown enchantment '" + e.getKey() + "'");
                    int level = ((Number) e.getValue()).intValue();
                    if (name.equals("stored_enchantments") && meta instanceof org.bukkit.inventory.meta.EnchantmentStorageMeta storage) storage.addStoredEnchant(enchantment, level, true);
                    else meta.addEnchant(enchantment, level, true);
                }
            }
            case "potion_contents" -> {
                if (!(meta instanceof org.bukkit.inventory.meta.PotionMeta potion)) throw new IllegalArgumentException("This item holds no potion: meta " + meta.getClass().getSimpleName());
                String id = value instanceof Map<?, ?> m && m.get("potion") != null ? String.valueOf(m.get("potion")) : String.valueOf(value);
                var key = org.bukkit.NamespacedKey.fromString(id);
                var type = key == null ? null : org.bukkit.Registry.POTION.get(key);
                if (type == null) throw new IllegalArgumentException("Unknown potion '" + id + "'");
                potion.setBasePotionType(type);
            }
            default -> {
                if (!FerriteItemMeta.keepComponent(meta, name, value)) throw new UnsupportedOperationException("Ferrite: item component '" + name + "' is not implemented: ItemMeta has no field for it");
            }
        }
    }

    private static net.kyori.adventure.text.Component component(net.kyori.adventure.text.serializer.gson.GsonComponentSerializer gson, Object value) {
        String s = String.valueOf(value);
        if (s.startsWith("{") || s.startsWith("[") || s.startsWith("\"")) return gson.deserialize(s);
        return net.kyori.adventure.text.Component.text(s);
    }

    // ---- a small SNBT reader: compounds, lists, quoted and bare strings, numbers and booleans ----------
    private Map<String, Object> components() {
        var out = new LinkedHashMap<String, Object>();
        if (text.isEmpty()) return out;
        if (text.charAt(0) != '[') throw new IllegalArgumentException("Expected '[' in item components: " + text);
        pos = 1;
        skip();
        while (pos < text.length() && text.charAt(pos) != ']') {
            String key = bare();
            skip();
            Object value = Boolean.TRUE;
            if (peek() == '=') { pos++; skip(); value = value(); }
            out.put(key, value);
            skip();
            if (peek() == ',') { pos++; skip(); }
        }
        return out;
    }

    private char peek() { return pos < text.length() ? text.charAt(pos) : '\0'; }
    private void skip() { while (pos < text.length() && Character.isWhitespace(text.charAt(pos))) pos++; }

    private String bare() { return bare(true); }

    /** A bare word; namespaced ids ({@code minecraft:stone}) keep their colon except where ':' separates a compound key. */
    private String bare(boolean colonInside) {
        int start = pos;
        String stop = colonInside ? "=,]}" : "=,]}:";
        while (pos < text.length() && stop.indexOf(text.charAt(pos)) < 0 && !Character.isWhitespace(text.charAt(pos))) pos++;
        if (start == pos) throw new IllegalArgumentException("Unexpected '" + peek() + "' at " + pos + " in item components");
        return text.substring(start, pos);
    }

    private Object value() {
        skip();
        char c = peek();
        if (c == '{') {
            pos++;
            var map = new LinkedHashMap<String, Object>();
            skip();
            while (peek() != '}') {
                if (pos >= text.length()) throw new IllegalArgumentException("Unclosed '{' in item components");
                String key = (peek() == '"' || peek() == '\'') ? quoted() : bare(false);
                skip();
                if (peek() != ':') throw new IllegalArgumentException("Expected ':' at " + pos);
                pos++;
                map.put(key, value());
                skip();
                if (peek() == ',') { pos++; skip(); }
            }
            pos++;
            return map;
        }
        if (c == '[') {
            pos++;
            var list = new ArrayList<Object>();
            skip();
            while (peek() != ']') {
                if (pos >= text.length()) throw new IllegalArgumentException("Unclosed '[' in item components");
                list.add(value());
                skip();
                if (peek() == ',') { pos++; skip(); }
            }
            pos++;
            return list;
        }
        if (c == '"' || c == '\'') return quoted();
        String word = bare();
        return switch (word) {
            case "true" -> Boolean.TRUE;
            case "false" -> Boolean.FALSE;
            default -> number(word);
        };
    }

    private static Object number(String word) {
        String body = word.length() > 1 ? word.substring(0, word.length() - 1) : word;
        try {
            return switch (Character.toLowerCase(word.charAt(word.length() - 1))) {
                case 'b' -> Byte.parseByte(body);
                case 's' -> Short.parseShort(body);
                case 'l' -> Long.parseLong(body);
                case 'f' -> Float.parseFloat(body);
                case 'd' -> Double.parseDouble(body);
                default -> word.contains(".") ? (Object) Double.parseDouble(word) : (Object) Integer.parseInt(word);
            };
        } catch (NumberFormatException notNumber) { return word; }
    }

    private String quoted() {
        char quote = text.charAt(pos++);
        var out = new StringBuilder();
        while (pos < text.length() && text.charAt(pos) != quote) {
            char c = text.charAt(pos++);
            if (c == '\\' && pos < text.length()) c = text.charAt(pos++);
            out.append(c);
        }
        if (pos >= text.length()) throw new IllegalArgumentException("Unclosed string in item components");
        pos++;
        return out.toString();
    }
}
