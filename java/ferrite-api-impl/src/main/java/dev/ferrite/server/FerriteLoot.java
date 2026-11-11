package dev.ferrite.server;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.entity.Ageable;
import org.bukkit.entity.Entity;
import org.bukkit.entity.HumanEntity;
import org.bukkit.entity.LivingEntity;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.Damageable;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.loot.LootContext;

/**
 * Vanilla 26.3 loot table evaluation for the Bukkit API layer.
 *
 * <p>Tables are read from a generated data pack ({@code data/<namespace>/loot_table/...json})
 * together with the sibling {@code predicate}, {@code tags/item}, {@code tags/enchantment} and
 * {@code tags/entity_type} directories. Pools, rolls, bonus rolls, entries, conditions and
 * functions are evaluated in vanilla order, and every produced stack is a
 * {@link FerriteItemStack} so plugin code sees the same item/meta path as the rest of Ferrite.
 *
 * <p>Bukkit's {@link LootContext} only exposes the location, luck, killer and looted entity, so
 * the killer's main hand is the tool for tool/fortune predicates. Conditions and functions that
 * depend on information Bukkit does not expose (explosion radius, block entity data, scoreboards,
 * item components this API does not model) are treated as satisfied/opaque, while a condition
 * whose exposed predicate fails returns no loot.
 */
final class FerriteLoot {
    private FerriteLoot() {}

    /** A loaded table; references to other tables are resolved while loading. */
    static final class Table {
        static final Table EMPTY = new Table(List.of(), List.of());
        final List<Pool> pools;
        final List<Function> functions;

        Table(List<Pool> pools, List<Function> functions) {
            this.pools = pools;
            this.functions = functions;
        }
    }

    private static final class Pool {
        final NumberProvider rolls;
        final NumberProvider bonusRolls;
        final List<Condition> conditions;
        final List<Function> functions;
        final List<Entry> entries;

        Pool(NumberProvider rolls, NumberProvider bonusRolls, List<Condition> conditions,
                List<Function> functions, List<Entry> entries) {
            this.rolls = rolls;
            this.bonusRolls = bonusRolls;
            this.conditions = conditions;
            this.functions = functions;
            this.entries = entries;
        }
    }

    private static final class Entry {
        final int weight;
        final int quality;
        final List<Condition> conditions;
        final List<Function> functions;
        final Kind kind;

        Entry(int weight, int quality, List<Condition> conditions, List<Function> functions, Kind kind) {
            this.weight = weight;
            this.quality = quality;
            this.conditions = conditions;
            this.functions = functions;
            this.kind = kind;
        }
    }

    private sealed interface Kind permits ItemKind, TagKind, ChildrenKind, TableKind, EmptyKind {}

    private record ItemKind(String id) implements Kind {}

    private record TagKind(List<String> items) implements Kind {}

    private record ChildrenKind(char mode, List<Entry> children) implements Kind {}

    private record TableKind(Table table) implements Kind {}

    private record EmptyKind() implements Kind {}

    @FunctionalInterface
    private interface Condition {
        boolean test(Eval eval);
    }

    @FunctionalInterface
    private interface Function {
        void apply(ItemStack stack, Eval eval);
    }

    @FunctionalInterface
    private interface NumberProvider {
        int getInt(java.util.Random random);

        default float getFloat(java.util.Random random) {
            return getInt(random);
        }
    }

    private record EnchantmentPredicate(List<String> enchantments, int min, int max) {}

    /** The part of a loot context the evaluator reads, resolved once per roll. */
    private static final class Eval {
        final java.util.Random random;
        final float luck;
        final int looting;
        final Entity killer;
        final Entity thisEntity;
        final ItemStack tool;
        final Location location;

        Eval(java.util.Random random, float luck, int looting, Entity killer, Entity thisEntity,
                ItemStack tool, Location location) {
            this.random = random;
            this.luck = luck;
            this.looting = looting;
            this.killer = killer;
            this.thisEntity = thisEntity;
            this.tool = tool;
            this.location = location;
        }

        int enchantmentLevel(String id) {
            var normalized = normalize(id);
            if (normalized.equals("minecraft:looting")) return looting;
            var enchantment = enchantment(normalized);
            if (enchantment == null || tool == null) return 0;
            return tool.getEnchantmentLevel(enchantment);
        }
    }

    /** Loads one table and every predicate/tag it references. */
    static Table load(Path dataFolder, NamespacedKey key) {
        if (dataFolder == null || key == null) return Table.EMPTY;
        var loader = new Loader(dataFolder);
        return loader.load(key.getNamespace() + ":" + key.getKey(), Set.of());
    }

    /** Rolls a loaded table. A null random is replaced with a fresh source. */
    static List<ItemStack> roll(Table table, java.util.Random random, LootContext context) {
        var rng = random == null ? new java.util.Random() : random;
        var eval = eval(rng, context);
        var out = new ArrayList<ItemStack>();
        rollInto(table, eval, out);
        return out;
    }

    /** Fills empty inventory slots, splitting stacks to respect the inventory's stack size. */
    static void fill(Table table, Inventory inventory, java.util.Random random, LootContext context) {
        var rng = random == null ? new java.util.Random() : random;
        var drops = roll(table, rng, context);
        int maxStack = Math.max(1, inventory.getMaxStackSize());
        var slots = new ArrayList<Integer>();
        for (int slot = 0; slot < inventory.getSize(); slot++) {
            var present = inventory.getItem(slot);
            if (present == null || present.getType().isAir() || present.getAmount() <= 0) slots.add(slot);
        }
        java.util.Collections.shuffle(slots, rng);
        int next = 0;
        for (var stack : drops) {
            int limit = Math.max(1, Math.min(maxStack, stack.getMaxStackSize()));
            int remaining = stack.getAmount();
            while (remaining > 0 && next < slots.size()) {
                int count = Math.min(limit, remaining);
                var placed = stack.clone();
                placed.setAmount(count);
                inventory.setItem(slots.get(next++), placed);
                remaining -= count;
            }
            if (next >= slots.size()) return;
        }
    }

