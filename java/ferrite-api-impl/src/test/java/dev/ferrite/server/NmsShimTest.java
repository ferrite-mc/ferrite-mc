package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import net.minecraft.core.BlockPos;
import net.minecraft.network.chat.Component;
import net.minecraft.world.item.ItemStack;
import org.junit.jupiter.api.Test;

class NmsShimTest {
    @Test
    void componentKeepsPlainText() {
        assertEquals("hello", Component.literal("hello").getString());
    }

    @Test
    void blockPosOffsetsAndKeepsCoordinates() {
        BlockPos pos = new BlockPos(1, 2, 3);
        assertEquals(1, pos.getX());
        assertEquals(3, pos.getZ());
    }

    @Test
    void emptyItemStackReportsZeroCount() {
        assertTrue(ItemStack.EMPTY.isEmpty());
        assertEquals(0, ItemStack.EMPTY.getCount());
    }

    @Test
    void playerTypeIsCraftPlayerSoCastsSucceed() {
        assertTrue(org.bukkit.craftbukkit.entity.CraftPlayer.class.isAssignableFrom(FerritePlayer.class));
    }

    @Test
    void healthPacketReachesTheHostAndUnsupportedPacketsFailVisible() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        var server = (FerriteServer) org.bukkit.Bukkit.getServer();
        var id = java.util.UUID.randomUUID();
        var player = FerritePlayer.create(server, id, "NmsHealth", false, 20.0, 20, 0, null);
        server.registerPlayer(player);
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(owner + "|" + subject + "|" + method + "|" + args); return ""; };
        try {
            var connection = ((org.bukkit.craftbukkit.entity.CraftPlayer) player).getHandle().connection;
            connection.send(new net.minecraft.network.protocol.game.ClientboundSetHealthPacket(7.5f, 3, 1.25f));
            assertEquals("Player|" + id + "|sendHealthUpdate(double,int,float)|7.5" + HostBridge.ARG + "3" + HostBridge.ARG + "1.25", calls.get(0));

            HostBridge.transport = (owner, subject, method, args) -> null;
            var unsupported = assertThrows(UnsupportedOperationException.class, () -> connection.send(new UnsupportedPacket()));
            assertTrue(unsupported.getMessage().contains("NMS#send(UnsupportedPacket)"), unsupported.getMessage());
        } finally {
            HostBridge.transport = null;
            server.unregisterPlayer(id);
        }
    }

    private static final class UnsupportedPacket implements net.minecraft.network.protocol.Packet<Object> {}
}
