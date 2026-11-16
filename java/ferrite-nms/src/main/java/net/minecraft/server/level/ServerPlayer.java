// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.server.level;

import dev.ferrite.nms.NmsUnsupported;
import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.PlayerSnapshot;
import java.util.List;
import java.util.Locale;
import java.util.Objects;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntitySnapshotFactory;
import org.bukkit.GameMode;

/** Snapshot handle to an online player; reads are values and writes are owner-routed. */
public final class ServerPlayer extends Entity {
    private final PlayerSnapshot player;

    private ServerPlayer(PlayerSnapshot player) {
        super(EntitySnapshotFactory.forPlayer(player));
        this.player = Objects.requireNonNull(player, "player");
    }

    public static ServerPlayer of(UUID playerId) {
        Objects.requireNonNull(playerId, "playerId");
        return new ServerPlayer(NmsHostBinding.require().player(playerId));
    }

    public String getName() {
        return player.name();
    }

    public String getScoreboardName() {
        return player.name();
    }

    public String gameMode() {
        return player.gameMode();
    }

    public GameMode bukkitGameMode() {
        try {
            return GameMode.valueOf(player.gameMode().trim().toUpperCase(Locale.ROOT));
        } catch (IllegalArgumentException invalid) {
            throw new IllegalStateException(
                    "Host returned unknown 26.3 game mode: " + player.gameMode(), invalid);
        }
    }

    public float getHealth() {
        return player.health();
    }

    public int getFoodLevel() {
        return player.foodLevel();
    }

    /** The immutable host snapshot behind this handle. */
    public PlayerSnapshot playerSnapshot() {
        return player;
    }

    public CompletableFuture<Void> setGameModeAsync(GameMode gameMode) {
        Objects.requireNonNull(gameMode, "gameMode");
        return setGameModeAsync(gameMode.name());
    }

    public CompletableFuture<Void> setGameModeAsync(String gameMode) {
        Objects.requireNonNull(gameMode, "gameMode");
        if (gameMode.isBlank()) {
            throw new IllegalArgumentException("gameMode must not be blank");
        }
        GameMode parsed;
        try {
            parsed = GameMode.valueOf(gameMode.trim().toUpperCase(Locale.ROOT));
        } catch (IllegalArgumentException invalid) {
            throw new IllegalArgumentException("unknown 26.3 gameMode: " + gameMode, invalid);
        }
        return NmsHostBinding.require().setGameMode(getUUID(), parsed.name());
    }

    public CompletableFuture<Void> setHealthAsync(float health) {
        if (!Float.isFinite(health) || health < 0.0F) {
            throw new IllegalArgumentException("health must be a finite non-negative value");
        }
        return NmsHostBinding.require().setHealth(getUUID(), health);
    }

    public void setGameMode(String gameMode) {
        throw NmsUnsupported.operation(
                "net.minecraft.server.level.ServerPlayer",
                "setGameMode",
                "synchronous game-mode mutation is not allowed across regions; use setGameModeAsync(String)");
    }

    public void setHealth(float health) {
        throw NmsUnsupported.operation(
                "net.minecraft.server.level.ServerPlayer",
                "setHealth",
                "synchronous health mutation is not allowed across regions; use setHealthAsync(float)");
    }

    public Object getConnection() {
        throw NmsUnsupported.operation(
                "net.minecraft.server.level.ServerPlayer",
                "getConnection",
                "ServerGamePacketListenerImpl and live packet connections are not exposed");
    }

    public int openMenu(Object menu) {
        throw NmsUnsupported.operation(
                "net.minecraft.server.level.ServerPlayer",
                "openMenu",
                "NMS menus and container windows are not supported in the first slice");
    }

    /** Convenience for plugins that already hold a passenger list; never a live player object. */
    @Override
    public List<UUID> passengerIds() {
        return entitySnapshot().passengers();
    }

    @Override
    public String toString() {
        return "ServerPlayer{" + getUUID() + ", " + getName() + "}";
    }
}
