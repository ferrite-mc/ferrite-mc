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

    @Test void withoutAGeneratorTheCallSaysItIsNotImplemented() {
        HostBridge.transport = (owner, subject, method, args) -> null;
        var plains = Registry.BIOME.get(NamespacedKey.fromString("minecraft:plains"));
        var e = assertThrows(UnsupportedOperationException.class,
            () -> world.locateNearestBiome(new Location(world, 0, 64, 0), 100, plains));
        assertTrue(e.getMessage().contains("locateBiome"), e.getMessage());
    }
}
