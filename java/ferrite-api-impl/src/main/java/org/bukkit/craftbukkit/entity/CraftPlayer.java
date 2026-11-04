package org.bukkit.craftbukkit.entity;

/** Compatibility player type: plugins cast to it and call {@link #getHandle()} to reach NMS. */
public abstract class CraftPlayer extends CraftHumanEntity {
    private net.minecraft.server.level.ServerPlayer handle;
    /** The Bukkit view of this player. */
    public org.bukkit.entity.Player getBukkitEntity() { return (org.bukkit.entity.Player) this; }
    /** The NMS player; one instance per Bukkit player. */
    public synchronized net.minecraft.server.level.ServerPlayer getHandle() {
        if (handle == null) handle = new net.minecraft.server.level.ServerPlayer(this);
        return handle;
    }
}
