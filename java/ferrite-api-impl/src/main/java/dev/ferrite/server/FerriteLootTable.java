package dev.ferrite.server;

import java.nio.file.Path;
import java.util.Collection;
import java.util.Random;
import java.util.concurrent.ConcurrentHashMap;
import org.bukkit.NamespacedKey;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.ItemStack;
import org.bukkit.loot.LootContext;
import org.bukkit.loot.LootTable;

/** A handle to one vanilla loot table, evaluated by {@link FerriteLoot}. */
final class FerriteLootTable implements LootTable {
    private static final ConcurrentHashMap<NamespacedKey, FerriteLootTable> CACHE = new ConcurrentHashMap<>();
    private final NamespacedKey key;
    private volatile FerriteLoot.Table table;

    private FerriteLootTable(NamespacedKey key) { this.key = key; }

    private FerriteLootTable(NamespacedKey key, FerriteLoot.Table table) {
        this.key = key;
        this.table = table;
    }

    /** The table for a key, or null when the vanilla data has no such file. */
    static LootTable get(NamespacedKey key) {
        var cached = CACHE.get(key);
        if (cached != null) return cached;
        var file = Bootstrap.dataFile("data/" + key.getNamespace() + "/loot_table/" + key.getKey() + ".json");
        if (file == null || !file.isFile()) return null;
        return CACHE.computeIfAbsent(key, FerriteLootTable::new);
    }

    /** Loads a table from an explicit data-pack root; used by tests and generated packs. */
    static LootTable at(NamespacedKey key, Path dataFolder) {
        java.util.Objects.requireNonNull(key, "key");
        java.util.Objects.requireNonNull(dataFolder, "dataFolder");
        return new FerriteLootTable(key, FerriteLoot.load(dataFolder, key));
    }

    private FerriteLoot.Table table() {
        var loaded = table;
        if (loaded == null) {
            var data = Bootstrap.dataFile("data");
            table = loaded = data == null ? FerriteLoot.Table.EMPTY : FerriteLoot.load(data.toPath(), key);
        }
        return loaded;
    }

    @Override public NamespacedKey getKey() { return key; }

    @Override public Collection<ItemStack> populateLoot(Random random, LootContext context) {
        java.util.Objects.requireNonNull(random, "random");
        java.util.Objects.requireNonNull(context, "context");
        return FerriteLoot.roll(table(), random, context);
    }

    @Override public void fillInventory(Inventory inventory, Random random, LootContext context) {
        java.util.Objects.requireNonNull(inventory, "inventory");
        FerriteLoot.fill(table(), inventory, random, context);
    }
}
