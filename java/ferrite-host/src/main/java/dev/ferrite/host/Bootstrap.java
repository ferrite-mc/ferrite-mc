package dev.ferrite.host;

/**
 * The Rust→Java entry points fixed by contract v1, plus the v2 state transport helpers.
 *
 * <p>Every method returns a JSON string and converts exceptions to an {@code {ok:false,...}}
 * envelope, so a plugin error never escapes through JNI as an unhandled Java exception.
 */
public final class Bootstrap {
    /** Minimum supported contract version in {@code docs/plugin-b-host-contract.md}. */
    public static final long MIN_CONTRACT_VERSION = 2;
    /** Contract version in {@code docs/plugin-b-host-contract.md}. */
    public static final long CONTRACT_VERSION = 3;

    private Bootstrap() {
    }

    public static String start(String configJson) {
        return guarded(() -> HostRuntime.start(configJson));
    }

    public static String loadPlugin(String pluginJson) {
        return guarded(() -> HostRuntime.instance().loadPlugin(pluginJson));
    }

    public static String fireEvent(String name, String payloadJson) {
        return guarded(() -> HostRuntime.instance().fireEvent(name, payloadJson));
    }

    public static String runTask(long handle) {
        return guarded(() -> HostRuntime.instance().runTask(handle));
    }

    public static String runTaskAsync(long handle) {
        return guarded(() -> HostRuntime.instance().runTaskAsync(handle));
    }

    public static String forgetTask(long handle) {
        return guarded(() -> HostRuntime.instance().forgetTask(handle));
    }

    public static String runCommand(String originJson, String commandLine) {
        return guarded(() -> HostRuntime.instance().runCommand(originJson, commandLine));
    }

    public static String shutdown(String reasonJson) {
        return guarded(() -> HostRuntime.shutdown(reasonJson));
    }

    public static String status() {
        return guarded(HostRuntime::status);
    }

    /** v2 transport helper: sends one {@code world.block_get} request envelope. */
    public static String worldBlockGet(String requestJson) {
        return guarded(() -> HostBridge.call("world.block_get", requestJson));
    }

    /** v2 transport helper: sends one {@code world.block_set_async} request envelope. */
    public static String worldBlockSetAsync(String requestJson) {
        return guarded(() -> HostBridge.call("world.block_set_async", requestJson));
    }

    /** v2 transport helper: sends one {@code world.chunk_loaded} request envelope. */
    public static String worldChunkLoaded(String requestJson) {
        return guarded(() -> HostBridge.call("world.chunk_loaded", requestJson));
    }

    /** v2 transport helper: sends one {@code player.state_get} request envelope. */
    public static String playerStateGet(String requestJson) {
        return guarded(() -> HostBridge.call("player.state_get", requestJson));
    }

    /** v2 transport helper: sends one {@code player.inventory_get} request envelope. */
    public static String playerInventoryGet(String requestJson) {
        return guarded(() -> HostBridge.call("player.inventory_get", requestJson));
    }

    /** v2 transport helper: sends one {@code player.inventory_set_async} request envelope. */
    public static String playerInventorySetAsync(String requestJson) {
        return guarded(() -> HostBridge.call("player.inventory_set_async", requestJson));
    }

    /** v2 transport helper: sends one {@code entity.state_get} request envelope. */
    public static String entityStateGet(String requestJson) {
        return guarded(() -> HostBridge.call("entity.state_get", requestJson));
    }

    /** v1/v3 capability discovery helper: sends {@code host.info}. */
    public static String hostInfo() {
        return guarded(() -> HostBridge.call("host.info", "{}"));
    }

    /** v3 transport helper: sends one {@code world.chunk_status} request envelope. */
    public static String worldChunkStatus(String requestJson) {
        return guarded(() -> HostBridge.call("world.chunk_status", requestJson));
    }

    /** v3 transport helper: sends one {@code world.loaded_chunks} request envelope. */
    public static String worldLoadedChunks(String requestJson) {
        return guarded(() -> HostBridge.call("world.loaded_chunks", requestJson));
    }

    /** v3 transport helper: sends one {@code world.entities_snapshot} request envelope. */
    public static String worldEntitiesSnapshot(String requestJson) {
        return guarded(() -> HostBridge.call("world.entities_snapshot", requestJson));
    }

    /** v3 transport helper: sends one {@code world.biome_get} request envelope. */
    public static String worldBiomeGet(String requestJson) {
        return guarded(() -> HostBridge.call("world.biome_get", requestJson));
    }

    @FunctionalInterface
    private interface Call {
        String run();
    }

    private static String guarded(Call call) {
        try {
            return call.run();
        } catch (Throwable error) {
            String message = HostRuntime.errorSummary(error);
            String stack = HostRuntime.stackTrace(error);
            try {
                HostBridge.log(4, "server", "host call failed: " + message + "\n" + stack);
            } catch (Throwable ignored) {
                // The native logger is unavailable while the JVM is still starting.
            }
            return HostRuntime.errorResponse("java_exception", message, stack);
        }
    }
}
