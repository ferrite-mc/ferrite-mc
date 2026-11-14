package dev.ferrite.server;

import java.util.ArrayList;
import java.util.List;
import org.bukkit.Bukkit;
import org.bukkit.Color;
import org.bukkit.Material;
import org.bukkit.Particle;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

/** `Player#spawnParticle` sends the particle's data to the host, validated like Bukkit does. */
class PlayerParticleTest {
    private static final java.util.UUID WORLD_ID = java.util.UUID.fromString("00000000-0000-0000-0000-0000000000b1");
    private static final java.util.UUID PLAYER_ID = java.util.UUID.fromString("00000000-0000-0000-0000-0000000000b2");
    private static FerriteServer server;
    private static org.bukkit.entity.Player player;
    private final List<String[]> sent = new ArrayList<>();

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        server = (FerriteServer) Bukkit.getServer();
        var world = FerriteWorld.create(server, WORLD_ID, "particle_world", 0L, 0, 64, 0);
        server.registerWorld(world);
        player = FerritePlayer.create(server, PLAYER_ID, "Sparkle", false, 20.0, 20, 0, new org.bukkit.Location(world, 0.5, 65.0, 0.5));
        server.registerPlayer(player);
    }

    @AfterAll static void stop() {
        server.unregisterWorld(WORLD_ID);
        server.unregisterPlayer(PLAYER_ID);
    }

    @AfterEach void reset() { HostBridge.transport = null; }

    private String[] capture(Runnable action) {
        sent.clear();
        HostBridge.transport = (owner, subject, method, args) -> { sent.add(new String[]{method, args}); return ""; };
        action.run();
        assertEquals(1, sent.size());
        assertTrue(sent.get(0)[0].startsWith("spawnParticleData("), sent.get(0)[0]);
        return sent.get(0)[1].split(String.valueOf(HostBridge.ARG), -1);
    }

    @Test void everyOverloadReachesTheHostWithTheFullArgumentList() {
        var at = player.getLocation();
        var plain = capture(() -> player.spawnParticle(Particle.FLAME, at, 5));
        assertEquals(14, plain.length);
        assertEquals("5", plain[4]);
        assertEquals("", plain[11], "a particle without data sends an empty spec");
        assertEquals("false", plain[12]);
        assertEquals("DEFAULT", plain[13]);
        var coordinates = capture(() -> player.spawnParticle(Particle.FLAME, 1.0, 2.0, 3.0, 4, 0.1, 0.2, 0.3, 0.4));
        assertEquals("1.0", coordinates[1]);
        assertEquals("4", coordinates[4]);
        assertEquals("0.3", coordinates[7]);
    }

    @Test void dustAndColourDataBecomeSpecs() {
        var dust = capture(() -> player.spawnParticle(Particle.DUST, 0, 64, 0, 1, new Particle.DustOptions(Color.fromRGB(255, 0, 0), 1.5f)));
        assertEquals("dust:16711680:1.5", dust[11]);
        var transition = capture(() -> player.spawnParticle(Particle.DUST_COLOR_TRANSITION, 0, 64, 0, 1, new Particle.DustTransition(Color.RED, Color.BLUE, 2f)));
        assertEquals("transition:" + Color.RED.asRGB() + ":" + Color.BLUE.asRGB() + ":2.0", transition[11]);
        var effect = capture(() -> player.spawnParticle(Particle.ENTITY_EFFECT, 0, 64, 0, 1, Color.fromARGB(128, 10, 20, 30)));
        assertEquals("color:" + Color.fromARGB(128, 10, 20, 30).asARGB(), effect[11]);
        var spell = capture(() -> player.spawnParticle(Particle.EFFECT, 0, 64, 0, 1, new Particle.Spell(Color.WHITE, 0.5f)));
        assertEquals("spell:" + Color.WHITE.asARGB() + ":0.5", spell[11]);
    }

    @Test void blockItemAndScalarDataBecomeSpecs() {
        var block = capture(() -> player.spawnParticle(Particle.BLOCK, 0, 64, 0, 1, Material.STONE.createBlockData()));
        assertEquals("block:" + FerriteBlockStates.stateId(Material.STONE.createBlockData().getAsString()), block[11]);
        var item = capture(() -> player.spawnParticle(Particle.ITEM, 0, 64, 0, 1, new org.bukkit.inventory.ItemStack(Material.DIAMOND, 3)));
        assertEquals("item:minecraft:diamond:3", item[11]);
        var shriek = capture(() -> player.spawnParticle(Particle.SHRIEK, 0, 64, 0, 1, 20));
        assertEquals("int:20", shriek[11]);
        var charge = capture(() -> player.spawnParticle(Particle.SCULK_CHARGE, 0, 64, 0, 1, 0.5f));
        assertEquals("float:0.5", charge[11]);
    }

    @Test void dataThatDoesNotFitTheParticleIsRejectedLikeBukkit() {
        HostBridge.transport = (owner, subject, method, args) -> { fail("nothing may reach the host"); return ""; };
        assertThrows(IllegalArgumentException.class, () -> player.spawnParticle(Particle.DUST, 0, 64, 0, 1));
        assertThrows(IllegalArgumentException.class, () -> player.spawnParticle(Particle.DUST, 0, 64, 0, 1, Material.STONE.createBlockData()));
        assertThrows(IllegalArgumentException.class, () -> player.spawnParticle(Particle.FLAME, 0, 64, 0, 1, Color.RED));
        assertThrows(IllegalArgumentException.class, () -> player.spawnParticle(Particle.ITEM, 0, 64, 0, 1, new org.bukkit.inventory.ItemStack(Material.AIR)));
        assertThrows(NullPointerException.class, () -> player.spawnParticle(null, 0, 64, 0, 1));
    }

    @Test void theVirtualHostIsTheUnresolvedHandshakeAddress() {
        HostBridge.transport = (owner, subject, method, args) -> "play.example.com:25565";
        var host = player.getVirtualHost();
        assertEquals("play.example.com", host.getHostString());
        assertEquals(25565, host.getPort());
        assertTrue(host.isUnresolved());
        HostBridge.transport = (owner, subject, method, args) -> HostBridge.NULL;
        assertNull(player.getVirtualHost());
        assertNull(PlayerMisc.parseVirtualHost("nonsense"));
        assertNull(PlayerMisc.parseVirtualHost("host:99999"));
        assertNull(player.getHAProxyAddress());
    }

    /** Players cannot be cloned or spawned, have no SNBT and no pick item (vanilla `Player`). */
    @Test void playersRefuseCopyAndSpawnAndHaveNoSnbtOrPickItem() {
        HostBridge.transport = (owner, subject, method, args) -> { fail("answered without the host: " + method); return ""; };
        assertThrows(UnsupportedOperationException.class, () -> player.copy());
        assertThrows(UnsupportedOperationException.class, () -> player.copy(player.getLocation()));
        assertThrows(UnsupportedOperationException.class, () -> player.spawnAt(player.getLocation(), org.bukkit.event.entity.CreatureSpawnEvent.SpawnReason.CUSTOM));
        assertThrows(UnsupportedOperationException.class, () -> player.spawnAt(player.getLocation()));
        assertNull(player.getAsString());
        assertNull(player.getPickItemStack());
        assertNull(player.getMemory(org.bukkit.entity.memory.MemoryKey.HOME));
    }
}
