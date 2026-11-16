// Clean-room Ferrite NMS test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.NmsHostException;
import dev.ferrite.nms.test.FakeNmsHost;
import java.util.UUID;
import java.util.concurrent.TimeUnit;
import net.minecraft.server.level.ServerPlayer;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class ServerPlayerTest {
    private FakeNmsHost host;

    @BeforeEach
    void installFakeHost() {
        host = new FakeNmsHost();
        NmsHostBinding.install(host);
    }

    @AfterEach
    void removeFakeHost() {
        try {
            NmsHostBinding.uninstall(host);
        } catch (IllegalStateException ignored) {
            // Deliberately uninstalled by a test.
        }
        host.close();
    }

    @Test
    void readsThePlayerSnapshot() {
        ServerPlayer player = ServerPlayer.of(FakeNmsHost.PLAYER_ID);
        assertEquals(FakeNmsHost.PLAYER_ID, player.getUUID());
        assertEquals("Steve", player.getName());
        assertEquals("minecraft:player", player.getTypeKey());
        assertEquals("SURVIVAL", player.gameMode());
        assertEquals(20.0F, player.getHealth());
        assertEquals(20, player.getFoodLevel());
        assertEquals(1.5, player.getX());
        assertEquals(65.0, player.getY());
        assertEquals(2.5, player.getZ());
        assertEquals(90.0F, player.getYRot());
        assertEquals(10.0F, player.getXRot());
        assertEquals(FakeNmsHost.WORLD_ID, player.level().getUUID());
        assertTrue(player.passengerIds().isEmpty());
    }

    @Test
    void unknownPlayersFailWithTheHostCode() {
        NmsHostException error = assertThrows(NmsHostException.class,
                () -> ServerPlayer.of(UUID.randomUUID()));
        assertEquals("unknown_player", error.code());
    }

    @Test
    void safeWritesAreAsynchronousHostOperations() throws Exception {
        ServerPlayer player = ServerPlayer.of(FakeNmsHost.PLAYER_ID);
        player.setHealthAsync(15.0F).get(5, TimeUnit.SECONDS);
        player.setGameModeAsync("CREATIVE").get(5, TimeUnit.SECONDS);

        assertEquals(new FakeNmsHost.HealthWrite(FakeNmsHost.PLAYER_ID, 15.0F), host.healthWrites.get(0));
        assertEquals(new FakeNmsHost.GameModeWrite(FakeNmsHost.PLAYER_ID, "CREATIVE"), host.gameModeWrites.get(0));
    }

    @Test
    void invalidWritesAreRefusedBeforeTouchingTheHost() {
        ServerPlayer player = ServerPlayer.of(FakeNmsHost.PLAYER_ID);
        assertThrows(IllegalArgumentException.class, () -> player.setHealthAsync(-1.0F));
        assertThrows(IllegalArgumentException.class, () -> player.setHealthAsync(Float.NaN));
        assertThrows(IllegalArgumentException.class, () -> player.setGameModeAsync(" "));
        assertTrue(host.healthWrites.isEmpty());
        assertTrue(host.gameModeWrites.isEmpty());
    }
}
