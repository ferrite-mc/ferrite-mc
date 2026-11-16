// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms;

/** The host serves a Minecraft version other than the one this NMS surface was built for. */
public final class NmsVersionMismatchException extends IllegalStateException {
    private static final long serialVersionUID = 1L;

    public NmsVersionMismatchException(String message) {
        super(message);
    }
}
