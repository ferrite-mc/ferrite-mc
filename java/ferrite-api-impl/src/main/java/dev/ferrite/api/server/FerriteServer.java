package dev.ferrite.api.server;

import dev.ferrite.api.FerriteRuntime;
import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.Locale;
import java.util.UUID;
import java.util.logging.Logger;
import net.kyori.adventure.key.Key;
import net.kyori.adventure.text.Component;
import org.bukkit.World;
import org.bukkit.command.ConsoleCommandSender;
import org.bukkit.command.PluginCommand;
import org.bukkit.entity.Player;
import org.bukkit.plugin.PluginManager;
import org.bukkit.scheduler.BukkitScheduler;

/**
 * The first {@link org.bukkit.Server} slice: identity, worlds, players, manager,
 * scheduler, console and command lookup.
 *
 * <p>Every other Server method inherits {@link UnsupportedServerBase} and fails with an
 * {@code UnsupportedOperationException} naming the method and plugin.
 */
public final class FerriteServer extends UnsupportedServerBase {

    private final FerriteRuntime runtime;

    public FerriteServer(FerriteRuntime runtime) {
        this.runtime = runtime;
    }

    @Override
    public String getName() {
        return runtime.serverName();
    }

    @Override
    public String getVersion() {
        return runtime.version();
    }

    @Override
    public String getBukkitVersion() {
        return runtime.bukkitVersion();
    }

    @Override
    public String getMinecraftVersion() {
        return runtime.minecraftVersion();
    }

    @Override
    public Logger getLogger() {
        return runtime.logger();
    }

    @Override
    public PluginManager getPluginManager() {
        return runtime.plugins();
    }

    @Override
    public BukkitScheduler getScheduler() {
        return runtime.scheduler();
    }

    @Override
    public List<World> getWorlds() {
        return new ArrayList<World>(runtime.worlds());
    }

    @Override
    public World getWorld(String name) {
        return runtime.world(name);
    }

    @Override
    public World getWorld(UUID uniqueId) {
        return runtime.world(uniqueId);
    }

    @Override
    public World getWorld(Key key) {
        String value = key.value();
        int colon = value.indexOf(':');
        return runtime.world(colon >= 0 ? value.substring(colon + 1) : value);
    }

    @Override
    public Player getPlayer(String name) {
        if (name == null) {
            return null;
        }
        String lower = name.toLowerCase(Locale.ROOT);
        for (Player player : runtime.onlinePlayers()) {
            if (player.getName().toLowerCase(Locale.ROOT).startsWith(lower)) {
                return player;
            }
        }
        return null;
    }

    @Override
    public Player getPlayerExact(String name) {
        return runtime.playerExact(name);
    }

    @Override
    public Player getPlayer(UUID uniqueId) {
        return runtime.player(uniqueId);
    }

    @Override
    public UUID getPlayerUniqueId(String name) {
        Player player = runtime.playerExact(name);
        return player == null ? null : player.getUniqueId();
    }

    @Override
    public Collection<? extends Player> getOnlinePlayers() {
        return List.copyOf(runtime.onlinePlayers());
    }

    @Override
    public int getMaxPlayers() {
        return runtime.maxPlayers();
    }

    @Override
    public int getPort() {
        return runtime.port();
    }

    @Override
    public boolean isPrimaryThread() {
        return runtime.isPrimaryThread();
    }

    @Override
    public ConsoleCommandSender getConsoleSender() {
        return runtime.consoleSender();
    }

    @Override
    public PluginCommand getPluginCommand(String name) {
        return runtime.commandDispatcher().get(name);
    }

    @Override
    public Component permissionMessage() {
        return Component.text(
                "I'm sorry, but you do not have permission to perform this command. "
                        + "Please contact the server administrators if you believe that this is in error.");
    }

    @Override
    public int broadcastMessage(String message) {
        int count = 0;
        for (Player player : runtime.onlinePlayers()) {
            player.sendMessage(message);
            count++;
        }
        runtime.consoleSender().sendMessage(message);
        return count;
    }
}
