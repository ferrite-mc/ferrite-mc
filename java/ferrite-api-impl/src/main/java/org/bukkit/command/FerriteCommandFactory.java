package org.bukkit.command;

import org.bukkit.plugin.Plugin;

/**
 * Creates Bukkit's {@link PluginCommand} from the facade.
 *
 * <p>{@code PluginCommand}'s constructor is package-private, the same way CraftBukkit
 * creates it. Keeping the factory in this package lets the facade use the real API class
 * instead of shipping a lookalike.
 */
public final class FerriteCommandFactory {

    private FerriteCommandFactory() {
    }

    public static PluginCommand create(String name, Plugin plugin) {
        return new PluginCommand(name, plugin);
    }
}
