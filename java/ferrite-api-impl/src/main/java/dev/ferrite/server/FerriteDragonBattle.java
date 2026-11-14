package dev.ferrite.server;

import java.util.Collection;
import java.util.List;
import java.util.Objects;
import java.util.UUID;
import org.bukkit.Location;
import org.bukkit.World;
import org.bukkit.boss.BarColor;
import org.bukkit.boss.BarStyle;
import org.bukkit.boss.BossBar;
import org.bukkit.boss.DragonBattle;
import org.bukkit.entity.EnderCrystal;
import org.bukkit.entity.EnderDragon;

/**
 * {@code World#getEnderDragonBattle} for the End's fight. The host owns the fight state: reads come
 * from the snapshot the End region publishes each tick, and changes are queued for its next tick,
 * so both only move while a player is in the End. The boss bar is detached: the bar clients see is
 * driven by the host, and changing this object does not change it.
 */
final class FerriteDragonBattle implements DragonBattle {
    private final World world;
    private final UUID id;
    private BossBar bar;

    FerriteDragonBattle(World world) {
        this.world = Objects.requireNonNull(world, "world");
        this.id = world.getUID();
    }

    /** started, killed, gateways, dragon_alive, stage, portal_level. */
    private String[] state() {
        return ((String) HostBridge.call("World", id, "dragonBattle()", String.class)).split(",", -1);
    }

    @Override public EnderDragon getEnderDragon() {
        var dragons = world.getEntitiesByClass(EnderDragon.class);
        return dragons.isEmpty() ? null : dragons.iterator().next();
    }

    @Override public synchronized BossBar getBossBar() {
        if (bar == null) bar = org.bukkit.Bukkit.createBossBar("Ender Dragon", BarColor.PINK, BarStyle.SOLID);
        return bar;
    }

    @Override public Location getEndPortalLocation() {
        String level = state()[5];
        return level.equals("-") ? null : new Location(world, 0, Integer.parseInt(level), 0);
    }

    @Override public boolean generateEndPortal(boolean withPortals) {
        if (state()[5].equals("-") && !hasPodiumFloor()) return false;
        HostBridge.callVoid("World", id, "dragonBattleGeneratePortal(boolean)", withPortals);
        return true;
    }

    private boolean hasPodiumFloor() {
        return world.getHighestBlockYAt(0, 0) > world.getMinHeight();
    }

    @Override public boolean hasBeenPreviouslyKilled() { return Boolean.parseBoolean(state()[1]); }

    @Override public void setPreviouslyKilled(boolean killed) {
        HostBridge.callVoid("World", id, "dragonBattleSetKilled(boolean)", killed);
    }

    @Override public void initiateRespawn() { initiateRespawn(List.of()); }

    @Override public boolean initiateRespawn(Collection<EnderCrystal> crystals) {
        Objects.requireNonNull(crystals, "crystals");
        var state = state();
        if (!Boolean.parseBoolean(state[1]) || Boolean.parseBoolean(state[3]) || !state[4].equals("none")) return false;
        HostBridge.callVoid("World", id, "dragonBattleInitiateRespawn()");
        return true;
    }

    @Override public RespawnPhase getRespawnPhase() {
        var stage = state()[4];
        return stage.equals("none") ? RespawnPhase.NONE : RespawnPhase.valueOf(stage.toUpperCase(java.util.Locale.ROOT));
    }

    @Override public boolean setRespawnPhase(RespawnPhase phase) {
        Objects.requireNonNull(phase, "phase");
        HostBridge.callVoid("World", id, "dragonBattleSetPhase(String)", phase.name().toLowerCase(java.util.Locale.ROOT));
        return true;
    }

    @Override public void resetCrystals() {
        throw new UnsupportedOperationException("Ferrite: DragonBattle#resetCrystals is not implemented: the host restores spike crystals only as part of a respawn");
    }

    @Override public int getGatewayCount() { return Integer.parseInt(state()[2]); }

    @Override public boolean spawnNewGateway() {
        if (getGatewayCount() >= 20) return false;
        HostBridge.callVoid("World", id, "dragonBattleSpawnGateway()");
        return true;
    }

    @Override public void spawnNewGateway(io.papermc.paper.math.Position position) {
        throw new UnsupportedOperationException("Ferrite: DragonBattle#spawnNewGateway(Position) is not implemented: gateways open only on the vanilla ring");
    }

    /** Crystals that stand around the exit portal (the four a respawn is started from). */
    @Override public List<EnderCrystal> getRespawnCrystals() {
        var level = getEndPortalLocation();
        if (level == null) return List.of();
        return world.getEntitiesByClass(EnderCrystal.class).stream()
            .filter(c -> Math.abs(c.getLocation().getX()) <= 4 && Math.abs(c.getLocation().getZ()) <= 4
                && Math.abs(c.getLocation().getY() - level.getY()) <= 8)
            .toList();
    }

    /** Crystals the dragon can heal from: those within the radius vanilla searches (32 blocks). */
    @Override public List<EnderCrystal> getHealingCrystals() {
        return world.getEntitiesByClass(EnderCrystal.class).stream()
            .filter(c -> c.getLocation().distanceSquared(new Location(world, 0, c.getLocation().getY(), 0)) <= 100 * 100)
            .toList();
    }
}
