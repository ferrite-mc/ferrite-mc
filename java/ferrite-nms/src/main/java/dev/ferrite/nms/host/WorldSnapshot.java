// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms.host;

import java.util.Objects;
import java.util.UUID;

/** Immutable view of a Rust world as returned by the host. */
public record WorldSnapshot(
        UUID id,
        String name,
        String dimensionKey,
        String environment,
        long seed,
        long dayTime,
        double spawnX,
        double spawnY,
        double spawnZ,
        int minY,
        int maxY) {
    public WorldSnapshot {
        Objects.requireNonNull(id, "id");
        Objects.requireNonNull(name, "name");
        Objects.requireNonNull(dimensionKey, "dimensionKey");
        Objects.requireNonNull(environment, "environment");
        if (minY > maxY) {
            throw new IllegalArgumentException("minY must not exceed maxY");
        }
    }
}
