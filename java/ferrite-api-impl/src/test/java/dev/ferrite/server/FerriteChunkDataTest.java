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
}
