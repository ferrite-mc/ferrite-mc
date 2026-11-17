// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.world.entity;

import dev.ferrite.nms.NmsUnsupported;
import dev.ferrite.nms.host.EntitySnapshot;
import dev.ferrite.nms.host.EntityState;
import dev.ferrite.nms.host.NmsHostBinding;
import dev.ferrite.nms.host.NmsHostException;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import net.minecraft.core.BlockPos;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.phys.Vec3;

/**
 * Snapshot entity handle. Reads return immutable values; the only writes are owner-routed
 * futures. Java never receives a live Rust entity object.
 */
public class Entity {
    private final EntitySnapshot snapshot;

    protected Entity(EntitySnapshot snapshot) {
        this.snapshot = Objects.requireNonNull(snapshot, "snapshot");
    }

    public static Entity of(UUID entityId) {
        Objects.requireNonNull(entityId, "entityId");
        return new Entity(HostStateReader.readEntity(entityId));
    }

    public UUID getUUID() {
        return snapshot.id();
    }

    public String getTypeKey() {
        return snapshot.typeKey();
    }

    public ServerLevel level() {
        return ServerLevel.of(snapshot.worldId());
    }

    public double getX() {
        return snapshot.x();
    }

    public double getY() {
        return snapshot.y();
    }

    public double getZ() {
        return snapshot.z();
    }

    public float getYRot() {
        return snapshot.yaw();
    }

    public float getXRot() {
        return snapshot.pitch();
    }

    public Vec3 getDeltaMovement() {
        return new Vec3(snapshot.velocityX(), snapshot.velocityY(), snapshot.velocityZ());
    }

    public Vec3 position() {
        return new Vec3(snapshot.x(), snapshot.y(), snapshot.z());
    }

    public BlockPos blockPosition() {
        return new BlockPos(
                (int) Math.floor(snapshot.x()),
                (int) Math.floor(snapshot.y()),
                (int) Math.floor(snapshot.z()));
    }

    public boolean isPassenger() {
        return snapshot.vehicleId() != null;
    }

    public boolean isVehicle() {
        return !snapshot.passengers().isEmpty();
    }

    public List<UUID> passengerIds() {
        return snapshot.passengers();
    }

    public boolean owns(UUID entityId) {
        return snapshot.id().equals(entityId);
    }

    /** The optional live-state block, or null when the host did not publish one. */
    public EntityState entityState() {
        return snapshot.state();
    }

    public String customName() {
        return requireEntityState("customName").customName();
    }

    public boolean isCustomNameVisible() {
        return requireEntityState("isCustomNameVisible").customNameVisible();
    }

    public boolean isSilent() {
        return requireEntityState("isSilent").silent();
    }

    public boolean isNoGravity() {
        return requireEntityState("isNoGravity").noGravity();
    }

    public boolean isGlowing() {
        return requireEntityState("isGlowing").glowing();
    }

    public boolean isInvisible() {
        return requireEntityState("isInvisible").invisible();
    }

    public Map<String, String> equipment() {
        return requireEntityState("equipment").equipment();
    }

    public String equipmentSlot(String slot) {
        return requireEntityState("equipmentSlot").equipmentItem(slot);
    }

    private EntityState requireEntityState(String accessor) {
        EntityState state = snapshot.state();
        if (state == null) {
            throw NmsUnsupported.operation(
                    "net.minecraft.world.entity.Entity",
                    accessor,
                    "the host snapshot did not include EntityState");
        }
        return state;
    }

    /** The 26.3 network entity id. */
    public int getId() {
        return requireState("getId").networkId();
    }

    /** Whether the host reported this entity as removed. */
    public boolean isRemoved() {
        return requireState("isRemoved").removed();
    }

    public int tickCount() {
        return requireState("tickCount").ticksLived();
    }

    public int getFireTicks() {
        return requireState("getFireTicks").fireTicks();
    }

    public float getFallDistance() {
        return requireState("getFallDistance").fallDistance();
    }

    public boolean onGround() {
        return requireState("onGround").onGround();
    }

    public int getPortalCooldown() {
        return requireState("getPortalCooldown").portalCooldown();
    }

    /** The Bukkit {@code Pose} name from the host, or null when the host omitted it. */
    public String poseName() {
        return requireState("poseName").pose();
    }

    /**
     * Live validity: whether the host still knows this entity and has not marked it removed.
     * An unknown entity is false; any other host failure is propagated.
     */
    public boolean isValid() {
        if (snapshot.state() != null && snapshot.state().removed()) {
            return false;
        }
        try {
            HostStateReader.readEntity(getUUID());
            return true;
        } catch (NmsHostException unknown) {
            if ("unknown_entity".equals(unknown.code())
                    || "entity_not_found".equals(unknown.code())) {
                return false;
            }
            throw unknown;
        }
    }

    private EntityState requireState(String accessor) {
        EntityState state = snapshot.state();
        if (state == null) {
            throw NmsUnsupported.operation(
                    "net.minecraft.world.entity.Entity",
                    accessor,
                    "the host snapshot did not include EntityState; publish the optional EntityState "
                            + "block from NmsHost.entity(UUID) to serve live entity-state reads");
        }
        return state;
    }

