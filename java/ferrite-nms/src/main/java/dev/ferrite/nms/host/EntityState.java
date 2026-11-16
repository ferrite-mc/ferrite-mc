// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms.host;

import java.util.Locale;
import java.util.Map;
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
        String pose,
        String customName,
        boolean customNameVisible,
        boolean silent,
        boolean noGravity,
        boolean glowing,
        boolean invisible,
        Map<String, String> equipment) {
    /** Canonical 8-argument constructor preserving source compatibility for existing callers. */
    public EntityState(
            int networkId,
            boolean removed,
            int ticksLived,
            int fireTicks,
            float fallDistance,
            boolean onGround,
            int portalCooldown,
            String pose) {
        this(
                networkId,
                removed,
                ticksLived,
                fireTicks,
                fallDistance,
                onGround,
                portalCooldown,
                pose,
                null,
                false,
                false,
                false,
                false,
                false,
                Map.of());
    }

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
        if (customName != null && customName.isBlank()) {
            throw new IllegalArgumentException("customName must be null or non-blank");
        }
        equipment = equipment == null ? Map.of() : Map.copyOf(equipment);
    }

    /** Normalizes a Bukkit/26.3 equipment slot name (`HAND`, `OFF_HAND`, `FEET`, `LEGS`, `CHEST`, `HEAD`). */
    public static String normalizeEquipmentSlot(String slot) {
        Objects.requireNonNull(slot, "slot");
        return switch (slot.trim().toUpperCase(Locale.ROOT)) {
            case "HAND", "MAIN_HAND", "MAINHAND" -> "HAND";
            case "OFF_HAND", "OFFHAND" -> "OFF_HAND";
            case "FEET", "BOOTS" -> "FEET";
            case "LEGS", "LEGGINGS" -> "LEGS";
            case "CHEST", "CHESTPLATE" -> "CHEST";
            case "HEAD", "HELMET" -> "HEAD";
            default -> throw new NmsHostException(
                    "invalid_equipment_slot", "unknown 26.3 equipment slot: " + slot);
        };
    }

    /** The Bukkit {@code Pose} name, or null when the host did not publish one. */
    public String pose() {
        return pose == null ? null : pose.trim();
    }

    /** Returns the item key in {@code slot}, or null if empty. */
    public String equipmentItem(String slot) {
        return equipment.get(normalizeEquipmentSlot(slot));
    }

    @Override
    public String toString() {
        return "EntityState{networkId=" + networkId + ", removed=" + removed
                + ", pose=" + Objects.toString(pose, "unknown") + "}";
    }
}
