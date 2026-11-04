package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import org.bukkit.World.Environment;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

class WorldDimensionTest {
    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerWorld("00000000-0000-0000-0000-0000000000d1", "dimtest", 0, 0, 64, 0);
        Bootstrap.registerWorld("00000000-0000-0000-0000-0000000000d2", "dimtest_nether", 0, 0, 64, 0);
        Bootstrap.registerWorld("00000000-0000-0000-0000-0000000000d3", "dimtest_the_end", 0, 0, 64, 0);
    }

    @org.junit.jupiter.api.AfterAll static void stop() {
        for (var name : new String[]{"dimtest", "dimtest_nether", "dimtest_the_end"}) {
            var world = org.bukkit.Bukkit.getWorld(name);
            if (world != null) ((FerriteServer) org.bukkit.Bukkit.getServer()).unregisterWorld(world.getUID());
        }
    }

    @Test void overworldNetherAndEndKeepTheirVanillaLimits() {
        var overworld = org.bukkit.Bukkit.getWorld("dimtest");
        assertEquals(Environment.NORMAL, overworld.getEnvironment());
        assertEquals(-64, overworld.getMinHeight());
        assertEquals(320, overworld.getMaxHeight());
        assertEquals(63, overworld.getSeaLevel());
        assertTrue(overworld.hasSkyLight());
        assertEquals("minecraft:overworld", overworld.key().asString());

        var nether = org.bukkit.Bukkit.getWorld("dimtest_nether");
        assertEquals(Environment.NETHER, nether.getEnvironment());
        assertEquals(0, nether.getMinHeight());
        assertEquals(256, nether.getMaxHeight());
        assertEquals(128, nether.getLogicalHeight());
        assertEquals(8.0, nether.getCoordinateScale());
        assertTrue(nether.hasCeiling());
        assertTrue(nether.isUltraWarm());
        assertFalse(nether.isBedWorks());

        var end = org.bukkit.Bukkit.getWorld("dimtest_the_end");
        assertEquals(Environment.THE_END, end.getEnvironment());
        assertEquals("minecraft:the_end", end.key().asString());
        assertSame(overworld.getPersistentDataContainer(), overworld.getPersistentDataContainer());
    }

    @Test void worldKeepsMetadataSpawnSettingsAndChunksOfABox() {
        var world = org.bukkit.Bukkit.getWorld("dimtest");
        assertFalse(world.hasMetadata("flag"));
        assertTrue(world.getMetadata("flag").isEmpty());
        assertEquals(-1, world.getSpawnLimit(org.bukkit.entity.SpawnCategory.MONSTER));
        world.setSpawnLimit(org.bukkit.entity.SpawnCategory.MONSTER, 40);
        assertEquals(40, world.getSpawnLimit(org.bukkit.entity.SpawnCategory.MONSTER));
        world.setSpawnFlags(false, false);
        assertFalse(world.getAllowMonsters());
        assertFalse(world.getAllowAnimals());
        assertEquals(4, world.getIntersectingChunks(new org.bukkit.util.BoundingBox(0, 0, 0, 20, 10, 20)).size());
        assertTrue(world.getNearbyEntities(new org.bukkit.Location(world, 100000, 64, 100000), 5, 5, 5).isEmpty());
        assertNull(world.getEntity(java.util.UUID.randomUUID()));
    }
}