    private static Eval eval(java.util.Random random, LootContext context) {
        float luck = 0.0f;
        Entity killer = null;
        Entity thisEntity = null;
        Location location = null;
        if (context != null) {
            try {
                luck = context.getLuck();
            } catch (RuntimeException ignored) {
                // A host-backed context that cannot answer behaves like no luck.
            }
            try {
                killer = context.getKiller();
            } catch (RuntimeException ignored) {
                // No killer information.
            }
            try {
                thisEntity = context.getLootedEntity();
            } catch (RuntimeException ignored) {
                // No looted entity information.
            }
            try {
                location = context.getLocation();
            } catch (RuntimeException ignored) {
                // No location information.
            }
        }

        ItemStack tool = null;
        if (killer instanceof HumanEntity human) {
            try {
                tool = human.getInventory().getItemInMainHand();
            } catch (RuntimeException ignored) {
                // No tool, so tool predicates fail and fortune/looting stay at zero.
            }
        }

        int looting = 0;
        if (context != null) {
            try {
                // Paper marks this deprecated, but plugins still set it through the builder.
                looting = context.getLootingModifier();
            } catch (RuntimeException ignored) {
                // Fall back to the killer's main hand below.
            }
        }
        if (looting <= 0 && tool != null) {
            var enchantment = enchantment("minecraft:looting");
            if (enchantment != null) looting = tool.getEnchantmentLevel(enchantment);
        }

        return new Eval(random, luck, looting, killer, thisEntity, tool, location);
    }

    private static void rollInto(Table table, Eval eval, List<ItemStack> out) {
        int start = out.size();
        for (var pool : table.pools) {
            if (!conditions(pool.conditions, eval)) continue;
            int rolls = Math.max(0, pool.rolls.getInt(eval.random));
            float bonus = pool.bonusRolls.getFloat(eval.random) * eval.luck;
            if (bonus > 0) rolls += (int) Math.floor(bonus);
            for (int i = 0; i < rolls; i++) {
                var entry = select(pool.entries, eval);
                if (entry == null || !conditions(entry.conditions, eval)) continue;
                var stacks = new ArrayList<ItemStack>();
                expand(entry, eval, stacks);
                for (var stack : stacks) functions(pool.functions, stack, eval);
                out.addAll(stacks);
            }
        }
        for (int i = start; i < out.size(); i++) functions(table.functions, out.get(i), eval);
        out.removeIf(stack -> stack == null || stack.getType().isAir() || stack.getAmount() <= 0);
    }

    private static void expand(Entry entry, Eval eval, List<ItemStack> out) {
        int start = out.size();
        switch (entry.kind) {
            case ItemKind item -> add(out, item.id(), 1);
            case TagKind tag -> {
                if (!tag.items().isEmpty()) {
                    var id = tag.items().get(eval.random.nextInt(tag.items().size()));
                    add(out, id, 1);
                }
            }
            case ChildrenKind children -> {
                for (var child : children.children()) {
                    if (!conditions(child.conditions, eval)) continue;
                    int before = out.size();
                    expand(child, eval, out);
                    if (children.mode() == 'a') break;
                    if (children.mode() == 's' && out.size() > before) break;
                }
            }
            case TableKind reference -> rollInto(reference.table(), eval, out);
            case EmptyKind ignored -> {
                // Explicitly no drop.
            }
        }
        for (int i = start; i < out.size(); i++) functions(entry.functions, out.get(i), eval);
    }

    private static void add(List<ItemStack> out, String id, int amount) {
        var material = Material.matchMaterial(normalize(id));
        if (material == null || material.isAir()) return;
        out.add(new FerriteItemStack(material, amount));
    }

    private static Entry select(List<Entry> entries, Eval eval) {
        long total = 0;
        for (var entry : entries) total += weight(entry, eval.luck);
        if (total <= 0) return null;
        int roll = eval.random.nextInt((int) Math.min(total, Integer.MAX_VALUE));
        long cursor = 0;
        for (var entry : entries) {
            long weight = weight(entry, eval.luck);
            if (weight <= 0) continue;
            cursor += weight;
            if (roll < cursor) return entry;
        }
        return null;
    }

    private static long weight(Entry entry, float luck) {
        return Math.max(0, (long) Math.floor(entry.weight + luck * entry.quality));
    }

    private static boolean conditions(List<Condition> conditions, Eval eval) {
        for (var condition : conditions) if (!condition.test(eval)) return false;
        return true;
    }

    private static void functions(List<Function> functions, ItemStack stack, Eval eval) {
        for (var function : functions) function.apply(stack, eval);
        if (stack.getAmount() < 0) stack.setAmount(0);
    }

    private static String normalize(String id) {
        return id.contains(":") ? id : "minecraft:" + id;
    }

    private static Enchantment enchantment(String id) {
        var key = NamespacedKey.fromString(normalize(id));
        return key == null ? null : Registry.ENCHANTMENT.get(key);
    }

    /** Reads a generated table and all referenced files. */
    private static final class Loader {
        private final Path dataFolder;
        private final Map<String, Table> tables = new HashMap<>();
        private final Map<String, Condition> predicates = new HashMap<>();
        private final Map<String, List<String>> tags = new HashMap<>();
        private final Map<String, List<String>> enchantmentIds = new HashMap<>();
        private final Set<String> loading = new HashSet<>();
        private final Set<String> loadingPredicates = new HashSet<>();
        private final Set<String> loadingTags = new HashSet<>();
        private String namespace = "minecraft";

        Loader(Path dataFolder) {
            this.dataFolder = dataFolder;
        }

        Table load(String id) {
            return load(id, Set.of());
        }

        Table load(String id, Set<String> inline) {
            var normalized = normalize(id);
            var cached = tables.get(normalized);
            if (cached != null) return cached;
            if (!loading.add(normalized)) return Table.EMPTY;
            var previous = namespace;
            int colon = normalized.indexOf(':');
            if (colon > 0) namespace = normalized.substring(0, colon);
            try {
                var json = read("loot_table", normalized);
                if (json == null) {
                    tables.put(normalized, Table.EMPTY);
                    return Table.EMPTY;
                }
                var table = parseTable(normalized, json);
                tables.put(normalized, table);
                return table;
            } catch (RuntimeException malformed) {
                // A bad table or reference must not take the host down; it simply drops nothing.
                tables.put(normalized, Table.EMPTY);
                return Table.EMPTY;
            } finally {
                namespace = previous;
                loading.remove(normalized);
            }
        }

