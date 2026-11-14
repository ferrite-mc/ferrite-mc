package net.minecraft.server.level;

public class ServerPlayer extends net.minecraft.world.entity.player.Player {
    public net.minecraft.server.network.ServerGamePacketListenerImpl connection;
    public ServerPlayer(org.bukkit.craftbukkit.entity.CraftPlayer player) {
        super(player);
        this.connection = new net.minecraft.server.network.ServerGamePacketListenerImpl(this);
    }
    @Override public org.bukkit.craftbukkit.entity.CraftPlayer getBukkitEntity() { return (org.bukkit.craftbukkit.entity.CraftPlayer) player; }
    @Override public ServerLevel level() { return ServerLevel.forWorld(player.getWorld()); }
    public ServerLevel serverLevel() { return level(); }
    public int getPing() { return player.getPing(); }
    public String getIpAddress() { var a = player.getAddress(); return a == null ? "" : a.getAddress().getHostAddress(); }
    public net.minecraft.server.MinecraftServer getServer() { return net.minecraft.server.MinecraftServer.get(); }
}
