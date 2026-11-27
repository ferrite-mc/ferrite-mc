package dev.ferrite.api;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertSame;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.api.block.FerriteBlock;
import dev.ferrite.api.block.FerriteBlockState;
import dev.ferrite.api.entity.FerritePlayer;
import dev.ferrite.api.item.FerriteItemNbt;
import dev.ferrite.api.registry.VanillaData;
import dev.ferrite.api.scoreboard.FerriteOfflinePlayer;
import dev.ferrite.api.test.TestServer;
import dev.ferrite.api.world.FerriteWorld;
import java.util.ListIterator;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.atomic.AtomicReference;
import net.kyori.adventure.text.Component;
import org.bukkit.Chunk;
import org.bukkit.ChunkSnapshot;
import org.bukkit.GameMode;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.block.Biome;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.ItemMeta;
import org.junit.jupiter.api.Test;

/** Coverage for the remaining facade methods backed by existing server state. */
class PaperApiRemainingMethodsTest {

    @Test
    void gameRuleStringWritesParseVanillaValueTypes() throws Exception {
        try (TestServer server = new TestServer()) {
            server.onMain(() -> {
                FerriteWorld world = server.runtime().world("world");
                assertEquals("true", world.getGameRuleValue("doDaylightCycle"));

                assertTrue(world.setGameRuleValue("doDaylightCycle", "false"));
                assertEquals("false", world.getGameRuleValue("doDaylightCycle"));
                assertTrue(world.setGameRuleValue("doDaylightCycle", "TRUE"));
                assertEquals("true", world.getGameRuleValue("doDaylightCycle"));

                assertFalse(world.setGameRuleValue("doDaylightCycle", "sometimes"));
                assertFalse(world.setGameRuleValue("notARule", "true"));
            });
        }
    }

    @Test
    void chunkSnapshotsCaptureBlocksBiomesTemperaturesAndAsyncLoads() throws Exception {
        try (TestServer server = new TestServer()) {
            server.onMain(() -> {
                FerriteWorld world = server.runtime().world("world");
                world.getBlockAt(1, 64, 2).setType(Material.DIAMOND_BLOCK);
                Chunk chunk = world.getChunkAt(0, 0);

                ChunkSnapshot snapshot = chunk.getChunkSnapshot(true, true, true, false);
                assertEquals(0, snapshot.getX());
                assertEquals(0, snapshot.getZ());
                assertEquals("world", snapshot.getWorldName());
                assertEquals("minecraft:world", snapshot.getWorldKey().asString());
                assertEquals(Material.DIAMOND_BLOCK, snapshot.getBlockType(1, 64, 2));
                assertEquals(Material.AIR, snapshot.getBlockType(1, 65, 2));
                assertEquals(64, snapshot.getHighestBlockYAt(1, 2));
                assertEquals(Biome.PLAINS, snapshot.getBiome(1, 64, 2));
                assertEquals(0.8, snapshot.getRawBiomeTemperature(1, 64, 2), 1e-9);
                assertEquals(world.getFullTime(), snapshot.getCaptureFullTime());
                assertTrue(snapshot.isSectionEmpty(5));
                assertFalse(snapshot.isSectionEmpty(4));
                assertTrue(snapshot.contains(org.bukkit.Bukkit.createBlockData(Material.DIAMOND_BLOCK)));
                assertFalse(snapshot.contains(org.bukkit.Bukkit.createBlockData(Material.GOLD_BLOCK)));
                assertTrue(snapshot.contains(Biome.PLAINS));
                assertFalse(snapshot.contains(Biome.DESERT));
                assertEquals(0, snapshot.getBlockSkyLight(0, 64, 0));
                assertEquals(0, snapshot.getBlockEmittedLight(0, 64, 0));
                assertThrows(IndexOutOfBoundsException.class, () -> snapshot.getBlockType(16, 64, 0));

                ChunkSnapshot empty = world.getEmptyChunkSnapshot(3, 4, true, true);
                assertEquals(3, empty.getX());
                assertEquals(Material.AIR, empty.getBlockType(0, 64, 0));
                assertEquals(Biome.PLAINS, empty.getBiome(0, 64, 0));
                assertEquals(0.8, empty.getRawBiomeTemperature(0, 64, 0), 1e-9);

                AtomicReference<Chunk> callbackChunk = new AtomicReference<>();
                world.getChunkAtAsync(8, 8, true, false, callbackChunk::set);
                assertNotNull(callbackChunk.get());
                assertEquals(8, callbackChunk.get().getX());
                assertTrue(world.isChunkLoaded(8, 8));

                CompletableFuture<Chunk> future = world.getChunkAtAsync(9, 9);
                assertEquals(9, future.join().getZ());

                assertTrue(world.unloadChunk(8, 8, true));
                assertFalse(world.isChunkLoaded(8, 8));
                assertFalse(world.unloadChunk(8, 8, true));
            });
        }
    }

