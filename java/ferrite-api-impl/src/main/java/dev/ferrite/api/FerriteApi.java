package dev.ferrite.api;

import java.lang.reflect.Field;
import java.util.Objects;
import java.util.concurrent.atomic.AtomicReference;
import org.bukkit.Bukkit;

/**
 * Installs the Ferrite Bukkit facade as the process-wide Bukkit server.
 *
 * <p>{@link Bukkit#setServer(org.bukkit.Server)} is the documented Bukkit seam a server
 * implementation uses. Installing the facade also makes static Bukkit helpers
 * ({@code Bukkit.getPluginManager()}, {@code Event#callEvent()}, ...) reach the facade.
 * Only one runtime can be installed at a time; the host (Lane 1) owns the lifecycle and
 * must call {@link #uninstall(FerriteRuntime)} before shutting the JVM down.
 */
public final class FerriteApi {

    private static final AtomicReference<FerriteRuntime> INSTALLED = new AtomicReference<>();

    private FerriteApi() {
    }

    public static synchronized FerriteRuntime install(FerriteRuntime runtime) {
        Objects.requireNonNull(runtime, "runtime");
        if (INSTALLED.get() != null) {
            throw new IllegalStateException(
                    "A Ferrite Bukkit facade is already installed; the host may start only one");
        }
        // Bukkit.setServer also logs the build-info version, so install it before the
        // facade claims the process; a failure must not leave INSTALLED set.
        Bukkit.setServer(runtime.server());
        INSTALLED.set(runtime);
        return runtime;
    }

    public static FerriteRuntime runtime() {
        FerriteRuntime runtime = INSTALLED.get();
        if (runtime == null) {
            throw new IllegalStateException(
                    "No Ferrite Bukkit facade is installed; call FerriteApi.install first");
        }
        return runtime;
    }

    public static boolean isInstalled() {
        return INSTALLED.get() != null;
    }

    public static synchronized void uninstall(FerriteRuntime runtime) {
        Objects.requireNonNull(runtime, "runtime");
        if (!INSTALLED.compareAndSet(runtime, null)) {
            return;
        }
        clearBukkitServer();
        runtime.shutdown();
    }

    /**
     * {@code Bukkit.setServer(null)} is not usable: after assigning the field it calls
     * {@code server.getLogger()} and would throw. Tests need a clean Bukkit between
     * runtimes, so clear the private static field directly.
     */
    private static void clearBukkitServer() {
        try {
            Field field = Bukkit.class.getDeclaredField("server");
            field.setAccessible(true);
            field.set(null, null);
        } catch (ReflectiveOperationException exception) {
            throw new IllegalStateException("could not uninstall the Bukkit server facade", exception);
        }
    }
}
