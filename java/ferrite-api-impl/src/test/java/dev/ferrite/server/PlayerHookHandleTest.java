package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class PlayerHookHandleTest {
    private FerriteServer server;
    private UUID id;
    private org.bukkit.entity.Player player;
    private final List<String> calls = new ArrayList<>();

    @BeforeEach void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        server = (FerriteServer) org.bukkit.Bukkit.getServer();
        id = UUID.randomUUID();
        player = FerritePlayer.create(server, id, "Angler", false, 20.0, 20, 0, null);
        server.registerPlayer(player);
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(owner + "|" + method + "|" + args); return ""; };
    }

    @AfterEach void stop() { HostBridge.transport = null; server.unregisterPlayer(id); }

    @Test void aCastBobberIsAHandleWhoseIdTheHostReceives() {
        var hook = player.launchProjectile(org.bukkit.entity.FishHook.class);
        assertNotNull(hook);
        assertEquals(org.bukkit.entity.EntityType.FISHING_BOBBER, hook.getType());
        var cast = calls.stream().filter(c -> c.contains("|castBobber(String)|")).findFirst().orElseThrow();
        assertTrue(cast.endsWith(hook.getUniqueId().toString()), cast);
    }
}
