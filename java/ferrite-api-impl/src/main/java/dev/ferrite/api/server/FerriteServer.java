package dev.ferrite.api.server;

import dev.ferrite.api.FerriteRuntime;
import dev.ferrite.api.inventory.FerriteCustomInventory;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.Locale;
import java.util.Objects;
import java.util.Set;
import java.util.UUID;
import java.util.logging.Logger;
import net.kyori.adventure.key.Key;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import org.bukkit.Material;
import org.bukkit.World;
import org.bukkit.block.data.BlockData;
import org.bukkit.command.CommandMap;
import org.bukkit.command.ConsoleCommandSender;
import org.bukkit.command.PluginCommand;
import org.bukkit.entity.Player;
import org.bukkit.event.inventory.InventoryType;
import org.bukkit.help.HelpMap;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.InventoryHolder;
import org.bukkit.plugin.Plugin;
import org.bukkit.plugin.PluginManager;
import org.bukkit.plugin.messaging.Messenger;
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
    public BlockData createBlockData(Material material) {
        Objects.requireNonNull(material, "material");
        if (!material.isBlock()) {
            throw new IllegalArgumentException("Material " + material + " is not a block");
        }
        String key = material.getKey().asString();
        return (BlockData) Proxy.newProxyInstance(
                BlockData.class.getClassLoader(),
                new Class<?>[] {BlockData.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "getMaterial" -> material;
                    case "getAsString" -> key;
                    case "matches" -> args != null
                            && args.length == 1
                            && args[0] instanceof BlockData other
                            && other.getMaterial() == material;
                    case "clone" -> proxy;
                    case "equals" -> args != null && args.length == 1 && proxy == args[0];
                    case "hashCode" -> material.hashCode();
                    case "toString" -> "FerriteBlockData{" + key + "}";
                    default -> throw new UnsupportedOperationException(
                            "Ferrite BlockData does not support " + method.getName());
                });
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
    public HelpMap getHelpMap() {
        return runtime.helpMap();
    }

    @Override
    public CommandMap getCommandMap() {
        return runtime.commandDispatcher().commandMap();
    }

    @Override
    public Messenger getMessenger() {
        return runtime.messenger();
    }

    @Override
    public void sendPluginMessage(Plugin source, String channel, byte[] message) {
        runtime.messenger().sendPluginMessage(source, null, channel, message);
    }

    @Override
    public Set<String> getListeningPluginChannels() {
        return runtime.messenger().getIncomingChannels();
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

    @Override
    public Inventory createInventory(InventoryHolder owner, InventoryType type) {
        return FerriteCustomInventory.createTyped(runtime, owner, type, null);
    }

    @Override
    public Inventory createInventory(InventoryHolder owner, InventoryType type, Component title) {
        return FerriteCustomInventory.createTyped(runtime, owner, type, title);
    }

    @Override
    public Inventory createInventory(InventoryHolder owner, InventoryType type, String title) {
        Component comp = title == null
                ? null
                : LegacyComponentSerializer.legacySection().deserialize(title);
        return FerriteCustomInventory.createTyped(runtime, owner, type, comp);
    }

    @Override
    public Inventory createInventory(InventoryHolder owner, int size) {
        return FerriteCustomInventory.createChest(runtime, owner, size, null);
    }

    @Override
    public Inventory createInventory(InventoryHolder owner, int size, Component title) {
        return FerriteCustomInventory.createChest(runtime, owner, size, title);
    }

    @Override
    public Inventory createInventory(InventoryHolder owner, int size, String title) {
        Component comp = title == null
                ? null
                : LegacyComponentSerializer.legacySection().deserialize(title);
        return FerriteCustomInventory.createChest(runtime, owner, size, comp);
    }
}