        private Table parseTable(String id, JsonObject json) {
            var previous = namespace;
            int colon = id.indexOf(':');
            if (colon > 0) namespace = id.substring(0, colon);
            try {
                var pools = new ArrayList<Pool>();
                for (var element : elements(json.get("pools"))) {
                    if (element.isJsonObject()) pools.add(parsePool(element.getAsJsonObject()));
                }
                return new Table(List.copyOf(pools), functions(json));
            } finally {
                namespace = previous;
            }
        }

        private Pool parsePool(JsonObject json) {
            var entries = new ArrayList<Entry>();
            for (var element : elements(json.get("entries"))) entries.add(parseEntry(element));
            return new Pool(
                    number(json.get("rolls"), 1),
                    number(json.get("bonus_rolls"), 0),
                    conditions(json),
                    functions(json),
                    List.copyOf(entries));
        }

        private Entry parseEntry(JsonElement element) {
            if (!element.isJsonObject()) return emptyEntry();
            var json = element.getAsJsonObject();
            int weight = integer(json.get("weight"), 1);
            int quality = integer(json.get("quality"), 0);
            var type = string(json, "type", "");
            Kind kind = switch (type) {
                case "minecraft:item" -> {
                    var name = string(json, "name", "");
                    yield name.isEmpty() ? new EmptyKind() : new ItemKind(normalize(name));
                }
                case "minecraft:tag" -> {
                    var items = itemRefs(member(json, "items", "tag"));
                    yield items.isEmpty() ? new EmptyKind() : new TagKind(items);
                }
                case "minecraft:alternatives", "minecraft:sequence", "minecraft:group" -> {
                    var children = new ArrayList<Entry>();
                    for (var child : elements(member(json, "children", "entries"))) children.add(parseEntry(child));
                    char mode = type.equals("minecraft:alternatives") ? 'a'
                            : type.equals("minecraft:sequence") ? 's' : 'g';
                    yield new ChildrenKind(mode, List.copyOf(children));
                }
                case "minecraft:loot_table" -> tableEntry(json);
                case "minecraft:empty" -> new EmptyKind();
                default -> new EmptyKind();
            };
            return new Entry(weight, quality, conditions(json), functions(json), kind);
        }

        private Kind tableEntry(JsonObject json) {
            var value = member(json, "value", "name");
            if (value == null) return new EmptyKind();
            if (value.isJsonPrimitive() && value.getAsJsonPrimitive().isString()) {
                return new TableKind(load(value.getAsString()));
            }
            if (value.isJsonObject()) {
                return new TableKind(parseTable(namespace + ":inline#" + Integer.toHexString(value.hashCode()),
                        value.getAsJsonObject()));
            }
            return new EmptyKind();
        }

        private Entry emptyEntry() {
            return new Entry(1, 0, List.of(), List.of(), new EmptyKind());
        }

        private List<Condition> conditions(JsonObject json) {
            var out = new ArrayList<Condition>();
            for (var element : elements(member(json, "condition", "conditions"))) {
                if (element.isJsonArray()) {
                    for (var term : element.getAsJsonArray()) {
                        var condition = condition(term);
                        if (condition != null) out.add(condition);
                    }
                } else {
                    var condition = condition(element);
                    if (condition != null) out.add(condition);
                }
            }
            return List.copyOf(out);
        }

        private Condition condition(JsonElement element) {
            if (element == null || element.isJsonNull()) return null;
            if (element.isJsonPrimitive() && element.getAsJsonPrimitive().isString()) {
                return predicate(element.getAsString());
            }
            if (element.isJsonArray()) {
                var terms = new ArrayList<Condition>();
                for (var term : element.getAsJsonArray()) {
                    var condition = condition(term);
                    if (condition != null) terms.add(condition);
                }
                return eval -> FerriteLoot.conditions(terms, eval);
            }
            if (!element.isJsonObject()) return eval -> true;
            var json = element.getAsJsonObject();
            return switch (string(json, "type", "")) {
                case "minecraft:random_chance" -> {
                    float chance = decimal(json.get("chance"), 0);
                    yield eval -> eval.random.nextFloat() < chance;
                }
                case "minecraft:random_chance_with_enchanted_bonus" -> {
                    float unenchanted = decimal(json.get("unenchanted_chance"), 0);
                    var enchanted = json.has("enchanted_chance") && json.get("enchanted_chance").isJsonObject()
                            ? json.getAsJsonObject("enchanted_chance") : null;
                    float base = enchanted == null ? unenchanted : decimal(enchanted.get("base"), unenchanted);
                    float perLevel = enchanted == null ? 0 : decimal(enchanted.get("per_level_above_first"), 0);
                    var enchantment = string(json, "enchantment", "minecraft:looting");
                    yield eval -> {
                        int level = eval.enchantmentLevel(enchantment);
                        float chance = level <= 0 ? unenchanted : base + perLevel * (level - 1);
                        return eval.random.nextFloat() < chance;
                    };
                }
                case "minecraft:random_chance_with_looting" -> {
                    float chance = decimal(json.get("chance"), 0);
                    float multiplier = decimal(json.get("looting_multiplier"), 0);
                    yield eval -> eval.random.nextFloat() < chance + multiplier * eval.looting;
                }
                case "minecraft:killed_by_player" -> eval -> eval.killer != null;
                case "minecraft:survives_explosion", "minecraft:value_check", "minecraft:weather_check",
                        "minecraft:location_check", "minecraft:damage_source_properties",
                        "minecraft:enchantment_active_check" -> eval -> true;
                case "minecraft:match_tool" -> matchTool(json.get("predicate"));
                case "minecraft:match_block" -> matchBlock(json);
                case "minecraft:block_state_property" -> matchBlock(json);
                case "minecraft:entity_properties" -> entityProperties(string(json, "entity", "this"), json.get("predicate"));
                case "minecraft:table_bonus" -> tableBonus(json);
                case "minecraft:inverted" -> {
                    var term = condition(json.get("term"));
                    yield eval -> term == null || !term.test(eval);
                }
                case "minecraft:any_of" -> anyOf(json);
                case "minecraft:all_of" -> allOf(json);
                case "minecraft:reference" -> predicate(string(json, "name", ""));
                default -> eval -> true;
            };
        }

