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
    public void discard() { bukkit.remove(); }
    @Override public boolean equals(Object other) { return other instanceof Entity e && e.bukkit.getUniqueId().equals(bukkit.getUniqueId()); }
    @Override public int hashCode() { return bukkit.getUniqueId().hashCode(); }
}
