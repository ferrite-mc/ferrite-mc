package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.entity.FerriteEntitySnapshot;
import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.test.TestServer;
import dev.ferrite.api.world.FerriteWorld;
import dev.ferrite.api.world.WorldSpec;
import java.util.Collection;
import java.util.List;
import java.util.UUID;
import org.bukkit.Chunk;
import org.bukkit.GameMode;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.World;
import org.bukkit.block.Block;
import org.bukkit.entity.Arrow;
import org.bukkit.entity.Cow;
import org.bukkit.entity.Entity;
import org.bukkit.entity.EntityType;
import org.bukkit.entity.LivingEntity;
import org.bukkit.entity.Pig;
import org.bukkit.entity.Player;
import org.bukkit.util.BoundingBox;
import org.junit.jupiter.api.Test;

class WorldReadsTest {

    @Test
    void chunkReadsLoadingGenerationAndSeeingPlayersWork() throws Exception {
        try (TestServer testServer = new TestServer(b -> b
                .world(WorldSpec.of("world", World.Environment.NORMAL))
                .world(WorldSpec.of("world_nether", World.Environment.NETHER)))) {
            FerriteRuntime runtime = testServer.runtime();
            testServer.onMain(() -> {
                FerriteWorld overworld = runtime.world("world");
                FerriteWorld nether = runtime.world("world_nether");

                assertFalse(overworld.isChunkLoaded(2, -3));
                assertFalse(overworld.isChunkGenerated(2, -3));
                assertFalse(overworld.isChunkInUse(2, -3));
                assertEquals(0, overworld.getLoadedChunks().length);
                assertFalse(overworld.loadChunk(2, -3, false));

                Chunk chunk = overworld.getChunkAt(2, -3);
                assertTrue(overworld.isChunkLoaded(2, -3));
                assertTrue(overworld.isChunkLoaded(chunk));
                assertTrue(overworld.isChunkGenerated(2, -3));
                assertEquals(Chunk.LoadLevel.TICKING, chunk.getLoadLevel());
                assertArrayEquals(new Chunk[] {chunk}, overworld.getLoadedChunks());

                // Unloaded chunk handle with generate=false does not load it.
                Chunk unloaded = overworld.getChunkAt(10, 10, false);
                assertFalse(overworld.isChunkLoaded(unloaded));
                assertEquals(Chunk.LoadLevel.UNLOADED, unloaded.getLoadLevel());
                assertTrue(overworld.getPlayersSeeingChunk(unloaded).isEmpty());

                // Block lookup delegates to world and validates chunk-local bounds.
                overworld.getBlockAt(35, 64, -40).setType(Material.DIAMOND_BLOCK);
                assertEquals(Material.DIAMOND_BLOCK, chunk.getBlock(3, 64, 8).getType());
                assertThrows(IllegalArgumentException.class, () -> chunk.getBlock(-1, 64, 0));
                assertThrows(IllegalArgumentException.class, () -> chunk.getBlock(0, 64, 16));

                Block blockInChunk = overworld.getBlockAt(35, 64, -40);
                assertEquals(chunk, overworld.getChunkAt(blockInChunk));

                // Wrong-world guards.
                assertFalse(nether.isChunkLoaded(chunk));
                assertTrue(nether.getPlayersSeeingChunk(chunk).isEmpty());
                assertThrows(IllegalArgumentException.class, () -> nether.loadChunk(chunk));
                assertThrows(IllegalArgumentException.class, () -> nether.getChunkAt(blockInChunk));

                // Players seeing chunk & in-use checks.
                FerritePlayer nearbyPlayer = runtime.playerJoined(runtime.createPlayer(
                        "ChunkViewer", UUID.randomUUID(), false, GameMode.SURVIVAL, overworld, 34.0, 64.0, -45.0));
                FerritePlayer farPlayer = runtime.playerJoined(runtime.createPlayer(
                        "FarViewer", UUID.randomUUID(), false, GameMode.SURVIVAL, overworld, 1000.0, 64.0, 1000.0));
                FerritePlayer netherPlayer = runtime.playerJoined(runtime.createPlayer(
                        "NetherViewer", UUID.randomUUID(), false, GameMode.SURVIVAL, nether, 34.0, 64.0, -45.0));

                Collection<Player> seeing = overworld.getPlayersSeeingChunk(chunk);
                assertEquals(List.of(nearbyPlayer), List.copyOf(seeing));
                assertTrue(overworld.isChunkInUse(2, -3));
                assertFalse(seeing.contains(farPlayer));
                assertFalse(seeing.contains(netherPlayer));
                assertThrows(UnsupportedOperationException.class, () -> seeing.add(farPlayer));

                // Explicit refusals for unmodelled chunk mutations and setBiome.
                assertThrows(UnsupportedOperationException.class, () -> chunk.unload(false));
                assertThrows(
                        UnsupportedOperationException.class,
                        () -> overworld.setBiome(35, 64, -40, org.bukkit.block.Biome.DESERT));

                // Biome reads work on loaded chunks, fail explicitly on unloaded chunks without
                // loading them, and resolve 26.3 registry ids cleanly.
                assertEquals(org.bukkit.block.Biome.PLAINS, overworld.getBiome(35, -40));
                assertEquals(org.bukkit.block.Biome.PLAINS, overworld.getBiome(35, 64, -40));
                assertEquals(org.bukkit.block.Biome.PLAINS, overworld.getComputedBiome(35, 64, -40));
                assertEquals(
                        org.bukkit.block.Biome.PLAINS,
                        ((dev.ferrite.api.world.FerriteChunk) chunk).getBiome(3, 64, 8));

                assertThrows(IllegalStateException.class, () -> overworld.getBiome(160, 64, 160));
                assertFalse(overworld.isChunkLoaded(10, 10));

                nether.loadChunk(0, 0, true);
                assertEquals(org.bukkit.block.Biome.NETHER_WASTES, nether.getBiome(0, 64, 0));
                assertEquals(
                        org.bukkit.block.Biome.DEEP_DARK,
                        FerriteWorld.resolveBiome("minecraft:deep_dark"));
                assertThrows(
                        IllegalStateException.class,
                        () -> FerriteWorld.resolveBiome("minecraft:not_a_real_biome"));
            });
        }
    }

