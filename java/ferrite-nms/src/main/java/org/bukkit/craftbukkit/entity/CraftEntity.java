// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package org.bukkit.craftbukkit.entity;

import dev.ferrite.nms.NmsUnsupported;
import dev.ferrite.nms.host.NmsHostException;
import io.papermc.paper.entity.TeleportFlag;
import java.util.ArrayList;
import java.util.List;
import java.util.Objects;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.phys.Vec3;
import org.bukkit.Bukkit;
import org.bukkit.Location;
import org.bukkit.World;
import org.bukkit.entity.EntityType;
import org.bukkit.event.player.PlayerTeleportEvent;
import org.bukkit.util.Vector;

/**
 * Compatibility base that connects the first-slice Bukkit entity surface to immutable NMS
 * snapshot handles and owner-routed async writes. It never stores a live {@code Entity} or
 * {@code ServerLevel} reference.
 */
public abstract class CraftEntity implements org.bukkit.entity.Entity {
    protected CraftEntity() {}

    public net.minecraft.world.entity.Entity getHandle() {
        return net.minecraft.world.entity.Entity.of(getUniqueId());
    }

    @Override
    public Location getLocation() {
        net.minecraft.world.entity.Entity handle = getHandle();
        World world = resolveWorldOrNull(handle.level().getUUID());
        return new Location(
                world,
                handle.getX(),
                handle.getY(),
                handle.getZ(),
                handle.getYRot(),
                handle.getXRot());
    }

    @Override
    public Location getLocation(Location loc) {
        if (loc == null) {
            return null;
        }
        net.minecraft.world.entity.Entity handle = getHandle();
        loc.setWorld(resolveWorldOrNull(handle.level().getUUID()));
        loc.setX(handle.getX());
        loc.setY(handle.getY());
        loc.setZ(handle.getZ());
        loc.setYaw(handle.getYRot());
        loc.setPitch(handle.getXRot());
        return loc;
    }

    @Override
    public Vector getVelocity() {
        Vec3 velocity = getHandle().getDeltaMovement();
        return new Vector(velocity.x(), velocity.y(), velocity.z());
    }

    @Override
    public World getWorld() {
        ServerLevel level = getHandle().level();
        World world = resolveWorldOrNull(level.getUUID());
        if (world != null) {
            return world;
        }
        if (Bukkit.getServer() != null) {
            throw new NmsHostException(
                    "unknown_world", "world " + level.getUUID() + " is not present on the Bukkit server");
        }
        throw NmsUnsupported.operation(
                "org.bukkit.craftbukkit.entity.CraftEntity",
                "getWorld",
                "no Bukkit Server facade is installed for world "
                        + level.getUUID()
                        + "; use getHandle().level() for the NMS snapshot");
    }

    @Override
    public EntityType getType() {
        String typeKey = getHandle().getTypeKey();
        for (EntityType type : EntityType.values()) {
            if (type.getKey().asString().equals(typeKey)) {
                return type;
            }
        }
        throw new NmsHostException(
                "unknown_entity_type", "Host returned unknown 26.3 entity type: " + typeKey);
    }

    @Override
    public org.bukkit.entity.Entity getVehicle() {
        net.minecraft.world.entity.Entity vehicle = getHandle().getVehicle();
        if (vehicle == null) {
            return null;
        }
        org.bukkit.entity.Entity bukkitVehicle = resolveBukkitEntityOrNull(vehicle.getUUID());
        if (bukkitVehicle != null) {
            return bukkitVehicle;
        }
        throw NmsUnsupported.operation(
                "org.bukkit.craftbukkit.entity.CraftEntity",
                "getVehicle",
                "no Bukkit Entity facade is available for vehicle "
                        + vehicle.getUUID()
                        + "; use getHandle().getVehicle()");
    }

    @Override
    public List<org.bukkit.entity.Entity> getPassengers() {
        List<net.minecraft.world.entity.Entity> passengers = getHandle().getPassengers();
        if (passengers.isEmpty()) {
            return List.of();
        }
        List<org.bukkit.entity.Entity> resolved = new ArrayList<>(passengers.size());
        for (net.minecraft.world.entity.Entity passenger : passengers) {
            org.bukkit.entity.Entity bukkitPassenger = resolveBukkitEntityOrNull(passenger.getUUID());
            if (bukkitPassenger == null) {
                throw NmsUnsupported.operation(
                        "org.bukkit.craftbukkit.entity.CraftEntity",
                        "getPassengers",
                        "no Bukkit Entity facade is available for passenger "
                                + passenger.getUUID()
                                + "; use getHandle().getPassengers()");
            }
            resolved.add(bukkitPassenger);
        }
        return List.copyOf(resolved);
    }

    @Override
    public CompletableFuture<Boolean> teleportAsync(
            Location location, PlayerTeleportEvent.TeleportCause cause, TeleportFlag... flags) {
        if (location == null || location.getWorld() == null) {
            throw new IllegalArgumentException("Teleport location and world must not be null");
        }
        Objects.requireNonNull(cause, "cause");
        if (flags != null && flags.length > 0) {
            throw NmsUnsupported.operation(
                    "org.bukkit.craftbukkit.entity.CraftEntity",
                    "teleportAsync",
                    "TeleportFlag options are not supported in the first slice");
        }
        ServerLevel target = ServerLevel.of(location.getWorld().getUID());
        return getHandle()
                .teleportAsync(
                        target,
                        location.getX(),
                        location.getY(),
                        location.getZ(),
                        location.getYaw(),
                        location.getPitch())
                .thenApply(ignored -> Boolean.TRUE);
    }

    @Override
    public boolean teleport(Location location) {
        throw syncTeleportUnsupported();
    }

    @Override
    public boolean teleport(Location location, PlayerTeleportEvent.TeleportCause cause) {
        throw syncTeleportUnsupported();
    }

    @Override
    public boolean teleport(
            Location location, PlayerTeleportEvent.TeleportCause cause, TeleportFlag... flags) {
        throw syncTeleportUnsupported();
    }

    @Override
    public boolean teleport(org.bukkit.entity.Entity destination) {
        throw syncTeleportUnsupported();
    }

    @Override
    public boolean teleport(
            org.bukkit.entity.Entity destination, PlayerTeleportEvent.TeleportCause cause) {
        throw syncTeleportUnsupported();
    }

    @Override
    public void setVelocity(Vector velocity) {
        throw NmsUnsupported.operation(
                "org.bukkit.craftbukkit.entity.CraftEntity",
                "setVelocity",
                "direct velocity mutation is not exposed in the first snapshot slice");
    }

    private static UnsupportedOperationException syncTeleportUnsupported() {
        return NmsUnsupported.operation(
                "org.bukkit.craftbukkit.entity.CraftEntity",
                "teleport",
                "a synchronous teleport would block the calling Java thread on a region; use teleportAsync(Location)");
    }

    private static World resolveWorldOrNull(UUID worldId) {
        return Bukkit.getServer() == null ? null : Bukkit.getWorld(worldId);
    }

    private static org.bukkit.entity.Entity resolveBukkitEntityOrNull(UUID entityId) {
        return Bukkit.getServer() == null ? null : Bukkit.getEntity(entityId);
    }
}
