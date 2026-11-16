// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms.bridge;

import java.util.Objects;

/**
 * A bridge lookup or registration failed.
 *
 * <p>The {@link #code() code} is a stable lowercase token. It is deliberately separate from
 * {@link dev.ferrite.nms.host.NmsHostException}: a missing provider is a Lane 1 wiring error,
 * while a host error means the provider was reached but the world/player/entity state was not.
 * Neither is replaced by a null handle or a fallback object.
 */
public final class NmsBridgeException extends RuntimeException {
    private static final long serialVersionUID = 1L;

    private final String code;

    public NmsBridgeException(String code, String message) {
        super(Objects.requireNonNull(code, "code") + ": " + Objects.requireNonNull(message, "message"));
        this.code = code;
    }

    public NmsBridgeException(String code, String message, Throwable cause) {
        super(Objects.requireNonNull(code, "code") + ": " + Objects.requireNonNull(message, "message"), cause);
        this.code = code;
    }

    public String code() {
        return code;
    }
}
