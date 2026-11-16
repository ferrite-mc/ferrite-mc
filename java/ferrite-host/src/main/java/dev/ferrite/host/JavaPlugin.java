package dev.ferrite.host;

import java.nio.file.Path;

/**
 * Base class for a plugin main class.
 *
 * <p>Lane 2's Bukkit {@code JavaPlugin} extends this class and adds the Bukkit API. This class
 * intentionally exposes host accessors as {@code plugin*} to avoid clashing with Bukkit method
 * names and return types.
 */
public abstract class JavaPlugin implements HostPlugin {
    private HostContext context;
    private PluginSpec spec;
    private HostLogger logger;
    private volatile boolean enabled;

    final void attach(HostContext context, PluginSpec spec) {
        this.context = context;
        this.spec = spec;
        this.logger = new HostLogger(spec.name());
    }

    @Override
    public final String pluginName() {
        return requireSpec().name();
    }

    @Override
    public final String pluginVersion() {
        return requireSpec().version();
    }

    @Override
    public final PluginSpec pluginSpec() {
        return requireSpec();
    }

    @Override
    public final Path pluginDataFolder() {
        return requireSpec().dataFolder();
    }

    @Override
    public final HostLogger pluginLogger() {
        if (logger == null) {
            throw new IllegalStateException("the plugin has not been attached to the host yet");
        }
        return logger;
    }

    @Override
    public final HostContext hostContext() {
        if (context == null) {
            throw new IllegalStateException("the plugin has not been attached to the host yet");
        }
        return context;
    }

    @Override
    public final boolean isPluginEnabled() {
        return enabled;
    }

    @Override
    public final void setPluginEnabled(boolean enabled) {
        this.enabled = enabled;
    }

    @Override
    public void onLoad() throws Exception {
    }

    @Override
    public void onEnable() throws Exception {
    }

    @Override
    public void onDisable() throws Exception {
    }

    private PluginSpec requireSpec() {
        if (spec == null) {
            throw new IllegalStateException("the plugin has not been attached to the host yet");
        }
        return spec;
    }
}
