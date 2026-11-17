package dev.ferrite.api.entity;

import dev.ferrite.api.FerriteRuntime;
import dev.ferrite.api.internal.Unsupported;
import dev.ferrite.api.inventory.FerriteCustomInventory;
import dev.ferrite.api.inventory.FerriteInventoryView;
import dev.ferrite.api.world.FerriteWorld;
import java.util.Objects;
import java.util.UUID;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import org.bukkit.GameMode;
import org.bukkit.Location;
import org.bukkit.Server;
import org.bukkit.World;
import org.bukkit.entity.Player;
import org.bukkit.event.inventory.InventoryCloseEvent;
import org.bukkit.event.inventory.InventoryOpenEvent;
import org.bukkit.event.player.PlayerKickEvent;
import org.bukkit.event.player.PlayerTeleportEvent;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.InventoryView;
import org.bukkit.inventory.PlayerInventory;
import org.bukkit.permissions.Permission;

/**
 * First {@link org.bukkit.entity.Player} slice: identity, world/location, messages,
 * permission fallback, game mode, health/food and the basic inventory.
 */
public final class FerritePlayer extends UnsupportedPlayerBase {

    private final FerriteRuntime runtime;
    private final PlayerState state;
    private final FerritePlayerInventory inventory;
    private FerriteInventoryView openView;

    public FerritePlayer(FerriteRuntime runtime, PlayerState state) {
        this.runtime = runtime;
        this.state = state;
        this.inventory = new FerritePlayerInventory(this);
        state.setInventory(inventory);
    }

    public PlayerState state() {
        return state;
    }

    @Override
    public String getName() {
        return state.name();
    }

    @Override
    public UUID getUniqueId() {
        return state.uniqueId();
    }

    @Override
    public boolean isOnline() {
        return state.online();
    }

    @Override
    public Server getServer() {
        return runtime.server();
    }

    @Override
    public World getWorld() {
        return state.world();
    }

    @Override
    public Location getLocation() {
        return new Location(state.world(), state.x(), state.y(), state.z(), state.yaw(), state.pitch());
    }

    @Override
    public boolean teleport(Location location) {
        return teleport(location, PlayerTeleportEvent.TeleportCause.PLUGIN);
    }

    @Override
    public boolean teleport(Location location, PlayerTeleportEvent.TeleportCause cause) {
        if (location == null || location.getWorld() == null) {
            throw new IllegalArgumentException("Teleport location and world must not be null");
        }
        FerriteWorld world = runtime.world(location.getWorld().getName());
        if (world == null) {
            throw new IllegalArgumentException("Unknown Ferrite world: " + location.getWorld().getName());
        }
        state.setWorld(world);
        state.setPosition(location.getX(), location.getY(), location.getZ(), location.getYaw(), location.getPitch());
        return true;
    }

    @Override
    public void sendMessage(String message) {
        state.messages().add(message);
        runtime.logger().info("[" + state.name() + "] " + message);
    }

    @Override
    public void sendMessage(String... messages) {
        for (String message : messages) {
            sendMessage(message);
        }
    }

    @Override
    public void sendMessage(UUID sender, String message) {
        sendMessage(message);
    }

    @Override
    public void sendMessage(UUID sender, String... messages) {
        sendMessage(messages);
    }

    @Override
    public Component name() {
        return Component.text(state.name());
    }

    @Override
    public boolean isPermissionSet(String permission) {
        return false;
    }

    @Override
    public boolean isPermissionSet(Permission permission) {
        return false;
    }

    @Override
    public boolean hasPermission(String permission) {
        return runtime.permissionResolver()
                .hasPermission(state.name(), state.uniqueId(), state.op(), permission);
    }

    @Override
    public boolean hasPermission(Permission permission) {
        return hasPermission(permission.getName());
    }

    @Override
    public boolean isOp() {
        return state.op();
    }

    @Override
    public void setOp(boolean op) {
        state.setOp(op);
    }

    @Override
    public GameMode getGameMode() {
        return state.gameMode();
    }

