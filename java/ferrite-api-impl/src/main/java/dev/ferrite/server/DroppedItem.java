package dev.ferrite.server;

import java.lang.reflect.Proxy;
import java.util.UUID;

/**
 * The handle World#dropItem and friends return: the entity handle plus the item state a plugin
 * may read and change. Reads come from the live host entity and fall back to the values this
 * handle was created with; writes are forwarded so the next read sees them.
 */
final class DroppedItem {
    private DroppedItem() { }

    private static final int DEFAULT_PICKUP_DELAY = 10;

    static org.bukkit.entity.Item of(org.bukkit.Location at, org.bukkit.inventory.ItemStack stack, org.bukkit.World world, UUID uuid) {
        var state = new State(stack);
        return (org.bukkit.entity.Item) Proxy.newProxyInstance(DroppedItem.class.getClassLoader(), new Class<?>[]{org.bukkit.entity.Item.class}, (self, method, args) -> {
            Object[] values = args == null ? new Object[0] : args;
            switch (method.getName()) {
                case "getItemStack": return readStack(uuid, state);
                case "setItemStack": {
                    var replacement = (org.bukkit.inventory.ItemStack) values[0];
                    if (replacement == null) throw new IllegalArgumentException("ItemStack cannot be null");
                    HostBridge.callVoid("Item", uuid, "setItemStack(ItemStack)", replacement);
                    state.stack = replacement.clone();
                    return null;
                }
                case "getPickupDelay": { Integer value = readInt(uuid, "getPickupDelay()"); return value != null ? value : state.pickupDelay; }
                case "setPickupDelay": {
                    int value = (int) values[0];
                    HostBridge.callVoid("Item", uuid, "setPickupDelay(int)", value);
                    state.pickupDelay = value;
                    return null;
                }
                case "getOwner": { UUID value = readUuid(uuid, "getOwner()"); return value != null ? value : state.owner; }
                case "setOwner": {
                    UUID value = (UUID) values[0];
                    HostBridge.callVoid("Item", uuid, "setOwner(UUID)", value);
                    state.owner = value;
                    return null;
                }
                case "getThrower": { UUID value = readUuid(uuid, "getThrower()"); return value != null ? value : state.thrower; }
                case "setThrower": {
                    UUID value = (UUID) values[0];
                    HostBridge.callVoid("Item", uuid, "setThrower(UUID)", value);
                    state.thrower = value;
                    return null;
                }
                case "isUnlimitedLifetime": { Boolean value = readBoolean(uuid, "isUnlimitedLifetime()"); return value != null ? value : state.unlimitedLifetime; }
                case "setUnlimitedLifetime": {
                    boolean value = (boolean) values[0];
                    HostBridge.callVoid("Item", uuid, "setUnlimitedLifetime(boolean)", value);
                    state.unlimitedLifetime = value;
                    return null;
                }
                case "canMobPickup": { Boolean value = readBoolean(uuid, "canMobPickup()"); return value != null ? value : state.canMobPickup; }
                case "setCanMobPickup": {
                    boolean value = (boolean) values[0];
                    HostBridge.callVoid("Item", uuid, "setCanMobPickup(boolean)", value);
                    state.canMobPickup = value;
                    return null;
                }
                case "canPlayerPickup": { Boolean value = readBoolean(uuid, "canPlayerPickup()"); return value != null ? value : state.canPlayerPickup; }
                case "setCanPlayerPickup": {
                    boolean value = (boolean) values[0];
                    HostBridge.callVoid("Item", uuid, "setCanPlayerPickup(boolean)", value);
                    state.canPlayerPickup = value;
                    return null;
                }
                case "willAge": { Boolean value = readBoolean(uuid, "willAge()"); return value != null ? value : state.willAge; }
                case "setWillAge": {
                    boolean value = (boolean) values[0];
                    HostBridge.callVoid("Item", uuid, "setWillAge(boolean)", value);
                    state.willAge = value;
                    return null;
                }
                case "getHealth": { Integer value = readInt(uuid, "getHealth()"); return value != null ? value : state.health; }
                case "setHealth": {
                    int value = (int) values[0];
                    if (value < 0) throw new IllegalArgumentException("health cannot be negative");
                    HostBridge.callVoid("Item", uuid, "setHealth(int)", value);
                    state.health = value;
                    return null;
                }
                default: break;
            }
            Object result = SpawnedEntity.common(self, method.getName(), values, org.bukkit.entity.EntityType.ITEM, at, world, uuid);
            if (result != SpawnedEntity.NOT_HANDLED) return result;
            throw new UnsupportedOperationException("Ferrite: not implemented: Item#" + method.getName());
        });
    }

    private static org.bukkit.inventory.ItemStack readStack(UUID uuid, State state) {
        try {
            var reply = (String) HostBridge.call("Item", uuid, "getItemStack()", String.class);
            var parts = reply.split(java.util.regex.Pattern.quote(String.valueOf(HostBridge.ITEM)), -1);
            if (parts.length == 2) {
                var material = org.bukkit.Material.matchMaterial(parts[0]);
                int amount = Integer.parseInt(parts[1]);
                if (material != null && amount > 0) return new org.bukkit.inventory.ItemStack(material, amount);
            }
        } catch (UnsupportedOperationException | IllegalStateException | IllegalArgumentException offline) { /* keep the local value */ }
        return state.stack.clone();
    }

    private static Integer readInt(UUID uuid, String method) {
        try { return (Integer) HostBridge.call("Item", uuid, method, Integer.class); }
        catch (UnsupportedOperationException | IllegalStateException | IllegalArgumentException offline) { return null; }
    }

    private static Boolean readBoolean(UUID uuid, String method) {
        try { return (Boolean) HostBridge.call("Item", uuid, method, Boolean.class); }
        catch (UnsupportedOperationException | IllegalStateException | IllegalArgumentException offline) { return null; }
    }

    private static UUID readUuid(UUID uuid, String method) {
        try { return (UUID) HostBridge.call("Item", uuid, method, UUID.class); }
        catch (UnsupportedOperationException | IllegalStateException | IllegalArgumentException offline) { return null; }
    }

    /** The values a handle remembers while the host has no item entity yet. */
    private static final class State {
        private org.bukkit.inventory.ItemStack stack;
        private int pickupDelay = DEFAULT_PICKUP_DELAY;
        private UUID owner;
        private UUID thrower;
        private boolean unlimitedLifetime;
        private boolean canMobPickup = true;
        private boolean canPlayerPickup = true;
        private boolean willAge = true;
        private int health = 5;

        private State(org.bukkit.inventory.ItemStack stack) { this.stack = stack.clone(); }
    }
}
