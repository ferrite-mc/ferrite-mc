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
        int foodLevel) {
    public PlayerSnapshot {
        Objects.requireNonNull(id, "id");
        Objects.requireNonNull(name, "name");
        Objects.requireNonNull(worldId, "worldId");
        Objects.requireNonNull(gameMode, "gameMode");
        if (!Float.isFinite(health) || health < 0.0F) {
            throw new IllegalArgumentException("health must be a finite non-negative value");
        }
        if (foodLevel < 0 || foodLevel > 20) {
            throw new IllegalArgumentException("foodLevel must be within 0..20");
        }
    }
}
