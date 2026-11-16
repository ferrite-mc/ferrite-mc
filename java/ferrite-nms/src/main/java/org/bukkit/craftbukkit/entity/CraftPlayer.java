// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package org.bukkit.craftbukkit.entity;

import dev.ferrite.nms.NmsUnsupported;
import net.minecraft.server.level.ServerPlayer;
import org.bukkit.GameMode;

/**
 * Compatibility base for player handles; reads resolve immutable host snapshots and synchronous
 * cross-region writes fail explicitly in favor of {@link ServerPlayer} async writes.
 */
public abstract class CraftPlayer extends CraftEntity implements org.bukkit.entity.Player {
    protected CraftPlayer() {}

    @Override
    public ServerPlayer getHandle() {
        return ServerPlayer.of(getUniqueId());
    }

    @Override
    public String getName() {
        return getHandle().getName();
    }

    @Override
    public GameMode getGameMode() {
        return getHandle().bukkitGameMode();
    }

    @Override
    public void setGameMode(GameMode mode) {
        throw NmsUnsupported.operation(
                "org.bukkit.craftbukkit.entity.CraftPlayer",
                "setGameMode",
                "synchronous game-mode mutation is not allowed across regions; use getHandle().setGameModeAsync(GameMode)");
    }

    @Override
    public double getHealth() {
        return getHandle().getHealth();
    }

    @Override
    public void setHealth(double health) {
        throw NmsUnsupported.operation(
                "org.bukkit.craftbukkit.entity.CraftPlayer",
                "setHealth",
                "synchronous health mutation is not allowed across regions; use getHandle().setHealthAsync(float)");
    }

    @Override
    public int getFoodLevel() {
        return getHandle().getFoodLevel();
    }

    @Override
    public void setFoodLevel(int value) {
        throw NmsUnsupported.operation(
                "org.bukkit.craftbukkit.entity.CraftPlayer",
                "setFoodLevel",
                "food-level mutation is not in the first host write contract");
    }
}
