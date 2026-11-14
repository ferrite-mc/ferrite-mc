package net.minecraft.server.level;

import net.minecraft.world.entity.Entity;

public class ServerLevel extends net.minecraft.world.level.Level {
    public ServerLevel(org.bukkit.World world) { super(world); }
    public org.bukkit.World getWorld() { return world; }
    public net.minecraft.server.MinecraftServer getServer() { return net.minecraft.server.MinecraftServer.get(); }
    public java.util.List<ServerPlayer> players() {
        java.util.List<ServerPlayer> list = new java.util.ArrayList<>();
        for (org.bukkit.entity.Player p : world.getPlayers()) if (p instanceof org.bukkit.craftbukkit.entity.CraftPlayer c) list.add(c.getHandle());
        return list;
    }
    public long getSeed() { return world.getSeed(); }
    public net.minecraft.core.BlockPos getSharedSpawnPos() { var l = world.getSpawnLocation(); return new net.minecraft.core.BlockPos(l.getBlockX(), l.getBlockY(), l.getBlockZ()); }
    public String dimensionKey() { return world.getKey().toString(); }
    public java.util.List<Entity> getAllEntities() {
        var out = new java.util.ArrayList<Entity>();
        for (var e : world.getEntities()) out.add(Entity.of(e));
        return out;
    }
    public Entity getEntity(java.util.UUID id) { var e = world.getEntity(id); return e == null ? null : Entity.of(e); }
}
