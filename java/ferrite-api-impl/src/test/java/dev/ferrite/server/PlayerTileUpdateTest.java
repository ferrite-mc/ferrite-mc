package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.List;
import org.bukkit.Material;
import org.bukkit.inventory.ItemStack;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

/** Player#sendBlockUpdate for the tile entity families the host models, and the explicit refusals for the rest. */
class PlayerTileUpdateTest {
    private static final String WORLD_ID = "00000000-0000-0000-0000-0000000000e7";
    private static final String PLAYER_ID = "00000000-0000-0000-0000-0000000000e8";
    private static org.bukkit.entity.Player player;
    private final List<String[]> calls = new ArrayList<>();

    @BeforeAll static void start() throws Exception {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerWorld(WORLD_ID, "tile_world", 0, 0, 64, 0);
        Bootstrap.registerPlayer(PLAYER_ID, "TileViewer", false);
        var ensure = Bootstrap.class.getDeclaredMethod("ensurePlayer", String.class, String.class);
        ensure.setAccessible(true);
        player = (org.bukkit.entity.Player) ensure.invoke(null, PLAYER_ID, "TileViewer");
    }

    @AfterAll static void stop() {
        var server = (FerriteServer) org.bukkit.Bukkit.getServer();
        server.unregisterPlayer(player.getUniqueId());
        var world = org.bukkit.Bukkit.getWorld("tile_world");
        if (world != null) server.unregisterWorld(world.getUID());
    }

    @BeforeEach void goHome() {
        player.teleport(new org.bukkit.Location(org.bukkit.Bukkit.getWorld("tile_world"), 0, 65, 0, 0f, 0f));
        calls.clear();
        HostBridge.transport = (owner, subject, method, args) -> {
            var parts = new ArrayList<String>();
            parts.add(owner);
            parts.add(subject);
            parts.add(method);
            for (var part : String.valueOf(args).split(java.util.regex.Pattern.quote(String.valueOf(HostBridge.ARG)), -1)) parts.add(part);
            calls.add(parts.toArray(String[]::new));
            return "";
        };
    }

    @AfterEach void reset() { HostBridge.transport = null; }

    private org.bukkit.Location at() { return new org.bukkit.Location(player.getWorld(), 4, 70, -3); }

    private static <T extends org.bukkit.block.TileState> T tile(Class<T> api, Material type, org.bukkit.inventory.Inventory snapshot, java.util.Map<String, Object> extra) {
        return api.cast(Proxy.newProxyInstance(PlayerTileUpdateTest.class.getClassLoader(), new Class<?>[]{api}, (self, method, args) -> switch (method.getName()) {
            case "getType" -> type;
            case "getSnapshotInventory" -> snapshot;
            case "toString" -> "Tile[" + type + "]";
            case "hashCode" -> System.identityHashCode(self);
            case "equals" -> self == args[0];
            default -> {
                if (extra.containsKey(method.getName())) yield extra.get(method.getName());
                throw new UnsupportedOperationException(method.getName());
            }
        }));
    }

    private org.bukkit.inventory.Inventory inventory(int size, java.util.Map<Integer, ItemStack> items) {
        var inventory = org.bukkit.Bukkit.createInventory(null, size);
        items.forEach(inventory::setItem);
        return inventory;
    }

    @Test void aChestSnapshotIsSentToThatPlayerWithItsItems() {
        var chest = tile(org.bukkit.block.Chest.class, Material.CHEST, inventory(27, java.util.Map.of(0, new ItemStack(Material.STONE, 64), 13, new ItemStack(Material.DIAMOND, 3))), java.util.Map.of());
        player.sendBlockUpdate(at(), chest);
        assertEquals(1, calls.size());
        var call = calls.get(0);
        assertEquals("Player", call[0]);
        assertEquals(PLAYER_ID, call[1]);
        assertEquals("sendTileSnapshot(Location,String,List)", call[2]);
        assertTrue(call[3].contains("4.0") && call[3].contains("70.0") && call[3].contains("-3.0"), call[3]);
        assertEquals("minecraft:chest", call[4]);
        var entries = call[5].split(String.valueOf(HostBridge.ITEM));
        assertArrayEquals(new String[]{"item,0,minecraft:stone,64", "item,13,minecraft:diamond,3"}, entries);
    }

