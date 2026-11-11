package dev.ferrite.server;

import java.lang.reflect.Proxy;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.UUID;
import org.bukkit.Location;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.entity.Entity;

/** A detached, read-only view of an entity stored in a structure template. */
final class StructureEntityView {
    private StructureEntityView() {}

    static Entity of(CapturedStructure.StructureEntity entry) {
        var key = NamespacedKey.fromString(String.valueOf(entry.nbt().get("id")));
        var type = key == null ? null : Registry.ENTITY_TYPE.get(key);
        if (type == null) throw new IllegalStateException("Unknown structure entity type: " + key);
        var words = entry.nbt().get("UUID");
        UUID uuid = words instanceof int[] a && a.length == 4
            ? new UUID(((long) a[0] << 32) | (a[1] & 0xffffffffL), ((long) a[2] << 32) | (a[3] & 0xffffffffL))
            : UUID.nameUUIDFromBytes((Snbt.of(entry.nbt()) + ":" + entry.x() + ":" + entry.y() + ":" + entry.z()).getBytes(StandardCharsets.UTF_8));
        float yaw = 0, pitch = 0;
        if (entry.nbt().get("Rotation") instanceof List<?> rotation && rotation.size() == 2
                && rotation.get(0) instanceof Number y && rotation.get(1) instanceof Number p) {
            yaw = y.floatValue(); pitch = p.floatValue();
        }
        var location = new Location(null, entry.x(), entry.y(), entry.z(), yaw, pitch);
        Class<?> api = type.getEntityClass() == null ? Entity.class : type.getEntityClass();
        return (Entity) Proxy.newProxyInstance(StructureEntityView.class.getClassLoader(), new Class<?>[] {api}, (self, method, args) -> {
            return switch (method.getName()) {
                case "getType" -> type;
                case "getUniqueId" -> uuid;
                case "getLocation" -> {
                    if (args == null || args.length == 0) yield location.clone();
                    var target = (Location) args[0];
                    if (target == null) yield null;
                    target.setWorld(null);
                    target.setX(location.getX()); target.setY(location.getY()); target.setZ(location.getZ());
                    target.setYaw(location.getYaw()); target.setPitch(location.getPitch());
                    yield target;
                }
                case "getWorld" -> null;
                case "isValid" -> false;
                case "isDead" -> false;
                case "hashCode" -> System.identityHashCode(self);
                case "equals" -> self == args[0];
                case "toString" -> "StructureEntity{" + type + " " + uuid + "}";
                default -> throw new UnsupportedOperationException("Ferrite: structure entity snapshot does not support " + method.getName());
            };
        });
    }
}
