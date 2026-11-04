package dev.ferrite.server;

import java.lang.reflect.Proxy;
import java.util.UUID;

/** The handle World#dropItem returns: the entity handle plus what was dropped. */
final class DroppedItem {
    private DroppedItem() { }

    static org.bukkit.entity.Item of(org.bukkit.Location at, org.bukkit.inventory.ItemStack stack, org.bukkit.World world, UUID uuid) {
        return (org.bukkit.entity.Item) Proxy.newProxyInstance(DroppedItem.class.getClassLoader(), new Class<?>[]{org.bukkit.entity.Item.class}, (self, method, args) -> {
            if (method.getName().equals("getItemStack")) return stack.clone();
            Object result = SpawnedEntity.common(self, method.getName(), args == null ? new Object[0] : args, org.bukkit.entity.EntityType.ITEM, at, world, uuid);
            if (result != SpawnedEntity.NOT_HANDLED) return result;
            throw new UnsupportedOperationException("Ferrite: not implemented: Item#" + method.getName());
        });
    }
}
