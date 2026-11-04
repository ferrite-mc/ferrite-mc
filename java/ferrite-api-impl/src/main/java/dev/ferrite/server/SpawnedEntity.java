package dev.ferrite.server;

import java.lang.reflect.Proxy;
import java.util.UUID;

/**
 * The handle {@code World#spawnEntity} returns. The game owns the entity from then on, so the handle
 * only says what was spawned and where; anything that would reach the live entity is refused.
 */
final class SpawnedEntity {
    private SpawnedEntity() { }

    static org.bukkit.entity.Entity of(org.bukkit.entity.EntityType type, org.bukkit.Location at, org.bukkit.World world) {
        var uuid = UUID.randomUUID();
        Class<?> api = type.getEntityClass() != null ? type.getEntityClass() : org.bukkit.entity.Entity.class;
        return (org.bukkit.entity.Entity) Proxy.newProxyInstance(SpawnedEntity.class.getClassLoader(), new Class<?>[]{api}, (self, method, args) -> {
            switch (method.getName()) {
                case "getLocation": return at.clone();
                case "getWorld": return world;
                case "getUniqueId": return uuid;
                case "getType": return type;
                case "hashCode": return uuid.hashCode();
                case "equals": return self == args[0];
                case "toString": return "SpawnedEntity{" + type + " at " + at + "}";
                default: throw new UnsupportedOperationException("Ferrite: not implemented: " + type.name() + "#" + method.getName());
            }
        });
    }
}
