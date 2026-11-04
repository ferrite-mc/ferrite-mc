package dev.ferrite.server;

import java.util.*;
import org.bukkit.FluidCollisionMode;
import org.bukkit.Material;
import org.bukkit.block.Block;
import org.bukkit.block.BlockFace;
import org.bukkit.util.RayTraceResult;
import org.bukkit.util.Vector;

/** Ray tracing, names, sounds and chat answered from the eye position and the block world. */
@SuppressWarnings({"deprecation", "unchecked", "rawtypes"})
public abstract class PlayerSight extends PlayerStubs {
    private net.kyori.adventure.text.Component customName;
    private net.kyori.adventure.text.Component listName;

    // ---- names -------------------------------------------------------------------------------
    @Override public net.kyori.adventure.text.Component name() { return net.kyori.adventure.text.Component.text(getName()); }
    @Override public net.kyori.adventure.text.Component customName() { return customName; }
    @Override public void customName(net.kyori.adventure.text.Component name) { customName = name; }
    @Override public net.kyori.adventure.text.Component playerListName() { return listName != null ? listName : name(); }
    @Override public void playerListName(net.kyori.adventure.text.Component name) { listName = name; }
    @Override public net.kyori.adventure.text.Component teamDisplayName() {
        var team = getScoreboard().getEntryTeam(getName());
        return team == null ? name() : team.displayName().append(name());
    }
    @Override public net.kyori.adventure.sound.Sound.Source soundSource() { return net.kyori.adventure.sound.Sound.Source.PLAYER; }
    @Override public org.bukkit.scoreboard.Scoreboard getScoreboard() { return org.bukkit.Bukkit.getScoreboardManager().getMainScoreboard(); }

    // ---- sounds ------------------------------------------------------------------------------
    @Override public org.bukkit.Sound getHurtSound() { return org.bukkit.Sound.ENTITY_PLAYER_HURT; }
    @Override public org.bukkit.Sound getHurtSound(org.bukkit.damage.DamageSource source) { return org.bukkit.Sound.ENTITY_PLAYER_HURT; }
    @Override public org.bukkit.Sound getDeathSound() { return org.bukkit.Sound.ENTITY_PLAYER_DEATH; }
    @Override public org.bukkit.Sound getFallDamageSound(int fallHeight) { return fallHeight > 4 ? getFallDamageSoundBig() : getFallDamageSoundSmall(); }
    @Override public org.bukkit.Sound getFallDamageSoundBig() { return org.bukkit.Sound.ENTITY_PLAYER_BIG_FALL; }
    @Override public org.bukkit.Sound getFallDamageSoundSmall() { return org.bukkit.Sound.ENTITY_PLAYER_SMALL_FALL; }
    @Override public org.bukkit.Sound getSwimSound() { return org.bukkit.Sound.ENTITY_PLAYER_SWIM; }
    @Override public org.bukkit.Sound getSwimSplashSound() { return org.bukkit.Sound.ENTITY_PLAYER_SPLASH; }
    @Override public org.bukkit.Sound getSwimHighSpeedSplashSound() { return org.bukkit.Sound.ENTITY_PLAYER_SPLASH_HIGH_SPEED; }
    @Override public org.bukkit.Sound getDrinkingSound(org.bukkit.inventory.ItemStack item) { Objects.requireNonNull(item, "item"); return org.bukkit.Sound.ENTITY_GENERIC_DRINK; }
    @Override public org.bukkit.Sound getEatingSound(org.bukkit.inventory.ItemStack item) { Objects.requireNonNull(item, "item"); return org.bukkit.Sound.ENTITY_GENERIC_EAT; }

    // ---- combat cooldown ---------------------------------------------------------------------
    @Override public float getCooledAttackStrength(float adjustTicks) { return 1.0f; }

    // ---- chat --------------------------------------------------------------------------------
    @Override public void chat(String message) {
        Objects.requireNonNull(message, "message");
        if (message.startsWith("/")) { performCommand(message.substring(1)); return; }
        String result = Bootstrap.fireChat(getUniqueId().toString(), getName(), message);
        if (result != null) org.bukkit.Bukkit.broadcastMessage("<" + getName() + "> " + result);
    }

