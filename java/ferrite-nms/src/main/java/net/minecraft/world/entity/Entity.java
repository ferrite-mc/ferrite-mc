// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.world.entity;

import dev.ferrite.nms.host.EntitySnapshot;
import dev.ferrite.nms.host.NmsHostBinding;
import java.util.ArrayList;
import java.util.List;
import java.util.Objects;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
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
        return new Entity(NmsHostBinding.require().entity(entityId));
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
        return NmsHostBinding.require()
                .teleport(getUUID(), target.getUUID(), x, y, z, yaw, pitch);
    }

    public CompletableFuture<Void> teleportAsync(
            double x, double y, double z, float yaw, float pitch) {
        return NmsHostBinding.require()
                .teleport(getUUID(), snapshot.worldId(), x, y, z, yaw, pitch);
    }

    @Override
    public String toString() {
        return "Entity{" + getUUID() + ", " + getTypeKey() + "}";
    }
}
