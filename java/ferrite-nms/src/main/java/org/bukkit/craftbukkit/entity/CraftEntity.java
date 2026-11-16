// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package org.bukkit.craftbukkit.entity;

/** Compatibility base that adds the NMS snapshot handle; Bukkit entity behaviour stays in Lane 2. */
public abstract class CraftEntity implements org.bukkit.entity.Entity {
    protected CraftEntity() {}

    public net.minecraft.world.entity.Entity getHandle() {
        return net.minecraft.world.entity.Entity.of(getUniqueId());
    }
}
