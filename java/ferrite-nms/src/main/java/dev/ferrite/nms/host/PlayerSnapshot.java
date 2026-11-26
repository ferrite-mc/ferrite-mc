// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms.host;

import java.util.Objects;
import java.util.UUID;

/** Immutable view of an online player. Never a live Rust player pointer. */
public record PlayerSnapshot(
        UUID id,
        String name,
        UUID worldId,
        double x,
        double y,
        double z,
        float yaw,
        float pitch,
        String gameMode,
        float health,
        int foodLevel,
        EntityState entityState,
        PlayerState state) {
    /** A snapshot without the optional entity/player state blocks. */
    public PlayerSnapshot(
            UUID id,
            String name,
            UUID worldId,
            double x,
            double y,
            double z,
            float yaw,
            float pitch,
            String gameMode,
            float health,
            int foodLevel) {
        this(id, name, worldId, x, y, z, yaw, pitch, gameMode, health, foodLevel, null, null);
    }

    public PlayerSnapshot {
        Objects.requireNonNull(id, "id");
        Objects.requireNonNull(name, "name");
        if (name.isBlank()) {
            throw new IllegalArgumentException("name must not be blank");
        }
        Objects.requireNonNull(worldId, "worldId");
        if (!Double.isFinite(x) || !Double.isFinite(y) || !Double.isFinite(z)) {
            throw new IllegalArgumentException("position must be finite");
        }
        if (!Float.isFinite(yaw) || !Float.isFinite(pitch)) {
            throw new IllegalArgumentException("rotation must be finite");
        }
        Objects.requireNonNull(gameMode, "gameMode");
        if (gameMode.isBlank()) {
            throw new IllegalArgumentException("gameMode must not be blank");
        }
        if (!Float.isFinite(health) || health < 0.0F) {
            throw new IllegalArgumentException("health must be a finite non-negative value");
        }
        if (foodLevel < 0 || foodLevel > 20) {
            throw new IllegalArgumentException("foodLevel must be within 0..20");
        }
    }

    /** Returns this snapshot with the optional entity/player state blocks attached. */
    public PlayerSnapshot withState(EntityState entityState, PlayerState state) {
        return new PlayerSnapshot(
                id,
                name,
                worldId,
                x,
                y,
                z,
                yaw,
                pitch,
                gameMode,
                health,
                foodLevel,
                entityState,
                state);
    }
}
