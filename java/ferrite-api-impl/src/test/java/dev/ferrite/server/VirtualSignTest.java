package dev.ferrite.server;

import java.util.UUID;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class VirtualSignTest {
    @Test void virtual_sign_preserves_coordinates_and_side() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        var player = FerritePlayer.create((FerriteServer) org.bukkit.Bukkit.getServer(), UUID.fromString("00000000-0000-0000-0000-000000000007"), "Test", false, 20, 20, 0, null);
        var calls = new java.util.ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method + "|" + args); return ""; };
        try {
            player.openVirtualSign(io.papermc.paper.math.Position.block(1, 64, -2), org.bukkit.block.sign.Side.BACK);
            assertEquals("openVirtualSign(int,int,int,Side)|1" + HostBridge.ARG + "64" + HostBridge.ARG + "-2" + HostBridge.ARG + "BACK", calls.getFirst());
        } finally { HostBridge.transport = null; }
    }
}
