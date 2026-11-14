package net.minecraft.server;

public class MinecraftServer {
    private static final MinecraftServer INSTANCE = new MinecraftServer();
    public static MinecraftServer get() { return INSTANCE; }
    private org.bukkit.Server server() { return org.bukkit.Bukkit.getServer(); }
    public String getMotd() { return server().getMotd(); }
    public int getMaxPlayers() { return server().getMaxPlayers(); }
    public int getPlayerCount() { return server().getOnlinePlayers().size(); }
    public int getTickCount() { return server().getCurrentTick(); }
    public String getServerVersion() { return server().getMinecraftVersion(); }
    public boolean isDedicatedServer() { return true; }
    public boolean isRunning() { return !server().isStopping(); }
    public boolean usesAuthentication() { return server().getOnlineMode(); }
    public int getPort() { return server().getPort(); }
    public java.util.List<net.minecraft.server.level.ServerLevel> getAllLevels() {
        java.util.List<net.minecraft.server.level.ServerLevel> list = new java.util.ArrayList<>();
        for (org.bukkit.World w : server().getWorlds()) list.add(net.minecraft.server.level.ServerLevel.forWorld(w));
        return list;
    }
}
