package dev.ferrite.server;

import java.lang.reflect.Proxy;

/**
 * The detached entity {@code Structure#getEntities} returns and {@code EntityTransformer} receives:
 * a read-only view of a captured snapshot that is not in any world. It answers what the snapshot
 * holds (type, UUID, position, velocity, health, custom name) and refuses everything else, so a
 * plugin never mistakes it for a live entity.
 */
final class StructureEntity {
    private StructureEntity() { }

    /** {@code at} is the entity's position: relative to the structure for {@code getEntities}, in the world while placing. */
    static org.bukkit.entity.Entity of(FerriteEntitySnapshot snapshot, org.bukkit.Location at) {
        var type = snapshot.getEntityType();
        Class<?> api = type.getEntityClass() != null ? type.getEntityClass() : org.bukkit.entity.Entity.class;
        return (org.bukkit.entity.Entity) Proxy.newProxyInstance(StructureEntity.class.getClassLoader(), new Class<?>[]{api}, (self, method, args) -> switch (method.getName()) {
            case "getType" -> type;
            case "getUniqueId" -> snapshot.getUniqueId();
            case "getLocation" -> at.clone();
            case "getWorld" -> at.getWorld();
            case "getVelocity" -> snapshot.getVelocity();
            case "getCustomName" -> snapshot.getName();
            case "getHealth" -> {
                if (Double.isNaN(snapshot.getHealth())) throw new UnsupportedOperationException("Ferrite: the snapshot of " + type.name() + " carries no health");
                yield snapshot.getHealth();
            }
            case "isValid" -> false;
            case "isDead" -> false;
            case "createSnapshot" -> snapshot;
            case "hashCode" -> java.util.Objects.hash(snapshot.getUniqueId(), type);
            case "equals" -> self == args[0];
            case "toString" -> "StructureEntity{" + type + " " + snapshot.getUniqueId() + "}";
            default -> throw new UnsupportedOperationException("Ferrite: not implemented: StructureEntity(" + type.name() + ")#" + method.getName());
        });
    }
}
