package dev.ferrite.api.entity;

import dev.ferrite.api.internal.Unsupported;
import dev.ferrite.api.world.FerriteWorld;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.Map;
import java.util.Objects;
import java.util.UUID;
import org.bukkit.Bukkit;
import org.bukkit.Location;
import org.bukkit.entity.Entity;
import org.bukkit.entity.EntityType;
import org.bukkit.entity.LivingEntity;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.ItemStack;
import org.bukkit.util.Vector;

/**
 * Clean-room immutable snapshot facade for non-player {@link Entity} / {@link LivingEntity}
 * instances returned by {@link FerriteWorld}. Unmodelled methods fail loudly with
 * {@link UnsupportedOperationException}.
 */
public final class FerriteEntitySnapshot {

    private FerriteEntitySnapshot() {}

    public static Entity create(
            FerriteWorld world,
            UUID uuid,
            EntityType type,
            String name,
            String customName,
            double x,
            double y,
            double z,
            float yaw,
            float pitch,
            double vx,
            double vy,
            double vz,
            Double health,
            int networkId) {
        return create(
                world, uuid, type, name, customName, x, y, z, yaw, pitch, vx, vy, vz, health,
                networkId, Map.of());
    }

    public static Entity create(
            FerriteWorld world,
            UUID uuid,
            EntityType type,
            String name,
            String customName,
            double x,
            double y,
            double z,
            float yaw,
            float pitch,
            double vx,
            double vy,
            double vz,
            Double health,
            int networkId,
            Map<EquipmentSlot, ItemStack> equipment) {
        return create(
                world, uuid, type, name, customName, x, y, z, yaw, pitch, vx, vy, vz, health,
                networkId, equipment, Map.of());
    }

    public static Entity create(
            FerriteWorld world,
            UUID uuid,
            EntityType type,
            String name,
            String customName,
            double x,
            double y,
            double z,
            float yaw,
            float pitch,
            double vx,
            double vy,
            double vz,
            Double health,
            int networkId,
            Map<EquipmentSlot, ItemStack> equipment,
            Map<EquipmentSlot, Float> dropChances) {
        Objects.requireNonNull(world, "world");
        Objects.requireNonNull(uuid, "uuid");
        Objects.requireNonNull(type, "type");
        Class<?> entityInterface = resolveInterface(type, health != null);
        InvocationHandler handler = new SnapshotHandler(
                entityInterface,
                world,
                uuid,
                type,
                name != null ? name : type.name(),
                customName,
                x,
                y,
                z,
                yaw,
                pitch,
                vx,
                vy,
                vz,
                health,
                networkId,
                equipment,
                dropChances);
        return (Entity) Proxy.newProxyInstance(
                FerriteEntitySnapshot.class.getClassLoader(),
                new Class<?>[] {entityInterface},
                handler);
    }

    private static Class<?> resolveInterface(EntityType type, boolean living) {
        Class<? extends Entity> specific = type.getEntityClass();
        if (specific != null && specific.isInterface()) {
            return specific;
        }
        return living ? LivingEntity.class : Entity.class;
    }

    private static final class SnapshotHandler implements InvocationHandler {
        private final Class<?> entityInterface;
        private final FerriteWorld world;
        private final UUID uuid;
        private final EntityType type;
        private final String name;
        private final String customName;
        private final double x;
        private final double y;
        private final double z;
        private final float yaw;
        private final float pitch;
        private final double vx;
        private final double vy;
        private final double vz;
        private final Double health;
        private final int networkId;
        private final Map<EquipmentSlot, ItemStack> initialEquipment;
        private final Map<EquipmentSlot, Float> initialDropChances;
        private FerriteEntityEquipment equipment;

        private SnapshotHandler(
                Class<?> entityInterface,
                FerriteWorld world,
                UUID uuid,
                EntityType type,
                String name,
                String customName,
                double x,
                double y,
                double z,
                float yaw,
                float pitch,
                double vx,
                double vy,
                double vz,
                Double health,
                int networkId,
                Map<EquipmentSlot, ItemStack> initialEquipment,
                Map<EquipmentSlot, Float> initialDropChances) {
            this.entityInterface = entityInterface;
            this.world = world;
            this.uuid = uuid;
            this.type = type;
            this.name = name;
            this.customName = customName;
            this.x = x;
            this.y = y;
            this.z = z;
            this.yaw = yaw;
            this.pitch = pitch;
            this.vx = vx;
            this.vy = vy;
            this.vz = vz;
            this.health = health;
            this.networkId = networkId;
            this.initialEquipment = initialEquipment != null ? initialEquipment : Map.of();
            this.initialDropChances = initialDropChances != null ? initialDropChances : Map.of();
        }

        @Override
        public Object invoke(Object proxy, Method method, Object[] args) {
            String methodName = method.getName();
            int paramCount = method.getParameterCount();

            if ("equals".equals(methodName) && paramCount == 1) {
                Object other = args[0];
                return other instanceof Entity e && uuid.equals(e.getUniqueId());
            }
            if ("hashCode".equals(methodName) && paramCount == 0) {
                return uuid.hashCode();
            }
            if ("toString".equals(methodName) && paramCount == 0) {
                return "FerriteEntitySnapshot{type=" + type + ",uuid=" + uuid + "}";
            }
            if ("getUniqueId".equals(methodName) && paramCount == 0) {
                return uuid;
            }
            if ("getName".equals(methodName) && paramCount == 0) {
                return customName != null ? customName : name;
            }
            if ("getScoreboardEntryName".equals(methodName) && paramCount == 0) {
                return uuid.toString();
            }
            if ("getCustomName".equals(methodName) && paramCount == 0) {
                return customName;
            }
            if ("isCustomNameVisible".equals(methodName) && paramCount == 0) {
                return customName != null;
            }
            if ("getWorld".equals(methodName) && paramCount == 0) {
                return world;
            }
            if ("getType".equals(methodName) && paramCount == 0) {
                return type;
            }
            if ("getEntityId".equals(methodName) && paramCount == 0) {
                return networkId;
            }
            if ("getLocation".equals(methodName) && paramCount == 0) {
                return new Location(world, x, y, z, yaw, pitch);
            }
            if ("getLocation".equals(methodName) && paramCount == 1) {
                Location target = (Location) args[0];
                if (target == null) {
                    return null;
                }
                target.setWorld(world);
                target.setX(x);
                target.setY(y);
                target.setZ(z);
                target.setYaw(yaw);
                target.setPitch(pitch);
                return target;
            }
            if ("getVelocity".equals(methodName) && paramCount == 0) {
                return new Vector(vx, vy, vz);
            }
            if ("isValid".equals(methodName) && paramCount == 0) {
                return true;
            }
            if ("isDead".equals(methodName) && paramCount == 0) {
                return health != null && health <= 0.0;
            }
            if ("getServer".equals(methodName) && paramCount == 0) {
                return Bukkit.getServer();
            }
            if ("getHealth".equals(methodName) && paramCount == 0 && health != null) {
                return health;
            }
            if ("getMaxHealth".equals(methodName) && paramCount == 0 && health != null) {
                return 20.0;
            }
            if ("getEquipment".equals(methodName)
                    && paramCount == 0
                    && LivingEntity.class.isAssignableFrom(entityInterface)) {
                if (equipment == null) {
                    equipment = new FerriteEntityEquipment(
                            (Entity) proxy, uuid, initialEquipment, initialDropChances);
                }
                return equipment;
            }
            throw Unsupported.exception(entityInterface.getSimpleName() + "#" + methodName);
        }
    }
}
