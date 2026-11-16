// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package org.bukkit.craftbukkit.entity;

import net.minecraft.server.level.ServerPlayer;

/** Compatibility base for Lane 2's player implementation; behaviour remains Lane 2's job. */
public abstract class CraftPlayer extends CraftEntity implements org.bukkit.entity.Player {
    protected CraftPlayer() {}

    @Override
    public ServerPlayer getHandle() {
        return ServerPlayer.of(getUniqueId());
    }
}
