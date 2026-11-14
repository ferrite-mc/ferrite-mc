package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import net.minecraft.world.entity.Entity;
import net.minecraft.world.phys.Vec3;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

class NmsEntityHandleTest {
    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
    }

    @Test void vectorMathMatchesNms() {
        var v = new Vec3(3, 0, 4);
        assertEquals(5.0, v.length(), 1e-9);
        assertEquals(25.0, v.lengthSqr(), 1e-9);
        assertEquals(new Vec3(0.6, 0, 0.8).x, v.normalize().x, 1e-9);
        assertEquals(new Vec3(6, 0, 8), v.scale(2));
        assertEquals(new Vec3(4, 1, 5), v.add(new Vec3(1, 1, 1)));
        assertEquals(Vec3.ZERO, Vec3.ZERO.normalize());
        assertEquals(4.0, v.toBukkit().getZ(), 1e-9);
    }

    @Test void entityHandlesReadAndWriteThroughBukkit() {
        var player = org.bukkit.Bukkit.getOnlinePlayers().stream().findFirst();
        // Bootstrap registers no online player by default; exercise the wrapper on the one the test fixture adds.
        Bootstrap.registerPlayer("00000000-0000-0000-0000-0000000000f1", "NmsHandle", false);
        var bukkit = org.bukkit.Bukkit.getPlayer("NmsHandle");
        assertNotNull(bukkit);
        Entity handle = Entity.of(bukkit);
        assertTrue(handle instanceof net.minecraft.server.level.ServerPlayer);
        assertEquals(bukkit.getUniqueId(), handle.getUUID());
        assertFalse(handle.isPassenger());
        assertTrue(handle.getPassengers().isEmpty());
        assertNull(handle.getVehicle());
        assertTrue(handle.addTag("nms_tag"));
        assertTrue(handle.getTags().contains("nms_tag"));
        assertTrue(handle.removeTag("nms_tag"));
        assertEquals(handle.getBbHeight(), (float) bukkit.getHeight());
    }
}
