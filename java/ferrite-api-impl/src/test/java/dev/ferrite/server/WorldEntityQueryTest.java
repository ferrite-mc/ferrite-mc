package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.UUID;
import org.bukkit.Bukkit;
import org.bukkit.Location;
import org.bukkit.World;
import org.bukkit.entity.Cow;
import org.bukkit.entity.Entity;
import org.bukkit.entity.EntityType;
import org.bukkit.entity.Player;
import org.bukkit.entity.Zombie;
import org.bukkit.util.BoundingBox;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class WorldEntityQueryTest {
    private static final UUID WORLD_ID = UUID.fromString("00000000-0000-0000-0000-0000000000e4");
    private static final UUID OTHER_WORLD_ID = UUID.fromString("00000000-0000-0000-0000-0000000000e5");
    private static final UUID PLAYER_ID = UUID.fromString("00000000-0000-0000-0000-0000000000e6");
    private static final UUID COW_ID = UUID.fromString("00000000-0000-0000-0000-0000000000e7");
    private static final UUID ZOMBIE_ID = UUID.fromString("00000000-0000-0000-0000-0000000000e8");
    private static World world;
    private static World other;
    private static Player player;

    private final Map<UUID, String> kinds = new HashMap<>();
    private final Map<UUID, Location> places = new HashMap<>();
    private final Set<UUID> present = new HashSet<>();
    private boolean duplicate;

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerWorld(WORLD_ID.toString(), "entity_query", 0, 0, 64, 0);
        Bootstrap.registerWorld(OTHER_WORLD_ID.toString(), "entity_query_other", 0, 0, 64, 0);
        Bootstrap.registerPlayer(PLAYER_ID.toString(), "EntityQuery", false, 20.0, 20, 0, WORLD_ID.toString(), 0, 64, 0, 0, 0);
        world = Bukkit.getWorld(WORLD_ID);
        other = Bukkit.getWorld(OTHER_WORLD_ID);
        player = Bukkit.getPlayer(PLAYER_ID);
    }

    @BeforeEach void hostEntities() {
        kinds.put(COW_ID, "minecraft:cow");
        places.put(COW_ID, new Location(world, 5, 64, 0));
        kinds.put(ZOMBIE_ID, "minecraft:zombie");
        places.put(ZOMBIE_ID, new Location(world, 7, 64, 0));
        present.add(COW_ID);
        present.add(ZOMBIE_ID);
        HostBridge.transport = (owner, subject, method, args) -> {
            if (owner.equals("World") && method.equals("getEntities()")) {
                if (!subject.equals(WORLD_ID.toString())) return "";
                var reply = new StringBuilder();
                for (var id : present) {
                    append(reply, id);
                    if (duplicate && id.equals(COW_ID)) append(reply, id);
                }
                return reply.toString();
            }
            if (owner.equals("Entity")) {
                UUID id = UUID.fromString(subject);
                if (method.equals("isValid()")) return Boolean.toString(present.contains(id));
                if (method.equals("getEntityPosition()")) {
                    var at = places.get(id);
                    return at.getX() + String.valueOf(HostBridge.ITEM) + at.getY() + HostBridge.ITEM
                            + at.getZ() + HostBridge.ITEM + at.getYaw() + HostBridge.ITEM + at.getPitch();
                }
            }
            return null;
        };
    }

    @AfterEach void clearHost() {
        HostBridge.transport = null;
    }

    @AfterAll static void stop() {
        var server = (FerriteServer) Bukkit.getServer();
        for (var id : new UUID[]{WORLD_ID, OTHER_WORLD_ID}) {
            var registered = Bukkit.getWorld(id);
            if (registered != null) server.unregisterWorld(id);
        }
        server.unregisterPlayer(PLAYER_ID);
    }

    @Test void returnsPlayersAndStableNonPlayerWrappers() {
        var first = world.getEntities();
        assertTrue(first.size() >= 3);
        assertSame(player, find(first, PLAYER_ID));
        var cow = find(first, COW_ID);
        assertNotNull(cow);
        assertInstanceOf(Cow.class, cow);
        assertEquals(EntityType.COW, cow.getType());
        assertEquals(5.0, cow.getLocation().getX());
        assertInstanceOf(Zombie.class, find(first, ZOMBIE_ID));

        var second = world.getEntities();
        assertSame(cow, find(second, COW_ID));
        assertSame(player, find(second, PLAYER_ID));
        assertSame(cow, world.getEntity(COW_ID));
        assertTrue(world.getEntitiesByClass(Cow.class).contains(cow));
        assertTrue(world.getLivingEntities().size() >= 3);
        assertTrue(world.getEntityCount() >= 3);
    }

    @Test void nearbyHonoursBoundsFiltersAndEdges() {
        var locationQuery = world.getNearbyEntities(new Location(world, 0, 64, 0), 5, 0, 0, e -> e.getType() == EntityType.COW);
        assertEquals(1, locationQuery.size());
        assertEquals(COW_ID, locationQuery.iterator().next().getUniqueId());

        assertFalse(new BoundingBox(0, 63, -1, 5, 65, 1).contains(5, 64, 0));
        var edge = world.getNearbyEntities(new BoundingBox(0, 63, -1, 5.001, 65, 1), e -> e.getUniqueId().equals(COW_ID));
        assertEquals(1, edge.size());
        var outside = world.getNearbyEntities(new BoundingBox(0, 63, -1, 5, 65, 1), e -> e.getUniqueId().equals(COW_ID));
        assertTrue(outside.isEmpty());
        var zombie = world.getNearbyEntities(new BoundingBox(-10, 63, -10, 10, 65, 10), e -> e.getType() == EntityType.ZOMBIE);
        assertEquals(1, zombie.size());
    }

    @Test void filtersByWorldAndDoesNotDuplicateRepeatedRecords() {
        assertTrue(other.getEntities().isEmpty());
        duplicate = true;
        var entities = world.getEntities();
        assertTrue(entities.size() >= 3);
        assertEquals(1, entities.stream().filter(e -> e.getUniqueId().equals(COW_ID)).count());
    }

    @Test void removedEntitiesLeaveTheQueryAndBecomeInvalid() {
        var cow = find(world.getEntities(), COW_ID);
        present.remove(COW_ID);
        var after = world.getEntities();
        assertNull(find(after, COW_ID));
        assertFalse(cow.isValid());
        assertTrue(cow.isDead());
    }

    @Test void unsupportedKindsFailExplicitly() {
        kinds.put(COW_ID, "minecraft:not_a_real_entity");
        assertThrows(UnsupportedOperationException.class, () -> world.getEntities());
    }

    private void append(StringBuilder reply, UUID id) {
        if (!reply.isEmpty()) reply.append(HostBridge.ITEM);
        var at = places.get(id);
        reply.append(id).append(WorldEntities.FIELD).append(kinds.get(id)).append(WorldEntities.FIELD)
                .append(at.getX()).append(WorldEntities.FIELD).append(at.getY()).append(WorldEntities.FIELD)
                .append(at.getZ()).append(WorldEntities.FIELD).append(at.getYaw()).append(WorldEntities.FIELD)
                .append(at.getPitch());
    }

    private static Entity find(List<Entity> entities, UUID id) {
        for (var entity : entities) if (entity.getUniqueId().equals(id)) return entity;
        return null;
    }
}
