package dev.ferrite.host;

/**
 * Adapter handle returned by {@link HostApi#loadPlugin} when Lane 2 loads a Bukkit/Paper plugin.
 *
 * <p>Keeps {@code ferrite-host} free of Paper API dependencies while preserving per-plugin
 * classloader isolation and host lifecycle tracking.
 */
public interface LoadedHostPlugin {
    HostPlugin plugin();

    void enable() throws Exception;

    void disable() throws Exception;

    AutoCloseable classLoader();
}
