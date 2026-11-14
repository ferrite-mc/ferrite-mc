package net.minecraft.world.entity;

/** Entity as NMS names it; every method reads through the Bukkit entity. */
public class Entity {
    protected final org.bukkit.entity.Entity bukkit;
    public Entity(org.bukkit.entity.Entity bukkit) { this.bukkit = java.util.Objects.requireNonNull(bukkit, "bukkit"); }
    public org.bukkit.entity.Entity getBukkitEntity() { return bukkit; }
    public java.util.UUID getUUID() { return bukkit.getUniqueId(); }
    public int getId() { return bukkit.getEntityId(); }
    public double getX() { return bukkit.getLocation().getX(); }
    public double getY() { return bukkit.getLocation().getY(); }
    public double getZ() { return bukkit.getLocation().getZ(); }
    public float getYRot() { return bukkit.getLocation().getYaw(); }
    public float getXRot() { return bukkit.getLocation().getPitch(); }
    public net.minecraft.world.phys.Vec3 position() { var l = bukkit.getLocation(); return new net.minecraft.world.phys.Vec3(l.getX(), l.getY(), l.getZ()); }
    public net.minecraft.core.BlockPos blockPosition() { var l = bukkit.getLocation(); return new net.minecraft.core.BlockPos(l.getBlockX(), l.getBlockY(), l.getBlockZ()); }
    public net.minecraft.world.level.Level level() { return new net.minecraft.server.level.ServerLevel(bukkit.getWorld()); }
    public net.minecraft.network.chat.Component getName() { return net.minecraft.network.chat.Component.literal(bukkit.getName()); }
    public net.minecraft.network.chat.Component getDisplayName() { return net.minecraft.network.chat.Component.literal(bukkit.getName()); }
    public boolean isAlive() { return !bukkit.isDead(); }
    public boolean isRemoved() { return bukkit.isDead(); }
    public boolean isInvulnerable() { return bukkit.isInvulnerable(); }
    public boolean isSilent() { return bukkit.isSilent(); }
    public boolean isShiftKeyDown() { return bukkit instanceof org.bukkit.entity.Player p && p.isSneaking(); }
    public boolean isSprinting() { return bukkit instanceof org.bukkit.entity.Player p && p.isSprinting(); }
    public int getRemainingFireTicks() { return bukkit.getFireTicks(); }
    public void setRemainingFireTicks(int ticks) { bukkit.setFireTicks(ticks); }
    public void setInvulnerable(boolean value) { bukkit.setInvulnerable(value); }
    public void setSilent(boolean value) { bukkit.setSilent(value); }
    /** Velocity in blocks per tick (NMS {@code getDeltaMovement}). */
    public net.minecraft.world.phys.Vec3 getDeltaMovement() { var v = bukkit.getVelocity(); return new net.minecraft.world.phys.Vec3(v.getX(), v.getY(), v.getZ()); }
    public void setDeltaMovement(net.minecraft.world.phys.Vec3 v) { bukkit.setVelocity(v.toBukkit()); }
    public void setDeltaMovement(double x, double y, double z) { bukkit.setVelocity(new org.bukkit.util.Vector(x, y, z)); }
    public void setPos(double x, double y, double z) { var l = bukkit.getLocation(); bukkit.teleport(new org.bukkit.Location(l.getWorld(), x, y, z, l.getYaw(), l.getPitch())); }
    public void teleportTo(double x, double y, double z) { setPos(x, y, z); }
    public void setYRot(float yaw) { bukkit.setRotation(yaw, bukkit.getLocation().getPitch()); }
    public void setXRot(float pitch) { bukkit.setRotation(bukkit.getLocation().getYaw(), pitch); }
    public boolean onGround() { return bukkit.isOnGround(); }
    public boolean isOnFire() { return bukkit.getFireTicks() > 0; }
    public boolean isInWater() { return bukkit.isInWater(); }
    public boolean isNoGravity() { return !bukkit.hasGravity(); }
    public void setNoGravity(boolean value) { bukkit.setGravity(!value); }
    public float getBbWidth() { return (float) bukkit.getWidth(); }
    public float getBbHeight() { return (float) bukkit.getHeight(); }
    public double getEyeY() { return bukkit instanceof org.bukkit.entity.LivingEntity l ? l.getEyeLocation().getY() : getY() + bukkit.getHeight() * 0.85; }
    public boolean hasCustomName() { return bukkit.customName() != null; }
    public net.minecraft.network.chat.Component getCustomName() { var n = bukkit.customName(); return n == null ? null : net.minecraft.network.chat.Component.literal(net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(n)); }
    public java.util.Set<String> getTags() { return bukkit.getScoreboardTags(); }
    public boolean addTag(String tag) { return bukkit.addScoreboardTag(tag); }
    public boolean removeTag(String tag) { return bukkit.removeScoreboardTag(tag); }
    /** The entities riding this one, each wrapped the way {@link #of} wraps a Bukkit entity. */
    public java.util.List<Entity> getPassengers() { var out = new java.util.ArrayList<Entity>(); for (var p : bukkit.getPassengers()) out.add(of(p)); return out; }
    public boolean isVehicle() { return !bukkit.getPassengers().isEmpty(); }
    public boolean isPassenger() { return bukkit.getVehicle() != null; }
    public Entity getVehicle() { var v = bukkit.getVehicle(); return v == null ? null : of(v); }
    public boolean hasPassenger(Entity other) { return bukkit.getPassengers().contains(other.bukkit); }
    public boolean startRiding(Entity vehicle) { return vehicle.bukkit.addPassenger(bukkit); }
    public void stopRiding() { var v = bukkit.getVehicle(); if (v != null) v.removePassenger(bukkit); }
    /** The NMS handle for a Bukkit entity: players and other living entities keep their richer wrappers. */
    public static Entity of(org.bukkit.entity.Entity entity) {
        if (entity instanceof org.bukkit.craftbukkit.entity.CraftPlayer p) return p.getHandle();
        if (entity instanceof org.bukkit.entity.LivingEntity l) return new LivingEntity(l);
        return new Entity(entity);
    }
    public void discard() { bukkit.remove(); }
    @Override public boolean equals(Object other) { return other instanceof Entity e && e.bukkit.getUniqueId().equals(bukkit.getUniqueId()); }
    @Override public int hashCode() { return bukkit.getUniqueId().hashCode(); }
}