    @Test
    void blockAndBlockStateExposeDataBackedProperties() throws Exception {
        try (TestServer server = new TestServer()) {
            server.onMain(() -> {
                FerriteWorld world = server.runtime().world("world");
                world.getChunkAt(0, 0);
                world.getBlockAt(1, 64, 2).setType(Material.STONE);
                FerriteBlock stone = (FerriteBlock) world.getBlockAt(1, 64, 2);
                assertEquals("block.minecraft.stone", stone.getTranslationKey());
                assertEquals("block.minecraft.stone", stone.translationKey());
                assertTrue(stone.isSuffocating());
                assertEquals(0.8, stone.getTemperature(), 1e-9);
                assertEquals(0.4, stone.getHumidity(), 1e-9);

                FerriteBlock air = (FerriteBlock) world.getBlockAt(1, 65, 2);
                assertFalse(air.isSuffocating());
                assertFalse(air.isSolid());

                FerriteBlockState state = (FerriteBlockState) stone.getState();
                assertTrue(state.isPlaced());
                assertTrue(state.isCollidable());
                assertTrue(state.isSuffocating());
                assertEquals(1, state.getDrops(null, null).size());
                assertEquals(Material.STONE, state.getDrops(null, null).iterator().next().getType());

                world.getBlockAt(3, 64, 3).setType(Material.TORCH);
                FerriteBlock torch = (FerriteBlock) world.getBlockAt(3, 64, 3);
                assertEquals(15, torch.getLightLevel());
                FerriteBlockState torchState = (FerriteBlockState) torch.getState();
                assertEquals(15, torchState.getLightLevel());
                assertEquals(Material.TORCH, torchState.getDrops(null, null).iterator().next().getType());
            });
        }
    }

    @Test
    void itemStacksExposeTranslationEffectiveNameAndBinarySerialization() {
        ItemStack diamond = new ItemStack(Material.DIAMOND);
        assertEquals("item.minecraft.diamond", diamond.translationKey());
        assertEquals("item.minecraft.diamond", diamond.getTranslationKey());
        assertEquals(Component.translatable("item.minecraft.diamond"), diamond.effectiveName());

        ItemStack stone = new ItemStack(Material.STONE);
        assertEquals("block.minecraft.stone", stone.translationKey());

        ItemStack named = new ItemStack(Material.DIAMOND_SWORD);
        ItemMeta meta = named.getItemMeta();
        assertNotNull(meta);
        meta.setDisplayName("Excalibur");
        named.setItemMeta(meta);
        assertEquals(Component.text("Excalibur"), named.effectiveName());

        byte[] bytes = named.serializeAsBytes();
        assertTrue(bytes.length > 0);
        ItemStack decoded = FerriteItemNbt.decodeItemBytes(bytes, Material.AIR, 1);
        assertEquals(Material.DIAMOND_SWORD, decoded.getType());
        assertEquals(1, decoded.getAmount());
        assertEquals("Excalibur", decoded.getItemMeta().getDisplayName());
        assertEquals(0, new ItemStack(Material.AIR).serializeAsBytes().length);

        // Facade stacks are already server-side, so conversion is a no-op.
        assertSame(stone, stone.ensureServerConversions());
    }

    @Test
    void materialRegistryReflectsRealBlockMembershipAndTranslationKeys() {
        assertTrue(VanillaData.get().available());
        assertTrue(VanillaData.get().isBlock("minecraft:stone"));
        assertFalse(VanillaData.get().isBlock("minecraft:diamond"));

        assertEquals("block.minecraft.stone", Material.STONE.translationKey());
        assertTrue(Material.STONE.isBlock());
        assertFalse(Material.COAL.isBlock());
        assertEquals("item.minecraft.diamond", Material.DIAMOND.translationKey());
        org.bukkit.block.BlockType stoneType = org.bukkit.Registry.BLOCK.get(
                org.bukkit.NamespacedKey.minecraft("stone"));
        assertNotNull(stoneType);
        assertEquals("block.minecraft.stone", stoneType.getTranslationKey());
        assertNull(org.bukkit.Registry.BLOCK.get(org.bukkit.NamespacedKey.minecraft("coal")));
    }

