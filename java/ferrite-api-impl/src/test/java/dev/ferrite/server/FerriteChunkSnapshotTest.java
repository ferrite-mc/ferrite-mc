package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.assertEquals;
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

    @Test void aChunkSnapshotRefusesBlockAndLightDataItDidNotCapture() {
        var snapshot = FerriteChunkSnapshot.ofChunk(3, -4, "world", "00000000-0000-0000-0000-000000000001", Key.key("minecraft", "overworld"), -64, 320, 42L, true);
        for (java.util.function.Supplier<?> query : java.util.List.<java.util.function.Supplier<?>>of(
                () -> snapshot.getBlockType(0, 0, 0),
                () -> snapshot.getBlockData(0, 0, 0),
                () -> snapshot.getData(0, 0, 0),
                () -> snapshot.getBlockSkyLight(0, 0, 0),
                () -> snapshot.getBlockEmittedLight(0, 0, 0),
                () -> snapshot.getHighestBlockYAt(0, 0),
                () -> snapshot.isSectionEmpty(0),
                () -> snapshot.contains(org.bukkit.Material.AIR.createBlockData()))) {
            var refused = assertThrows(UnsupportedOperationException.class, query::get);
            assertTrue(refused.getMessage().contains("does not capture block"), refused.getMessage());
        }
        // Coordinates are still checked before the refusal, and biome queries still work from the host.
        assertThrows(IndexOutOfBoundsException.class, () -> snapshot.getBlockType(16, 0, 0));
    }
}
