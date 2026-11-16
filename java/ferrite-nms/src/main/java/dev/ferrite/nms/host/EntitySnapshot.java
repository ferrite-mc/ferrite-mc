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
        Objects.requireNonNull(worldId, "worldId");
        passengers = passengers == null ? List.of() : List.copyOf(passengers);
    }
}
