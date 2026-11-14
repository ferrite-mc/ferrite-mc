package dev.ferrite.server;

import java.util.ArrayList;
import java.util.List;
import org.bukkit.Bukkit;
import org.bukkit.Material;
import org.bukkit.entity.Firework;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

/** The handle `fireworkBoost` and `World#spawnEntity(FIREWORK_ROCKET)` return reaches the live rocket. */
class FireworkHandleTest {
    private static final java.util.UUID WORLD_ID = java.util.UUID.fromString("00000000-0000-0000-0000-0000000000c1");
    private static final java.util.UUID PLAYER_ID = java.util.UUID.fromString("00000000-0000-0000-0000-0000000000c2");
    private static FerriteServer server;
    private static org.bukkit.World world;
    private final List<String> sent = new ArrayList<>();

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        server = (FerriteServer) Bukkit.getServer();
        world = FerriteWorld.create(server, WORLD_ID, "firework_world", 0L, 0, 64, 0);
        server.registerWorld(world);
    }

    @AfterAll static void stop() { server.unregisterWorld(WORLD_ID); }

    @AfterEach void reset() { HostBridge.transport = null; }

    private Firework rocket(java.util.UUID attachedTo) {
        var meta = (org.bukkit.inventory.meta.FireworkMeta) new org.bukkit.inventory.ItemStack(Material.FIREWORK_ROCKET).getItemMeta();
        meta.setPower(2);
        return SpawnedEntity.firework(new org.bukkit.Location(world, 0, 70, 0), world, java.util.UUID.randomUUID(), attachedTo, meta);
    }

    @Test void lifeAndLifetimeAreReadFromAndWrittenToTheHostRocket() {
        HostBridge.transport = (owner, subject, method, args) -> {
            sent.add(method + "|" + args);
            return switch (method) { case "getFireworkLife()" -> "7"; case "getFireworkMaxLife()" -> "31"; default -> ""; };
        };
        var firework = rocket(null);
        assertEquals(7, firework.getLife());
        assertEquals(31, firework.getMaxLife());
        assertTrue(firework.setLife(3));
        assertTrue(firework.setMaxLife(50));
        firework.setTicksFlown(4);
        firework.setTicksToDetonate(60);
        assertEquals(7, firework.getTicksFlown());
        assertEquals(31, firework.getTicksToDetonate());
        firework.detonate();
        assertEquals(List.of("getFireworkLife()|", "getFireworkMaxLife()|", "setFireworkLife(int)|3", "setFireworkMaxLife(int)|50",
            "setFireworkLife(int)|4", "setFireworkMaxLife(int)|60", "getFireworkLife()|", "getFireworkMaxLife()|", "detonate()|"), sent);
    }

    @Test void aFireworkRocketStackHasFireworkMetaWithPowerAndStars() {
        var stack = new org.bukkit.inventory.ItemStack(Material.FIREWORK_ROCKET);
        var meta = assertInstanceOf(org.bukkit.inventory.meta.FireworkMeta.class, stack.getItemMeta());
        assertEquals(0, meta.getPower());
        assertFalse(meta.hasEffects());
        meta.setPower(3);
        meta.addEffect(org.bukkit.FireworkEffect.builder().withColor(org.bukkit.Color.RED).with(org.bukkit.FireworkEffect.Type.BALL).build());
        meta.addEffects(org.bukkit.FireworkEffect.builder().withColor(org.bukkit.Color.BLUE).build());
        assertEquals(2, meta.getEffectsSize());
        assertThrows(IllegalArgumentException.class, () -> meta.setPower(128));
        assertThrows(IndexOutOfBoundsException.class, () -> meta.removeEffect(2));
        meta.removeEffect(0);
        assertEquals(1, meta.getEffects().size());
        stack.setItemMeta(meta);
        var back = (org.bukkit.inventory.meta.FireworkMeta) stack.getItemMeta();
        assertEquals(3, back.getPower());
        assertEquals(meta, back, "an equal copy");
        back.clearEffects();
        assertNotEquals(meta, back);
    }

    @Test void aRocketKnowsItsMetaAndWhoItIsAttachedTo() {
        var firework = rocket(PLAYER_ID);
        assertEquals(2, firework.getFireworkMeta().getPower());
        assertEquals(PLAYER_ID, firework.getSpawningEntity());
        assertNull(firework.getAttachedTo(), "the player is not online here");
        assertFalse(firework.isShotAtAngle());
        assertEquals(2, ((org.bukkit.inventory.meta.FireworkMeta) firework.getItem().getItemMeta()).getPower());
        assertThrows(UnsupportedOperationException.class, () -> firework.setItem(firework.getItem()));
        assertThrows(UnsupportedOperationException.class, () -> firework.setFireworkMeta(firework.getFireworkMeta()));
        assertThrows(UnsupportedOperationException.class, () -> firework.setShotAtAngle(true));
        assertNull(rocket(null).getSpawningEntity());
    }

    @Test void aWorldSpawnedRocketIsTheSameKindOfHandle() {
        HostBridge.transport = (owner, subject, method, args) -> method.equals("getFireworkMaxLife()") ? "20" : "";
        var firework = world.spawnEntity(new org.bukkit.Location(world, 0, 70, 0), org.bukkit.entity.EntityType.FIREWORK_ROCKET);
        assertInstanceOf(Firework.class, firework);
        assertEquals(20, ((Firework) firework).getMaxLife());
    }
}
