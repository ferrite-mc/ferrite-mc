package dev.ferrite.host;

/** A Rust host op returned an error envelope. */
public final class HostRpcException extends RuntimeException {
    private final String code;

    public HostRpcException(String code, String message) {
        super(message);
        this.code = code;
    }

    /** Contract error code, for example {@code plugin_not_found}. */
    public String code() {
        return code;
    }
}
