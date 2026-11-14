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
        if (type == org.bukkit.entity.EntityType.FIREWORK_ROCKET) return firework(at, world, uuid, null, (org.bukkit.inventory.meta.FireworkMeta) new org.bukkit.inventory.ItemStack(org.bukkit.Material.FIREWORK_ROCKET).getItemMeta());
        Class<?> api = type.getEntityClass() != null ? type.getEntityClass() : org.bukkit.entity.Entity.class;
        return (org.bukkit.entity.Entity) Proxy.newProxyInstance(SpawnedEntity.class.getClassLoader(), new Class<?>[]{api}, (self, method, args) -> {
            Object result = common(self, method.getName(), args == null ? new Object[0] : args, type, at, world, uuid);
            if (result != NOT_HANDLED) return result;
            throw new UnsupportedOperationException("Ferrite: not implemented: " + type.name() + "#" + method.getName());
        });
    }

    /**
     * A firework rocket handle: age, lifetime and detonation reach the live rocket; what a rocket
     * was built from is fixed when it spawns, so the meta and attachment are read-only.
     */
    static org.bukkit.entity.Firework firework(org.bukkit.Location at, org.bukkit.World world, UUID uuid, UUID attachedTo, org.bukkit.inventory.meta.FireworkMeta meta) {
        var type = org.bukkit.entity.EntityType.FIREWORK_ROCKET;
        return (org.bukkit.entity.Firework) Proxy.newProxyInstance(SpawnedEntity.class.getClassLoader(), new Class<?>[]{org.bukkit.entity.Firework.class}, (self, method, args) -> {
            Object[] a = args == null ? new Object[0] : args;
            switch (method.getName()) {
                case "getLife", "getTicksFlown": { String t = ask(uuid, "getFireworkLife()"); return t == null ? 0 : Integer.parseInt(t); }
                case "getMaxLife", "getTicksToDetonate": { String t = ask(uuid, "getFireworkMaxLife()"); return t == null ? 0 : Integer.parseInt(t); }
                case "setLife": tell(uuid, "setFireworkLife(int)", String.valueOf((int) a[0])); return true;
                case "setMaxLife": tell(uuid, "setFireworkMaxLife(int)", String.valueOf((int) a[0])); return true;
                case "setTicksFlown": tell(uuid, "setFireworkLife(int)", String.valueOf((int) a[0])); return null;
                case "setTicksToDetonate": tell(uuid, "setFireworkMaxLife(int)", String.valueOf((int) a[0])); return null;
                case "getItem": { var stack = new org.bukkit.inventory.ItemStack(org.bukkit.Material.FIREWORK_ROCKET); stack.setItemMeta(meta.clone()); return stack; }
                case "setItem": throw new UnsupportedOperationException("Ferrite: a launched firework's item is fixed when it spawns");
                case "detonate": tell(uuid, "detonate()"); return null;
                case "isDetonated": { String t = ask(uuid, "isValid()"); return t != null && !Boolean.parseBoolean(t); }
                case "getFireworkMeta": return meta.clone();
                case "setFireworkMeta": throw new UnsupportedOperationException("Ferrite: a launched firework's explosions and flight are fixed when it spawns");
                case "getAttachedTo", "getBoostedEntity": return attachedTo == null ? null : org.bukkit.Bukkit.getPlayer(attachedTo);
                case "getSpawningEntity": return attachedTo;
                case "setAttachedTo": throw new UnsupportedOperationException("Ferrite: a firework cannot be attached after it spawns");
                case "isShotAtAngle": return false;
                case "setShotAtAngle": if ((boolean) a[0]) throw new UnsupportedOperationException("Ferrite: crossbow-angled fireworks are not supported"); return null;
                default: break;
            }
            Object result = common(self, method.getName(), a, type, at, world, uuid);
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
