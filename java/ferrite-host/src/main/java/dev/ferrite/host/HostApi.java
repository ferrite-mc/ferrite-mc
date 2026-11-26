package dev.ferrite.host;

import java.util.Map;
import java.util.UUID;

/**
 * The implementation Lane 2 provides for Bukkit/Paper semantics.
 *
 * <p>Every method has a safe default so the host can run with {@link NoopHostApi} before Lane 2
 * exists. Implementations are constructed with a no-argument constructor on the host
 * classloader, then {@link #onHostStarted} is called before any plugin loads.
 */
public interface HostApi {
    /** Called once, before any plugin metadata is loaded. */
    default void onHostStarted(HostContext context) throws Exception {
    }

    /**
     * Optional plugin loader hook for Lane 2 (Bukkit/Paper facade).
     *
     * <p>Returns {@code null} by default so {@link NoopHostApi} and native host-plugin tests keep
     * using {@link JavaPlugin}.
     */
    default LoadedHostPlugin loadPlugin(HostContext context, PluginSpec spec) throws Exception {
        return null;
    }

    /** Called after a plugin's {@code onEnable} succeeded. */
    default void onPluginEnabled(HostPlugin plugin) throws Exception {
    }

    /** Called after a plugin's {@code onDisable}. Errors are logged, never fatal. */
    default void onPluginDisabled(HostPlugin plugin) {
    }

    /**
     * Fires one of the contract v1 events. The payload map is unmodifiable; a listener that
     * wants to change server state must do so through its own API objects.
     */
    default EventResult fireEvent(String name, Map<String, Object> payload) throws Exception {
        return EventResult.pass();
    }

    /** Runs a plugin command. Returning {@code notHandled()} leaves it to the server. */
    default CommandResult runCommand(CommandOrigin origin, String line) throws Exception {
        return CommandResult.notHandled();
    }

    /**
     * v2 state candidate. Lane 1 owns the transport; this SPI method is for a future in-process
     * server adapter and is deliberately unsupported by default.
     */
    default Map<String, Object> blockGet(Map<String, Object> request) {
        throw unsupported("world.block_get");
    }

    /** v2 async write candidate. Acceptance does not mean the block was applied. */
    default Map<String, Object> blockSetAsync(Map<String, Object> request) {
        throw unsupported("world.block_set_async");
    }

    /** v2 chunk-read candidate. */
    default boolean chunkLoaded(String world, int x, int z) {
        throw unsupported("world.chunk_loaded");
    }

    /** v2 player snapshot candidate. */
    default Map<String, Object> playerStateGet(UUID uuid) {
        throw unsupported("player.state_get");
    }

    /** v2 entity snapshot candidate. */
    default Map<String, Object> entityStateGet(UUID uuid) {
        throw unsupported("entity.state_get");
    }

    private static UnsupportedOperationException unsupported(String op) {
        return new UnsupportedOperationException(
                op + " is not implemented by this HostApi; Lane 1 owns the native transport");
    }

    /** Called before any plugin is disabled. */
    default void onHostStopping() {
    }
}