    // ---- ray tracing -------------------------------------------------------------------------
    private record Hit(Block block, BlockFace face, Vector at) {}

    private static boolean isFluid(Material m) { return m == Material.WATER || m == Material.LAVA; }

    /** Walks the eye ray block by block (3D DDA) and returns each block entered. */
    private List<Block> walk(int max) {
        var eye = getEyeLocation();
        var world = eye.getWorld();
        var dir = eye.getDirection().normalize();
        var blocks = new ArrayList<Block>();
        int x = eye.getBlockX(), y = eye.getBlockY(), z = eye.getBlockZ();
        int sx = dir.getX() > 0 ? 1 : -1, sy = dir.getY() > 0 ? 1 : -1, sz = dir.getZ() > 0 ? 1 : -1;
        double tx = dir.getX() == 0 ? Double.MAX_VALUE : ((sx > 0 ? x + 1 : x) - eye.getX()) / dir.getX();
        double ty = dir.getY() == 0 ? Double.MAX_VALUE : ((sy > 0 ? y + 1 : y) - eye.getY()) / dir.getY();
        double tz = dir.getZ() == 0 ? Double.MAX_VALUE : ((sz > 0 ? z + 1 : z) - eye.getZ()) / dir.getZ();
        double dx = dir.getX() == 0 ? Double.MAX_VALUE : Math.abs(1 / dir.getX());
        double dy = dir.getY() == 0 ? Double.MAX_VALUE : Math.abs(1 / dir.getY());
        double dz = dir.getZ() == 0 ? Double.MAX_VALUE : Math.abs(1 / dir.getZ());
        blocks.add(world.getBlockAt(x, y, z));
        double t;
        while (true) {
            if (tx <= ty && tx <= tz) { x += sx; t = tx; tx += dx; }
            else if (ty <= tz) { y += sy; t = ty; ty += dy; }
            else { z += sz; t = tz; tz += dz; }
            if (t > max) break;
            blocks.add(world.getBlockAt(x, y, z));
        }
        return blocks;
    }

    private Hit firstHit(int max, FluidCollisionMode mode) {
        Block previous = null;
        for (Block block : walk(max)) {
            var type = block.getType();
            boolean fluid = isFluid(type);
            boolean solid = !type.isAir() && !fluid;
            if (solid || (fluid && mode == FluidCollisionMode.ALWAYS)) {
                BlockFace face = BlockFace.SELF;
                if (previous != null) {
                    var found = block.getFace(previous);
                    if (found != null) face = found;
                }
                return new Hit(block, face, new Vector(block.getX() + 0.5, block.getY() + 0.5, block.getZ() + 0.5));
            }
            previous = block;
        }
        return null;
    }

    private static FluidCollisionMode mode(com.destroystokyo.paper.block.TargetBlockInfo.FluidMode mode) {
        return switch (mode) {
            case NEVER -> FluidCollisionMode.NEVER;
            case SOURCE_ONLY -> FluidCollisionMode.SOURCE_ONLY;
            case ALWAYS -> FluidCollisionMode.ALWAYS;
        };
    }

    @Override public Block getTargetBlockExact(int max, FluidCollisionMode mode) {
        if (max < 0) throw new IllegalArgumentException("maxDistance must be non-negative");
        var hit = firstHit(max, mode);
        return hit == null ? null : hit.block();
    }
    @Override public Block getTargetBlock(int max, com.destroystokyo.paper.block.TargetBlockInfo.FluidMode mode) { return getTargetBlockExact(max, mode(mode)); }
    @Override public BlockFace getTargetBlockFace(int max, FluidCollisionMode mode) { var hit = firstHit(max, mode); return hit == null ? null : hit.face(); }
    @Override public BlockFace getTargetBlockFace(int max, com.destroystokyo.paper.block.TargetBlockInfo.FluidMode mode) { return getTargetBlockFace(max, mode(mode)); }
    @Override public com.destroystokyo.paper.block.TargetBlockInfo getTargetBlockInfo(int max, com.destroystokyo.paper.block.TargetBlockInfo.FluidMode mode) {
        var hit = firstHit(max, mode(mode));
        return hit == null ? null : new com.destroystokyo.paper.block.TargetBlockInfo(hit.block(), hit.face());
    }
    @Override public RayTraceResult rayTraceBlocks(double max, FluidCollisionMode mode) {
        var hit = firstHit((int) Math.ceil(max), mode);
        return hit == null ? null : new RayTraceResult(hit.at(), hit.block(), hit.face());
    }

