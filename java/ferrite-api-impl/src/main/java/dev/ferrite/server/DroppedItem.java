package dev.ferrite.server;

import java.lang.reflect.Proxy;
import java.util.UUID;

/**
 * The handle {@code World#dropItem} returns. The game owns the item entity from then on, so the handle
 * only describes what was dropped and where; anything that would need to reach the live entity is refused.
 */
final class DroppedItem {
    private DroppedItem() { }

    static org.bukkit.entity.Item of(org.bukkit.Location at, org.bukkit.inventory.ItemStack stack, org.bukkit.World world) {
        var uuid = UUID.randomUUID();
        return (org.bukkit.entity.Item) Proxy.newProxyInstance(DroppedItem.class.getClassLoader(), new Class<?>[]{org.bukkit.entity.Item.class}, (self, method, args) -> {
            switch (method.getName()) {
                case "getItemStack": return stack.clone();
                case "getLocation": return at.clone();
                case "getWorld": return world;
                case "getUniqueId": return uuid;
                case "getType": return org.bukkit.entity.EntityType.ITEM;
                case "hashCode": return uuid.hashCode();
                case "equals": return self == args[0];
                case "toString": return "DroppedItem{" + stack + " at " + at + "}";
                default: throw new UnsupportedOperationException("Ferrite: not implemented: Item#" + method.getName());
            }
        });
    }
}