    @Test void everyContainerFamilyMapsToItsBlockEntityType() {
        record Case(Class<? extends org.bukkit.block.TileState> api, Material type, String entity) {}
        var cases = List.of(
            new Case(org.bukkit.block.Chest.class, Material.TRAPPED_CHEST, "minecraft:trapped_chest"),
            new Case(org.bukkit.block.Barrel.class, Material.BARREL, "minecraft:barrel"),
            new Case(org.bukkit.block.ShulkerBox.class, Material.RED_SHULKER_BOX, "minecraft:shulker_box"),
            new Case(org.bukkit.block.ShulkerBox.class, Material.SHULKER_BOX, "minecraft:shulker_box"));
        for (var c : cases) {
            calls.clear();
            player.sendBlockUpdate(at(), tile(c.api(), c.type(), inventory(27, java.util.Map.of()), java.util.Map.of()));
            assertEquals(c.entity(), calls.get(0)[4], c.type().name());
            assertEquals("", calls.get(0)[5], "an empty container sends no items");
        }
    }

    @Test void aFurnaceSnapshotCarriesItsTimers() {
        var inventory = org.bukkit.Bukkit.createInventory(null, org.bukkit.event.inventory.InventoryType.FURNACE);
        inventory.setItem(0, new ItemStack(Material.IRON_ORE, 5));
        var furnace = tile(org.bukkit.block.Furnace.class, Material.BLAST_FURNACE, inventory,
            java.util.Map.of("getBurnTime", (short) 12, "getCookTime", (short) 30, "getCookTimeTotal", 100));
        player.sendBlockUpdate(at(), furnace);
        assertEquals("minecraft:blast_furnace", calls.get(0)[4]);
        assertArrayEquals(new String[]{"item,0,minecraft:iron_ore,5", "furnace,12,0,30,100"}, calls.get(0)[5].split(String.valueOf(HostBridge.ITEM)));
    }

    @Test void unmodelledFamiliesAndItemMetadataAreRefusedBeforeTheHost() {
        var banner = tile(org.bukkit.block.Banner.class, Material.WHITE_BANNER, null, java.util.Map.of());
        var error = assertThrows(UnsupportedOperationException.class, () -> player.sendBlockUpdate(at(), banner));
        assertTrue(error.getMessage().contains("WHITE_BANNER") && error.getMessage().contains("banner, skull"), error.getMessage());
        var skull = tile(org.bukkit.block.Skull.class, Material.PLAYER_HEAD, null, java.util.Map.of());
        assertThrows(UnsupportedOperationException.class, () -> player.sendBlockUpdate(at(), skull));

        var named = new ItemStack(Material.STONE);
        var meta = named.getItemMeta();
        meta.setDisplayName("named");
        named.setItemMeta(meta);
        var chest = tile(org.bukkit.block.Chest.class, Material.CHEST, inventory(27, java.util.Map.of(2, named)), java.util.Map.of());
        var refused = assertThrows(UnsupportedOperationException.class, () -> player.sendBlockUpdate(at(), chest));
        assertTrue(refused.getMessage().contains("slot 2"), refused.getMessage());
        assertTrue(calls.isEmpty(), "nothing reaches the host: " + calls.size());
    }

    @Test void nullArgumentsAreRejected() {
        var chest = tile(org.bukkit.block.Chest.class, Material.CHEST, inventory(27, java.util.Map.of()), java.util.Map.of());
        assertThrows(NullPointerException.class, () -> player.sendBlockUpdate(null, chest));
        assertThrows(NullPointerException.class, () -> player.sendBlockUpdate(at(), null));
        assertTrue(calls.isEmpty());
    }
}
