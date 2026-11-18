package dev.ferrite.api.command;

import dev.ferrite.api.FerriteRuntime;
import java.util.LinkedHashMap;
import java.util.Objects;
import org.bukkit.command.Command;
import org.bukkit.command.SimpleCommandMap;

/**
 * Live {@link org.bukkit.command.CommandMap} backed by {@link SimpleCommandMap} and wired to
 * {@link FerriteHelpMap} so plugin and dynamically registered commands become discoverable.
 */
public final class FerriteCommandMap extends SimpleCommandMap {

    private final FerriteRuntime runtime;

    public FerriteCommandMap(FerriteRuntime runtime) {
        super(Objects.requireNonNull(runtime, "runtime").server(), new LinkedHashMap<>());
        this.runtime = runtime;
    }

    @Override
    public boolean register(String label, String fallbackPrefix, Command command) {
        boolean registered = super.register(label, fallbackPrefix, command);
        // SimpleCommandMap.<init> calls setDefaultCommands() -> register() before this.runtime
        // is assigned, and before FerriteRuntime.helpMap is constructed.
        if (runtime != null && runtime.helpMap() != null) {
            runtime.helpMap().registerCommandTopic(command);
        }
        return registered;
    }
}