        private Condition anyOf(JsonObject json) {
            var terms = new ArrayList<Condition>();
            for (var term : elements(json.get("terms"))) {
                var condition = condition(term);
                if (condition != null) terms.add(condition);
            }
            return eval -> {
                for (var term : terms) if (term.test(eval)) return true;
                return false;
            };
        }

        private Condition allOf(JsonObject json) {
            var terms = new ArrayList<Condition>();
            for (var term : elements(json.get("terms"))) {
                var condition = condition(term);
                if (condition != null) terms.add(condition);
            }
            return eval -> FerriteLoot.conditions(terms, eval);
        }

        private Condition matchTool(JsonElement value) {
            if (value == null || !value.isJsonObject()) return eval -> true;
            var predicate = value.getAsJsonObject();
            var items = itemRefs(predicate.get("items"));
            var enchantments = enchantmentPredicates(predicate);
            return eval -> toolMatches(eval.tool, items, enchantments);
        }

        private boolean toolMatches(ItemStack tool, List<String> items, List<EnchantmentPredicate> enchantments) {
            if (tool == null || tool.getType().isAir()) return false;
            if (!items.isEmpty() && !items.contains(tool.getType().getKey().toString())) return false;
            for (var predicate : enchantments) {
                int level = 0;
                for (var id : predicate.enchantments()) {
                    var enchantment = enchantment(id);
                    if (enchantment != null) level = Math.max(level, tool.getEnchantmentLevel(enchantment));
                }
                if (level < predicate.min() || level > predicate.max()) return false;
            }
            return true;
        }

        private List<EnchantmentPredicate> enchantmentPredicates(JsonObject predicate) {
            var out = new ArrayList<EnchantmentPredicate>();
            var predicates = predicate.has("predicates") && predicate.get("predicates").isJsonObject()
                    ? predicate.getAsJsonObject("predicates") : null;
            var entries = predicates == null ? null : predicates.get("minecraft:enchantments");
            if (entries != null && entries.isJsonArray()) {
                for (var element : entries.getAsJsonArray()) {
                    if (!element.isJsonObject()) continue;
                    var json = element.getAsJsonObject();
                    var ids = new ArrayList<String>();
                    for (var id : enchantmentRefs(json.get("enchantments"))) ids.add(id);
                    if (ids.isEmpty()) continue;
                    int min = 0;
                    int max = Integer.MAX_VALUE;
                    var levels = json.get("levels");
                    if (levels != null && levels.isJsonObject()) {
                        min = integer(levels.getAsJsonObject().get("min"), 0);
                        max = integer(levels.getAsJsonObject().get("max"), Integer.MAX_VALUE);
                    } else if (levels != null && levels.isJsonPrimitive() && levels.getAsJsonPrimitive().isNumber()) {
                        min = max = levels.getAsInt();
                    }
                    out.add(new EnchantmentPredicate(ids, min, max));
                }
            }
            return out;
        }

        private Condition matchBlock(JsonObject json) {
            var blocks = new ArrayList<String>();
            for (var id : blockRefs(member(json, "blocks", "block"))) blocks.add(id);
            var properties = new LinkedHashMap<String, String>();
            var state = member(json, "state", "properties");
            if (state != null && state.isJsonObject()) {
                for (var entry : state.getAsJsonObject().entrySet()) {
                    properties.put(entry.getKey(), entry.getValue().isJsonPrimitive()
                            ? entry.getValue().getAsString() : entry.getValue().toString());
                }
            }
            return eval -> blockMatches(eval.location, blocks, properties);
        }

        private boolean blockMatches(Location location, List<String> blocks, Map<String, String> properties) {
            if (location == null || location.getWorld() == null) return true;
            try {
                var block = location.getBlock();
                var id = block.getType().getKey().toString();
                if (!blocks.isEmpty() && !blocks.contains(id)) return false;
                if (properties.isEmpty()) return true;
                var state = block.getBlockData().getAsString();
                int open = state.indexOf('[');
                var present = new HashMap<String, String>();
                if (open >= 0) {
                    for (var pair : state.substring(open + 1, state.length() - 1).split(",")) {
                        int equals = pair.indexOf('=');
                        if (equals > 0) present.put(pair.substring(0, equals), pair.substring(equals + 1));
                    }
                }
                for (var entry : properties.entrySet()) {
                    if (!entry.getValue().equals(present.get(entry.getKey()))) return false;
                }
                return true;
            } catch (RuntimeException unavailable) {
                // A context without block data keeps the condition opaque.
                return true;
            }
        }

        private Condition entityProperties(String role, JsonElement value) {
            if (value == null || !value.isJsonObject()) return eval -> true;
            var predicate = value.getAsJsonObject();
            return eval -> {
                var entity = switch (role) {
                    case "this", "this_entity" -> eval.thisEntity;
                    case "killer", "attacker", "direct_attacker", "last_damage_player" -> eval.killer;
                    default -> null;
                };
                return entity != null && entityMatches(entity, predicate, eval);
            };
        }

