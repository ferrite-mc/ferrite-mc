package net.minecraft.world.level;

/** A world as NMS sees it; wraps the Bukkit world. */
public class Level {
    protected final org.bukkit.World world;
    public Level(org.bukkit.World world) { this.world = java.util.Objects.requireNonNull(world, "world"); }
    public org.bukkit.World getWorld() { return world; }
    public boolean isClientSide() { return false; }
    public long getGameTime() { return world.getGameTime(); }
    public long getDayTime() { return world.getTime(); }
    public int getSeaLevel() { return world.getSeaLevel(); }
    public int getMinY() { return world.getMinHeight(); }
    public int getMaxY() { return world.getMaxHeight() - 1; }
    public int getHeight() { return world.getMaxHeight() - world.getMinHeight(); }
    public boolean isRaining() { return world.hasStorm(); }
    public boolean isThundering() { return world.isThundering(); }
    @Override public boolean equals(Object other) { return other instanceof Level l && l.world.getUID().equals(world.getUID()); }
    @Override public int hashCode() { return world.getUID().hashCode(); }
    @Override public String toString() { return "Level[" + world.getName() + "]"; }
}
