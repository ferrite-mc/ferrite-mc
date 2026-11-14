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
                if (extra.containsKey(method.getName())) {
                    var value = extra.get(method.getName());
                    yield value instanceof java.util.function.IntFunction<?> byIndex ? byIndex.apply((int) args[0]) : value;
                }
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

    private static com.destroystokyo.paper.profile.PlayerProfile profile(String name, java.util.UUID id, boolean textures) {
        return profile(name, id, textures ? "abc" : null);
    }

    private static com.destroystokyo.paper.profile.PlayerProfile profile(String name, java.util.UUID id, String texture) {
        return (com.destroystokyo.paper.profile.PlayerProfile) Proxy.newProxyInstance(PlayerTileUpdateTest.class.getClassLoader(), new Class<?>[]{com.destroystokyo.paper.profile.PlayerProfile.class}, (self, method, args) -> switch (method.getName()) {
            case "getName" -> name;
            case "getUniqueId" -> id;
            case "getProperties" -> texture != null ? java.util.Set.of(new com.destroystokyo.paper.profile.ProfileProperty("textures", texture)) : java.util.Set.<com.destroystokyo.paper.profile.ProfileProperty>of();
            case "toString" -> "Profile";
            case "hashCode" -> 1;
            case "equals" -> self == args[0];
            default -> throw new UnsupportedOperationException(method.getName());
        });
    }

    @Test void dispensersDroppersHoppersBrewingStandsAndCraftersSendTheirItems() {
        record Case(Class<? extends org.bukkit.block.Container> api, Material type, String entity, int size) {}
        for (var c : List.of(
                new Case(org.bukkit.block.Dispenser.class, Material.DISPENSER, "minecraft:dispenser", 9),
                new Case(org.bukkit.block.Dropper.class, Material.DROPPER, "minecraft:dropper", 9),
                new Case(org.bukkit.block.Hopper.class, Material.HOPPER, "minecraft:hopper", 5),
                new Case(org.bukkit.block.BrewingStand.class, Material.BREWING_STAND, "minecraft:brewing_stand", 5),
                new Case(org.bukkit.block.Crafter.class, Material.CRAFTER, "minecraft:crafter", 9))) {
            calls.clear();
            var container = tile(c.api(), c.type(), inventory(9, java.util.Map.of(c.size() - 1, new ItemStack(Material.STONE, 3))), java.util.Map.of());
            player.sendBlockUpdate(at(), container);
            assertEquals(c.entity(), calls.get(0)[4], c.type().name());
            assertEquals("item," + (c.size() - 1) + ",minecraft:stone,3", calls.get(0)[5], c.type().name());
        }
    }

    @Test void aCampfireSendsItsFourSlotsAndAChiseledBookshelfItsSix() {
        var campfire = tile(org.bukkit.block.Campfire.class, Material.CAMPFIRE, null, java.util.Map.of(
            "getSize", 4, "getItem", (java.util.function.IntFunction<ItemStack>) slot -> slot == 3 ? new ItemStack(Material.BEEF, 1) : null));
        player.sendBlockUpdate(at(), campfire);
        assertEquals("minecraft:campfire", calls.get(0)[4]);
        assertEquals("item,3,minecraft:beef,1", calls.get(0)[5]);
        calls.clear();
        var shelf = tile(org.bukkit.block.ChiseledBookshelf.class, Material.CHISELED_BOOKSHELF, inventory(9, java.util.Map.of(5, new ItemStack(Material.BOOK))), java.util.Map.of());
        player.sendBlockUpdate(at(), shelf);
        assertEquals("minecraft:chiseled_bookshelf", calls.get(0)[4]);
        assertEquals("item,5,minecraft:book,1", calls.get(0)[5]);
    }

    @Test void aBannerSendsItsPatternLayers() {
        var layers = List.of(
            new org.bukkit.block.banner.Pattern(org.bukkit.DyeColor.RED, org.bukkit.block.banner.PatternType.STRIPE_TOP),
            new org.bukkit.block.banner.Pattern(org.bukkit.DyeColor.LIGHT_BLUE, org.bukkit.block.banner.PatternType.CROSS));
        var banner = tile(org.bukkit.block.Banner.class, Material.WHITE_BANNER, null, java.util.Map.of("getPatterns", layers));
        player.sendBlockUpdate(at(), banner);
        assertEquals("minecraft:banner", calls.get(0)[4]);
        assertArrayEquals(new String[]{"pattern,minecraft:stripe_top,red", "pattern,minecraft:cross,light_blue"}, calls.get(0)[5].split(String.valueOf(HostBridge.ITEM)));
    }

    @Test void aSkullSendsItsProfileTexturesAndRefusesSoundsAndUnsafeProperties() {
        var id = java.util.UUID.fromString("069a79f4-44e9-4726-a5be-fca90e38aaf5");
        var plain = new java.util.HashMap<String, Object>();
        plain.put("getPlayerProfile", profile("Notch", id, false));
        plain.put("getNoteBlockSound", null);
        player.sendBlockUpdate(at(), tile(org.bukkit.block.Skull.class, Material.PLAYER_HEAD, null, plain));
        assertEquals("minecraft:skull", calls.get(0)[4]);
        assertEquals("profile,Notch," + id, calls.get(0)[5]);
        calls.clear();
        var mob = new java.util.HashMap<String, Object>();
        mob.put("getPlayerProfile", null);
        mob.put("getNoteBlockSound", null);
        player.sendBlockUpdate(at(), tile(org.bukkit.block.Skull.class, Material.ZOMBIE_HEAD, null, mob));
        assertEquals("", calls.get(0)[5]);
        calls.clear();
        var textured = new java.util.HashMap<String, Object>();
        textured.put("getPlayerProfile", profile("Notch", id, true));
        textured.put("getNoteBlockSound", null);
        player.sendBlockUpdate(at(), tile(org.bukkit.block.Skull.class, Material.PLAYER_HEAD, null, textured));
        assertArrayEquals(new String[]{"profile,Notch," + id, "property,textures,abc,"}, calls.get(0)[5].split(String.valueOf(HostBridge.ITEM)));
        calls.clear();
        var unsafe = new java.util.HashMap<String, Object>();
        unsafe.put("getPlayerProfile", profile("Notch", id, "a,b"));
        unsafe.put("getNoteBlockSound", null);
        assertThrows(IllegalArgumentException.class, () -> player.sendBlockUpdate(at(), tile(org.bukkit.block.Skull.class, Material.PLAYER_HEAD, null, unsafe)));
        var sound = new java.util.HashMap<String, Object>();
        sound.put("getPlayerProfile", null);
        sound.put("getNoteBlockSound", org.bukkit.NamespacedKey.minecraft("entity.pig.ambient"));
        assertThrows(UnsupportedOperationException.class, () -> player.sendBlockUpdate(at(), tile(org.bukkit.block.Skull.class, Material.PLAYER_HEAD, null, sound)));
        assertTrue(calls.isEmpty());
    }

    @Test void unmodelledFamiliesAreRefusedBeforeTheHost() {
        var beacon = tile(org.bukkit.block.Beacon.class, Material.BEACON, null, java.util.Map.of());
        var error = assertThrows(UnsupportedOperationException.class, () -> player.sendBlockUpdate(at(), beacon));
        assertTrue(error.getMessage().contains("BEACON") && error.getMessage().contains("no other block entity model"), error.getMessage());
        assertTrue(calls.isEmpty(), "nothing reaches the host: " + calls.size());
    }

    @Test void aStackWithMetadataSendsItAsAHexEncodedMetaEntry() {
        var sword = new ItemStack(Material.DIAMOND_SWORD);
        var meta = sword.getItemMeta();
        meta.setDisplayName("named");
        meta.addEnchant(org.bukkit.enchantments.Enchantment.SHARPNESS, 3, true);
        sword.setItemMeta(meta);
        var chest = tile(org.bukkit.block.Chest.class, Material.CHEST, inventory(27, java.util.Map.of(2, sword)), java.util.Map.of());
        player.sendBlockUpdate(at(), chest);
        var entries = calls.get(0)[5].split(String.valueOf(HostBridge.ITEM));
        assertEquals("item,2,minecraft:diamond_sword,1", entries[0]);
        assertTrue(entries[1].startsWith("meta,2,"), entries[1]);
        var hex = entries[1].substring("meta,2,".length());
        var bytes = new byte[hex.length() / 2];
        for (int i = 0; i < bytes.length; i++) bytes[i] = (byte) Integer.parseInt(hex.substring(2 * i, 2 * i + 2), 16);
        var fields = new String(bytes, java.nio.charset.StandardCharsets.UTF_8).split("\u001e", -1);
        assertTrue(fields[0].contains("named"), fields[0]);
        assertEquals("minecraft:sharpness=3", fields[2]);
        // A stack without metadata adds no meta entry.
        calls.clear();
        var plain = tile(org.bukkit.block.Chest.class, Material.CHEST, inventory(27, java.util.Map.of(0, new ItemStack(Material.STONE))), java.util.Map.of());
        player.sendBlockUpdate(at(), plain);
        assertEquals("item,0,minecraft:stone,1", calls.get(0)[5]);
    }

    @Test void wearOnASnapshotStackIsRefusedBeforeTheHost() {
        var pick = new ItemStack(Material.DIAMOND_PICKAXE);
        var meta = (org.bukkit.inventory.meta.Damageable) pick.getItemMeta();
        meta.setDamage(20);
        pick.setItemMeta(meta);
        var chest = tile(org.bukkit.block.Chest.class, Material.CHEST, inventory(27, java.util.Map.of(1, pick)), java.util.Map.of());
        var refused = assertThrows(UnsupportedOperationException.class, () -> player.sendBlockUpdate(at(), chest));
        assertTrue(refused.getMessage().contains("slot 1"), refused.getMessage());
        assertTrue(calls.isEmpty(), "nothing reaches the host: " + calls.size());
    }

    @Test void nullArgumentsAreRejected() {
        var chest = tile(org.bukkit.block.Chest.class, Material.CHEST, inventory(27, java.util.Map.of()), java.util.Map.of());
        assertThrows(NullPointerException.class, () -> player.sendBlockUpdate(null, chest));
        assertThrows(NullPointerException.class, () -> player.sendBlockUpdate(at(), null));
        assertTrue(calls.isEmpty());
    }

    @Test void jukeboxLecternCommandBlockAndSpawnerSendTheirSettings() {
        var disc = new ItemStack(Material.MUSIC_DISC_CAT);
        player.sendBlockUpdate(at(), tile(org.bukkit.block.Jukebox.class, Material.JUKEBOX, null, java.util.Map.of("getRecord", disc)));
        assertEquals("minecraft:jukebox", calls.get(0)[4]);
        assertEquals("item,0,minecraft:music_disc_cat,1", calls.get(0)[5]);
        calls.clear();
        var book = new ItemStack(Material.WRITABLE_BOOK);
        player.sendBlockUpdate(at(), tile(org.bukkit.block.Lectern.class, Material.LECTERN, inventory(1, java.util.Map.of(0, book)), java.util.Map.of("getPage", 2)));
        assertEquals("minecraft:lectern", calls.get(0)[4]);
        assertArrayEquals(new String[]{"item,0,minecraft:writable_book,1", "put,Page,2"}, calls.get(0)[5].split(String.valueOf(HostBridge.ITEM)));
        calls.clear();
        player.sendBlockUpdate(at(), tile(org.bukkit.block.CommandBlock.class, Material.COMMAND_BLOCK, null, java.util.Map.of("getCommand", "say hi, all", "getName", "Boss")));
        assertEquals("minecraft:command_block", calls.get(0)[4]);
        assertArrayEquals(new String[]{"put,Command,say hi, all", "put,CustomName,Boss"}, calls.get(0)[5].split(String.valueOf(HostBridge.ITEM)));
        calls.clear();
        var spawner = new java.util.HashMap<String, Object>();
        spawner.put("getSpawnedType", org.bukkit.entity.EntityType.ZOMBIE);
        for (var name : new String[]{"getDelay", "getMinSpawnDelay", "getMaxSpawnDelay", "getSpawnCount", "getMaxNearbyEntities", "getRequiredPlayerRange", "getSpawnRange"}) spawner.put(name, 5);
        player.sendBlockUpdate(at(), tile(org.bukkit.block.CreatureSpawner.class, Material.SPAWNER, null, spawner));
        assertEquals("minecraft:mob_spawner", calls.get(0)[4]);
        var entries = calls.get(0)[5].split(String.valueOf(HostBridge.ITEM));
        assertEquals("spawn,minecraft:zombie", entries[0]);
        assertEquals("put,SpawnRange,5", entries[entries.length - 1]);
    }

    @Test void sculkSensorBeehiveAndStructureBlockSendTheirSettingsAndABedHasNoEntity() {
        player.sendBlockUpdate(at(), tile(org.bukkit.block.SculkSensor.class, Material.SCULK_SENSOR, null, java.util.Map.of("getLastVibrationFrequency", 9)));
        assertEquals("minecraft:sculk_sensor", calls.get(0)[4]);
        assertEquals("put,last_vibration_frequency,9", calls.get(0)[5]);
        calls.clear();
        player.sendBlockUpdate(at(), tile(org.bukkit.block.SculkSensor.class, Material.CALIBRATED_SCULK_SENSOR, null, java.util.Map.of("getLastVibrationFrequency", 1)));
        assertEquals("minecraft:calibrated_sculk_sensor", calls.get(0)[4]);
        calls.clear();
        var hive = new java.util.HashMap<String, Object>();
        hive.put("getFlower", new org.bukkit.Location(player.getWorld(), 7, 66, -8));
        player.sendBlockUpdate(at(), tile(org.bukkit.block.Beehive.class, Material.BEEHIVE, null, hive));
        assertEquals("minecraft:beehive", calls.get(0)[4]);
        assertEquals("flower,7,66,-8", calls.get(0)[5]);
        calls.clear();
        var structure = new java.util.HashMap<String, Object>();
        structure.put("getStructureName", "test:house");
        structure.put("getAuthor", "me");
        structure.put("getMetadata", "");
        structure.put("getRelativePosition", new org.bukkit.util.BlockVector(1, 2, 3));
        structure.put("getStructureSize", new org.bukkit.util.BlockVector(4, 5, 6));
        structure.put("getRotation", org.bukkit.block.structure.StructureRotation.CLOCKWISE_180);
        structure.put("getMirror", org.bukkit.block.structure.Mirror.LEFT_RIGHT);
        structure.put("getUsageMode", org.bukkit.block.structure.UsageMode.SAVE);
        structure.put("isIgnoreEntities", true);
        structure.put("isShowAir", false);
        structure.put("isBoundingBoxVisible", true);
        structure.put("getIntegrity", 0.5f);
        structure.put("getSeed", 77L);
        player.sendBlockUpdate(at(), tile(org.bukkit.block.Structure.class, Material.STRUCTURE_BLOCK, null, structure));
        assertEquals("minecraft:structure_block", calls.get(0)[4]);
        var entries = java.util.Arrays.asList(calls.get(0)[5].split(String.valueOf(HostBridge.ITEM)));
        assertTrue(entries.containsAll(java.util.List.of("put,name,test:house", "put,rotation,180", "put,mirror,left_right", "put,mode,save", "put,sizeZ,6", "put,seed,77", "put,integrity,0.5")), entries.toString());
        calls.clear();
        var bed = tile(org.bukkit.block.Bed.class, Material.RED_BED, null, java.util.Map.of());
        assertThrows(UnsupportedOperationException.class, () -> player.sendBlockUpdate(at(), bed));
        assertTrue(calls.isEmpty());
    }
}
