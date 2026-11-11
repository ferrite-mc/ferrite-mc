package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.ArrayList;
import java.util.List;
import java.util.Random;
import java.util.UUID;
import org.bukkit.Location;
import org.bukkit.NamespacedKey;
import org.bukkit.block.structure.Mirror;
import org.bukkit.block.structure.StructureRotation;
import org.bukkit.entity.EntityType;
import org.bukkit.util.BlockVector;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

/** Entity capture and placement, answered by a scripted host: the three non-player entity behaviours of a structure. */
class StructureEntitiesTest {
    private static final char FIELD = FerriteEntitySnapshot.FIELD;
    private static final String SEP = String.valueOf(HostBridge.ITEM);
    private static final String WORLD_ID = "00000000-0000-0000-0000-0000000000f9";
    private static final UUID PIG = UUID.fromString("00000000-0000-0000-0000-0000000000a1");
    private static final String PIG_SNBT = "{id:\"minecraft:pig\",Pos:[101.5d,71.0d,100.25d],Rotation:[90.0f,5.0f],UUID:[I;0,0,0,161],Health:8.0f}";
    private static org.bukkit.World world;

    private final List<String> queries = new ArrayList<>();
    private final List<String[]> spawns = new ArrayList<>();
    private String entityReply = "";

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerWorld(WORLD_ID, "structure_entity_world", 0, 0, 64, 0);
        world = org.bukkit.Bukkit.getWorld("structure_entity_world");
    }

    @AfterAll static void stop() {
        if (world != null) ((FerriteServer) org.bukkit.Bukkit.getServer()).unregisterWorld(world.getUID());
    }

    @BeforeEach void host() {
        HostBridge.transport = (owner, subject, method, args) -> {
            if (!owner.equals("World")) return null;
            if (method.equals("ferriteEntities(int,int,int,int)")) { queries.add(args); return entityReply; }
            if (method.equals("ferriteBlockEntities(int,int,int,int,int,int)")) return "";
            if (method.equals("ferriteSpawnSnapshot(String,String,double,double,double,double)")) { spawns.add(args.split(String.valueOf(HostBridge.ARG), -1)); return "true"; }
            return null;
        };
    }

    @AfterEach void unhost() { HostBridge.transport = null; }

    private static String record(String kind, double x, double y, double z, UUID uuid, String snbt) {
        var nbt = java.util.Base64.getEncoder().encodeToString(snbt.getBytes(java.nio.charset.StandardCharsets.UTF_8));
        return String.join(String.valueOf(FIELD), kind, String.valueOf(x), String.valueOf(y), String.valueOf(z), "90.0", "5.0",
            uuid.toString(), "8.0", "0.0", "0.25", "0.0", HostBridge.NULL, nbt);
    }

    private org.bukkit.structure.Structure capture(boolean includeEntities) {
        for (int x = 0; x < 2; x++) for (int y = 0; y < 2; y++) for (int z = 0; z < 2; z++) world.getBlockAt(100 + x, 70 + y, 100 + z).setType(org.bukkit.Material.AIR);
        var structure = org.bukkit.Bukkit.getStructureManager().createStructure();
        structure.fill(new Location(world, 100, 70, 100), new BlockVector(2, 2, 2), includeEntities);
        return structure;
    }

    private void pigAndAStrayOutsideTheBox() {
        entityReply = record("minecraft:pig", 101.5, 71.0, 100.25, PIG, PIG_SNBT) + SEP
            + record("minecraft:cow", 101.5, 90.0, 100.5, UUID.fromString("00000000-0000-0000-0000-0000000000a2"), "{id:\"minecraft:cow\"}");
    }

    @Test void fillReadsTheEntitiesInsideTheBoxFromTheHost() {
        pigAndAStrayOutsideTheBox();
        var structure = capture(true);
        assertEquals(List.of(String.join(String.valueOf(HostBridge.ARG), "6", "6", "6", "6")), queries, "one query over the chunk the box touches");
        assertEquals(1, structure.getEntityCount(), "the cow stands above the box");
        var pig = structure.getEntities().get(0);
        assertEquals(EntityType.PIG, pig.getType());
        assertEquals(PIG, pig.getUniqueId());
        assertEquals(1.5, pig.getLocation().getX(), 1e-9);
        assertEquals(1.0, pig.getLocation().getY(), 1e-9);
        assertEquals(0.25, pig.getLocation().getZ(), 1e-9);
        assertEquals(0.25, pig.getVelocity().getY(), 1e-9);
        assertFalse(pig.isValid(), "a structure entity is not in any world");
        assertEquals(PIG_SNBT, pig.createSnapshot().getAsString());
    }

    @Test void fillWithoutEntitiesNeverAsksTheHostAndKeepsNone() {
        pigAndAStrayOutsideTheBox();
        var structure = capture(false);
        assertTrue(queries.isEmpty());
        assertEquals(0, structure.getEntityCount());
        assertTrue(structure.getEntities().isEmpty());
    }

    @Test void aWorldTheHostCannotEnumerateCapturesBlocksOnly() {
        HostBridge.transport = (owner, subject, method, args) -> method.equals("ferriteBlockEntities(int,int,int,int,int,int)") ? "" : null;
        var structure = capture(true);
        assertEquals(0, structure.getEntityCount());
        assertEquals(8, structure.getPalettes().get(0).getBlockCount());
    }

    @Test void placeSpawnsTheSnapshotThroughTheHostUnderAFreshIdentity() {
        pigAndAStrayOutsideTheBox();
        var structure = capture(true);
        structure.place(new Location(world, 200, 70, 300), true, StructureRotation.NONE, Mirror.NONE, 0, 1f, new Random(1));
        assertEquals(1, spawns.size());
        var call = spawns.get(0);
        assertEquals(PIG_SNBT, call[0]);
        assertNotEquals(PIG.toString(), call[1], "the copy gets its own UUID");
        assertEquals(201.5, Double.parseDouble(call[2]), 1e-9);
        assertEquals(71.0, Double.parseDouble(call[3]), 1e-9);
        assertEquals(300.25, Double.parseDouble(call[4]), 1e-9);
        assertEquals(90.0, Double.parseDouble(call[5]), 1e-9);

        spawns.clear();
        structure.place(new Location(world, 200, 70, 300), false, StructureRotation.NONE, Mirror.NONE, 0, 1f, new Random(1));
        assertTrue(spawns.isEmpty(), "includeEntities=false places blocks only");
    }

    @Test void placeTurnsEntityPositionsAndFacingLikeBlocks() {
        pigAndAStrayOutsideTheBox();
        var structure = capture(true);
        structure.place(new Location(world, 200, 70, 300), true, StructureRotation.CLOCKWISE_90, Mirror.NONE, 0, 1f, new Random(1));
        var call = spawns.get(0);
        assertEquals(200 + (1.0 - 0.25), Double.parseDouble(call[2]), 1e-9);
        assertEquals(300 + 1.5, Double.parseDouble(call[4]), 1e-9);
        assertEquals(-180.0, Double.parseDouble(call[5]), 1e-9, "90 + 90 degrees wraps to -180");
    }

    @Test void entityTransformersSeeEachEntityAndCanDropIt() {
        pigAndAStrayOutsideTheBox();
        var structure = capture(true);
        var seen = new ArrayList<String>();
        structure.place(new Location(world, 200, 70, 300), true, StructureRotation.NONE, Mirror.NONE, 0, 1f, new Random(1), List.of(),
            List.of((region, x, y, z, entity, turned) -> { seen.add(entity.getType() + "@" + x + "," + y + "," + z + " " + turned); return false; }));
        assertEquals(List.of("PIG@201,71,300 false"), seen);
        assertTrue(spawns.isEmpty(), "a transformer returning false drops the entity");

        structure.place(new Location(world, 200, 70, 300), true, StructureRotation.NONE, Mirror.NONE, 0, 1f, new Random(1), List.of(),
            List.of((region, x, y, z, entity, turned) -> true));
        assertEquals(1, spawns.size());
    }

    @Test void aSnapshotFromTheHostBecomesALiveEntityThroughTheSpawnOp() {
        var snapshot = FerriteEntitySnapshot.fromRecord(record("minecraft:pig", 1, 2, 3, PIG, PIG_SNBT));
        var entity = snapshot.createEntity(new Location(world, 10, 70, 20));
        assertEquals(1, spawns.size());
        assertEquals(PIG_SNBT, spawns.get(0)[0]);
        assertEquals(entity.getUniqueId().toString(), spawns.get(0)[1]);
        assertNotEquals(PIG, entity.getUniqueId());
        assertEquals(EntityType.PIG, entity.getType());
    }

    @Test void aRefilledTemplateReportsTheEntitiesItCaptured() throws Exception {
        pigAndAStrayOutsideTheBox();
        var manager = org.bukkit.Bukkit.getStructureManager();
        var key = new NamespacedKey("ferrite_test", "entity_template_" + System.nanoTime());
        manager.registerStructure(key, capture(false));
        manager.saveStructure(key);
        var template = manager.loadStructure(key, false);
        assertEquals(0, template.getEntityCount(), "template files carry no entities");
        template.fill(new Location(world, 100, 70, 100), new BlockVector(2, 2, 2), true);
        assertEquals(1, template.getEntityCount());
        assertEquals(PIG, template.getEntities().get(0).getUniqueId());
        manager.deleteStructure(key);
    }
}
