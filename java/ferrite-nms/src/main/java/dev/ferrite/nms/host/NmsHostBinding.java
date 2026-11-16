// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package dev.ferrite.nms.host;

import dev.ferrite.nms.FerriteNmsVersion;
import java.util.Objects;

/**
 * Process-wide attachment point for the one plugin host.
 *
 * <p>The JVM is process-wide, so the bound host is too. Installing a second host replaces the
 * first and returns it; callers that own the JVM decide when that is legitimate. There is no
 * fallback host: using an NMS wrapper before attachment is an explicit error, never a fake read
 * or a silent no-op.
 */
public final class NmsHostBinding {
    private static volatile NmsHost host;

    private NmsHostBinding() {}

    /** Verifies and installs {@code next}, returning the previously installed host (or null). */
    public static synchronized NmsHost install(NmsHost next) {
        Objects.requireNonNull(next, "host");
        FerriteNmsVersion.requireSupported(next.version());
        NmsHost previous = host;
        host = next;
        return previous;
    }

    /** Removes {@code expected}; a mismatch means two owners disagree about the JVM. */
    public static synchronized void uninstall(NmsHost expected) {
        Objects.requireNonNull(expected, "host");
        if (host == null) {
            throw new IllegalStateException("No Ferrite NMS host is installed");
        }
        if (host != expected) {
            throw new IllegalStateException("The installed Ferrite NMS host is not the one being uninstalled");
        }
        host = null;
    }

    /** The attached host, or an explicit error naming the missing bridge. */
    public static NmsHost require() {
        NmsHost current = host;
        if (current == null) {
            throw new IllegalStateException(
                    "Ferrite NMS host bridge is not attached: Lane 1's HostBridge adapter must call "
                            + "NmsHostBinding.install(host) before plugins use world, player, entity or "
                            + "registry-backed NMS surface. No fallback state exists.");
        }
        return current;
    }
}
