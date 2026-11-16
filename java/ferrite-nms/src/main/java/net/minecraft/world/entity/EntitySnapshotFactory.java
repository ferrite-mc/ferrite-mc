// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.world.entity;

import dev.ferrite.nms.host.EntitySnapshot;
import dev.ferrite.nms.host.PlayerSnapshot;
import java.util.List;
import java.util.Objects;

/** Builds the entity part of a specialised wrapper without duplicating player fields. */
public final class EntitySnapshotFactory {
    private EntitySnapshotFactory() {}

    public static EntitySnapshot forPlayer(PlayerSnapshot player) {
        Objects.requireNonNull(player, "player");
        return new EntitySnapshot(
                player.id(),
                "minecraft:player",
                player.worldId(),
                player.x(),
                player.y(),
                player.z(),
                player.yaw(),
                player.pitch(),
                0.0,
                0.0,
                0.0,
                null,
                List.of());
    }
}
