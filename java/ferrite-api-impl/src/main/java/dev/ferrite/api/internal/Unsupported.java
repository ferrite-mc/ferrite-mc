package dev.ferrite.api.internal;

/**
 * The single place that builds the facade's explicit unsupported-API error.
 *
 * <p>Unsupported Bukkit methods must never return {@code null}, an empty collection or a
 * no-op that looks successful. Every refusal names the API method and, when the call
 * happens inside a plugin lifecycle/event/command/task callback, the plugin that made it.
 */
public final class Unsupported {

    private Unsupported() {
    }

    public static UnsupportedOperationException exception(String api) {
        String plugin = FerriteContext.currentPlugin();
        String suffix = plugin == null ? "" : " (called by plugin '" + plugin + "')";
        return new UnsupportedOperationException(
                api
                        + " is not implemented by the Ferrite Bukkit API facade"
                        + " for Paper API 26.3.build.227-stable"
                        + suffix);
    }
}
