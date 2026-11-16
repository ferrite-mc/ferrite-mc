// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms.host;

/**
 * Optional player state an entity-only snapshot cannot express. {@link PlayerSnapshot#state()} is
 * nullable: a host that does not publish these fields leaves it null, and the adapter refuses the
 * matching read explicitly instead of returning a fabricated default.
 */
public record PlayerState(
        int ping,
        String locale,
        float saturation,
        float exhaustion,
        int level,
        float experienceProgress,
        int totalExperience,
        float walkSpeed,
        float flySpeed,
        boolean allowFlight,
        boolean flying,
        boolean sneaking,
        boolean sprinting,
        int remainingAir,
        int maximumAir,
        String displayName,
        String playerListName,
        long playerTime,
        long playerTimeOffset,
        boolean playerTimeRelative) {
    public PlayerState {
        if (ping < 0) {
            throw new IllegalArgumentException("ping must not be negative");
        }
        if (locale == null || locale.isBlank()) {
            throw new IllegalArgumentException("locale must not be blank");
        }
        if (!Float.isFinite(saturation) || saturation < 0.0F) {
            throw new IllegalArgumentException("saturation must be a finite non-negative value");
        }
        if (!Float.isFinite(exhaustion) || exhaustion < 0.0F) {
            throw new IllegalArgumentException("exhaustion must be a finite non-negative value");
        }
        if (level < 0) {
            throw new IllegalArgumentException("level must not be negative");
        }
        if (!Float.isFinite(experienceProgress)
                || experienceProgress < 0.0F
                || experienceProgress > 1.0F) {
            throw new IllegalArgumentException("experienceProgress must be within 0..1");
        }
        if (totalExperience < 0) {
            throw new IllegalArgumentException("totalExperience must not be negative");
        }
        if (!Float.isFinite(walkSpeed) || walkSpeed <= 0.0F) {
            throw new IllegalArgumentException("walkSpeed must be a finite positive value");
        }
        if (!Float.isFinite(flySpeed) || flySpeed <= 0.0F) {
            throw new IllegalArgumentException("flySpeed must be a finite positive value");
        }
        if (maximumAir < 0) {
            throw new IllegalArgumentException("maximumAir must not be negative");
        }
        if (displayName != null && displayName.isBlank()) {
            throw new IllegalArgumentException("displayName must be null or non-blank");
        }
        if (playerListName != null && playerListName.isBlank()) {
            throw new IllegalArgumentException("playerListName must be null or non-blank");
        }
    }
}
