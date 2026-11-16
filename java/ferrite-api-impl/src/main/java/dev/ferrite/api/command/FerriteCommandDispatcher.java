package dev.ferrite.api.command;

import dev.ferrite.api.FerriteRuntime;
import dev.ferrite.api.entity.FerritePlayer;
import java.util.Arrays;
import java.util.LinkedHashMap;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import org.bukkit.command.CommandException;
import org.bukkit.command.CommandSender;
import org.bukkit.command.PluginCommand;
import org.bukkit.entity.Player;
import org.bukkit.event.player.PlayerCommandPreprocessEvent;

/**
 * Resolves a command line to a registered {@link PluginCommand} and executes it.
 *
 * <p>The host calls {@link #dispatch(CommandSender, String)} when it receives a chat
 * command. For players the Bukkit {@code PlayerCommandPreprocessEvent} runs first and can
 * cancel or rewrite the line. Permission checks are Bukkit's:
 * {@link PluginCommand#execute} calls {@code testPermission} before the executor.
 */
public final class FerriteCommandDispatcher {

    private static final String UNKNOWN_COMMAND =
            "Unknown command. Type \"/help\" for help.";

    private final FerriteRuntime runtime;
    private final Map<String, PluginCommand> commands = new LinkedHashMap<>();

    public FerriteCommandDispatcher(FerriteRuntime runtime) {
        this.runtime = runtime;
    }

    public void register(PluginCommand command) {
        Objects.requireNonNull(command, "command");
        String pluginName = command.getPlugin().getName().toLowerCase(Locale.ROOT);
        registerKey(command.getName(), command);
        registerKey(pluginName + ":" + command.getName(), command);
        for (String alias : command.getAliases()) {
            registerKey(alias, command);
            registerKey(pluginName + ":" + alias, command);
        }
    }

    private void registerKey(String key, PluginCommand command) {
        if (key == null || key.isBlank()) {
            return;
        }
        String normalized = key.toLowerCase(Locale.ROOT);
        PluginCommand existing = commands.putIfAbsent(normalized, command);
        if (existing != null && existing != command) {
            runtime.logger().warning("Command label /" + normalized + " is already owned by "
                    + existing.getPlugin().getName() + "; keeping that registration for "
                    + command.getPlugin().getName());
        }
    }

    public PluginCommand get(String name) {
        return name == null ? null : commands.get(name.toLowerCase(Locale.ROOT));
    }

    /**
     * Dispatch one command line (with or without a leading slash).
     *
     * @return whether the command was recognized and executed
     */
    public boolean dispatch(CommandSender sender, String line) {
        Objects.requireNonNull(sender, "sender");
        Objects.requireNonNull(line, "line");
        runtime.requirePrimaryThread("Bukkit command dispatch");

        String message = line.startsWith("/") ? line.substring(1) : line;
        message = message.trim();
        if (message.isEmpty()) {
            return false;
        }

        if (sender instanceof FerritePlayer player) {
            PlayerCommandPreprocessEvent event =
                    new PlayerCommandPreprocessEvent(player, "/" + message);
            runtime.plugins().callEvent(event);
            if (event.isCancelled()) {
                return true;
            }
            message = event.getMessage().trim();
            if (message.startsWith("/")) {
                message = message.substring(1);
            }
            if (message.isEmpty()) {
                return true;
            }
        }

        String[] parts = message.split("\\s+");
        String label = parts[0].toLowerCase(Locale.ROOT);
        String[] args = Arrays.copyOfRange(parts, 1, parts.length);

        PluginCommand command = commands.get(label);
        if (command == null) {
            sender.sendMessage(UNKNOWN_COMMAND);
            return false;
        }
        if (!command.getPlugin().isEnabled()) {
            sender.sendMessage("Plugin " + command.getPlugin().getName()
                    + " is disabled; /" + label + " is unavailable");
            return true;
        }
        // Bukkit passes the command's own label to the executor, not the namespaced
        // lookup key: /plugin:command runs with label "command".
        int pluginPrefix = label.indexOf(':');
        String executionLabel = pluginPrefix >= 0 && pluginPrefix + 1 < label.length()
                ? label.substring(pluginPrefix + 1)
                : label;
        try {
            return command.execute(sender, executionLabel, args);
        } catch (CommandException exception) {
            sender.sendMessage(exception.getMessage());
            return true;
        }
    }

    /** Every registered command, keyed by label and by {@code plugin:label}. */
    public Map<String, PluginCommand> commands() {
        return Map.copyOf(commands);
    }

    /** Used by {@link Player} command senders in tests; the host calls {@link #dispatch}. */
    public boolean dispatchPlayer(Player player, String line) {
        return dispatch(player, line);
    }
}