    /** The vehicle this entity rides, or null. */
    public Entity getVehicle() {
        UUID vehicleId = snapshot.vehicleId();
        return vehicleId == null ? null : Entity.of(vehicleId);
    }

    /** Immutable passenger list; each passenger is read back through the host. */
    public List<Entity> getPassengers() {
        List<Entity> passengers = new ArrayList<>(snapshot.passengers().size());
        for (UUID passengerId : snapshot.passengers()) {
            passengers.add(Entity.of(passengerId));
        }
        return List.copyOf(passengers);
    }

    /** The immutable host snapshot behind this handle. */
    public EntitySnapshot entitySnapshot() {
        return snapshot;
    }

    public CompletableFuture<Void> teleportAsync(
            ServerLevel target, double x, double y, double z, float yaw, float pitch) {
        Objects.requireNonNull(target, "target");
        validateTeleportCoordinates(x, y, z, yaw, pitch);
        return NmsHostBinding.require()
                .teleport(getUUID(), target.getUUID(), x, y, z, yaw, pitch);
    }

    public CompletableFuture<Void> teleportAsync(
            double x, double y, double z, float yaw, float pitch) {
        validateTeleportCoordinates(x, y, z, yaw, pitch);
        return NmsHostBinding.require()
                .teleport(getUUID(), snapshot.worldId(), x, y, z, yaw, pitch);
    }

    public CompletableFuture<Void> setEquipmentAsync(String slot, String itemKey) {
        String normalizedSlot = EntityState.normalizeEquipmentSlot(slot);
        if (itemKey != null && itemKey.isBlank()) {
            throw new IllegalArgumentException("itemKey must be null or non-blank");
        }
        return NmsHostBinding.require().setEquipment(getUUID(), normalizedSlot, itemKey);
    }

    public CompletableFuture<Void> setVelocityAsync(double vx, double vy, double vz) {
        if (!Double.isFinite(vx) || !Double.isFinite(vy) || !Double.isFinite(vz)) {
            throw new IllegalArgumentException("velocity must be finite");
        }
        return NmsHostBinding.require().setVelocity(getUUID(), vx, vy, vz);
    }

    public CompletableFuture<Void> setRotationAsync(float yaw, float pitch) {
        if (!Float.isFinite(yaw) || !Float.isFinite(pitch)) {
            throw new IllegalArgumentException("rotation must be finite");
        }
        return NmsHostBinding.require().setRotation(getUUID(), yaw, pitch);
    }

    public CompletableFuture<Void> setPoseAsync(String pose) {
        Objects.requireNonNull(pose, "pose");
        if (pose.isBlank()) {
            throw new IllegalArgumentException("pose must not be blank");
        }
        return NmsHostBinding.require().setPose(getUUID(), pose.trim());
    }

    public CompletableFuture<Void> setCustomNameAsync(String customName, boolean visible) {
        if (customName != null && customName.isBlank()) {
            throw new IllegalArgumentException("customName must be null or non-blank");
        }
        return NmsHostBinding.require().setCustomName(getUUID(), customName, visible);
    }

    public CompletableFuture<Void> addPassengerAsync(UUID passengerId) {
        Objects.requireNonNull(passengerId, "passengerId");
        if (getUUID().equals(passengerId)) {
            throw new IllegalArgumentException("entity cannot be its own passenger");
        }
        return NmsHostBinding.require().addPassenger(getUUID(), passengerId);
    }

    public CompletableFuture<Void> removePassengerAsync(UUID passengerId) {
        Objects.requireNonNull(passengerId, "passengerId");
        return NmsHostBinding.require().removePassenger(getUUID(), passengerId);
    }

    public void setPos(double x, double y, double z) {
        throw NmsUnsupported.operation(
                "net.minecraft.world.entity.Entity",
                "setPos",
                "synchronous position mutation would bypass owning-region routing; use teleportAsync(...)");
    }

    public boolean teleportTo(ServerLevel target, double x, double y, double z) {
        throw NmsUnsupported.operation(
                "net.minecraft.world.entity.Entity",
                "teleportTo",
                "a synchronous teleport would block the calling Java thread on a region; use teleportAsync(...)");
    }

    public void setDeltaMovement(Vec3 velocity) {
        throw NmsUnsupported.operation(
                "net.minecraft.world.entity.Entity",
                "setDeltaMovement",
                "direct velocity mutation is not exposed in the first snapshot slice");
    }

    public org.bukkit.entity.Entity getBukkitEntity() {
        throw NmsUnsupported.operation(
                "net.minecraft.world.entity.Entity",
                "getBukkitEntity",
                "snapshot handles are detached from Bukkit facades so weak bridge caches never retain them");
    }

    private static void validateTeleportCoordinates(
            double x, double y, double z, float yaw, float pitch) {
        if (!Double.isFinite(x) || !Double.isFinite(y) || !Double.isFinite(z)) {
            throw new IllegalArgumentException("teleport position must be finite");
        }
        if (!Float.isFinite(yaw) || !Float.isFinite(pitch)) {
            throw new IllegalArgumentException("teleport rotation must be finite");
        }
    }

    @Override
    public String toString() {
        return "Entity{" + getUUID() + ", " + getTypeKey() + "}";
    }
}
