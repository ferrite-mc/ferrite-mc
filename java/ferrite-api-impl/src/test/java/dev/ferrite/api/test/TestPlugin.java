package dev.ferrite.api.test;

import java.util.List;
import java.util.concurrent.CopyOnWriteArrayList;
import org.bukkit.command.Command;
import org.bukkit.command.CommandSender;
import org.bukkit.plugin.java.JavaPlugin;

/** Minimal JavaPlugin used by the facade unit tests. */
public class TestPlugin extends JavaPlugin {

    private final List<String> lifecycle = new CopyOnWriteArrayList<>();
    private final List<String> commands = new CopyOnWriteArrayList<>();

    @Override
    public void onLoad() {
        lifecycle.add("onLoad");
    }

    @Override
    public void onEnable() {
        lifecycle.add("onEnable");
    }

    @Override
    public void onDisable() {
        lifecycle.add("onDisable");
    }

    @Override
    public boolean onCommand(CommandSender sender, Command command, String label, String[] args) {
        commands.add(label + "|" + sender.getName() + "|" + String.join(",", args));
        sender.sendMessage("ran " + label);
        return true;
    }

    public List<String> lifecycle() {
        return lifecycle;
    }

    public List<String> commands() {
        return commands;
    }
}
