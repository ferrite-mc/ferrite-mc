// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms.host;

import java.util.Objects;

/**
 * Immutable block state read from a world. {@code properties} is the comma-separated inner part
 * of the 26.3 block-state string, or an empty string for a state without properties.
 */
public record BlockStateSnapshot(int globalId, String name, String properties, boolean air) {
    public BlockStateSnapshot {
        if (globalId < 0) {
            throw new IllegalArgumentException("globalId must not be negative");
        }
        Objects.requireNonNull(name, "name");
        properties = properties == null ? "" : properties;
    }

    /** The canonical {@code minecraft:oak_stairs[facing=north]} form. */
    public String asString() {
        return properties.isEmpty() ? name : name + "[" + properties + "]";
    }
}
