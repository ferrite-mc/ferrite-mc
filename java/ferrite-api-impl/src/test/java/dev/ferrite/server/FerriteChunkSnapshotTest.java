package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import net.kyori.adventure.key.Key;
import org.junit.jupiter.api.Test;

class FerriteChunkSnapshotTest {
    private static FerriteChunkSnapshot snapshot() {
        return new FerriteChunkSnapshot(3, -4, "world", "00000000-0000-0000-0000-000000000001", Key.key("minecraft", "overworld"), -64, 320, 42L);
    }

    @Test void reportsItsWorldKeyAndCoordinates() {
        var snapshot = snapshot();
        assertEquals(Key.key("minecraft", "overworld"), snapshot.getWorldKey());
        assertEquals("world", snapshot.getWorldName());
        assertEquals(3, snapshot.getX());
        assertEquals(-4, snapshot.getZ());
        assertEquals(42L, snapshot.getCaptureFullTime());
    }

    @Test void rejectsBlocksOutsideTheChunkOrWorldHeight() {
        var snapshot = snapshot();
        assertThrows(IndexOutOfBoundsException.class, () -> snapshot.getBlockType(16, 0, 0));
        assertThrows(IndexOutOfBoundsException.class, () -> snapshot.getBlockType(0, 320, 0));
        assertEquals(org.bukkit.Material.AIR, snapshot.getBlockType(0, 0, 0));
    }

    /** The host's small test snapshot: 16 blocks high, air except for stone at (1, 2, 1). */
    private static String captured() {
        int count = 16 * 256;
        int[] blocks = new int[count];
        blocks[(2 * 16 + 1) * 16 + 1] = 1;
        int skyAt = 1 * 256;
        int blockAt = (2 * 16 + 1) * 16 + 1;
        StringBuilder sky = new StringBuilder("0".repeat(count));
        StringBuilder block = new StringBuilder("0".repeat(count));
        sky.setCharAt(skyAt, 'f');
        block.setCharAt(blockAt, '3');
        int[] biomes = new int[4 * 16];
        int[] heights = new int[256];
        heights[1 * 16 + 1] = 2;
        return "1\n0\n16\n2\nminecraft:air\nminecraft:stone\n" + count + "\n" + join(blocks)
                + "\n" + sky + "\n" + block + "\n1\nminecraft:plains\n" + biomes.length + "\n"
                + join(biomes) + "\n256\n" + join(heights) + "\n";
    }

    private static String join(int[] values) {
        StringBuilder out = new StringBuilder();
        for (int i = 0; i < values.length; i++) {
            if (i > 0) out.append(',');
            out.append(values[i]);
        }
        return out.toString();
    }

    @Test void capturesBlocksLightBiomesAndHeights() {
        var snapshot = FerriteChunkSnapshot.capture(3, -4, "world",
                "00000000-0000-0000-0000-000000000001", Key.key("minecraft", "overworld"),
                0, 16, 42L, true, true, true, captured());
        assertEquals(org.bukkit.Material.STONE, snapshot.getBlockType(1, 2, 1));
        assertEquals(org.bukkit.Material.AIR, snapshot.getBlockType(0, 0, 0));
        assertEquals(15, snapshot.getBlockSkyLight(0, 1, 0));
        assertEquals(3, snapshot.getBlockEmittedLight(1, 2, 1));
        assertEquals(2, snapshot.getHighestBlockYAt(1, 1));
        assertEquals(org.bukkit.block.Biome.PLAINS, snapshot.getBiome(1, 2, 1));
        assertTrue(snapshot.contains(org.bukkit.block.Biome.PLAINS));
        assertFalse(snapshot.contains(org.bukkit.block.Biome.NETHER_WASTES));
        assertTrue(snapshot.contains(FerriteBlockData.create("minecraft:stone")));
        assertTrue(snapshot.contains(org.bukkit.Material.AIR.createBlockData()));
        assertFalse(snapshot.isSectionEmpty(0));
    }
}