    @Test
    void entityQueriesFilterByWorldClassLivingAndBoundingBoxWithDefensiveCopies() throws Exception {
        try (TestServer testServer = new TestServer(b -> b
                .world(WorldSpec.of("world", World.Environment.NORMAL))
                .world(WorldSpec.of("world_nether", World.Environment.NETHER)))) {
            FerriteRuntime runtime = testServer.runtime();
            testServer.onMain(() -> {
                FerriteWorld overworld = runtime.world("world");
                FerriteWorld nether = runtime.world("world_nether");

                FerritePlayer player = runtime.playerJoined(runtime.createPlayer(
                        "Explorer", UUID.randomUUID(), false, GameMode.SURVIVAL, overworld, 5.0, 64.0, 5.0));
                UUID pigId = UUID.randomUUID();
                Entity pig = FerriteEntitySnapshot.create(
                        overworld, pigId, EntityType.PIG, "Pig", "Wilbur", 6.0, 64.0, 5.0, 0.0f, 0.0f, 0.0, 0.0, 0.0, 10.0, 101);
                UUID arrowId = UUID.randomUUID();
                Entity arrow = FerriteEntitySnapshot.create(
                        overworld, arrowId, EntityType.ARROW, "Arrow", null, 50.0, 70.0, 50.0, 0.0f, 0.0f, 0.0, 0.0, 0.0, null, 102);
                UUID netherCowId = UUID.randomUUID();
                Entity netherCow = FerriteEntitySnapshot.create(
                        nether, netherCowId, EntityType.COW, "Cow", null, 5.0, 64.0, 5.0, 0.0f, 0.0f, 0.0, 0.0, 0.0, 10.0, 103);

                overworld.registerEntity(pig);
                overworld.registerEntity(arrow);
                nether.registerEntity(netherCow);

                assertEquals(3, overworld.getEntities().size());
                assertEquals(pig, overworld.getEntity(pigId));
                assertEquals(player, overworld.getEntity(player.getUniqueId()));
                assertNull(overworld.getEntity(netherCowId));
                assertNull(overworld.getEntity(UUID.randomUUID()));
                assertThrows(IllegalArgumentException.class, () -> nether.registerEntity(pig));

                // Living vs non-living filtering.
                List<LivingEntity> living = overworld.getLivingEntities();
                assertEquals(2, living.size());
                assertTrue(living.contains(player));
                assertTrue(living.contains((LivingEntity) pig));
                assertFalse(living.contains(arrow));

                // Class filtering.
                assertEquals(List.of((Pig) pig), List.copyOf(overworld.getEntitiesByClass(Pig.class)));
                assertTrue(overworld.getEntitiesByClass(Cow.class).isEmpty());
                Collection<Entity> multi = overworld.getEntitiesByClasses(Pig.class, Arrow.class);
                assertEquals(2, multi.size());
                assertTrue(multi.contains(pig));
                assertTrue(multi.contains(arrow));

                // Chunk entity listing.
                Chunk originChunk = overworld.getChunkAt(0, 0);
                List<Entity> chunkEntities = List.of(originChunk.getEntities());
                assertEquals(2, chunkEntities.size());
                assertTrue(chunkEntities.contains(player));
                assertTrue(chunkEntities.contains(pig));

                // Nearby entity queries (Location & BoundingBox) with predicate and wrong-world check.
                Collection<Entity> nearby = overworld.getNearbyEntities(
                        new Location(overworld, 5.5, 64.0, 5.0), 2.0, 2.0, 2.0, e -> e instanceof Pig);
                assertEquals(List.of(pig), List.copyOf(nearby));

                BoundingBox box = new BoundingBox(0.0, 60.0, 0.0, 10.0, 70.0, 10.0);
                Collection<Entity> inBox = overworld.getNearbyEntities(box, null);
                assertEquals(2, inBox.size());
                assertThrows(
                        IllegalArgumentException.class,
                        () -> overworld.getNearbyEntities(new Location(nether, 5.0, 64.0, 5.0), 2.0, 2.0, 2.0));

                // Defensive copies: mutating returned Location or attempting to mutate returned lists.
                Location pigLoc = pig.getLocation();
                pigLoc.setX(999.0);
                assertEquals(6.0, pig.getLocation().getX(), 1e-6);
                assertThrows(UnsupportedOperationException.class, () -> living.add(player));
                assertNotNull(pig.toString());
            });
        }
    }
}
