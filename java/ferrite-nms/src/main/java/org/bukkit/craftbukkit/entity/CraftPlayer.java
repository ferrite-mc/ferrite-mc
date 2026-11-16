// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package org.bukkit.craftbukkit.entity;

import dev.ferrite.nms.NmsUnsupported;
import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.NmsHostException;
import dev.ferrite.nms.host.PlayerSnapshot;
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
    public String getDisplayName() {
        return getHandle().displayName();
    }

    @Override
    public String getPlayerListName() {
        return getHandle().playerListName();
    }

    @Override
    public boolean isOnline() {
        try {
            PlayerSnapshot snapshot = NmsHostBinding.require().player(getUniqueId());
            return snapshot.entityState() == null || !snapshot.entityState().removed();
        } catch (NmsHostException unknown) {
            if ("unknown_player".equals(unknown.code())) {
                return false;
            }
            throw unknown;
        }
    }

    @Override
    public boolean isValid() {
        return isOnline();
    }

    @Override
    public int getPing() {
        return getHandle().ping();
    }

    @Override
    public String getLocale() {
        return getHandle().locale();
    }

    @Override
    public float getSaturation() {
        return getHandle().saturation();
    }

    @Override
    public float getExhaustion() {
        return getHandle().exhaustion();
    }

    @Override
    public int getLevel() {
        return getHandle().experienceLevel();
    }

    @Override
    public float getExp() {
        return getHandle().experienceProgress();
    }

    @Override
    public int getTotalExperience() {
        return getHandle().totalExperience();
    }

    @Override
    public float getWalkSpeed() {
        return getHandle().walkSpeed();
    }

    @Override
    public float getFlySpeed() {
        return getHandle().flySpeed();
    }

    @Override
    public boolean getAllowFlight() {
        return getHandle().mayFly();
    }

    @Override
    public boolean isFlying() {
        return getHandle().isFlying();
    }

    @Override
    public boolean isSneaking() {
        return getHandle().isSneaking();
    }

    @Override
    public boolean isSprinting() {
        return getHandle().isSprinting();
    }

    @Override
    public int getRemainingAir() {
        return getHandle().getRemainingAir();
    }

    @Override
    public int getMaximumAir() {
        return getHandle().getMaximumAir();
    }

    @Override
    public long getPlayerTime() {
        return getHandle().playerTime();
    }

    @Override
    public long getPlayerTimeOffset() {
        return getHandle().playerTimeOffset();
    }

    @Override
    public boolean isPlayerTimeRelative() {
        return getHandle().isPlayerTimeRelative();
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

    @Override
    public void setDisplayName(String name) {
        throw playerStateWriteRefused("setDisplayName");
    }

    @Override
    public void setPlayerListName(String name) {
        throw playerStateWriteRefused("setPlayerListName");
    }

    @Override
    public void setSneaking(boolean sneaking) {
        throw playerStateWriteRefused("setSneaking");
    }

    @Override
    public void setSprinting(boolean sprinting) {
        throw playerStateWriteRefused("setSprinting");
    }

    @Override
    public void setFlying(boolean flying) {
        throw playerStateWriteRefused("setFlying");
    }

    @Override
    public void setAllowFlight(boolean flight) {
        throw playerStateWriteRefused("setAllowFlight");
    }

    @Override
    public void setWalkSpeed(float speed) {
        throw playerStateWriteRefused("setWalkSpeed");
    }

    @Override
    public void setFlySpeed(float speed) {
        throw playerStateWriteRefused("setFlySpeed");
    }

    @Override
    public void setLevel(int level) {
        throw playerStateWriteRefused("setLevel");
    }

    @Override
    public void setExp(float exp) {
        throw playerStateWriteRefused("setExp");
    }

    @Override
    public void setTotalExperience(int exp) {
        throw playerStateWriteRefused("setTotalExperience");
    }

    @Override
    public void setSaturation(float value) {
        throw playerStateWriteRefused("setSaturation");
    }

    @Override
    public void setExhaustion(float value) {
        throw playerStateWriteRefused("setExhaustion");
    }

    @Override
    public void setRemainingAir(int ticks) {
        throw playerStateWriteRefused("setRemainingAir");
    }

    @Override
    public void setMaximumAir(int ticks) {
        throw playerStateWriteRefused("setMaximumAir");
    }

    @Override
    public void setPlayerTime(long time, boolean relative) {
        throw playerStateWriteRefused("setPlayerTime");
    }

    @Override
    public void resetPlayerTime() {
        throw playerStateWriteRefused("resetPlayerTime");
    }

    private static UnsupportedOperationException playerStateWriteRefused(String method) {
        return NmsUnsupported.operation(
                "org.bukkit.craftbukkit.entity.CraftPlayer",
                method,
                "live player-state mutation is not in NMS surface 2's host write contract; add an "
                        + "owner-routed host write op and an Async method, or use Lane 2's Bukkit Player "
                        + "facade for state it owns");
    }
}