        private boolean entityMatches(Entity entity, JsonObject predicate, Eval eval) {
            for (var entry : predicate.entrySet()) {
                String key = stripNamespace(entry.getKey());
                var value = entry.getValue();
                switch (key) {
                    case "type" -> {
                        if (!entityRefs(value).contains(entity.getType().getKey().toString())) return false;
                    }
                    case "flags" -> {
                        if (!value.isJsonObject()) continue;
                        for (var flag : value.getAsJsonObject().entrySet()) {
                            if (!flagMatches(entity, stripNamespace(flag.getKey()), flag.getValue().getAsBoolean())) return false;
                        }
                    }
                    case "vehicle" -> {
                        if (!value.isJsonObject()) continue;
                        var vehicle = entity.getVehicle();
                        if (vehicle == null) return false;
                        var type = member(value.getAsJsonObject(), "minecraft:entity_type", "entity_type", "type");
                        if (type != null && !entityRefs(type).contains(vehicle.getType().getKey().toString())) {
                            return false;
                        }
                    }
                    case "equipment" -> {
                        if (!value.isJsonObject()) continue;
                        var mainhand = member(value.getAsJsonObject(), "mainhand", "main_hand");
                        if (mainhand != null && mainhand.isJsonObject()) {
                            var toolPredicate = mainhand.getAsJsonObject();
                            if (!toolMatches(eval.tool, itemRefs(toolPredicate.get("items")),
                                    enchantmentPredicates(toolPredicate))) return false;
                        }
                    }
                    default -> {
                        // Opaque predicates (nbt, location, effects, ...) stay satisfied.
                    }
                }
            }
            return true;
        }

        private boolean flagMatches(Entity entity, String flag, boolean expected) {
            boolean actual = switch (flag) {
                case "is_on_fire" -> entity.getFireTicks() > 0;
                case "is_sneaking" -> entity.isSneaking();
                case "is_sprinting" -> entity instanceof org.bukkit.entity.Player player && player.isSprinting();
                case "is_swimming" -> entity instanceof org.bukkit.entity.LivingEntity living && living.isSwimming();
                case "is_on_ground" -> entity.isOnGround();
                case "is_invisible" -> entity.isInvisible();
                case "is_silent" -> entity.isSilent();
                case "is_gliding" -> entity instanceof org.bukkit.entity.LivingEntity living && living.isGliding();
                case "is_baby" -> entity instanceof Ageable ageable && !ageable.isAdult();
                default -> false;
            };
            return actual == expected;
        }

        private Condition tableBonus(JsonObject json) {
            var chances = new ArrayList<Float>();
            for (var element : elements(json.get("chances"))) chances.add(decimal(element, 0));
            var enchantment = string(json, "enchantment", "minecraft:fortune");
            return eval -> {
                if (chances.isEmpty()) return false;
                int level = Math.max(0, eval.enchantmentLevel(enchantment));
                float chance = chances.get(Math.min(level, chances.size() - 1));
                return eval.random.nextFloat() < chance;
            };
        }

        private Condition predicate(String id) {
            var normalized = qualify(id);
            var cached = predicates.get(normalized);
            if (cached != null) return cached;
            if (!loadingPredicates.add(normalized)) return eval -> true;
            try {
                var json = read("predicate", normalized);
                if (json == null) {
                    var satisfied = (Condition) eval -> true;
                    predicates.put(normalized, satisfied);
                    return satisfied;
                }
                var condition = json.has("condition") && !json.has("type")
                        ? condition(json.get("condition")) : condition(json);
                if (condition == null) condition = eval -> true;
                predicates.put(normalized, condition);
                return condition;
            } finally {
                loadingPredicates.remove(normalized);
            }
        }

        private List<Function> functions(JsonObject json) {
            var out = new ArrayList<Function>();
            for (var element : elements(member(json, "functions", "modifier"))) {
                if (element.isJsonArray()) {
                    for (var term : element.getAsJsonArray()) out.add(function(term));
                } else {
                    out.add(function(element));
                }
            }
            return List.copyOf(out);
        }

        private Function function(JsonElement element) {
            if (element == null || !element.isJsonObject()) return (stack, eval) -> {};
            var json = element.getAsJsonObject();
            var type = string(json, "type", "");
            Function function = switch (type) {
                case "minecraft:set_count" -> {
                    var count = number(json.get("count"), 0);
                    boolean add = bool(json.get("add"), false);
                    yield (stack, eval) -> stack.setAmount(
                            add ? Math.max(0, stack.getAmount() + count.getInt(eval.random)) : Math.max(0, count.getInt(eval.random)));
                }
                case "minecraft:set_nbt" -> {
                    var nbt = parseNbt(string(json, "nbt", ""));
                    yield (stack, eval) -> applyNbt(stack, nbt);
                }
                case "minecraft:enchant_randomly" -> {
                    var options = json.get("options");
                    boolean onlyCompatible = bool(json.get("only_compatible"), true);
                    yield (stack, eval) -> enchantRandomly(stack, options, onlyCompatible, eval);
                }
                case "minecraft:enchanted_count_increase" -> {
                    var count = number(json.get("count"), 0);
                    var enchantment = string(json, "enchantment", "minecraft:looting");
                    var limit = json.has("limit") ? number(json.get("limit"), 0) : null;
                    yield (stack, eval) -> {
                        int level = Math.max(0, eval.enchantmentLevel(enchantment));
                        if (level == 0) return;
                        int bonus = Math.max(0, count.getInt(eval.random)) * level;
                        if (limit != null) bonus = Math.min(bonus, Math.max(0, limit.getInt(eval.random)));
                        stack.setAmount(stack.getAmount() + bonus);
                    };
                }
                case "minecraft:apply_bonus" -> {
                    var enchantment = string(json, "enchantment", "minecraft:fortune");
                    var formula = string(json, "formula", "");
                    var parameters = json.has("parameters") && json.get("parameters").isJsonObject()
                            ? json.getAsJsonObject("parameters") : new JsonObject();
                    yield (stack, eval) -> applyBonus(stack, formula, parameters, eval, enchantment);
                }
                case "minecraft:limit_count" -> {
                    var limit = json.get("limit");
                    yield (stack, eval) -> {
                        if (limit == null) return;
                        int min = 0;
                        int max = Integer.MAX_VALUE;
                        if (limit.isJsonPrimitive() && limit.getAsJsonPrimitive().isNumber()) {
                            min = max = limit.getAsInt();
                        } else if (limit.isJsonObject()) {
                            var object = limit.getAsJsonObject();
                            min = integer(object.get("min"), 0);
                            max = integer(object.get("max"), Integer.MAX_VALUE);
                        }
                        stack.setAmount(Math.max(min, Math.min(max, stack.getAmount())));
                    };
                }
                case "minecraft:explosion_decay" -> (stack, eval) -> {};
                case "minecraft:set_damage" -> {
                    var damage = number(json.get("damage"), 0);
                    boolean add = bool(json.get("add"), false);
                    yield (stack, eval) -> {
                        int durability = stack.getType().getMaxDurability();
                        if (durability <= 0) return;
                        var meta = stack.getItemMeta();
                        if (!(meta instanceof Damageable damageable)) return;
                        int amount = (int) Math.floor(Math.max(0, Math.min(1, damage.getFloat(eval.random))) * durability);
                        damageable.setDamage(add ? Math.min(durability, damageable.getDamage() + amount) : amount);
                        stack.setItemMeta(meta);
                    };
                }
                case "minecraft:set_name" -> {
                    var name = string(json, "name", "");
                    yield (stack, eval) -> {
                        if (name.isEmpty()) return;
                        var meta = stack.getItemMeta();
                        if (meta == null) return;
                        meta.displayName(component(name));
                        stack.setItemMeta(meta);
                    };
                }
                default -> (stack, eval) -> {};
            };
            var conditions = conditions(json);
            if (conditions.isEmpty()) return function;
            return (stack, eval) -> {
                if (FerriteLoot.conditions(conditions, eval)) function.apply(stack, eval);
            };
        }

