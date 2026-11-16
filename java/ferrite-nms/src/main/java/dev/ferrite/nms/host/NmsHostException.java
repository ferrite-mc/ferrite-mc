// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms.host;

import java.util.Objects;

/**
 * A host operation failed, or the requested world/player/entity is not known to the host.
 *
 * <p>{@link #code()} is a stable lowercase token such as {@code unknown_world}. The wire error
 * code names come from Lane 1's contract once it exists; this port must map them explicitly
 * rather than reinterpreting an old code silently.
 */
public final class NmsHostException extends RuntimeException {
    private static final long serialVersionUID = 1L;

    private final String code;

    public NmsHostException(String code, String message) {
        super(Objects.requireNonNull(code, "code") + ": " + Objects.requireNonNull(message, "message"));
        this.code = code;
    }

    public NmsHostException(String code, String message, Throwable cause) {
        super(Objects.requireNonNull(code, "code") + ": " + Objects.requireNonNull(message, "message"), cause);
        this.code = code;
    }

    public String code() {
        return code;
    }
}
