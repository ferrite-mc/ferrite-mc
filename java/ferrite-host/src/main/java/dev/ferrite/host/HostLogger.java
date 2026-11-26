package dev.ferrite.host;

/** A logger that forwards to Rust's {@code ferrite_plugin} tracing target. */
public final class HostLogger {
    private final String plugin;

    HostLogger(String plugin) {
        this.plugin = plugin;
    }

    public void trace(String message) {
        HostBridge.log(0, plugin, message);
    }

    public void debug(String message) {
        HostBridge.log(1, plugin, message);
    }

    public void info(String message) {
        HostBridge.log(2, plugin, message);
    }

    public void warn(String message) {
        HostBridge.log(3, plugin, message);
    }

    public void error(String message) {
        HostBridge.log(4, plugin, message);
    }

    public void error(String message, Throwable error) {
        HostBridge.log(4, plugin, message + "\n" + HostRuntime.stackTrace(error));
    }

    /** Plugin name this logger is tagged with. */
    public String pluginName() {
        return plugin;
    }
}
