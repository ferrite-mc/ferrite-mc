package dev.ferrite.host;

/** A host-level failure, reported to Rust as a {@code java_exception}. */
public final class HostException extends RuntimeException {
    public HostException(String message) {
        super(message);
    }

    public HostException(String message, Throwable cause) {
        super(message, cause);
    }
}
