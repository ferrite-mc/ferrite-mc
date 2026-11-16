package dev.ferrite.fixtures.typical;

import java.util.List;
import org.bukkit.command.Command;
import org.bukkit.command.CommandExecutor;
import org.bukkit.command.CommandSender;
import org.bukkit.command.TabCompleter;

/** {@code /ferrite-hello} executor for the Lane 2 fixture. */
public final class HelloCommand implements CommandExecutor, TabCompleter {

    private final TypicalPlugin plugin;

    public HelloCommand(TypicalPlugin plugin) {
        this.plugin = plugin;
    }

    @Override
    public boolean onCommand(CommandSender sender, Command command, String label, String[] args) {
        String target = args.length > 0 ? args[0] : sender.getName();
        sender.sendMessage("Hello, " + target + "! " + plugin.getConfig().getString("greeting", ""));
        plugin.record("command:executed:label=" + label
                + ":sender=" + sender.getName()
                + ":args=" + String.join(",", args));
        return true;
    }

    @Override
    public List<String> onTabComplete(
            CommandSender sender, Command command, String alias, String[] args) {
        return args.length == 1 ? List.of("world", "ferrite") : List.of();
    }
}
