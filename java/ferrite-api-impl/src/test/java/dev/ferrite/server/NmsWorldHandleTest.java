package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.UUID;
import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** CraftWorld#getHandle is a per-world bridge, not a fresh wrapper that can read another world. */
class NmsWorldHandleTest {
    private static final String A = "00000000-0000-0000-0000-0000000000d1";
    private static final String B = "00000000-0000-0000-0000-0000000000d2";
    private static org.bukkit.World first;
    private static org.bukkit.World second;

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerWorld(A, "handle_world_a", 0, 10, 64, 20);
        Bootstrap.registerWorld(B, "handle_world_b", 1, -5, 70, 8);
        first = org.bukkit.Bukkit.getWorld("handle_world_a");
        second = org.bukkit.Bukkit.getWorld("handle_world_b");
    }

    @AfterAll static void stop() {
        var server = (FerriteServer) org.bukkit.Bukkit.getServer();
        server.unregisterWorld(UUID.fromString(A));
        server.unregisterWorld(UUID.fromString(B));
    }

    @Test void oneHandlePerWorldAndEveryReadStaysInThatWorld() {
        var a = ServerLevel.forWorld(first);
        var b = ServerLevel.forWorld(second);
        assertSame(a, ServerLevel.forWorld(first), "the handle is cached, so identity is stable");
        assertNotSame(a, b);
        assertSame(a, new org.bukkit.craftbukkit.CraftWorld(first).getHandle(), "CraftWorld uses the same bridge");
        assertEquals(first.getUID(), a.getUUID());
        assertEquals("handle_world_a", a.getName());
        assertEquals(10, a.getSharedSpawnPos().getX());
        assertEquals(20, a.getSharedSpawnPos().getZ());
        assertEquals(first.getKey().toString(), a.dimensionKey());

        Bootstrap.setBlockSnapshot(A, 5, 70, 6, "stone");
        Bootstrap.setBlockSnapshot(B, 5, 70, 6, "dirt");
        assertEquals(org.bukkit.Material.STONE, a.getBlockState(new BlockPos(5, 70, 6)).getBukkitMaterial());
        assertEquals(org.bukkit.Material.DIRT, b.getBlockState(new BlockPos(5, 70, 6)).getBukkitMaterial());
        assertEquals(org.bukkit.Material.DIRT, b.getBlockState(5, 70, 6).getBukkitMaterial());

        var chunk = b.getChunk(0, 0);
        assertEquals(0, chunk.getPosX());
        assertEquals(org.bukkit.Material.DIRT, chunk.getBlockState(5, 70, 6).getBukkitMaterial());
        assertSame(b, chunk.getLevel());
        assertThrows(IllegalArgumentException.class, () -> chunk.getBlockState(16, 70, 0));

        var entity = a.getBlockEntity(new BlockPos(5, 70, 6));
        assertEquals(new BlockPos(5, 70, 6), entity.getBlockPos());
        assertEquals(org.bukkit.Material.STONE, entity.getBlockState().getBukkitMaterial());
        assertSame(a, entity.getLevel());

        assertNull(b.getPlayerByUUID(UUID.randomUUID()));
        assertTrue(a.getAllEntities().isEmpty());
        assertNull(a.getEntity(UUID.randomUUID()));
        assertEquals(org.bukkit.Material.AIR, b.getBlockState(new BlockPos(0, 0, 0)).getBukkitMaterial());
    }

    @Test void anEntityHandleReadsAndWritesTheLiveEntity() {
        Bootstrap.registerPlayer("00000000-0000-0000-0000-0000000000d3", "HandleViewer", false);
        var player = org.bukkit.Bukkit.getPlayer("HandleViewer");
        assertNotNull(player);
        player.teleport(new org.bukkit.Location(first, 12.5, 65, -3.5, 90, 10));
        var handle = (net.minecraft.world.entity.LivingEntity) net.minecraft.world.entity.Entity.of(player);
        assertEquals(12.5, handle.getPosition().x, 1e-9);
        assertEquals(-3.5, handle.getPosition().z, 1e-9);
        assertEquals(12, handle.getBlockX());
        assertEquals(-4, handle.getBlockZ());
        assertEquals(90f, handle.getYRot(), 1e-6);
        assertSame(ServerLevel.forWorld(first), handle.level());
        assertFalse(handle.isInvisible());
        handle.setInvisible(true);
        assertTrue(player.isInvisible());

        player.getInventory().setItemInMainHand(new org.bukkit.inventory.ItemStack(org.bukkit.Material.DIAMOND_SWORD));
        var main = handle.getItemBySlot(net.minecraft.world.entity.EquipmentSlot.MAINHAND);
        assertEquals(org.bukkit.Material.DIAMOND_SWORD, main.toBukkit().getType());
        handle.setItemSlot(net.minecraft.world.entity.EquipmentSlot.OFFHAND,
                net.minecraft.world.item.ItemStack.of(new org.bukkit.inventory.ItemStack(org.bukkit.Material.SHIELD)));
        assertEquals(org.bukkit.Material.SHIELD, player.getInventory().getItemInOffHand().getType());

        var server = (FerriteServer) org.bukkit.Bukkit.getServer();
        server.unregisterPlayer(player.getUniqueId());
    }
}
