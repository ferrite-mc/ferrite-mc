package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import org.bukkit.HeightMap;
import org.bukkit.Material;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

class FerriteChunkDataTest {
    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerWorld("00000000-0000-0000-0000-0000000000c1", "chunkdata", 0, 0, 64, 0);
    }

    @org.junit.jupiter.api.AfterAll static void stop() {
        var world = org.bukkit.Bukkit.getWorld("chunkdata");
        if (world != null) ((FerriteServer) org.bukkit.Bukkit.getServer()).unregisterWorld(world.getUID());
    }

    private static org.bukkit.generator.ChunkGenerator.ChunkData data() {
        return org.bukkit.Bukkit.getServer().createChunkData(org.bukkit.Bukkit.getWorld("chunkdata"));
    }

    @Test void blocksRoundTripAndOutOfRangeIsAirOrIgnored() {
        var chunk = data();
        assertEquals(-64, chunk.getMinHeight());
        chunk.setBlock(3, 10, 4, Material.STONE);
        assertEquals(Material.STONE, chunk.getType(3, 10, 4));
        assertEquals(Material.AIR, chunk.getType(0, 0, 0));
        chunk.setBlock(16, 10, 0, Material.DIRT);
        chunk.setBlock(0, 1000, 0, Material.DIRT);
        assertEquals(Material.AIR, chunk.getType(16, 10, 0));
        assertEquals(Material.AIR, chunk.getType(0, 1000, 0));
        chunk.setBlock(3, 10, 4, Material.AIR);
        assertEquals(Material.AIR, chunk.getType(3, 10, 4));
    }

    @Test void regionsExcludeTheMaximumCornerAndClampToTheChunk() {
        var chunk = data();
        chunk.setRegion(0, 0, 0, 16, 3, 16, Material.BEDROCK);
        assertEquals(Material.BEDROCK, chunk.getType(15, 2, 15));
        assertEquals(Material.AIR, chunk.getType(15, 3, 15));
        chunk.setRegion(-5, 5, -5, 40, 6, 40, Material.GRASS_BLOCK);
        assertEquals(Material.GRASS_BLOCK, chunk.getType(0, 5, 0));
    }

    @Test void heightIsOneAboveTheHighestBlock() {
        var chunk = data();
        assertEquals(chunk.getMinHeight(), chunk.getHeight(HeightMap.WORLD_SURFACE, 1, 1));
        chunk.setBlock(1, 20, 1, Material.STONE);
        assertEquals(21, chunk.getHeight(HeightMap.WORLD_SURFACE, 1, 1));
    }

    @Test void emptySnapshotIsAllAirAndRangeChecked() {
        var snap = org.bukkit.Bukkit.getWorld("chunkdata").getEmptyChunkSnapshot(2, -3, false, false);
        assertEquals(2, snap.getX());
        assertEquals(-3, snap.getZ());
        assertEquals("chunkdata", snap.getWorldName());
        assertEquals(Material.AIR, snap.getBlockType(1, 70, 1));
        assertTrue(snap.isSectionEmpty(4));
        assertEquals(-64, snap.getHighestBlockYAt(0, 0));
        assertThrows(IndexOutOfBoundsException.class, () -> snap.getBlockType(16, 0, 0));
    }

    @Test void lootTablesExistOnlyForVanillaFilesAndAreCached() {
        var server = org.bukkit.Bukkit.getServer();
        var key = org.bukkit.NamespacedKey.minecraft("blocks/stone");
        var table = server.getLootTable(key);
        assertNotNull(table);
        assertSame(table, server.getLootTable(key));
        assertEquals(key, table.getKey());
        assertNull(server.getLootTable(org.bukkit.NamespacedKey.minecraft("blocks/no_such_table")));
        assertThrows(UnsupportedOperationException.class, () -> table.populateLoot(new java.util.Random(), null));
    }

    @Test void chunkStateComesFromTheHost() {
        var world = org.bukkit.Bukkit.getWorld("chunkdata");
        HostBridge.transport = (owner, subject, method, args) -> switch (method) {
            case "isChunkLoaded(int,int)" -> args.equals("3" + HostBridge.ARG + "4") ? "true" : "false";
            case "getLoadedChunkCoordinates()" -> "3,4" + HostBridge.ITEM + "-1,7";
            default -> null;
        };
        try {
            assertTrue(world.isChunkLoaded(3, 4));
            assertFalse(world.isChunkLoaded(4, 4));
            var chunks = world.getLoadedChunks();
            assertEquals(2, chunks.length);
            assertEquals(-1, chunks[1].getX());
            assertEquals(7, chunks[1].getZ());
            assertTrue(world.getChunkAt(3, 4).isLoaded());
            assertFalse(world.getChunkAt(0, 0).isLoaded());
        } finally { HostBridge.transport = null; }
    }

    @Test void asyncChunkLookupsHandBackTheChunkView() throws Exception {
        var world = org.bukkit.Bukkit.getWorld("chunkdata");
        var seen = new java.util.ArrayList<org.bukkit.Chunk>();
        world.getChunkAtAsync(5, -6, true, true, seen::add);
        assertEquals(1, seen.size());
        assertEquals(5, seen.get(0).getX());
        assertEquals(-6, seen.get(0).getZ());
        var future = world.getChunkAtAsync(new org.bukkit.Location(world, 33, 70, -17));
        assertEquals(2, future.get().getX());
        assertEquals(-2, future.get().getZ());
    }

    @Test void worldFlagsAreRememberedAndAutoSaveTellsTheHost() {
        var world = org.bukkit.Bukkit.getWorld("chunkdata");
        assertTrue(world.getPVP());
        assertFalse(world.isHardcore());
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method + "|" + args); return ""; };
        try {
            world.setPVP(false);
            world.setHardcore(true);
            world.setAutoSave(false);
        } finally { HostBridge.transport = null; }
        assertFalse(world.getPVP());
        assertTrue(world.isHardcore());
        assertFalse(world.isAutoSave());
        assertEquals(java.util.List.of("setAutoSave(boolean)|false"), calls);
        world.setPVP(true);
        world.setHardcore(false);
        world.setAutoSave(true);
    }
}
