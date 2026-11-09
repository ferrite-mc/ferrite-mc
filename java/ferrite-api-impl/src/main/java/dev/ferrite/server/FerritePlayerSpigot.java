package dev.ferrite.server;

/**
 * Spigot's extra player surface: legacy chat delivery, hidden viewers and connection details.
 * Messages are converted to their legacy text form and handed to the player's own message path.
 */
final class FerritePlayerSpigot extends org.bukkit.entity.Player.Spigot {
    private final org.bukkit.entity.Player player;

    FerritePlayerSpigot(org.bukkit.entity.Player player) {
        this.player = java.util.Objects.requireNonNull(player, "player");
    }

    @Override public void sendMessage(net.md_5.bungee.api.chat.BaseComponent component) {
        player.sendMessage(net.md_5.bungee.api.chat.TextComponent.toLegacyText(component));
    }

    @Override public void sendMessage(net.md_5.bungee.api.chat.BaseComponent... components) {
        player.sendMessage(net.md_5.bungee.api.chat.TextComponent.toLegacyText(components));
    }

    @Override public void sendMessage(java.util.UUID sender, net.md_5.bungee.api.chat.BaseComponent component) {
        player.sendMessage(sender, net.md_5.bungee.api.chat.TextComponent.toLegacyText(component));
    }

    @Override public void sendMessage(java.util.UUID sender, net.md_5.bungee.api.chat.BaseComponent... components) {
        player.sendMessage(sender, net.md_5.bungee.api.chat.TextComponent.toLegacyText(components));
    }

    @Override public void sendMessage(net.md_5.bungee.api.ChatMessageType position, net.md_5.bungee.api.chat.BaseComponent component) {
        send(position, null, component);
    }

    @Override public void sendMessage(net.md_5.bungee.api.ChatMessageType position, net.md_5.bungee.api.chat.BaseComponent... components) {
        send(position, null, components);
    }

    @Override public void sendMessage(net.md_5.bungee.api.ChatMessageType position, java.util.UUID sender, net.md_5.bungee.api.chat.BaseComponent component) {
        send(position, sender, component);
    }

    @Override public void sendMessage(net.md_5.bungee.api.ChatMessageType position, java.util.UUID sender, net.md_5.bungee.api.chat.BaseComponent... components) {
        send(position, sender, components);
    }

    private void send(net.md_5.bungee.api.ChatMessageType position, java.util.UUID sender, net.md_5.bungee.api.chat.BaseComponent... components) {
        String text = net.md_5.bungee.api.chat.TextComponent.toLegacyText(components);
        if (position == net.md_5.bungee.api.ChatMessageType.ACTION_BAR) player.sendActionBar(text);
        else if (sender == null) player.sendMessage(text);
        else player.sendMessage(sender, text);
    }

    @Override public java.net.InetSocketAddress getRawAddress() { return player.getAddress(); }

    /** Every online player this viewer is not allowed to see. */
    @Override public java.util.Set<org.bukkit.entity.Player> getHiddenPlayers() {
        var hidden = new java.util.LinkedHashSet<org.bukkit.entity.Player>();
        for (var other : player.getServer().getOnlinePlayers()) if (!player.canSee(other)) hidden.add(other);
        return hidden;
    }

    @Override public int getPing() { return player.getPing(); }

    /** Respawns a dead player at the world spawn; the host ignores living players. */
    @Override public void respawn() {
        HostBridge.callVoid("Player", player.getUniqueId(), "respawn()");
    }
}
