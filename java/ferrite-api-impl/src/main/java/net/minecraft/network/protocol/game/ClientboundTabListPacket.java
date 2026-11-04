package net.minecraft.network.protocol.game;

/** Tab list header and footer. */
public record ClientboundTabListPacket(net.minecraft.network.chat.Component header, net.minecraft.network.chat.Component footer) implements net.minecraft.network.protocol.Packet<Object> {}
