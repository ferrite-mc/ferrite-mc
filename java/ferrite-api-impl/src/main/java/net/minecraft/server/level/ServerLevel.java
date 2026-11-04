package net.minecraft.server.level;

public class ServerLevel extends net.minecraft.world.level.Level {
    public ServerLevel(org.bukkit.World world) { super(world); }
    public org.bukkit.World getWorld() { return world; }
    public net.minecraft.server.MinecraftServer getServer() { return net.minecraft.server.MinecraftServer.get(); }
    public java.util.List<ServerPlayer> players() {
        java.util.List<ServerPlayer> list = new java.util.ArrayList<>();
        for (org.bukkit.entity.Player p : world.getPlayers()) if (p instanceof org.bukkit.craftbukkit.entity.CraftPlayer c) list.add(c.getHandle());
        return list;
    }
}