    @Override public List<Block> getLineOfSight(Set<Material> transparent, int max) {
        var out = new ArrayList<Block>();
        for (Block block : walk(max)) {
            out.add(block);
            if (transparent == null ? !block.getType().isAir() : !transparent.contains(block.getType())) break;
        }
        return out;
    }
    @Override public Block getTargetBlock(Set<Material> transparent, int max) {
        var line = getLineOfSight(transparent, max);
        return line.get(line.size() - 1);
    }
    @Override public List<Block> getLastTwoTargetBlocks(Set<Material> transparent, int max) {
        var line = getLineOfSight(transparent, max);
        return line.size() < 2 ? line : new ArrayList<>(line.subList(line.size() - 2, line.size()));
    }

    @Override public boolean hasLineOfSight(org.bukkit.Location to) {
        var eye = getEyeLocation();
        if (!Objects.equals(eye.getWorld(), to.getWorld())) return false;
        var delta = to.toVector().subtract(eye.toVector());
        double length = delta.length();
        var step = delta.clone().normalize().multiply(0.25);
        var at = eye.toVector();
        for (double travelled = 0; travelled < length; travelled += 0.25) {
            var block = eye.getWorld().getBlockAt(at.getBlockX(), at.getBlockY(), at.getBlockZ());
            if (block.getType().isOccluding()) return false;
            at.add(step);
        }
        return true;
    }
    @Override public boolean hasLineOfSight(org.bukkit.entity.Entity other) {
        return hasLineOfSight(other instanceof org.bukkit.entity.LivingEntity living ? living.getEyeLocation() : other.getLocation());
    }

    private RayTraceResult entityHit(int max, boolean ignoreBlocks) {
        var eye = getEyeLocation();
        var dir = eye.getDirection();
        Double blockDistance = null;
        if (!ignoreBlocks) {
            var hit = firstHit(max, FluidCollisionMode.NEVER);
            if (hit != null) blockDistance = hit.at().distance(eye.toVector());
        }
        RayTraceResult best = null;
        double bestDistance = max;
        for (var other : getServer().getOnlinePlayers()) {
            if (other.equals(this) || !Objects.equals(other.getWorld(), getWorld())) continue;
            var result = other.getBoundingBox().rayTrace(eye.toVector(), dir, max);
            if (result == null) continue;
            double distance = result.getHitPosition().distance(eye.toVector());
            if (distance < bestDistance && (blockDistance == null || distance < blockDistance)) {
                bestDistance = distance;
                best = new RayTraceResult(result.getHitPosition(), other, result.getHitBlockFace());
            }
        }
        return best;
    }
    @Override public org.bukkit.entity.Entity getTargetEntity(int max, boolean ignoreBlocks) { var hit = entityHit(max, ignoreBlocks); return hit == null ? null : hit.getHitEntity(); }
    @Override public RayTraceResult rayTraceEntities(int max, boolean ignoreBlocks) { return entityHit(max, ignoreBlocks); }
    @Override public com.destroystokyo.paper.entity.TargetEntityInfo getTargetEntityInfo(int max, boolean ignoreBlocks) {
        var hit = entityHit(max, ignoreBlocks);
        return hit == null ? null : new com.destroystokyo.paper.entity.TargetEntityInfo(hit.getHitEntity(), hit.getHitPosition());
    }
}
