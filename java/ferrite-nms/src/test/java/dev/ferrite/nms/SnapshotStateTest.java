// Clean-room Ferrite NMS test. No Mojang code or assets.
package dev.ferrite.nms;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.ferrite.nms.host.EntitySnapshot;
import dev.ferrite.nms.host.EntityState;
import dev.ferrite.nms.host.PlayerSnapshot;
import dev.ferrite.nms.host.PlayerState;
import dev.ferrite.nms.test.FakeNmsHost;
import java.util.List;
import java.util.UUID;
import org.junit.jupiter.api.Test;

class SnapshotStateTest {
    @Test
    void entityStateRejectsImpossibleValues() {
        assertThrows(IllegalArgumentException.class,
                () -> new EntityState(-1, false, 0, 0, 0.0F, true, 0, "STANDING"));
        assertThrows(IllegalArgumentException.class,
                () -> new EntityState(1, false, -1, 0, 0.0F, true, 0, "STANDING"));
        assertThrows(IllegalArgumentException.class,
                () -> new EntityState(1, false, 0, -2, 0.0F, true, 0, "STANDING"));
        assertThrows(IllegalArgumentException.class,
                () -> new EntityState(1, false, 0, 0, Float.NaN, true, 0, "STANDING"));
        assertThrows(IllegalArgumentException.class,
                () -> new EntityState(1, false, 0, 0, -1.0F, true, 0, "STANDING"));
        assertThrows(IllegalArgumentException.class,
                () -> new EntityState(1, false, 0, 0, 0.0F, true, -1, "STANDING"));
        assertThrows(IllegalArgumentException.class,
                () -> new EntityState(1, false, 0, 0, 0.0F, true, 0, " "));
    }

    @Test
    void playerStateRejectsImpossibleValues() {
        PlayerState valid = new PlayerState(
                0, "en_us", 0.0F, 0.0F, 0, 0.0F, 0, 0.1F, 0.05F,
                false, false, false, false, 300, 300, null, null, 0L, 0L, true);

        assertThrows(IllegalArgumentException.class, () -> withPing(valid, -1));
        assertThrows(IllegalArgumentException.class, () -> withLocale(valid, " "));
        assertThrows(IllegalArgumentException.class, () -> withSaturation(valid, Float.NaN));
        assertThrows(IllegalArgumentException.class, () -> withSaturation(valid, -1.0F));
        assertThrows(IllegalArgumentException.class, () -> withExhaustion(valid, -1.0F));
        assertThrows(IllegalArgumentException.class, () -> withLevel(valid, -1));
        assertThrows(IllegalArgumentException.class, () -> withProgress(valid, 1.1F));
        assertThrows(IllegalArgumentException.class, () -> withTotalExperience(valid, -1));
        assertThrows(IllegalArgumentException.class, () -> withWalkSpeed(valid, 0.0F));
        assertThrows(IllegalArgumentException.class, () -> withFlySpeed(valid, Float.POSITIVE_INFINITY));
        assertThrows(IllegalArgumentException.class, () -> withMaximumAir(valid, -1));
        assertThrows(IllegalArgumentException.class, () -> withDisplayName(valid, " "));
        assertThrows(IllegalArgumentException.class, () -> withPlayerListName(valid, " "));
    }

    @Test
    void snapshotsKeepStateOptionalAndCopyPassengers() {
        UUID id = UUID.randomUUID();
        EntitySnapshot noState = new EntitySnapshot(
                id, "minecraft:pig", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, 0, 0, 0, null, List.of());
        assertNull(noState.state());

        EntityState state = new EntityState(9, false, 5, 0, 0.0F, true, 0, "STANDING");
        EntitySnapshot withState = noState.withState(state);
        assertEquals(state, withState.state());
        assertNull(noState.state(), "withState must not mutate the original snapshot");

        PlayerSnapshot player = new PlayerSnapshot(
                        id, "Alex", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, "SURVIVAL", 20.0F, 20)
                .withState(
                        state,
                        new PlayerState(
                                1, "en_us", 5.0F, 0.0F, 1, 0.0F, 1, 0.1F, 0.05F,
                                false, false, false, false, 300, 300, null, null, 0L, 0L, true));
        assertEquals(state, player.entityState());
        assertEquals(1, player.state().ping());
        assertTrue(player.toString().contains("Alex"));

        assertNull(new PlayerSnapshot(
                        id, "Alex", FakeNmsHost.WORLD_ID, 0, 64, 0, 0, 0, "SURVIVAL", 20.0F, 20)
                .state());
    }

    private static PlayerState withPing(PlayerState s, int ping) {
        return new PlayerState(ping, s.locale(), s.saturation(), s.exhaustion(), s.level(),
                s.experienceProgress(), s.totalExperience(), s.walkSpeed(), s.flySpeed(),
                s.allowFlight(), s.flying(), s.sneaking(), s.sprinting(), s.remainingAir(),
                s.maximumAir(), s.displayName(), s.playerListName(), s.playerTime(),
                s.playerTimeOffset(), s.playerTimeRelative());
    }

    private static PlayerState withLocale(PlayerState s, String locale) {
        return new PlayerState(s.ping(), locale, s.saturation(), s.exhaustion(), s.level(),
                s.experienceProgress(), s.totalExperience(), s.walkSpeed(), s.flySpeed(),
                s.allowFlight(), s.flying(), s.sneaking(), s.sprinting(), s.remainingAir(),
                s.maximumAir(), s.displayName(), s.playerListName(), s.playerTime(),
                s.playerTimeOffset(), s.playerTimeRelative());
    }

