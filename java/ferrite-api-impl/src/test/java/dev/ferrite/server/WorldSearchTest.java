package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.ArrayList;
import java.util.List;
import org.bukkit.Location;
import org.bukkit.Registry;
import org.bukkit.NamespacedKey;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

class WorldSearchTest {
    private static org.bukkit.World world;
    private final List<String> sent = new ArrayList<>();

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerWorld("00000000-0000-0000-0000-0000000000e7", "search_world", 0, 0, 64, 0);
        world = org.bukkit.Bukkit.getWorld("search_world");
    }

    @AfterEach void reset() { HostBridge.transport = null; }

    @org.junit.jupiter.api.AfterAll static void stop() {
        ((FerriteServer) org.bukkit.Bukkit.getServer()).unregisterWorld(world.getUID());
    }

    @Test void biomeSearchPicksTheNearestMatchAndReturnsAResult() {
        var plains = Registry.BIOME.get(NamespacedKey.fromString("minecraft:plains"));
        var desert = Registry.BIOME.get(NamespacedKey.fromString("minecraft:desert"));
        HostBridge.transport = (owner, subject, method, args) -> {
            sent.add(method + "|" + args);
            return args.startsWith("minecraft:plains") ? "300 70 0" : "50 70 0";
        };
        var found = world.locateNearestBiome(new Location(world, 0, 64, 0), 6400, plains, desert);
        assertEquals(desert, found.getBiome());
        assertEquals(50, found.getLocation().getBlockX());
        assertEquals(2, sent.size());
        assertTrue(sent.get(0).startsWith("locateBiome(String,int,int,int)|minecraft:plains"), sent.get(0));
    }

    @Test void searchesThatFindNothingReturnNull() {
        HostBridge.transport = (owner, subject, method, args) -> "";
        var plains = Registry.BIOME.get(NamespacedKey.fromString("minecraft:plains"));
        assertNull(world.locateNearestBiome(new Location(world, 0, 64, 0), 100, plains));
        var village = Registry.STRUCTURE.get(NamespacedKey.fromString("minecraft:village_plains"));
        assertNull(world.locateNearestStructure(new Location(world, 0, 64, 0), village, 50, false));
    }

    @Test void structureSearchAndContainmentGoThroughTheHost() {
        var village = Registry.STRUCTURE.get(NamespacedKey.fromString("minecraft:village_plains"));
        HostBridge.transport = (owner, subject, method, args) -> {
            sent.add(method);
            return method.startsWith("locateStructure") ? "-120 64 88" : "true";
        };
        var found = world.locateNearestStructure(new Location(world, 0, 64, 0), village, 50, false);
        assertEquals(village, found.getStructure());
        assertEquals(88, found.getLocation().getBlockZ());
        assertTrue(world.hasStructureAt(io.papermc.paper.math.Position.block(1, 64, 2), village));
        assertEquals(List.of("locateStructure(String,int,int,int)", "hasStructureAt(String,int,int,int)"), sent);
    }

    @Test void chunkStructuresComeFromTheHostStartList() {
        HostBridge.transport = (owner, subject, method, args) -> {
            sent.add(method + "|" + args);
            return "minecraft:village_plains 0 60 0 20 75 20\nminecraft:not_a_structure 1 2 3 4 5 6";
        };
        var all = world.getStructures(0, 0);
        assertEquals(1, all.size(), "unknown structures are skipped");
        var start = all.iterator().next();
        assertEquals("minecraft:village_plains", start.getStructure().getKey().toString());
        assertEquals(21, start.getBoundingBox().getWidthX(), "bounds are inclusive block corners");
        var village = Registry.STRUCTURE.get(NamespacedKey.fromString("minecraft:village_plains"));
        var desert = Registry.STRUCTURE.get(NamespacedKey.fromString("minecraft:desert_pyramid"));
        assertEquals(1, world.getStructures(0, 0, village).size());
        assertTrue(world.getStructures(0, 0, desert).isEmpty());
        assertTrue(sent.get(0).startsWith("getStructureStarts(int,int)|"), sent.get(0));
    }

    @Test void explorerMapsPointAtTheNearestStructureOrAreNull() {
        var server = org.bukkit.Bukkit.getServer();
        var village = Registry.STRUCTURE.get(NamespacedKey.fromString("minecraft:village_plains"));
        HostBridge.transport = (owner, subject, method, args) -> method.startsWith("locateStructure") ? "-120 64 88" : "";
        var item = server.createExplorerMap(world, new Location(world, 0, 64, 0), village, null, 50, false);
        assertEquals(org.bukkit.Material.FILLED_MAP, item.getType());
        var view = ((org.bukkit.inventory.meta.MapMeta) item.getItemMeta()).getMapView();
        assertEquals(-120, view.getCenterX());
        assertEquals(88, view.getCenterZ());
        HostBridge.transport = (owner, subject, method, args) -> "";
        assertNull(server.createExplorerMap(world, new Location(world, 0, 64, 0), village, null, 50, false));
    }

    @Test void generateTreeAsksTheHostToPlaceTheMatchingFeature() {
        sent.clear();
        HostBridge.transport = (owner, subject, method, args) -> {
            sent.add(method + "|" + args);
            return "true";
        };
        assertTrue(world.generateTree(new Location(world, 4, 70, 5), org.bukkit.TreeType.BIG_TREE));
        assertEquals(1, sent.size());
        assertTrue(sent.get(0).startsWith("placeFeature(Location,String)|"), sent.get(0));
        assertTrue(sent.get(0).endsWith("minecraft:fancy_oak"), sent.get(0));
        assertEquals("minecraft:oak", FerriteWorld.treeFeature(org.bukkit.TreeType.TREE));
        assertEquals("minecraft:red_poplar", FerriteWorld.treeFeature(org.bukkit.TreeType.RED_POPLAR));
        assertEquals("minecraft:pale_oak_creaking", FerriteWorld.treeFeature(org.bukkit.TreeType.PALE_OAK_CREAKING));
    }

    @Test void generateTreeWithABlockChangeDelegateReplaysTheHostBlocks() {
        sent.clear();
        HostBridge.transport = (owner, subject, method, args) -> {
            sent.add(method + "|" + args);
            return method.equals("featureBlocks(Location,String)")
                ? "4,70,5,minecraft:oak_log[axis=y]\n4,71,5,minecraft:oak_log[axis=y]" : "";
        };
        var delegate = new org.bukkit.BlockChangeDelegate() {
            private final java.util.Map<String, org.bukkit.block.data.BlockData> blocks = new java.util.HashMap<>();
            @Override public boolean setBlockData(int x, int y, int z, org.bukkit.block.data.BlockData data) {
                blocks.put(x + "," + y + "," + z, data);
                return true;
            }
            @Override public org.bukkit.block.data.BlockData getBlockData(int x, int y, int z) { return blocks.get(x + "," + y + "," + z); }
            @Override public int getHeight() { return 320; }
            @Override public boolean isEmpty(int x, int y, int z) { return getBlockData(x, y, z) == null; }
        };
        assertTrue(world.generateTree(new Location(world, 4, 70, 5), org.bukkit.TreeType.TREE, delegate));
        assertEquals(org.bukkit.Material.OAK_LOG, delegate.getBlockData(4, 70, 5).getMaterial());
        assertEquals(org.bukkit.Material.OAK_LOG, delegate.getBlockData(4, 71, 5).getMaterial());
        assertEquals(1, sent.size());
        assertTrue(sent.get(0).startsWith("featureBlocks(Location,String)|"), sent.get(0));
    }

    @Test void loadedTemplatesKnowTheirSizeAndPlaceThroughThePlaceCommand() throws Exception {
        HostBridge.transport = (owner, subject, method, args) -> { sent.add(method + "|" + args); return ""; };
        var manager = org.bukkit.Bukkit.getStructureManager();
        var key = NamespacedKey.fromString("minecraft:igloo/top");
        var igloo = manager.loadStructure(key);
        assertSame(igloo, manager.getStructure(key), "loading registers by default");
        assertTrue(igloo.getSize().getBlockX() > 0 && igloo.getSize().getBlockY() > 0);
        igloo.place(new Location(world, 4, 70, 5), false, org.bukkit.block.structure.StructureRotation.CLOCKWISE_90,
            org.bukkit.block.structure.Mirror.LEFT_RIGHT, 0, 0.5f, new java.util.Random(1));
        assertEquals(1, sent.size());
        assertTrue(sent.get(0).startsWith("placeTemplate(Location,String,String,String,String,String)|"), sent.get(0));
        assertTrue(sent.get(0).contains("minecraft:igloo/top") && sent.get(0).contains("clockwise_90") && sent.get(0).contains("left_right"), sent.get(0));
        assertNull(manager.loadStructure(NamespacedKey.fromString("minecraft:nope/none")));
    }

    @Test void withoutAGeneratorTheCallSaysItIsNotImplemented() {
        HostBridge.transport = (owner, subject, method, args) -> null;
        var plains = Registry.BIOME.get(NamespacedKey.fromString("minecraft:plains"));
        var e = assertThrows(UnsupportedOperationException.class,
            () -> world.locateNearestBiome(new Location(world, 0, 64, 0), 100, plains));
        assertTrue(e.getMessage().contains("locateBiome"), e.getMessage());
    }
}
