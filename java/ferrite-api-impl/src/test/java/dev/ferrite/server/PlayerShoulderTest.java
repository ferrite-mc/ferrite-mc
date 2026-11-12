package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import org.bukkit.entity.EntityType;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

/** Shoulder parrots through the host: reads, seating, clearing and releasing, with a host that keeps the table. */
class PlayerShoulderTest {
    private static final String WORLD_ID = "00000000-0000-0000-0000-0000000000e9";
    private static final String PLAYER_ID = "00000000-0000-0000-0000-0000000000ea";
    private static org.bukkit.entity.Player player;
    private final List<String[]> calls = new ArrayList<>();
    /** The host's shoulder table: side to "uuid,variant". */
    private final Map<String, String> table = new HashMap<>();

    @BeforeAll static void start() throws Exception {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerWorld(WORLD_ID, "shoulder_world", 0, 0, 64, 0);
        Bootstrap.registerPlayer(PLAYER_ID, "Shoulder", false);
        var ensure = Bootstrap.class.getDeclaredMethod("ensurePlayer", String.class, String.class);
        ensure.setAccessible(true);
        player = (org.bukkit.entity.Player) ensure.invoke(null, PLAYER_ID, "Shoulder");
    }

    @AfterAll static void stop() {
        var server = (FerriteServer) org.bukkit.Bukkit.getServer();
        server.unregisterPlayer(player.getUniqueId());
        var world = org.bukkit.Bukkit.getWorld("shoulder_world");
        if (world != null) server.unregisterWorld(world.getUID());
    }

    @BeforeEach void host() {
        player.teleport(new org.bukkit.Location(org.bukkit.Bukkit.getWorld("shoulder_world"), 0, 65, 0, 0f, 0f));
        calls.clear();
        table.clear();
        HostBridge.transport = (owner, subject, method, args) -> {
            var parts = args.split(java.util.regex.Pattern.quote(String.valueOf(HostBridge.ARG)), -1);
            calls.add(new String[]{owner, method, String.join("|", parts)});
            if (method.equals("getShoulder(String)")) return table.getOrDefault(parts[0], "");
            if (method.equals("shoulder(String,String)")) {
                var op = parts[1];
                if (op.equals("CLEAR") || op.equals("RELEASE")) table.remove(parts[0]);
                else table.put(parts[0], op.substring("SEAT:".length()) + ",0");
            }
            return "";
        };
    }

    @AfterEach void reset() { HostBridge.transport = null; }

    private long shoulderWrites() { return calls.stream().filter(call -> call[1].equals("shoulder(String,String)")).count(); }

    @Test void anEmptyShoulderReadsAsNull() {
        assertNull(player.getShoulderEntityLeft());
        assertNull(player.getShoulderEntityRight());
        assertNull(player.releaseLeftShoulderEntity());
        assertEquals(0, shoulderWrites(), "releasing an empty shoulder asks the host for nothing");
    }

    @Test void aSeatedParrotKeepsItsUuidAndTheOtherSideStaysEmpty() {
        var parrot = SpawnedEntity.of(EntityType.PARROT, player.getLocation(), player.getWorld(), UUID.randomUUID());
        player.setShoulderEntityRight(parrot);
        assertEquals("RIGHT|SEAT:" + parrot.getUniqueId(), calls.get(0)[2]);
        var read = player.getShoulderEntityRight();
        assertEquals(EntityType.PARROT, read.getType());
        assertEquals(parrot.getUniqueId(), read.getUniqueId());
        assertNull(player.getShoulderEntityLeft());
    }

    @Test void nullClearsWithoutReleasingAndOnlyParrotsAreAccepted() {
        var parrot = SpawnedEntity.of(EntityType.PARROT, player.getLocation(), player.getWorld(), UUID.randomUUID());
        player.setShoulderEntityLeft(parrot);
        player.setShoulderEntityLeft(null);
        assertEquals("LEFT|CLEAR", calls.get(calls.size() - 1)[2]);
        assertNull(player.getShoulderEntityLeft());
        calls.clear();
        var cow = SpawnedEntity.of(EntityType.COW, player.getLocation(), player.getWorld(), UUID.randomUUID());
        assertThrows(IllegalArgumentException.class, () -> player.setShoulderEntityLeft(cow));
        assertEquals(0, shoulderWrites(), "a refused entity never reaches the host");
    }

    @Test void releaseReturnsTheParrotAndEmptiesTheShoulder() {
        var parrot = SpawnedEntity.of(EntityType.PARROT, player.getLocation(), player.getWorld(), UUID.randomUUID());
        player.setShoulderEntityLeft(parrot);
        var released = player.releaseLeftShoulderEntity();
        assertEquals(parrot.getUniqueId(), released.getUniqueId());
        assertEquals("LEFT|RELEASE", calls.get(calls.size() - 1)[2]);
        assertNull(player.getShoulderEntityLeft());
        assertNull(player.releaseLeftShoulderEntity(), "a second release finds nothing");
    }

    @Test void withoutAHostNothingCanBeSeated() {
        HostBridge.transport = null;
        assertNull(player.getShoulderEntityLeft());
        assertNull(player.releaseRightShoulderEntity());
        var parrot = SpawnedEntity.of(EntityType.PARROT, player.getLocation(), player.getWorld(), UUID.randomUUID());
        assertThrows(UnsupportedOperationException.class, () -> player.setShoulderEntityLeft(parrot));
    }
}
