package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.util.Random;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.block.structure.Mirror;
import org.bukkit.block.structure.StructureRotation;
import org.bukkit.util.BlockVector;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

class CapturedStructureTest {
    private static org.bukkit.World world;

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerWorld("00000000-0000-0000-0000-0000000000f7", "structure_world", 0, 0, 64, 0);
        world = org.bukkit.Bukkit.getWorld("structure_world");
    }

    @org.junit.jupiter.api.AfterAll static void stop() {
        if (world != null) ((FerriteServer) org.bukkit.Bukkit.getServer()).unregisterWorld(world.getUID());
    }

    private static void build() {
        for (int x = 0; x < 2; x++) for (int y = 0; y < 2; y++) for (int z = 0; z < 3; z++) world.getBlockAt(100 + x, 70 + y, 100 + z).setType(Material.AIR);
        world.getBlockAt(100, 70, 100).setType(Material.STONE);
        world.getBlockAt(101, 70, 102).setType(Material.OAK_PLANKS);
    }

    @Test void fillCapturesTheRegionAndPlaceWritesItBack() {
        build();
        var structure = org.bukkit.Bukkit.getStructureManager().createStructure();
        structure.fill(new Location(world, 101, 71, 102), new Location(world, 100, 70, 100), false);
        assertEquals(new BlockVector(2, 2, 3), structure.getSize());
        assertEquals(1, structure.getPaletteCount());
        var blocks = structure.getPalettes().get(0).getBlocks();
        assertEquals(12, structure.getPalettes().get(0).getBlockCount());
        assertEquals(12, blocks.size());
        assertEquals(Material.STONE, blocks.stream().filter(b -> b.getX() == 0 && b.getY() == 0 && b.getZ() == 0).findFirst().orElseThrow().getType());

        structure.place(new Location(world, 200, 70, 200), false, StructureRotation.NONE, Mirror.NONE, 0, 1f, new Random(1));
        assertEquals(Material.STONE, world.getBlockAt(200, 70, 200).getType());
        assertEquals(Material.OAK_PLANKS, world.getBlockAt(201, 70, 202).getType());
        assertEquals(Material.AIR, world.getBlockAt(201, 71, 201).getType());
    }

    @Test void rotationAndMirrorMoveBlocksAroundTheOrigin() {
        build();
        var structure = org.bukkit.Bukkit.getStructureManager().createStructure();
        structure.fill(new Location(world, 100, 70, 100), new BlockVector(2, 2, 3), false);
        // plank at (1,0,2): clockwise 90 sends (x,z) to (-z,x) = (-2,1)
        structure.place(new Location(world, 300, 70, 300), false, StructureRotation.CLOCKWISE_90, Mirror.NONE, 0, 1f, new Random(1));
        assertEquals(Material.OAK_PLANKS, world.getBlockAt(298, 70, 301).getType());
        // left-right mirror flips z: plank at (1,0,-2)
        structure.place(new Location(world, 400, 70, 400), false, StructureRotation.NONE, Mirror.LEFT_RIGHT, 0, 1f, new Random(1));
        assertEquals(Material.OAK_PLANKS, world.getBlockAt(401, 70, 398).getType());
    }

    @Test void zeroIntegrityPlacesNothing() {
        build();
        var structure = org.bukkit.Bukkit.getStructureManager().createStructure();
        structure.fill(new Location(world, 100, 70, 100), new BlockVector(2, 2, 3), false);
        world.getBlockAt(500, 70, 500).setType(Material.DIRT);
        structure.place(new Location(world, 500, 70, 500), false, StructureRotation.NONE, Mirror.NONE, 0, 0f, new Random(2));
        assertEquals(Material.DIRT, world.getBlockAt(500, 70, 500).getType());
    }

    @Test void facingAndAxisTurnWithTheBlocks() {
        assertEquals("minecraft:oak_stairs[facing=east,half=top]", CapturedStructure.turn("minecraft:oak_stairs[facing=north,half=top]", StructureRotation.CLOCKWISE_90, Mirror.NONE));
        assertEquals("minecraft:oak_stairs[facing=south]", CapturedStructure.turn("minecraft:oak_stairs[facing=north]", StructureRotation.NONE, Mirror.LEFT_RIGHT));
        assertEquals("minecraft:oak_log[axis=z]", CapturedStructure.turn("minecraft:oak_log[axis=x]", StructureRotation.CLOCKWISE_90, Mirror.NONE));
        assertEquals("minecraft:stone", CapturedStructure.turn("minecraft:stone", StructureRotation.CLOCKWISE_180, Mirror.NONE));
    }

    @Test void saveAndLoadKeepSizeAndBlocks() throws Exception {
        build();
        var manager = org.bukkit.Bukkit.getStructureManager();
        var structure = manager.createStructure();
        structure.fill(new Location(world, 100, 70, 100), new BlockVector(2, 2, 3), false);
        var out = new ByteArrayOutputStream();
        manager.saveStructure(out, structure);
        var back = manager.loadStructure(new ByteArrayInputStream(out.toByteArray()));
        assertEquals(structure.getSize(), back.getSize());
        assertEquals(12, back.getPalettes().get(0).getBlockCount());
        var plank = back.getPalettes().get(0).getBlocks().stream().filter(b -> b.getX() == 1 && b.getZ() == 2 && b.getY() == 0).findFirst().orElseThrow();
        assertEquals(Material.OAK_PLANKS, plank.getType());
        assertEquals(12, manager.copy(back).getPalettes().get(0).getBlockCount());
    }

    @Test void filesLiveWhereVanillaLooksAndDeleteRemovesThem() throws Exception {
        build();
        var manager = org.bukkit.Bukkit.getStructureManager();
        var key = new NamespacedKey("ferrite_test", "saved_hut_" + System.nanoTime());
        var structure = manager.createStructure();
        structure.fill(new Location(world, 100, 70, 100), new BlockVector(2, 2, 3), false);
        manager.registerStructure(key, structure);
        manager.saveStructure(key);
        var file = manager.getStructureFile(key);
        assertTrue(file.isFile(), file.toString());
        var loaded = manager.loadStructure(file);
        assertEquals(12, loaded.getPalettes().get(0).getBlockCount());
        manager.deleteStructure(key);
        assertFalse(file.exists());
        assertNull(manager.getStructure(key));
        assertThrows(IllegalStateException.class, () -> manager.saveStructure(key));
    }

    @Test void blockStatePropertiesSurviveFillAndPlace() {
        world.getBlockAt(600, 70, 600).setBlockData(FerriteBlockData.create("minecraft:oak_stairs[facing=east,half=top]"));
        var structure = org.bukkit.Bukkit.getStructureManager().createStructure();
        structure.fill(new Location(world, 600, 70, 600), new BlockVector(1, 1, 1), false);
        assertEquals("minecraft:oak_stairs[facing=east,half=top]", structure.getPalettes().get(0).getBlocks().get(0).getBlockData().getAsString());
        structure.place(new Location(world, 610, 70, 610), false, StructureRotation.CLOCKWISE_90, Mirror.NONE, 0, 1f, new Random(1));
        assertEquals("minecraft:oak_stairs[facing=south,half=top]", world.getBlockAt(610, 70, 610).getBlockData().getAsString());
        world.getBlockAt(600, 70, 600).setType(Material.STONE);
        assertEquals("minecraft:stone", world.getBlockAt(600, 70, 600).getBlockData().getAsString());
    }

    /** Binary NBT of {id:"minecraft:chest", x:100, y:70, z:100, Items:[{Slot:0b, id:"minecraft:bread", count:2}]}. */
    private static String chestHex() throws Exception {
        var bytes = new ByteArrayOutputStream();
        var out = new java.io.DataOutputStream(bytes);
        out.writeByte(10); out.writeUTF("");
        out.writeByte(8); out.writeUTF("id"); out.writeUTF("minecraft:chest");
        out.writeByte(3); out.writeUTF("x"); out.writeInt(100);
        out.writeByte(3); out.writeUTF("y"); out.writeInt(70);
        out.writeByte(3); out.writeUTF("z"); out.writeInt(100);
        out.writeByte(9); out.writeUTF("Items"); out.writeByte(10); out.writeInt(1);
        out.writeByte(1); out.writeUTF("Slot"); out.writeByte(0);
        out.writeByte(8); out.writeUTF("id"); out.writeUTF("minecraft:bread");
        out.writeByte(3); out.writeUTF("count"); out.writeInt(2);
        out.writeByte(0);
        out.writeByte(0);
        var hex = new StringBuilder();
        for (byte b : bytes.toByteArray()) hex.append(String.format("%02x", b));
        return hex.toString();
    }

    @Test void blockEntityDataIsCapturedSavedAndWrittenBack() throws Exception {
        build();
        world.getBlockAt(100, 70, 100).setType(Material.CHEST);
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> {
            calls.add(method + " " + args);
            try {
                return method.startsWith("ferriteBlockEntities") ? "100,70,100," + chestHex() : "";
            } catch (Exception e) { throw new IllegalStateException(e); }
        };
        try {
            var manager = org.bukkit.Bukkit.getStructureManager();
            var structure = manager.createStructure();
            structure.fill(new Location(world, 100, 70, 100), new BlockVector(2, 2, 3), false);
            var out = new ByteArrayOutputStream();
            manager.saveStructure(out, structure);
            var back = manager.loadStructure(new ByteArrayInputStream(out.toByteArray()));
            calls.clear();
            back.place(new Location(world, 700, 70, 700), false, StructureRotation.NONE, Mirror.NONE, 0, 1f, new Random(1));
            var merge = calls.stream().filter(c -> c.startsWith("ferriteDataMerge")).findFirst().orElseThrow();
            assertTrue(merge.contains("700") && merge.contains("70"), merge);
            assertTrue(merge.contains("Items:[{Slot:0b,id:\"minecraft:bread\",count:2}]"), merge);
            assertFalse(merge.contains("x:"), "the old position is not written back: " + merge);
        } finally { HostBridge.transport = null; }
    }

    @Test void snbtWritesEveryTagType() {
        var map = new java.util.LinkedHashMap<String, Object>();
        map.put("b", (byte) 1); map.put("s", (short) 2); map.put("i", 3); map.put("l", 4L);
        map.put("f", 1.5f); map.put("d", 2.5d); map.put("t", "a\"b");
        map.put("ints", new int[] {1, 2}); map.put("list", java.util.List.of(1, 2));
        assertEquals("{b:1b,s:2s,i:3,l:4L,f:1.5f,d:2.5d,t:\"a\\\"b\",ints:[I;1,2],list:[1,2]}", Snbt.of(map));
    }

    @Test void aLoadedTemplateCanBeRefilledAndThenPlacesFromMemory() {
        build();
        var manager = org.bukkit.Bukkit.getStructureManager();
        var template = manager.loadStructure(new NamespacedKey("minecraft", "igloo/top"), false);
        assertNotNull(template);
        assertTrue(template.getPaletteCount() >= 1, "the file is read into palettes");
        template.fill(new Location(world, 100, 70, 100), new BlockVector(2, 2, 3), false);
        assertEquals(new BlockVector(2, 2, 3), template.getSize());
        template.place(new Location(world, 800, 70, 800), false, StructureRotation.NONE, Mirror.NONE, 0, 1f, new Random(1));
        assertEquals(Material.STONE, world.getBlockAt(800, 70, 800).getType());
    }
}
