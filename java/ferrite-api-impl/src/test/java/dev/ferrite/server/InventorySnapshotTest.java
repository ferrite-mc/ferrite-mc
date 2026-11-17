package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.UUID;
import org.bukkit.Bukkit;
import org.bukkit.Material;
import org.bukkit.entity.Player;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.Damageable;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** The host's NBT inventory snapshot reaches a plugin's Player inventory with components intact. */
class InventorySnapshotTest {
    private static final String PLAYER_ID = "00000000-0000-0000-0000-0000000000e1";

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        Bootstrap.registerPlayer(PLAYER_ID, "Snapshot", false);
    }

    @AfterAll static void stop() {
        ((FerriteServer) Bukkit.getServer()).unregisterPlayer(UUID.fromString(PLAYER_ID));
    }

    // Produced by the Rust snapshot builder for a diamond sword with damage=7.
    private static final String DAMAGE_7 = "CgAACAACaWQAF21pbmVjcmFmdDpkaWFtb25kX3N3b3JkAwAFY291bnQAAAABCgAKY29tcG9uZW50cwMAEG1pbmVjcmFmdDpkYW1hZ2UAAAAHAAA=";

    @Test void nbtSnapshotKeepsComponents() {
        var encoded = DAMAGE_7;
        Bootstrap.setInventorySnapshotNbt(PLAYER_ID, 0, encoded);

        var player = Bukkit.getPlayer(UUID.fromString(PLAYER_ID));
        var got = player.getInventory().getItem(0);
        assertNotNull(got);
        assertEquals(Material.DIAMOND_SWORD, got.getType());
        var read = (Damageable) got.getItemMeta();
        assertEquals(7, read.getDamage());
        var viaApi = org.bukkit.inventory.ItemStack.deserializeBytes(
                java.util.Base64.getDecoder().decode(encoded));
        assertEquals(7, ((Damageable) viaApi.getItemMeta()).getDamage());
    }

    @Test void clearingASnapshotRemovesTheStack() {
        var encoded = DAMAGE_7;
        Bootstrap.setInventorySnapshotNbt(PLAYER_ID, 0, encoded);
        Bootstrap.clearInventorySnapshot(PLAYER_ID, 0);
        assertNull(Bukkit.getPlayer(UUID.fromString(PLAYER_ID)).getInventory().getItem(0));
        Bootstrap.setInventorySnapshotNbt(PLAYER_ID, 0, encoded);
        Bootstrap.setInventorySnapshotNbt(PLAYER_ID, 0, "");
        assertNull(Bukkit.getPlayer(UUID.fromString(PLAYER_ID)).getInventory().getItem(0));
    }
}
