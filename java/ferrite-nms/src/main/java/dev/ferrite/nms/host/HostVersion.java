// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms.host;

import java.util.Objects;

/** Version triple reported by the attached plugin host. */
public record HostVersion(String minecraftVersion, int protocolVersion, int dataVersion) {
    public HostVersion {
        Objects.requireNonNull(minecraftVersion, "minecraftVersion");
        if (protocolVersion <= 0) {
            throw new IllegalArgumentException("protocolVersion must be positive");
        }
        if (dataVersion <= 0) {
            throw new IllegalArgumentException("dataVersion must be positive");
        }
    }
}