        private void enchantRandomly(ItemStack stack, JsonElement options, boolean onlyCompatible, Eval eval) {
            var ids = new ArrayList<String>();
            if (options == null || options.isJsonNull()) {
                ids.addAll(allEnchantments());
            } else if (options.isJsonPrimitive() && options.getAsJsonPrimitive().isString()) {
                var option = options.getAsString();
                if (option.startsWith("#")) ids.addAll(tag("enchantment", option.substring(1)));
                else ids.add(normalize(option));
            } else {
                for (var element : elements(options)) {
                    if (!element.isJsonPrimitive()) continue;
                    var option = element.getAsString();
                    if (option.startsWith("#")) ids.addAll(tag("enchantment", option.substring(1)));
                    else ids.add(normalize(option));
                }
            }
            var candidates = new ArrayList<Enchantment>();
            for (var id : ids) {
                var enchantment = enchantment(id);
                if (enchantment != null) candidates.add(enchantment);
            }
            if (onlyCompatible) candidates.removeIf(enchantment -> !compatible(enchantment, stack));
            if (candidates.isEmpty()) return;
            var enchantment = candidates.get(eval.random.nextInt(candidates.size()));
            int max = maxLevel(enchantment);
            int level = 1 + eval.random.nextInt(Math.max(1, max));
            stack.addUnsafeEnchantment(enchantment, level);
        }

        private boolean compatible(Enchantment enchantment, ItemStack stack) {
            // Vanilla EnchantRandomlyFunction: a plain book takes any enchantment.
            if (stack.getType() == Material.BOOK) return true;
            try {
                return enchantment.canEnchantItem(stack);
            } catch (RuntimeException unavailable) {
                return true;
            }
        }

        private int maxLevel(Enchantment enchantment) {
            try {
                return Math.max(1, enchantment.getMaxLevel());
            } catch (RuntimeException unavailable) {
                // Host data is not available in a plain JVM; single-level is the safe fallback.
                return 1;
            }
        }

        private List<String> allEnchantments() {
            var cached = enchantmentIds.get(namespace);
            if (cached != null) return cached;
            var folder = dataFolder.resolve(namespace).resolve("enchantment");
            var ids = new ArrayList<String>();
            if (Files.isDirectory(folder)) {
                try (var files = Files.walk(folder)) {
                    files.filter(Files::isRegularFile)
                            .filter(path -> path.toString().endsWith(".json"))
                            .sorted()
                            .forEach(path -> ids.add(namespace + ":"
                                    + folder.relativize(path).toString().replace('\\', '/')
                                            .replaceAll("\\.json$", "")));
                } catch (IOException ignored) {
                    // No enchantment data: enchant_randomly has no options.
                }
            }
            enchantmentIds.put(namespace, List.copyOf(ids));
            return enchantmentIds.get(namespace);
        }

        private List<String> enchantmentRefs(JsonElement value) {
            var out = new ArrayList<String>();
            for (var element : elements(value)) {
                if (!element.isJsonPrimitive()) continue;
                var id = element.getAsString();
                if (id.startsWith("#")) out.addAll(tag("enchantment", id.substring(1)));
                else out.add(normalize(id));
            }
            return out;
        }

        private List<String> itemRefs(JsonElement value) {
            return refs(value, "item");
        }

        private List<String> blockRefs(JsonElement value) {
            return refs(value, "block");
        }

        private List<String> entityRefs(JsonElement value) {
            return refs(value, "entity_type");
        }

        private List<String> refs(JsonElement value, String kind) {
            var out = new ArrayList<String>();
            for (var element : elements(value)) {
                if (element.isJsonPrimitive()) {
                    var id = element.getAsString();
                    if (id.startsWith("#")) out.addAll(tag(kind, id.substring(1)));
                    else out.add(normalize(id));
                } else if (element.isJsonObject()) {
                    out.addAll(refs(element.getAsJsonObject().get("id"), kind));
                }
            }
            return new ArrayList<>(new LinkedHashSet<>(out));
        }

        private List<String> tag(String kind, String id) {
            var key = kind + '|' + qualify(id);
            var cached = tags.get(key);
            if (cached != null) return cached;
            if (!loadingTags.add(key)) return List.of();
            try {
                var json = read("tags/" + kind, qualify(id));
                var values = new ArrayList<String>();
                if (json != null) {
                    for (var element : elements(json.get("values"))) {
                        String member = null;
                        if (element.isJsonPrimitive()) member = element.getAsString();
                        else if (element.isJsonObject() && element.getAsJsonObject().has("id")) {
                            member = element.getAsJsonObject().get("id").getAsString();
                        }
                        if (member == null) continue;
                        if (member.startsWith("#")) values.addAll(tag(kind, member.substring(1)));
                        else values.add(normalize(member));
                    }
                }
                var unique = List.copyOf(new LinkedHashSet<>(values));
                tags.put(key, unique);
                return unique;
            } finally {
                loadingTags.remove(key);
            }
        }

