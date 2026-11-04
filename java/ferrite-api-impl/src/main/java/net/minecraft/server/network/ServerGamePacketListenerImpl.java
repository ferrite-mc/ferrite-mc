package net.minecraft.server.network;

/**
 * Player connection. Only packets the host can express are sent; any other packet fails loudly
 * with its class name instead of being dropped, so a plugin never believes a packet arrived.
 */
public class ServerGamePacketListenerImpl {
    public net.minecraft.server.level.ServerPlayer player;
    private net.kyori.adventure.text.Component pendingSubtitle;
    private net.minecraft.network.protocol.game.ClientboundSetTitlesAnimationPacket pendingTimes;
    public ServerGamePacketListenerImpl(net.minecraft.server.level.ServerPlayer player) { this.player = player; }
    public net.minecraft.server.level.ServerPlayer getPlayer() { return player; }
    public void send(net.minecraft.network.protocol.Packet<?> packet) {
        org.bukkit.entity.Player target = player.getBukkitEntity();
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundSystemChatPacket chat) {
            if (chat.overlay()) target.sendActionBar(chat.content().adventure()); else target.sendMessage(chat.content().adventure());
            return;
        }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundSetActionBarTextPacket bar) { target.sendActionBar(bar.text().adventure()); return; }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundSetSubtitleTextPacket subtitle) { pendingSubtitle = subtitle.text().adventure(); return; }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundSetTitlesAnimationPacket times) { pendingTimes = times; return; }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundSetTitleTextPacket title) {
            var timing = pendingTimes == null ? net.kyori.adventure.title.Title.DEFAULT_TIMES
                    : net.kyori.adventure.title.Title.Times.times(java.time.Duration.ofMillis(pendingTimes.fadeIn() * 50L), java.time.Duration.ofMillis(pendingTimes.stay() * 50L), java.time.Duration.ofMillis(pendingTimes.fadeOut() * 50L));
            target.showTitle(net.kyori.adventure.title.Title.title(title.text().adventure(), pendingSubtitle == null ? net.kyori.adventure.text.Component.empty() : pendingSubtitle, timing));
            pendingSubtitle = null;
            return;
        }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundClearTitlesPacket clear) {
            if (clear.resetTimes()) { target.resetTitle(); pendingTimes = null; } else target.clearTitle();
            pendingSubtitle = null;
            return;
        }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundTabListPacket tab) { target.sendPlayerListHeaderAndFooter(tab.header().adventure(), tab.footer().adventure()); return; }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundContainerClosePacket) { target.closeInventory(); return; }
        if (packet instanceof net.minecraft.network.protocol.game.ClientboundSetExperiencePacket exp) { target.sendExperienceChange(exp.experienceProgress(), exp.experienceLevel()); return; }
        dev.ferrite.server.HostBridge.callVoid("NMS", target.getUniqueId().toString(), "send(" + packet.getClass().getSimpleName() + ")", packet.toString());
    }
    public void disconnect(net.minecraft.network.chat.Component reason) { player.getBukkitEntity().kick(reason.adventure()); }
    public void disconnect(String reason) { player.getBukkitEntity().kickPlayer(reason); }
}
