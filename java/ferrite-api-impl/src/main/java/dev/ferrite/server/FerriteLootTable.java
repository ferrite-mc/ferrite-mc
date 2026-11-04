package dev.ferrite.server;

import java.util.Collection;
import java.util.Random;
import java.util.concurrent.ConcurrentHashMap;
import org.bukkit.NamespacedKey;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.ItemStack;
import org.bukkit.loot.LootContext;
import org.bukkit.loot.LootTable;

/** A handle to a vanilla loot table. Rolling the table is not implemented. */
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
    @Override public Collection<ItemStack> populateLoot(Random random, LootContext context) { throw new UnsupportedOperationException("Ferrite: not implemented: LootTable#populateLoot"); }
    @Override public void fillInventory(Inventory inventory, Random random, LootContext context) { throw new UnsupportedOperationException("Ferrite: not implemented: LootTable#fillInventory"); }
}