        private JsonObject read(String folder, String id) {
            var key = NamespacedKey.fromString(normalize(id));
            if (key == null) return null;
            var path = dataFolder.resolve(key.getNamespace()).resolve(folder).resolve(key.getKey() + ".json");
            if (!Files.isRegularFile(path)) return null;
            try {
                var element = JsonParser.parseString(Files.readString(path));
                return element.isJsonObject() ? element.getAsJsonObject() : null;
            } catch (IOException | RuntimeException malformed) {
                return null;
            }
        }

        private String qualify(String id) {
            return id.contains(":") ? id : namespace + ":" + id;
        }
    }

    private static void applyBonus(ItemStack stack, String formula, JsonObject parameters, Eval eval, String enchantment) {
        int level = Math.max(0, eval.enchantmentLevel(enchantment));
        if (level <= 0) return;
        switch (formula) {
            case "minecraft:ore_drops" -> {
                int multiplier = 1 + eval.random.nextInt(level + 1);
                stack.setAmount(stack.getAmount() * multiplier);
            }
            case "minecraft:uniform_bonus_count" -> {
                int bonus = integer(parameters.get("bonusMultiplier"), 1);
                stack.setAmount(stack.getAmount() + eval.random.nextInt(Math.max(1, bonus * level + 1)));
            }
            case "minecraft:binomial_with_bonus_count" -> {
                int extra = integer(parameters.get("extra"), 0);
                float probability = decimal(parameters.get("probability"), 0);
                int successes = 0;
                for (int i = 0; i < Math.max(0, extra + level); i++) {
                    if (eval.random.nextFloat() < probability) successes++;
                }
                stack.setAmount(stack.getAmount() + successes);
            }
            default -> {
                // A formula this API does not model leaves the count alone.
            }
        }
    }

    @SuppressWarnings("unchecked")
    private static void applyNbt(ItemStack stack, Map<String, Object> nbt) {
        if (nbt == null || nbt.isEmpty()) return;
        if (nbt.get("Count") instanceof Number count) stack.setAmount(Math.max(0, count.intValue()));
        var meta = stack.getItemMeta();
        if (meta == null) return;

        if (nbt.get("display") instanceof Map<?, ?> display) {
            if (display.get("Name") instanceof String name) meta.displayName(component(name));
            if (display.get("Lore") instanceof List<?> lore) {
                var lines = new ArrayList<net.kyori.adventure.text.Component>();
                for (var line : lore) if (line != null) lines.add(component(String.valueOf(line)));
                meta.lore(lines);
            }
        }
        if (nbt.get("Enchantments") instanceof List<?> enchantments) {
            for (var entry : enchantments) {
                if (!(entry instanceof Map<?, ?> enchantment)) continue;
                var id = enchantment.get("id");
                var level = enchantment.get("lvl");
                if (id == null || !(level instanceof Number number)) continue;
                var resolved = enchantment(String.valueOf(id));
                if (resolved != null) meta.addEnchant(resolved, number.intValue(), true);
            }
        }
        if (nbt.get("Unbreakable") instanceof Number unbreakable && unbreakable.intValue() != 0) {
            meta.setUnbreakable(true);
        } else if (Boolean.TRUE.equals(nbt.get("Unbreakable"))) {
            meta.setUnbreakable(true);
        }
        if (nbt.get("CustomModelData") instanceof Number model) meta.setCustomModelData(model.intValue());
        if (nbt.get("RepairCost") instanceof Number repair && meta instanceof org.bukkit.inventory.meta.Repairable repairable) repairable.setRepairCost(repair.intValue());
        if (meta instanceof Damageable damageable && nbt.get("Damage") instanceof Number damage) {
            damageable.setDamage(damage.intValue());
        }
        if (nbt.get("components") instanceof Map<?, ?> components) {
            if (components.get("minecraft:enchantments") instanceof Map<?, ?> enchantments
                    && enchantments.get("levels") instanceof Map<?, ?> levels) {
                for (var entry : levels.entrySet()) {
                    var resolved = enchantment(String.valueOf(entry.getKey()));
                    if (resolved != null && entry.getValue() instanceof Number number) {
                        meta.addEnchant(resolved, number.intValue(), true);
                    }
                }
            }
            if (components.get("minecraft:custom_model_data") instanceof Number model) {
                meta.setCustomModelData(model.intValue());
            }
            if (components.get("minecraft:damage") instanceof Number damage && meta instanceof Damageable damageable) {
                damageable.setDamage(damage.intValue());
            }
            if (components.containsKey("minecraft:unbreakable")) meta.setUnbreakable(true);
        }
        stack.setItemMeta(meta);
    }

    private static net.kyori.adventure.text.Component component(String text) {
        try {
            return net.kyori.adventure.text.serializer.gson.GsonComponentSerializer.gson().deserialize(text);
        } catch (RuntimeException plain) {
            return net.kyori.adventure.text.Component.text(text);
        }
    }

    private static Map<String, Object> parseNbt(String text) {
        if (text == null || text.isBlank()) return Map.of();
        try {
            return new SnbtParser(text).compound();
        } catch (RuntimeException malformed) {
            return Map.of();
        }
    }

    private static NumberProvider number(JsonElement element, double fallback) {
        if (element == null || element.isJsonNull()) return constant(fallback);
        if (element.isJsonPrimitive()) {
            return constant(element.getAsJsonPrimitive().isNumber() ? element.getAsDouble() : fallback);
        }
        if (!element.isJsonObject()) return constant(fallback);
        var json = element.getAsJsonObject();
        return switch (string(json, "type", "minecraft:constant")) {
            case "minecraft:uniform" -> uniform(
                    decimal(json.get("min"), 0), decimal(json.get("max"), 0));
            case "minecraft:binomial" -> binomial(
                    integer(json.get("n"), 0), decimal(json.get("p"), 0));
            case "minecraft:constant" -> constant(decimal(json.get("value"), (float) fallback));
            default -> constant(fallback);
        };
    }

