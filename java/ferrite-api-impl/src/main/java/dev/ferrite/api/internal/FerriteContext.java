package dev.ferrite.api.internal;

import java.util.Objects;
import java.util.function.Supplier;

/**
 * Tracks which plugin is currently executing so unsupported-API errors can name it.
 *
 * <p>The host calls into the facade on the server main thread; the facade wraps every
 * plugin callback (lifecycle, listener, command, scheduler task) in
 * {@link #withPlugin(String, Runnable)}. Nested callbacks keep the previous value.
 */
public final class FerriteContext {

    private static final ThreadLocal<String> CURRENT_PLUGIN = new ThreadLocal<>();

    private FerriteContext() {
    }

    public static void withPlugin(String pluginName, Runnable action) {
        String previous = push(pluginName);
        try {
            action.run();
        } finally {
            pop(previous);
        }
    }

    public static <T> T withPlugin(String pluginName, Supplier<T> action) {
        String previous = push(pluginName);
        try {
            return action.get();
        } finally {
            pop(previous);
        }
    }

    /** Internal: enter a plugin callback while a checked exception may be thrown. */
    public static String push(String pluginName) {
        String previous = CURRENT_PLUGIN.get();
        CURRENT_PLUGIN.set(pluginName);
        return previous;
    }

    /** Internal: restore the plugin callback that was active before {@link #push}. */
    public static void pop(String previous) {
        if (previous == null) {
            CURRENT_PLUGIN.remove();
        } else {
            CURRENT_PLUGIN.set(previous);
        }
    }

    /** The plugin whose callback is running, or {@code null} outside plugin callbacks. */
    public static String currentPlugin() {
        return CURRENT_PLUGIN.get();
    }

    public static void requirePlugin(String pluginName) {
        Objects.requireNonNull(pluginName, "pluginName");
    }
}
