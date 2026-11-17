// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.server.level;

import dev.ferrite.nms.NmsUnsupported;
import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.NmsHostException;
import dev.ferrite.nms.host.PlayerSnapshot;
import dev.ferrite.nms.host.PlayerState;
import java.util.List;
import java.util.Locale;
import java.util.Objects;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntitySnapshotFactory;
import net.minecraft.world.entity.HostStateReader;
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
        return new ServerPlayer(HostStateReader.readPlayer(playerId));
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

    /** The optional player-state block, or null when the host did not publish one. */
    public PlayerState playerState() {
        return player.state();
    }

    public int ping() {
        return requirePlayerState("ping").ping();
    }

    public String locale() {
        return requirePlayerState("locale").locale();
    }

    public float saturation() {
        return requirePlayerState("saturation").saturation();
    }

    public float exhaustion() {
        return requirePlayerState("exhaustion").exhaustion();
    }

    public int experienceLevel() {
        return requirePlayerState("experienceLevel").level();
    }

    public float experienceProgress() {
        return requirePlayerState("experienceProgress").experienceProgress();
    }

    public int totalExperience() {
        return requirePlayerState("totalExperience").totalExperience();
    }

    public float walkSpeed() {
        return requirePlayerState("walkSpeed").walkSpeed();
    }

    public float flySpeed() {
        return requirePlayerState("flySpeed").flySpeed();
    }

    public boolean mayFly() {
        return requirePlayerState("mayFly").allowFlight();
    }

    public boolean isFlying() {
        return requirePlayerState("isFlying").flying();
    }

    public boolean isSneaking() {
        return requirePlayerState("isSneaking").sneaking();
    }

    public boolean isSprinting() {
        return requirePlayerState("isSprinting").sprinting();
    }

    public int getRemainingAir() {
        return requirePlayerState("getRemainingAir").remainingAir();
    }

    public int getMaximumAir() {
        return requirePlayerState("getMaximumAir").maximumAir();
    }

    /** Bukkit display name; falls back to the scoreboard name when the host omitted one. */
    public String displayName() {
        PlayerState state = player.state();
        return state == null || state.displayName() == null ? getName() : state.displayName();
    }

    /** Bukkit player-list name; falls back to the scoreboard name when the host omitted one. */
    public String playerListName() {
        PlayerState state = player.state();
        return state == null || state.playerListName() == null ? getName() : state.playerListName();
    }

    public long playerTime() {
        return requirePlayerState("playerTime").playerTime();
    }

    public long playerTimeOffset() {
        return requirePlayerState("playerTimeOffset").playerTimeOffset();
    }

    public boolean isPlayerTimeRelative() {
        return requirePlayerState("isPlayerTimeRelative").playerTimeRelative();
    }

    /** Whether the host still lists this player as online. */
    public boolean isOnline() {
        if (player.entityState() != null && player.entityState().removed()) {
            return false;
        }
        try {
            HostStateReader.readPlayer(getUUID());
            return true;
        } catch (NmsHostException unknown) {
            if ("unknown_player".equals(unknown.code())
                    || "player_not_found".equals(unknown.code())) {
                return false;
            }
            throw unknown;
        }
    }

    @Override
    public boolean isValid() {
        return isOnline();
    }

    private PlayerState requirePlayerState(String accessor) {
        PlayerState state = player.state();
        if (state == null) {
            throw NmsUnsupported.operation(
                    "net.minecraft.server.level.ServerPlayer",
                    accessor,
                    "the host snapshot did not include PlayerState; publish the optional PlayerState "
                            + "block from NmsHost.player(UUID) to serve player-state reads");
        }
        return state;
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

    public void setGameMode(GameMode gameMode) {
        throw NmsUnsupported.operation(
                "net.minecraft.server.level.ServerPlayer",
                "setGameMode",
                "synchronous game-mode mutation is not allowed across regions; use setGameModeAsync(GameMode)");
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

    public void setSaturation(float saturation) {
        throw playerStateWriteRefused("setSaturation");
    }

    public void setExhaustion(float exhaustion) {
        throw playerStateWriteRefused("setExhaustion");
    }

    public void setExperienceLevel(int level) {
        throw playerStateWriteRefused("setExperienceLevel");
    }

    public void setExperienceProgress(float progress) {
        throw playerStateWriteRefused("setExperienceProgress");
    }

    public void setTotalExperience(int totalExperience) {
        throw playerStateWriteRefused("setTotalExperience");
    }

    public void setWalkSpeed(float speed) {
        throw playerStateWriteRefused("setWalkSpeed");
    }

    public void setFlySpeed(float speed) {
        throw playerStateWriteRefused("setFlySpeed");
    }

    public void setMayFly(boolean mayFly) {
        throw playerStateWriteRefused("setMayFly");
    }

    public void setFlying(boolean flying) {
        throw playerStateWriteRefused("setFlying");
    }

    public void setSneaking(boolean sneaking) {
        throw playerStateWriteRefused("setSneaking");
    }

    public void setSprinting(boolean sprinting) {
        throw playerStateWriteRefused("setSprinting");
    }

    public void setRemainingAir(int air) {
        throw playerStateWriteRefused("setRemainingAir");
    }

    public void setMaximumAir(int air) {
        throw playerStateWriteRefused("setMaximumAir");
    }

    public CompletableFuture<Void> setFoodLevelAsync(int foodLevel) {
        if (foodLevel < 0 || foodLevel > 20) {
            throw new IllegalArgumentException("foodLevel must be within 0..20");
        }
        return NmsHostBinding.require().setFoodLevel(getUUID(), foodLevel);
    }

    private static UnsupportedOperationException playerStateWriteRefused(String method) {
        return NmsUnsupported.operation(
                "net.minecraft.server.level.ServerPlayer",
                method,
                "live player-state mutation is not in NMS surface 2's host write contract; add an "
                        + "owner-routed host write op and an Async method, or use Lane 2's Bukkit Player "
                        + "facade for state it owns");
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