    @Override
    public void setGameMode(GameMode gameMode) {
        state.setGameMode(gameMode);
    }

    @Override
    public double getHealth() {
        return state.health();
    }

    @Override
    public void setHealth(double health) {
        if (health < 0 || health > 20) {
            throw new IllegalArgumentException("health must be between 0 and 20, was " + health);
        }
        state.setHealth(health);
    }

    @Override
    public int getFoodLevel() {
        return state.foodLevel();
    }

    @Override
    public void setFoodLevel(int foodLevel) {
        if (foodLevel < 0 || foodLevel > 20) {
            throw new IllegalArgumentException("food level must be between 0 and 20, was " + foodLevel);
        }
        state.setFoodLevel(foodLevel);
    }

    @Override
    public PlayerInventory getInventory() {
        return inventory;
    }

    @Override
    public InventoryView getOpenInventory() {
        if (openView != null && !openView.isClosed()) {
            return openView;
        }
        return new FerriteInventoryView(
                runtime, this, inventory, inventory, Component.text("Crafting"));
    }

    @Override
    public InventoryView openInventory(Inventory topInventory) {
        Objects.requireNonNull(topInventory, "inventory");
        Component title = topInventory instanceof FerriteCustomInventory custom
                ? custom.title()
                : Component.text(topInventory.getType().getDefaultTitle());
        FerriteInventoryView view =
                new FerriteInventoryView(runtime, this, topInventory, inventory, title);
        openInventory(view);
        return openView == view ? view : getOpenInventory();
    }

    @Override
    public void openInventory(InventoryView view) {
        Objects.requireNonNull(view, "view");
        if (!runtime.isPrimaryThread()) {
            throw new IllegalStateException("Player#openInventory must run on the main thread");
        }
        if (!(view instanceof FerriteInventoryView ferriteView)) {
            throw Unsupported.exception("Player#openInventory(custom InventoryView)");
        }
        if (openView != null && !openView.isClosed()) {
            openView.close(InventoryCloseEvent.Reason.OPEN_NEW);
        }
        InventoryOpenEvent openEvent = new InventoryOpenEvent(ferriteView);
        runtime.plugins().callEvent(openEvent);
        if (openEvent.isCancelled()) {
            return;
        }
        this.openView = ferriteView;
        if (ferriteView.getTopInventory() instanceof FerriteCustomInventory custom) {
            custom.addViewer(this);
        }
    }

    @Override
    public void closeInventory() {
        closeInventory(InventoryCloseEvent.Reason.PLUGIN);
    }

    @Override
    public void closeInventory(InventoryCloseEvent.Reason reason) {
        Objects.requireNonNull(reason, "reason");
        if (!runtime.isPrimaryThread()) {
            throw new IllegalStateException("Player#closeInventory must run on the main thread");
        }
        if (openView != null && !openView.isClosed()) {
            openView.close(reason);
        }
        this.openView = null;
    }

    /** Clears {@code openView} when {@link FerriteInventoryView#close} completes. */
    public void clearOpenViewIf(FerriteInventoryView view) {
        if (this.openView == view) {
            this.openView = null;
        }
    }

    @Override
    public void kickPlayer(String message) {
        kick(Component.text(message == null ? "" : message), PlayerKickEvent.Cause.PLUGIN);
    }

    @Override
    public void kick(Component message, PlayerKickEvent.Cause cause) {
        String legacy = message == null ? "" : LegacyComponentSerializer.legacySection().serialize(message);
        state.kick(legacy);
        state.setOnline(false);
        runtime.logger().info("[" + state.name() + "] kicked: " + legacy);
    }

    @Override
    public Player.Spigot spigot() {
        throw Unsupported.exception("Player#spigot");
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof FerritePlayer other && other.state.uniqueId().equals(state.uniqueId());
    }

    @Override
    public int hashCode() {
        return state.uniqueId().hashCode();
    }

    @Override
    public String toString() {
        return "FerritePlayer{" + state.name() + "}";
    }
}
