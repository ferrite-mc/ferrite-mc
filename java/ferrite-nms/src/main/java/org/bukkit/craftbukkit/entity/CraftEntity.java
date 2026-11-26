// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package org.bukkit.craftbukkit.entity;

import dev.ferrite.nms.NmsUnsupported;
import dev.ferrite.nms.host.EntityState;
import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.NmsHostException;
import io.papermc.paper.entity.TeleportFlag;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Objects;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.phys.Vec3;
import org.bukkit.Bukkit;
import org.bukkit.Location;
import org.bukkit.World;
import org.bukkit.block.BlockFace;
import org.bukkit.entity.EntityType;
import org.bukkit.entity.Pose;
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
            if (type != EntityType.UNKNOWN && type.getKey().asString().equals(typeKey)) {
                return type;
            }
        }
        throw new NmsHostException(
                "unknown_entity_type", "Host returned unknown 26.3 entity type: " + typeKey);
    }

    @Override
    public boolean isInsideVehicle() {
        return getHandle().isPassenger();
    }

    @Override
    public int getEntityId() {
        return requireState("getEntityId").networkId();
    }

    @Override
    public boolean isDead() {
        return requireState("isDead").removed();
    }

    @Override
    public boolean isValid() {
        try {
            EntityState state = net.minecraft.world.entity.HostStateReader.readEntity(getUniqueId()).state();
            return state == null || !state.removed();
        } catch (NmsHostException unknown) {
            if ("unknown_entity".equals(unknown.code())
                    || "entity_not_found".equals(unknown.code())) {
                return false;
            }
            throw unknown;
        }
    }

    @Override
    public int getTicksLived() {
        return requireState("getTicksLived").ticksLived();
    }

    @Override
    public int getFireTicks() {
        return requireState("getFireTicks").fireTicks();
    }

    @Override
    public float getFallDistance() {
        return requireState("getFallDistance").fallDistance();
    }

    @Override
    public boolean isOnGround() {
        return requireState("isOnGround").onGround();
    }

    @Override
    public int getPortalCooldown() {
        return requireState("getPortalCooldown").portalCooldown();
    }

    /**
     * Bukkit uses the entity's yaw for facing. The mapping follows Bukkit's 0=south, 90=west,
     * 180=north, 270=east convention and includes the four diagonal sectors.
     */
    @Override
    public BlockFace getFacing() {
        BlockFace[] facing = {
            BlockFace.SOUTH,
            BlockFace.SOUTH_WEST,
            BlockFace.WEST,
            BlockFace.NORTH_WEST,
            BlockFace.NORTH,
            BlockFace.NORTH_EAST,
            BlockFace.EAST,
            BlockFace.SOUTH_EAST
        };
        double yaw = getHandle().getYRot() % 360.0;
        if (yaw < 0.0) {
            yaw += 360.0;
        }
        int sector = (int) Math.floor((yaw + 22.5) / 45.0) % facing.length;
        return facing[sector];
    }

    @Override
    public Pose getPose() {
        String pose = requireState("getPose").pose();
        if (pose == null) {
            throw NmsUnsupported.operation(
                    "org.bukkit.craftbukkit.entity.CraftEntity",
                    "getPose",
                    "the host EntityState did not include a pose; publish the 26.3 pose name to serve it");
        }
        try {
            return Pose.valueOf(pose.trim().toUpperCase(Locale.ROOT));
        } catch (IllegalArgumentException invalid) {
            throw new NmsHostException(
                    "unknown_pose", "Host returned unknown 26.3 entity pose: " + pose, invalid);
        }
    }

    @Override
    public String getCustomName() {
        return requireState("getCustomName").customName();
    }

    @Override
    public boolean isCustomNameVisible() {
        return requireState("isCustomNameVisible").customNameVisible();
    }

    @Override
    public boolean isSilent() {
        return requireState("isSilent").silent();
    }

    @Override
    public boolean hasGravity() {
        return !requireState("hasGravity").noGravity();
    }

    @Override
    public boolean isGlowing() {
        return requireState("isGlowing").glowing();
    }

    @Override
    public boolean isInvisible() {
        return requireState("isInvisible").invisible();
    }

    /** Returns the item key in {@code slot} (`HAND`, `OFF_HAND`, `FEET`, `LEGS`, `CHEST`, `HEAD`). */
    public String getEquipmentItemKey(String slot) {
        return requireState("getEquipmentItemKey").equipmentItem(slot);
    }

    /** Owner-routed async equipment slot write. */
    public CompletableFuture<Void> setEquipmentAsync(String slot, String itemKey) {
        return getHandle().setEquipmentAsync(slot, itemKey);
    }

    /** Owner-routed async velocity write. */
    public CompletableFuture<Void> setVelocityAsync(Vector velocity) {
        Objects.requireNonNull(velocity, "velocity");
        return getHandle().setVelocityAsync(velocity.getX(), velocity.getY(), velocity.getZ());
    }

    /** Owner-routed async rotation write. */
    public CompletableFuture<Void> setRotationAsync(float yaw, float pitch) {
        return getHandle().setRotationAsync(yaw, pitch);
    }

    /** Owner-routed async pose write. */
    public CompletableFuture<Void> setPoseAsync(Pose pose) {
        Objects.requireNonNull(pose, "pose");
        return getHandle().setPoseAsync(pose.name());
    }

    /** Owner-routed async custom-name write. */
    public CompletableFuture<Void> setCustomNameAsync(String customName, boolean visible) {
        return getHandle().setCustomNameAsync(customName, visible);
    }

    /** Owner-routed async passenger add. */
    public CompletableFuture<Void> addPassengerAsync(UUID passengerId) {
        return getHandle().addPassengerAsync(passengerId);
    }

    /** Owner-routed async passenger remove. */
    public CompletableFuture<Void> removePassengerAsync(UUID passengerId) {
        return getHandle().removePassengerAsync(passengerId);
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
        validateFiniteLocation(location);
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

    @Override
    public boolean addPassenger(org.bukkit.entity.Entity passenger) {
        throw entityWriteRefused("addPassenger");
    }

    @Override
    public boolean removePassenger(org.bukkit.entity.Entity passenger) {
        throw entityWriteRefused("removePassenger");
    }

    @Override
    public boolean eject() {
        throw entityWriteRefused("eject");
    }

    @Override
    public void setFireTicks(int ticks) {
        throw entityWriteRefused("setFireTicks");
    }

    @Override
    public void setFallDistance(float distance) {
        throw entityWriteRefused("setFallDistance");
    }

    @Override
    public void setPortalCooldown(int cooldown) {
        throw entityWriteRefused("setPortalCooldown");
    }

    @Override
    public void setTicksLived(int ticks) {
        throw entityWriteRefused("setTicksLived");
    }

    @Override
    public void setGlowing(boolean glowing) {
        throw entityWriteRefused("setGlowing");
    }

    @Override
    public void setInvulnerable(boolean invulnerable) {
        throw entityWriteRefused("setInvulnerable");
    }

    @Override
    public void setInvisible(boolean invisible) {
        throw entityWriteRefused("setInvisible");
    }

    @Override
    public void setSilent(boolean silent) {
        throw entityWriteRefused("setSilent");
    }

    @Override
    public void setGravity(boolean gravity) {
        throw entityWriteRefused("setGravity");
    }

    @Override
    public void setPose(Pose pose) {
        throw entityWriteRefused("setPose");
    }

    @Override
    public void setPose(Pose pose, boolean fixed) {
        throw entityWriteRefused("setPose");
    }

    private EntityState requireState(String accessor) {
        EntityState state = getHandle().entityState();
        if (state == null) {
            throw NmsUnsupported.operation(
                    "org.bukkit.craftbukkit.entity.CraftEntity",
                    accessor,
                    "the host snapshot did not include EntityState; publish the optional EntityState "
                            + "block from NmsHost.entity(UUID) to serve live entity-state reads");
        }
        return state;
    }

    private static UnsupportedOperationException entityWriteRefused(String method) {
        return NmsUnsupported.operation(
                "org.bukkit.craftbukkit.entity.CraftEntity",
                method,
                "live entity mutation is not in NMS surface 2's snapshot host contract; only "
                        + "teleportAsync is routed. Add an owner-routed host write op before exposing "
                        + "this method");
    }

    private static UnsupportedOperationException syncTeleportUnsupported() {
        return NmsUnsupported.operation(
                "org.bukkit.craftbukkit.entity.CraftEntity",
                "teleport",
                "a synchronous teleport would block the calling Java thread on a region; use teleportAsync(Location)");
    }

    private static void validateFiniteLocation(Location location) {
        if (!Double.isFinite(location.getX())
                || !Double.isFinite(location.getY())
                || !Double.isFinite(location.getZ())) {
            throw new IllegalArgumentException("teleport position must be finite");
        }
        if (!Float.isFinite(location.getYaw()) || !Float.isFinite(location.getPitch())) {
            throw new IllegalArgumentException("teleport rotation must be finite");
        }
    }

    private static World resolveWorldOrNull(UUID worldId) {
        return Bukkit.getServer() == null ? null : Bukkit.getWorld(worldId);
    }

    private static org.bukkit.entity.Entity resolveBukkitEntityOrNull(UUID entityId) {
        return Bukkit.getServer() == null ? null : Bukkit.getEntity(entityId);
    }
}
