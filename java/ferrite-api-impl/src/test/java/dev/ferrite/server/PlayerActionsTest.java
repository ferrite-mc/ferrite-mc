package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

/** Player actions that cross the host bridge: chat commands, breakBlock and the broadcast helpers. */
class PlayerActionsTest {
    private static final String WORLD_ID = "00000000-0000-0000-0000-0000000000e2";
    private static org.bukkit.entity.Player player;
    private final List<String> calls = new ArrayList<>();

    @BeforeAll static void start() throws Exception {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerWorld(WORLD_ID, "actions_world", 0, 0, 64, 0);
        Bootstrap.registerPlayer("00000000-0000-0000-0000-0000000000ab", "Actor", false);
        var ensure = Bootstrap.class.getDeclaredMethod("ensurePlayer", String.class, String.class);
        ensure.setAccessible(true);
        player = (org.bukkit.entity.Player) ensure.invoke(null, "00000000-0000-0000-0000-0000000000ab", "Actor");
    }

    @BeforeEach void goHome() {
        player.teleport(new org.bukkit.Location(org.bukkit.Bukkit.getWorld("actions_world"), 0, 65, 0));
    }

    @AfterEach void reset() { HostBridge.transport = null; }

    @AfterAll static void stop() {
        var world = org.bukkit.Bukkit.getWorld("actions_world");
        if (world != null) ((FerriteServer) org.bukkit.Bukkit.getServer()).unregisterWorld(world.getUID());
    }

    private void capture(String reply) {
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(owner + "|" + method + "|" + args); return reply; };
    }

    @Test void chatCommandsGoToTheHostAsThePlayerNotTheConsole() {
        capture("");
        player.chat("/gamemode creative");
        assertEquals(List.of("Player|chat(String)|/gamemode creative"), calls);
    }

    @Test void breakBlockAsksTheHostWithTheBlockCoordinatesAndReturnsItsVerdict() {
        var block = org.bukkit.Bukkit.getWorld("actions_world").getBlockAt(3, 70, -4);
        capture("true");
        assertTrue(player.breakBlock(block));
        capture("false");
        assertFalse(player.breakBlock(block));
        assertEquals("Player|breakBlock(int,int,int)|3" + HostBridge.ARG + "70" + HostBridge.ARG + "-4", calls.get(1).replace("|true", ""));
        assertThrows(NullPointerException.class, () -> player.breakBlock(null));
    }

    @Test void breakBlockRejectsABlockInAnotherWorld() {
        Bootstrap.registerWorld("00000000-0000-0000-0000-0000000000e3", "actions_other", 0, 0, 64, 0);
        try {
            var other = org.bukkit.Bukkit.getWorld("actions_other");
            capture("true");
            assertThrows(IllegalArgumentException.class, () -> player.breakBlock(other.getBlockAt(0, 64, 0)));
            assertTrue(calls.isEmpty(), "nothing reaches the host");
        } finally {
            ((FerriteServer) org.bukkit.Bukkit.getServer()).unregisterWorld(java.util.UUID.fromString("00000000-0000-0000-0000-0000000000e3"));
        }
    }

    @Test void oneArgumentSlotBreakIncludesThePlayerThemselves() {
        // Other test classes leave stub players online that cannot be compared; set them aside.
        var server = (FerriteServer) org.bukkit.Bukkit.getServer();
        var others = new ArrayList<org.bukkit.entity.Player>();
        for (var online : new ArrayList<>(server.getOnlinePlayers())) if (online != player) others.add(online);
        for (var other : others) server.unregisterPlayer(other.getUniqueId());
        try {
            capture("");
            player.broadcastSlotBreak(org.bukkit.inventory.EquipmentSlot.HEAD);
            assertEquals(1, calls.size());
            assertTrue(calls.get(0).startsWith("Player|broadcastSlotBreak(EquipmentSlot,Collection)|HEAD" + HostBridge.ARG), calls.get(0));
            assertTrue(calls.get(0).contains(player.getUniqueId().toString()), calls.get(0));
        } finally {
            for (var other : others) server.registerPlayer(other);
        }
    }

    @Test void hurtAnimationAndPickupAnimationEncodeViewersAndTheItemForTheHost() {
        capture("");
        player.broadcastHurtAnimation(List.of(player));
        assertEquals(List.of("Player|broadcastHurtAnimation(Collection)|" + player.getUniqueId()), calls);
        calls.clear();
        var itemId = java.util.UUID.fromString("00000000-0000-0000-0000-0000000000f1");
        var item = (org.bukkit.entity.Item) java.lang.reflect.Proxy.newProxyInstance(
            getClass().getClassLoader(), new Class<?>[] {org.bukkit.entity.Item.class},
            (proxy, method, args) -> method.getName().equals("getUniqueId") ? itemId : null);
        player.playPickupItemAnimation(item, 3);
        assertEquals(List.of("Player|playPickupItemAnimation(Item,int)|" + itemId + HostBridge.ARG + "3"), calls);
    }
}
