package net.minecraft.network.protocol.game;

/** Closes the open container. */
public record ClientboundContainerClosePacket(int containerId) implements net.minecraft.network.protocol.Packet<Object> {
    public int getContainerId() { return containerId; }
}
