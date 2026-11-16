// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms.host;

import java.util.Objects;

/**
 * Optional live-state fields an entity snapshot can carry. {@link EntitySnapshot#state()} is
 * nullable: a host that does not publish these fields leaves it null, and the adapter refuses the
 * matching read explicitly instead of inventing a value.
 */
public record EntityState(
        int networkId,
        boolean removed,
        int ticksLived,
        int fireTicks,
        float fallDistance,
        boolean onGround,
        int portalCooldown,
        String pose) {
    public EntityState {
        if (networkId < 0) {
            throw new IllegalArgumentException("networkId must not be negative");
        }
        if (ticksLived < 0) {
            throw new IllegalArgumentException("ticksLived must not be negative");
        }
        if (fireTicks < -1) {
            throw new IllegalArgumentException("fireTicks must be -1 (not burning) or non-negative");
        }
        if (!Float.isFinite(fallDistance) || fallDistance < 0.0F) {
            throw new IllegalArgumentException("fallDistance must be a finite non-negative value");
        }
        if (portalCooldown < 0) {
            throw new IllegalArgumentException("portalCooldown must not be negative");
        }
        if (pose != null && pose.isBlank()) {
            throw new IllegalArgumentException("pose must be null or a non-blank 26.3 pose name");
        }
    }

    /** The Bukkit {@code Pose} name, or null when the host did not publish one. */
    public String pose() {
        return pose == null ? null : pose.trim();
    }

    @Override
    public String toString() {
        return "EntityState{networkId=" + networkId + ", removed=" + removed
                + ", pose=" + Objects.toString(pose, "unknown") + "}";
    }
}
