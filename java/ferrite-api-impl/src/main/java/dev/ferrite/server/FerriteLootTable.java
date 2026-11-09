package dev.ferrite.server;

import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.List;
import java.util.Objects;
import java.util.Random;
import java.util.concurrent.ConcurrentHashMap;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.ItemStack;
import org.bukkit.loot.LootContext;
import org.bukkit.loot.LootTable;

/**
 * A handle to a vanilla loot table.
 *
 * <p>Ferrite evaluates the table in Rust (`ferrite-loot`, the same loader block drops use) and
 * returns plain item stacks: components, enchantments and other item metadata are outside the
 * loot model, so a rolled stack is a material and a count.
 */
final class FerriteLootTable implements LootTable {
    private static final ConcurrentHashMap<NamespacedKey, FerriteLootTable> CACHE = new ConcurrentHashMap<>();
    private final NamespacedKey key;

    private FerriteLootTable(NamespacedKey key) { this.key = key; }

    /** The table for a key, or null when the vanilla data has no such file. */
    static LootTable get(NamespacedKey key) {
        var cached = CACHE.get(key);
        if (cached != null) return cached;
        var file = Bootstrap.dataFile("data/" + key.getNamespace() + "/loot_table/" + key.getKey() + ".json");
        if (file == null || !file.isFile()) return null;
        return CACHE.computeIfAbsent(key, FerriteLootTable::new);
    }

    @Override public NamespacedKey getKey() { return key; }

    @Override public Collection<ItemStack> populateLoot(Random random, LootContext context) {
        Objects.requireNonNull(random, "random");
        int looting = context == null ? 0 : Math.max(0, context.getLootingModifier());
        String reply = (String) HostBridge.call("Server", "server", "rollLootTable(String,int)", String.class, key.toString(), looting);
        return decode(reply);
    }

    /** One `namespace:item count` line per stack; a null (unknown table) or empty reply is no drops. */
    static List<ItemStack> decode(String reply) {
        if (reply == null || reply.isEmpty()) return List.of();
        var stacks = new ArrayList<ItemStack>();
        for (String line : reply.split("\n")) {
            int space = line.lastIndexOf(' ');
            if (space <= 0) continue;
            Material material = Material.matchMaterial(line.substring(0, space));
            int count;
            try {
                count = Integer.parseInt(line.substring(space + 1));
            } catch (NumberFormatException malformed) {
                continue;
            }
            if (material == null || count <= 0) continue;
            stacks.add(new ItemStack(material, count));
        }
        return stacks;
    }

    @Override public void fillInventory(Inventory inventory, Random random, LootContext context) {
        Objects.requireNonNull(inventory, "inventory");
        Objects.requireNonNull(random, "random");
        var empty = new ArrayList<Integer>();
        for (int slot = 0; slot < inventory.getSize(); slot++) {
            ItemStack existing = inventory.getItem(slot);
            if (existing == null || existing.getType().isAir()) empty.add(slot);
        }
        Collections.shuffle(empty, random);
        int next = 0;
        for (ItemStack stack : populateLoot(random, context)) {
            while (stack.getAmount() > 0 && next < empty.size()) {
                int amount = Math.min(stack.getAmount(), stack.getMaxStackSize());
                ItemStack placed = stack.clone();
                placed.setAmount(amount);
                inventory.setItem(empty.get(next++), placed);
                stack.setAmount(stack.getAmount() - amount);
            }
            if (next >= empty.size()) return;
        }
    }
}
