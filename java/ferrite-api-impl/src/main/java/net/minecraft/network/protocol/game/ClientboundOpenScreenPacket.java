package net.minecraft.network.protocol.game;

/** Opens a container screen with a menu type and title. */
public final class ClientboundOpenScreenPacket implements net.minecraft.network.protocol.Packet<Object> {
    private final int containerId;
    private final net.minecraft.world.inventory.MenuType<?> type;
    private final net.minecraft.network.chat.Component title;

    public ClientboundOpenScreenPacket(int containerId, net.minecraft.world.inventory.MenuType<?> type, net.minecraft.network.chat.Component title) {
        this.containerId = containerId;
        this.type = java.util.Objects.requireNonNull(type, "type");
        this.title = java.util.Objects.requireNonNull(title, "title");
    }

    public int getContainerId() { return containerId; }
    public net.minecraft.world.inventory.MenuType<?> getType() { return type; }
    public net.minecraft.network.chat.Component getTitle() { return title; }
}
