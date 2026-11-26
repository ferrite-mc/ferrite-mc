package dev.ferrite.host;

import java.util.Map;

/**
 * The native side of the host boundary.
 *
 * <p>Rust binds both methods with {@code RegisterNatives} after the JVM starts. {@link #call}
 * returns the contract envelope, so a null answer means the native side failed before it could
 * serialize anything and is reported as an {@link IllegalStateException}.
 */
public final class HostBridge {
    private HostBridge() {
    }

    /** Calls one Rust RPC op and returns the JSON envelope. */
    public static native String call(String op, String requestJson);

    /** Logs to Rust's {@code tracing} target {@code ferrite_plugin}. */
    public static native void log(int level, String plugin, String message);

    /**
     * Zero-JSON JNI hot-path block read. Returns block state id ({@code >= 0}) or {@code -1} if
     * the chunk is not loaded or state provider is unavailable.
     */
    public static native int blockGetFast(String world, int x, int y, int z);

    /**
     * Zero-JSON JNI hot-path async block write. Returns monotonic request id ({@code > 0}) or
     * {@code -1} on failure.
     */
    public static native long blockSetFast(String world, int x, int y, int z, int stateId);

    /** Calls an op and unwraps the {@code result} object, throwing on an error envelope. */
    static Map<String, Object> callJson(String op, Map<String, Object> request) {
        String response = call(op, Json.stringify(request));
        if (response == null) {
            throw new IllegalStateException("the Ferrite host bridge returned no response for '" + op + "'");
        }
        Map<String, Object> envelope = Json.asObject(Json.parse(response));
        if (!Boolean.TRUE.equals(envelope.get("ok"))) {
            Map<String, Object> error = Json.asObject(envelope.get("error"));
            throw new HostRpcException(
                    Json.string(error, "code", "unknown"),
                    Json.string(error, "message", "the Ferrite host call failed"));
        }
        return Json.asObject(envelope.get("result"));
    }
}
