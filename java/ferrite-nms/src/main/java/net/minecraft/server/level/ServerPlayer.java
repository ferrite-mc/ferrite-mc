// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.server.level;

import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.PlayerSnapshot;
import java.util.List;
import java.util.Objects;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntitySnapshotFactory;

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

    public String gameMode() {
        return player.gameMode();
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

    public CompletableFuture<Void> setGameModeAsync(String gameMode) {
        Objects.requireNonNull(gameMode, "gameMode");
        if (gameMode.isBlank()) {
            throw new IllegalArgumentException("gameMode must not be blank");
        }
        return NmsHostBinding.require().setGameMode(getUUID(), gameMode);
    }

    public CompletableFuture<Void> setHealthAsync(float health) {
        if (!Float.isFinite(health) || health < 0.0F) {
            throw new IllegalArgumentException("health must be a finite non-negative value");
        }
        return NmsHostBinding.require().setHealth(getUUID(), health);
    }

    /** Convenience for plugins that already hold a passenger list; never a live player object. */
    public List<UUID> passengerIds() {
        return entitySnapshot().passengers();
    }

    @Override
    public String toString() {
        return "ServerPlayer{" + getUUID() + ", " + getName() + "}";
    }
}
