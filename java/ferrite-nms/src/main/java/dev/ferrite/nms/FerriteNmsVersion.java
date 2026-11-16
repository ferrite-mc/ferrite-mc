// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms;

import dev.ferrite.nms.host.HostVersion;
import java.util.Objects;

/**
 * The single place that pins the NMS surface to one Minecraft target.
 *
 * <p>Ferrite does not pretend to support several server versions. The constants are checked at
 * class initialization, and every host attachment goes through {@link #requireSupported(HostVersion)}.
 */
public final class FerriteNmsVersion {
    /** Minecraft Java version this module was written for. */
    public static final String MINECRAFT_VERSION = "26.3";

    /** Network protocol version of {@link #MINECRAFT_VERSION}. */
    public static final int PROTOCOL_VERSION = 777;

    /** Data version of the 26.3 vanilla data pack, as stamped into level.dat. */
    public static final int DATA_VERSION = 5023;

    /** Paper API artifact this module compiles against. */
    public static final String PAPER_API = "26.3.build.227-stable";

    /**
     * Version of the selected NMS surface itself. Bump when a wrapper method is added or its
     * meaning changes, so host adapters can tell which surface they are serving.
     */
    public static final int NMS_SURFACE_VERSION = 1;

    static {
        // These are compile-time constants; the check guards careless edits to this file rather
        // than a runtime mismatch. A runtime host mismatch is rejected by requireSupported.
        if (!"26.3".equals(MINECRAFT_VERSION) || PROTOCOL_VERSION != 777 || DATA_VERSION != 5023) {
            throw new ExceptionInInitializerError(
                    "Ferrite NMS is compiled for an inconsistent target: "
                            + MINECRAFT_VERSION + " / protocol " + PROTOCOL_VERSION + " / data " + DATA_VERSION);
        }
    }

    private FerriteNmsVersion() {}

    /** The version this NMS surface implements, for host adapters and fixtures. */
    public static HostVersion hostVersion() {
        return new HostVersion(MINECRAFT_VERSION, PROTOCOL_VERSION, DATA_VERSION);
    }

    /** Rejects a host that does not serve this exact 26.3 surface. */
    public static void requireSupported(HostVersion version) {
        Objects.requireNonNull(version, "version");
        requireSupported(version.minecraftVersion(), version.protocolVersion(), version.dataVersion());
    }

    /** Rejects a host version triple that is not this exact 26.3 surface. */
    public static void requireSupported(String minecraftVersion, int protocolVersion, int dataVersion) {
        if (MINECRAFT_VERSION.equals(minecraftVersion)
                && PROTOCOL_VERSION == protocolVersion
                && DATA_VERSION == dataVersion) {
            return;
        }
        throw new NmsVersionMismatchException(
                "Ferrite NMS supports Minecraft " + MINECRAFT_VERSION
                        + " (protocol " + PROTOCOL_VERSION + ", data " + DATA_VERSION + ") only; "
                        + "the host reports " + minecraftVersion
                        + " (protocol " + protocolVersion + ", data " + dataVersion + ")");
    }
}
