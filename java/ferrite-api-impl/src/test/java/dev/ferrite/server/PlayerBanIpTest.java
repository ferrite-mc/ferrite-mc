package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.ArrayList;
import java.util.List;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** `Player#banIp` goes through the server IP ban list, which the host turns into `/ban-ip`. */
class PlayerBanIpTest {
    private static org.bukkit.entity.Player player;
    private final List<String> calls = new ArrayList<>();

    @BeforeAll static void start() throws Exception {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerWorld("00000000-0000-0000-0000-0000000000e2", "banip_world", 0, 0, 64, 0);
        Bootstrap.registerPlayer("00000000-0000-0000-0000-0000000000ab", "Banned", false);
        var ensure = Bootstrap.class.getDeclaredMethod("ensurePlayer", String.class, String.class);
        ensure.setAccessible(true);
        player = (org.bukkit.entity.Player) ensure.invoke(null, "00000000-0000-0000-0000-0000000000ab", "Banned");
    }

    @AfterEach void reset() {
        HostBridge.transport = null;
        ((org.bukkit.BanList<java.net.InetAddress>) org.bukkit.Bukkit.getBanList(org.bukkit.BanList.Type.IP)).getBanEntries()
            .forEach(entry -> ((org.bukkit.ban.IpBanList) org.bukkit.Bukkit.getBanList(org.bukkit.BanList.Type.IP)).pardon(((org.bukkit.BanEntry<java.net.InetAddress>) entry).getBanTarget()));
    }

    private void host() {
        HostBridge.transport = (owner, subject, method, args) -> {
            calls.add(owner + "#" + method);
            return method.equals("getAddressText()") ? "203.0.113.9" : null;
        };
    }

    @Test void banIpBansThePlayersAddressWithReasonSourceAndExpiry() throws Exception {
        host();
        var expires = new java.util.Date(System.currentTimeMillis() + 60_000);
        var entry = player.banIp("spam", expires, "tester", false);
        assertEquals("203.0.113.9", entry.getTarget());
        assertEquals("spam", entry.getReason());
        assertEquals("tester", entry.getSource());
        assertEquals(expires, entry.getExpiration());
        assertTrue(org.bukkit.Bukkit.getBanList(org.bukkit.BanList.Type.IP).isBanned("203.0.113.9"));
        assertFalse(calls.stream().anyMatch(c -> c.contains("kick")), "no kick was requested: " + calls);
    }

    @Test void durationAndInstantOverloadsAgreeWithTheDateOverload() {
        host();
        var byDuration = player.banIp("a", java.time.Duration.ofMinutes(5), "s", false);
        long delta = byDuration.getExpiration().getTime() - System.currentTimeMillis();
        assertTrue(delta > 4 * 60_000 && delta <= 5 * 60_000, "expires in about five minutes: " + delta);
        var instant = java.time.Instant.now().plusSeconds(120);
        var byInstant = player.banIp("b", instant, "s", false);
        assertEquals(instant.toEpochMilli(), byInstant.getExpiration().getTime());
        var permanent = player.banIp("c", (java.util.Date) null, "s", false);
        assertNull(permanent.getExpiration());
    }

    @Test void kickingBansFirstThenDisconnectsThroughTheNormalKickPath() {
        host();
        // With Native off the kick command is accepted without a host, like every other player command here.
        assertDoesNotThrow(() -> player.banIp("bye", (java.util.Date) null, "s", true));
        assertTrue(org.bukkit.Bukkit.getBanList(org.bukkit.BanList.Type.IP).isBanned("203.0.113.9"));
    }
}