    private static NumberProvider constant(double value) {
        return new NumberProvider() {
            @Override public int getInt(java.util.Random random) { return (int) value; }
            @Override public float getFloat(java.util.Random random) { return (float) value; }
        };
    }

    private static NumberProvider uniform(double min, double max) {
        return new NumberProvider() {
            @Override public int getInt(java.util.Random random) {
                int low = (int) min;
                int high = (int) max;
                if (high <= low) return low;
                long span = (long) high - low + 1;
                return low + random.nextInt((int) Math.min(span, Integer.MAX_VALUE));
            }

            @Override public float getFloat(java.util.Random random) {
                if (max <= min) return (float) min;
                return (float) (min + random.nextFloat() * (max - min));
            }
        };
    }

    private static NumberProvider binomial(int n, double p) {
        return new NumberProvider() {
            @Override public int getInt(java.util.Random random) {
                int successes = 0;
                for (int i = 0; i < Math.max(0, n); i++) if (random.nextFloat() < p) successes++;
                return successes;
            }
        };
    }

    private static List<JsonElement> elements(JsonElement element) {
        if (element == null || element.isJsonNull()) return List.of();
        if (!element.isJsonArray()) return List.of(element);
        var out = new ArrayList<JsonElement>();
        element.getAsJsonArray().forEach(out::add);
        return out;
    }

    private static JsonElement member(JsonObject object, String... names) {
        for (var name : names) if (object.has(name)) return object.get(name);
        return null;
    }

    private static String string(JsonObject object, String name, String fallback) {
        var element = object.get(name);
        return element != null && element.isJsonPrimitive() && element.getAsJsonPrimitive().isString()
                ? element.getAsString() : fallback;
    }

    private static int integer(JsonElement element, int fallback) {
        return element != null && element.isJsonPrimitive() && element.getAsJsonPrimitive().isNumber()
                ? element.getAsInt() : fallback;
    }

    private static float decimal(JsonElement element, float fallback) {
        return element != null && element.isJsonPrimitive() && element.getAsJsonPrimitive().isNumber()
                ? element.getAsFloat() : fallback;
    }

    private static boolean bool(JsonElement element, boolean fallback) {
        return element != null && element.isJsonPrimitive() && element.getAsJsonPrimitive().isBoolean()
                ? element.getAsBoolean() : fallback;
    }

    private static String stripNamespace(String key) {
        int colon = key.indexOf(':');
        return colon >= 0 ? key.substring(colon + 1) : key;
    }

    /** A small SNBT reader for the compounds {@code set_nbt} carries. */
    private static final class SnbtParser {
        private final String text;
        private int index;

        SnbtParser(String text) {
            this.text = text;
        }

        Map<String, Object> compound() {
            skip();
            expect('{');
            var out = new LinkedHashMap<String, Object>();
            skip();
            if (peek() == '}') {
                index++;
                return out;
            }
            while (true) {
                skip();
                var key = key();
                skip();
                expect(':');
                out.put(key, value());
                skip();
                char next = index < text.length() ? text.charAt(index++) : '\0';
                if (next == '}') return out;
                if (next != ',') throw new IllegalArgumentException("Expected ',' or '}'");
            }
        }

        private String key() {
            skip();
            if (peek() == '"' || peek() == '\'') return quoted();
            int start = index;
            while (index < text.length() && text.charAt(index) != ':') index++;
            var key = text.substring(start, index).trim();
            if (key.isEmpty()) throw new IllegalArgumentException("Empty key");
            return key;
        }

        private Object value() {
            skip();
            char next = peek();
            if (next == '{') return compound();
            if (next == '[') return list();
            if (next == '"' || next == '\'') return quoted();
            return token();
        }

        private List<Object> list() {
            expect('[');
            var out = new ArrayList<Object>();
            skip();
            if (peek() == ']') {
                index++;
                return out;
            }
            while (true) {
                skip();
                out.add(value());
                skip();
                char next = index < text.length() ? text.charAt(index++) : '\0';
                if (next == ']') return out;
                if (next != ',') throw new IllegalArgumentException("Expected ',' or ']'");
            }
        }

        private String quoted() {
            char quote = text.charAt(index++);
            var out = new StringBuilder();
            while (index < text.length()) {
                char next = text.charAt(index++);
                if (next == quote) return out.toString();
                if (next == '\\' && index < text.length()) {
                    char escape = text.charAt(index++);
                    switch (escape) {
                        case 'n' -> out.append('\n');
                        case 'r' -> out.append('\r');
                        case 't' -> out.append('\t');
                        default -> out.append(escape);
                    }
                } else {
                    out.append(next);
                }
            }
            throw new IllegalArgumentException("Unterminated string");
        }

        private Object token() {
            int start = index;
            while (index < text.length()) {
                char next = text.charAt(index);
                if (next == ',' || next == '}' || next == ']' || Character.isWhitespace(next)) break;
                index++;
            }
            var token = text.substring(start, index).trim();
            if (token.isEmpty()) throw new IllegalArgumentException("Expected a value");
            if (token.equals("true")) return Boolean.TRUE;
            if (token.equals("false")) return Boolean.FALSE;
            try {
                if (token.endsWith("b") || token.endsWith("B")) return Byte.parseByte(token.substring(0, token.length() - 1));
                if (token.endsWith("s") || token.endsWith("S")) return Short.parseShort(token.substring(0, token.length() - 1));
                if (token.endsWith("l") || token.endsWith("L")) return Long.parseLong(token.substring(0, token.length() - 1));
                if (token.endsWith("f") || token.endsWith("F")) return Float.parseFloat(token.substring(0, token.length() - 1));
                if (token.endsWith("d") || token.endsWith("D")) return Double.parseDouble(token.substring(0, token.length() - 1));
                return Integer.parseInt(token);
            } catch (NumberFormatException notNumeric) {
                return token;
            }
        }

        private void skip() {
            while (index < text.length() && Character.isWhitespace(text.charAt(index))) index++;
        }

        private char peek() {
            return index < text.length() ? text.charAt(index) : '\0';
        }

        private void expect(char expected) {
            if (peek() != expected) throw new IllegalArgumentException("Expected '" + expected + "'");
            index++;
        }
    }
}
