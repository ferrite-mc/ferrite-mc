package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

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
}
