package org.bukkit.craftbukkit;
/** Compatibility server type; {@link #getServer()} hands plugins the NMS server facade. */
public final class CraftServer extends dev.ferrite.server.FerriteServer {
    public CraftServer(Settings settings) { super(settings); }
    public net.minecraft.server.MinecraftServer getServer() { return net.minecraft.server.MinecraftServer.get(); }
    /** Declared here (not only inherited) because command frameworks look it up with getDeclaredMethod. */
    @Override public org.bukkit.command.CommandMap getCommandMap() { return super.getCommandMap(); }
}
