package org.bukkit.craftbukkit;

/**
 * CraftBukkit compatibility shell for plugins compiled against a real server.
 *
 * <p>Ferrite has no {@code net.minecraft} classes, so methods whose signatures mention NMS types
 * cannot exist here; a plugin calling them gets {@link NoSuchMethodError} while the class itself
 * loads. This wrapper is deliberately not a {@link org.bukkit.World}: the live world is a Ferrite
 * implementation, so a cast to this type would fail in CraftBukkit too.
 */
public class CraftWorld {
    private final org.bukkit.World world;

    public CraftWorld(org.bukkit.World world) { this.world = java.util.Objects.requireNonNull(world, "world"); }

    public org.bukkit.World getWorld() { return world; }
    /** The NMS level for this world; the same cached handle every caller of this world gets. */
    public net.minecraft.server.level.ServerLevel getHandle() { return net.minecraft.server.level.ServerLevel.forWorld(world); }
    public java.util.UUID getUID() { return world.getUID(); }
    public String getName() { return world.getName(); }
    public org.bukkit.Location getSpawnLocation() { return world.getSpawnLocation(); }
    public org.bukkit.block.Block getBlockAt(int x, int y, int z) { return world.getBlockAt(x, y, z); }
    /** Spawns a dropped item through the live world's normal drop path. */
    public org.bukkit.entity.Item dropItem(org.bukkit.Location location, org.bukkit.inventory.ItemStack item) { return world.dropItem(location, item); }
    public org.bukkit.entity.Item dropItem(org.bukkit.Location location, org.bukkit.inventory.ItemStack item, java.util.function.Consumer<org.bukkit.entity.Item> function) { return world.dropItem(location, item, function); }
    public org.bukkit.entity.Item dropItemNaturally(org.bukkit.Location location, org.bukkit.inventory.ItemStack item) { return world.dropItemNaturally(location, item); }
    public org.bukkit.entity.Item dropItemNaturally(org.bukkit.Location location, org.bukkit.inventory.ItemStack item, java.util.function.Consumer<org.bukkit.entity.Item> function) { return world.dropItemNaturally(location, item, function); }
    public java.util.List<org.bukkit.entity.Player> getPlayers() { return world.getPlayers(); }
    public java.util.List<org.bukkit.entity.Entity> getEntities() { return world.getEntities(); }
    public java.util.List<org.bukkit.entity.LivingEntity> getLivingEntities() { return world.getLivingEntities(); }
    @Override public String toString() { return world.toString(); }
    @Override public boolean equals(Object other) { return other instanceof CraftWorld value && getUID().equals(value.getUID()); }
    @Override public int hashCode() { return getUID().hashCode(); }
}
