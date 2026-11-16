// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms.host;

import java.util.List;
import java.util.Objects;
import java.util.UUID;

/** Immutable view of a non-player entity, including its passenger/vehicle relationships. */
public record EntitySnapshot(
        UUID id,
        String typeKey,
        UUID worldId,
        double x,
        double y,
        double z,
        float yaw,
        float pitch,
        double velocityX,
        double velocityY,
        double velocityZ,
        UUID vehicleId,
        List<UUID> passengers) {
    public EntitySnapshot {
        Objects.requireNonNull(id, "id");
        Objects.requireNonNull(typeKey, "typeKey");
        if (typeKey.isBlank()) {
            throw new IllegalArgumentException("typeKey must not be blank");
        }
        Objects.requireNonNull(worldId, "worldId");
        if (!Double.isFinite(x) || !Double.isFinite(y) || !Double.isFinite(z)) {
            throw new IllegalArgumentException("position must be finite");
        }
        if (!Float.isFinite(yaw) || !Float.isFinite(pitch)) {
            throw new IllegalArgumentException("rotation must be finite");
        }
        if (!Double.isFinite(velocityX) || !Double.isFinite(velocityY) || !Double.isFinite(velocityZ)) {
            throw new IllegalArgumentException("velocity must be finite");
        }
        if (id.equals(vehicleId)) {
            throw new IllegalArgumentException("entity cannot be its own vehicle");
        }
        passengers = passengers == null ? List.of() : List.copyOf(passengers);
        if (passengers.contains(id)) {
            throw new IllegalArgumentException("entity cannot be its own passenger");
        }
    }
}
