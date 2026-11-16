// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms;

/**
 * Builds the explicit refusal used for NMS surface that Ferrite does not implement.
 *
 * <p>Every message names the class, the method and the reason. Plugins must be able to tell
 * "this Minecraft version has no such class" (a linkage error) from "Ferrite deliberately does
 * not support this" (this exception).
 */
public final class NmsUnsupported {
    private NmsUnsupported() {}

    public static UnsupportedOperationException operation(String className, String method, String reason) {
        return new UnsupportedOperationException(
                "Ferrite NMS " + FerriteNmsVersion.MINECRAFT_VERSION + " explicitly does not support "
                        + className + "#" + method + ": " + reason
                        + " (selected NMS surface " + FerriteNmsVersion.NMS_SURFACE_VERSION + ")");
    }

    public static UnsupportedOperationException feature(String surface, String reason) {
        return new UnsupportedOperationException(
                "Ferrite NMS " + FerriteNmsVersion.MINECRAFT_VERSION + " explicitly does not support "
                        + surface + ": " + reason);
    }
}