    @Test
    void playerSessionRespawnAndWhitelistShareStateWithOfflineHandles() throws Exception {
        try (TestServer server = new TestServer()) {
            server.onMain(() -> {
                FerriteWorld world = server.runtime().world("world");
                UUID id = UUID.randomUUID();
                FerritePlayer player = server.runtime().createPlayer(
                        "SessionHero", id, false, GameMode.SURVIVAL, world, 0.5, 65.0, 0.5);
                server.runtime().playerJoined(player);

                assertTrue(player.getFirstPlayed() > 0);
                assertTrue(player.getLastLogin() > 0);
                assertTrue(player.getLastSeen() >= player.getLastLogin());

                Location respawn = new Location(world, 12.5, 70.0, -4.5);
                player.setRespawnLocation(respawn);
                assertEquals(respawn, player.getBedLocation());
                assertEquals(respawn, player.getPotentialRespawnLocation());

                player.setRespawnLocation(null);
                assertNull(player.getPotentialRespawnLocation());

                player.setWhitelisted(true);
                assertTrue(player.isWhitelisted());
                FerriteOfflinePlayer offline = new FerriteOfflinePlayer(id, player.getName());
                assertTrue(offline.isWhitelisted());
                assertEquals(player.getFirstPlayed(), offline.getFirstPlayed());
                assertEquals(player.getLastPlayed(), offline.getLastPlayed());
                assertEquals(player.getLastLogin(), offline.getLastLogin());
                player.setWhitelisted(false);
                assertFalse(offline.isWhitelisted());

                server.runtime().playerQuit(id);
                assertFalse(player.isOnline());
                assertEquals(0L, offline.getFirstPlayed());
                assertTrue(player.getLastSeen() >= player.getLastLogin());
            });
        }
    }

    @Test
    void paperInternalBridgeExposesItemCodecProfilesCombatEntriesAndSkinParts() {
        dev.ferrite.api.server.FerriteInternalApiBridge bridge =
                new dev.ferrite.api.server.FerriteInternalApiBridge();

        ItemStack named = new ItemStack(Material.DIAMOND_SWORD);
        ItemMeta meta = named.getItemMeta();
        meta.setDisplayName("BridgeBlade");
        named.setItemMeta(meta);
        ItemStack decoded = bridge.deserializeItem(named.serializeAsBytes());
        assertEquals(Material.DIAMOND_SWORD, decoded.getType());
        assertEquals("BridgeBlade", decoded.getItemMeta().getDisplayName());
        assertTrue(bridge.deserializeItem(new byte[0]).isEmpty());

        io.papermc.paper.persistence.PersistentDataKey<String> dataKey = bridge.createPersistentDataKey(
                net.kyori.adventure.key.Key.key("ferrite", "probe"),
                org.bukkit.persistence.PersistentDataType.STRING);
        assertEquals(org.bukkit.NamespacedKey.fromString("ferrite:probe"), dataKey.key());
        assertEquals(org.bukkit.persistence.PersistentDataType.STRING, dataKey.dataType());

        assertEquals(
                io.papermc.paper.entity.poi.PoiType.Occupancy.IS_OCCUPIED,
                bridge.createOccupancy("is_occupied"));
        assertThrows(IllegalArgumentException.class, () -> bridge.createOccupancy("sometimes"));

        org.bukkit.damage.DamageSource source = bridge.createDamageSourceBuilder(
                        org.bukkit.damage.DamageType.ARROW)
                .build();
        io.papermc.paper.world.damagesource.CombatEntry entry = bridge.createCombatEntry(
                source, 4.5f, io.papermc.paper.world.damagesource.FallLocationType.GENERIC, 2.0f);
        assertEquals(source, entry.getDamageSource());
        assertEquals(4.5f, entry.getDamage(), 1e-6f);
        assertEquals(io.papermc.paper.world.damagesource.FallLocationType.GENERIC,
                entry.getFallLocationType());
        assertEquals(2.0f, entry.getFallDistance(), 1e-6f);

        com.destroystokyo.paper.SkinParts.Mutable parts = bridge.allSkinParts();
        assertEquals(0x7F, parts.getRaw());
        parts.setCapeEnabled(false);
        assertFalse(parts.hasCapeEnabled());
        assertEquals(0x7E, parts.getRaw());
        assertEquals(0x7E, parts.mutableCopy().getRaw());
    }

    @Test
    void inventoryIteratorSupportsSetAndRemoveButNotAdd() throws Exception {
        try (TestServer server = new TestServer()) {
            server.onMain(() -> {
                Inventory inventory = server.runtime().server().createInventory(null, 9);
                inventory.setItem(2, new ItemStack(Material.APPLE, 3));
                ListIterator<ItemStack> iterator = inventory.iterator(2);
                ItemStack first = iterator.next();
                assertEquals(Material.APPLE, first.getType());
                assertEquals(3, first.getAmount());

                iterator.set(new ItemStack(Material.BREAD, 1));
                assertEquals(Material.BREAD, inventory.getItem(2).getType());
                iterator.remove();
                assertNull(inventory.getItem(2));
                assertThrows(IllegalStateException.class, iterator::remove);
                assertThrows(IllegalStateException.class, () -> iterator.set(new ItemStack(Material.AIR)));
                assertThrows(
                        UnsupportedOperationException.class,
                        () -> iterator.add(new ItemStack(Material.APPLE)));
                assertThrows(IndexOutOfBoundsException.class, () -> inventory.iterator(10));
            });
        }
    }
}