    private static PlayerState withSaturation(PlayerState s, float value) {
        return new PlayerState(s.ping(), s.locale(), value, s.exhaustion(), s.level(),
                s.experienceProgress(), s.totalExperience(), s.walkSpeed(), s.flySpeed(),
                s.allowFlight(), s.flying(), s.sneaking(), s.sprinting(), s.remainingAir(),
                s.maximumAir(), s.displayName(), s.playerListName(), s.playerTime(),
                s.playerTimeOffset(), s.playerTimeRelative());
    }

    private static PlayerState withExhaustion(PlayerState s, float value) {
        return new PlayerState(s.ping(), s.locale(), s.saturation(), value, s.level(),
                s.experienceProgress(), s.totalExperience(), s.walkSpeed(), s.flySpeed(),
                s.allowFlight(), s.flying(), s.sneaking(), s.sprinting(), s.remainingAir(),
                s.maximumAir(), s.displayName(), s.playerListName(), s.playerTime(),
                s.playerTimeOffset(), s.playerTimeRelative());
    }

    private static PlayerState withLevel(PlayerState s, int value) {
        return new PlayerState(s.ping(), s.locale(), s.saturation(), s.exhaustion(), value,
                s.experienceProgress(), s.totalExperience(), s.walkSpeed(), s.flySpeed(),
                s.allowFlight(), s.flying(), s.sneaking(), s.sprinting(), s.remainingAir(),
                s.maximumAir(), s.displayName(), s.playerListName(), s.playerTime(),
                s.playerTimeOffset(), s.playerTimeRelative());
    }

    private static PlayerState withProgress(PlayerState s, float value) {
        return new PlayerState(s.ping(), s.locale(), s.saturation(), s.exhaustion(), s.level(),
                value, s.totalExperience(), s.walkSpeed(), s.flySpeed(),
                s.allowFlight(), s.flying(), s.sneaking(), s.sprinting(), s.remainingAir(),
                s.maximumAir(), s.displayName(), s.playerListName(), s.playerTime(),
                s.playerTimeOffset(), s.playerTimeRelative());
    }

    private static PlayerState withTotalExperience(PlayerState s, int value) {
        return new PlayerState(s.ping(), s.locale(), s.saturation(), s.exhaustion(), s.level(),
                s.experienceProgress(), value, s.walkSpeed(), s.flySpeed(),
                s.allowFlight(), s.flying(), s.sneaking(), s.sprinting(), s.remainingAir(),
                s.maximumAir(), s.displayName(), s.playerListName(), s.playerTime(),
                s.playerTimeOffset(), s.playerTimeRelative());
    }

    private static PlayerState withWalkSpeed(PlayerState s, float value) {
        return new PlayerState(s.ping(), s.locale(), s.saturation(), s.exhaustion(), s.level(),
                s.experienceProgress(), s.totalExperience(), value, s.flySpeed(),
                s.allowFlight(), s.flying(), s.sneaking(), s.sprinting(), s.remainingAir(),
                s.maximumAir(), s.displayName(), s.playerListName(), s.playerTime(),
                s.playerTimeOffset(), s.playerTimeRelative());
    }

    private static PlayerState withFlySpeed(PlayerState s, float value) {
        return new PlayerState(s.ping(), s.locale(), s.saturation(), s.exhaustion(), s.level(),
                s.experienceProgress(), s.totalExperience(), s.walkSpeed(), value,
                s.allowFlight(), s.flying(), s.sneaking(), s.sprinting(), s.remainingAir(),
                s.maximumAir(), s.displayName(), s.playerListName(), s.playerTime(),
                s.playerTimeOffset(), s.playerTimeRelative());
    }

    private static PlayerState withMaximumAir(PlayerState s, int value) {
        return new PlayerState(s.ping(), s.locale(), s.saturation(), s.exhaustion(), s.level(),
                s.experienceProgress(), s.totalExperience(), s.walkSpeed(), s.flySpeed(),
                s.allowFlight(), s.flying(), s.sneaking(), s.sprinting(), s.remainingAir(),
                value, s.displayName(), s.playerListName(), s.playerTime(),
                s.playerTimeOffset(), s.playerTimeRelative());
    }

    private static PlayerState withDisplayName(PlayerState s, String value) {
        return new PlayerState(s.ping(), s.locale(), s.saturation(), s.exhaustion(), s.level(),
                s.experienceProgress(), s.totalExperience(), s.walkSpeed(), s.flySpeed(),
                s.allowFlight(), s.flying(), s.sneaking(), s.sprinting(), s.remainingAir(),
                s.maximumAir(), value, s.playerListName(), s.playerTime(),
                s.playerTimeOffset(), s.playerTimeRelative());
    }

    private static PlayerState withPlayerListName(PlayerState s, String value) {
        return new PlayerState(s.ping(), s.locale(), s.saturation(), s.exhaustion(), s.level(),
                s.experienceProgress(), s.totalExperience(), s.walkSpeed(), s.flySpeed(),
                s.allowFlight(), s.flying(), s.sneaking(), s.sprinting(), s.remainingAir(),
                s.maximumAir(), s.displayName(), value, s.playerTime(),
                s.playerTimeOffset(), s.playerTimeRelative());
    }
}
