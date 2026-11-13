package dev.ferrite.server;

import java.lang.reflect.Proxy;
import java.util.UUID;

/**
 * The handle World#spawnEntity and friends return. It carries the id the game entity was given, so
 * reads (position, validity, velocity, health) and the few supported changes (remove, push, move,
 * hurt) reach the live entity. Anything else is refused as not implemented.
 */
final class SpawnedEntity {
    static final Object NOT_HANDLED = new Object();

    private SpawnedEntity() { }

    static org.bukkit.entity.Entity of(org.bukkit.entity.EntityType type, org.bukkit.Location at, org.bukkit.World world, UUID uuid) {
        Class<?> api = type.getEntityClass() != null ? type.getEntityClass() : org.bukkit.entity.Entity.class;
        return (org.bukkit.entity.Entity) Proxy.newProxyInstance(SpawnedEntity.class.getClassLoader(), new Class<?>[]{api}, (self, method, args) -> {
            Object result = common(self, method.getName(), args == null ? new Object[0] : args, type, at, world, uuid);
            if (result != NOT_HANDLED) return result;
            throw new UnsupportedOperationException("Ferrite: not implemented: " + type.name() + "#" + method.getName());
        });
    }

    private static String ask(UUID uuid, String method) {
        try { return (String) HostBridge.call("Entity", uuid, method, String.class); }
        catch (UnsupportedOperationException | IllegalStateException unknown) { return null; }
    }
    private static void tell(UUID uuid, String method, Object... args) { HostBridge.callVoid("Entity", uuid, method, args); }

    /** Shared by every handle; NOT_HANDLED when the method is not one of the supported ones. */
    static Object common(Object self, String name, Object[] args, org.bukkit.entity.EntityType type, org.bukkit.Location at, org.bukkit.World world, UUID uuid) {
        String sep = String.valueOf(HostBridge.ITEM);
        switch (name) {
            case "getUniqueId": return uuid;
            case "getType": return type;
            case "getWorld": return world;
            case "hashCode": return uuid.hashCode();
            case "equals": return self == args[0];
            case "toString": return "Entity{" + type + " " + uuid + "}";
            case "getLocation": {
                String text = ask(uuid, "getEntityPosition()");
                if (text == null) return at.clone();
                String[] p = text.split(sep);
                return new org.bukkit.Location(world, Double.parseDouble(p[0]), Double.parseDouble(p[1]), Double.parseDouble(p[2]), Float.parseFloat(p[3]), Float.parseFloat(p[4]));
            }
            case "isValid": { String text = ask(uuid, "isValid()"); return text == null || Boolean.parseBoolean(text); }
            case "isDead": { String text = ask(uuid, "isValid()"); return text != null && !Boolean.parseBoolean(text); }
            case "createSnapshot": { String text = ask(uuid, "getEntitySnapshot()"); return text != null ? FerriteEntitySnapshot.fromWire(text, text) : FerriteEntitySnapshot.of((org.bukkit.entity.Entity) self); }
            case "remove": tell(uuid, "remove()"); return null;
            case "isInsideVehicle", "leaveVehicle", "eject", "isEmpty": { String text = ask(uuid, name + "()"); return text != null ? Boolean.parseBoolean(text) : name.equals("isEmpty"); }
            case "addPassenger", "removePassenger": return HostBridge.callBoolean("Entity", uuid, name + "(Entity)", args[0]);
            case "getVelocity": {
                String text = ask(uuid, "getVelocity()");
                if (text == null) return new org.bukkit.util.Vector();
                String[] p = text.split(sep);
                return new org.bukkit.util.Vector(Double.parseDouble(p[0]), Double.parseDouble(p[1]), Double.parseDouble(p[2]));
            }
            case "setVelocity": {
                var v = (org.bukkit.util.Vector) args[0];
                tell(uuid, "setVelocity(Vector)", v.getX() + sep + v.getY() + sep + v.getZ());
                return null;
            }
            case "teleport": {
                if (args.length >= 1 && args[0] instanceof org.bukkit.Location l) { tell(uuid, "teleportTo(double,double,double)", l.getX(), l.getY(), l.getZ()); return true; }
                return NOT_HANDLED;
            }
            case "getHealth": { String text = ask(uuid, "getHealth()"); if (text == null) return NOT_HANDLED; return Double.parseDouble(text); }
            case "getMaxHealth": { String text = ask(uuid, "getMaxHealth()"); if (text == null) return NOT_HANDLED; return Double.parseDouble(text); }
            case "damage": { if (args.length == 1 && args[0] instanceof Double d) { tell(uuid, "damage(double)", d); return null; } return NOT_HANDLED; }
            default: return NOT_HANDLED;
        }
    }
}
