// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package org.bukkit.craftbukkit;

import java.util.UUID;
import net.minecraft.server.level.ServerLevel;

/**
 * Compatibility base for Lane 2's world implementation. It adds only the NMS handle; every
 * Bukkit {@code World} method remains Lane 2's responsibility.
 */
public abstract class CraftWorld implements org.bukkit.World {
    protected CraftWorld() {}

    /** A snapshot handle bound to this world's UID; it cannot read or write another world. */
    public final ServerLevel getHandle() {
        UUID worldId = getUID();
        return ServerLevel.of(worldId);
    }
}
