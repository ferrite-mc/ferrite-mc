package net.minecraft.server.network;

/**
 * Player connection. Only packets the host can express are sent; any other packet fails loudly
 * with its class name instead of being dropped, so a plugin never believes a packet arrived.
 */
public class ServerGamePacketListenerImpl {
    public net.minecraft.server.level.ServerPlayer player;
    public ServerGamePacketListenerImpl(net.minecraft.server.level.ServerPlayer player) { this.player = player; }
    public net.minecraft.server.level.ServerPlayer getPlayer() { return player; }
    public void send(net.minecraft.network.protocol.Packet<?> packet) {
        org.bukkit.entity.Player target = player.getBukkitEntity();
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundSystemChatPacket chat) {
            if (chat.overlay()) target.sendActionBar(chat.content().adventure()); else target.sendMessage(chat.content().adventure());
            return;
        }
        dev.ferrite.server.HostBridge.callVoid("NMS", target.getUniqueId().toString(), "send(" + packet.getClass().getSimpleName() + ")", packet.toString());
    }
    public void disconnect(net.minecraft.network.chat.Component reason) { player.getBukkitEntity().kick(reason.adventure()); }
    public void disconnect(String reason) { player.getBukkitEntity().kickPlayer(reason); }
}
