package dev.ferrite.host;

import java.nio.file.Path;

/**
 * A plugin the host can drive.
 *
 * <p>The accessors are deliberately named {@code plugin*} instead of Bukkit's
 * {@code getName}/{@code getLogger}/{@code getDataFolder}. That leaves those signatures free for
 * Lane 2's {@code org.bukkit.plugin.java.JavaPlugin}, so plugin classes can extend it without a
 * return-type collision.
 */
public interface HostPlugin {
    /** Plugin name from the descriptor. */
    String pluginName();

    /** Plugin version from the descriptor. */
    String pluginVersion();

    /** Parsed descriptor. */
    PluginSpec pluginSpec();

    /** {@code <data_dir>/<name>/}, created before {@code onLoad}. */
    Path pluginDataFolder();

    /** Logger tagged with the plugin name. */
    HostLogger pluginLogger();

    /** The host context handed to this plugin. */
    HostContext hostContext();

    /** Whether {@code onEnable} completed. */
    boolean isPluginEnabled();

    /** Used by the host while enabling and disabling. */
    void setPluginEnabled(boolean enabled);

    /** Called once before {@code onEnable}. */
    void onLoad() throws Exception;

    /** Called once. A task may only be scheduled after this method starts. */
    void onEnable() throws Exception;

    /** Called on shutdown, in reverse load order. */
    void onDisable() throws Exception;
}
